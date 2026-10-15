//! Edge refinement for coarse masks: a colour **guided filter** (He, Sun & Tang, "Guided
//! Image Filtering", ECCV 2010 / TPAMI 2013) snaps a mask that was predicted at low
//! resolution and scaled up to the edges of the full-size photo, so hair and shoulders
//! don't carry a rim of the old background.
//!
//! Locally the refined mask is a linear function of the photo's colours, q = aᵀI + b, with
//! (a, b) fitted to the coarse mask in each window. The coefficients are computed on a
//! smaller copy and scaled up ("Fast Guided Filter", He & Sun 2015), so a 12 MP photo
//! takes well under a second. Only an **edge band** is changed: where the coarse mask is
//! confidently inside or outside, it stays as it was, so textures in the subject or the
//! background can't punch holes or add specks.

use image::imageops::{self, FilterType};
use image::{GrayImage, Luma, RgbImage};
use rayon::prelude::*;

use crate::refine::shift_mask;

/// How the refinement is tuned (lengths in pixels of the full-size image).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuidedSettings {
    /// Window radius of the filter.
    pub radius: f32,
    /// Regularisation: larger keeps the coarse mask's smoothness, smaller follows the
    /// photo's edges more closely (colours 0–1).
    pub eps: f32,
    /// Half-width of the edge band that may change.
    pub band: f32,
    /// Local colour variance (sum over R, G, B) at which the refinement counts half;
    /// where the photo has less contrast than this, the coarse mask is mostly kept (the
    /// filter can't find an edge there and would only blur it).
    pub contrast: f32,
}

impl GuidedSettings {
    /// Settings for a mask predicted at `model_size`² and scaled up to `width`×`height`:
    /// the window and band grow with the upscale factor.
    pub fn for_upscale(model_size: u32, width: u32, height: u32) -> GuidedSettings {
        let scale = (width.max(height) as f32 / model_size as f32).max(1.0);
        GuidedSettings {
            radius: (scale * 2.0).clamp(2.0, 32.0),
            eps: 1e-3,
            band: (scale * 2.5).clamp(3.0, 48.0),
            contrast: 6e-2,
        }
    }
}

/// Box mean of one channel (`w`×`h`) with radius `r`, edges handled by averaging only the
/// pixels inside (like the reference implementation's N normalisation).
fn box_mean(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    // Integral image.
    let mut sum = vec![0f64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0f64;
        for x in 0..w {
            row += src[y * w + x] as f64;
            sum[(y + 1) * (w + 1) + x + 1] = sum[y * (w + 1) + x + 1] + row;
        }
    }
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, line)| {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for (x, o) in line.iter_mut().enumerate() {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let s = sum[y1 * (w + 1) + x1] - sum[y0 * (w + 1) + x1] - sum[y1 * (w + 1) + x0]
                + sum[y0 * (w + 1) + x0];
            *o = (s / ((x1 - x0) * (y1 - y0)) as f64) as f32;
        }
    });
    out
}

/// Solves the 3×3 symmetric system Σ a = c (Σ = [[xx, xy, xz], [xy, yy, yz], [xz, yz, zz]]).
#[allow(clippy::too_many_arguments)]
fn solve3(xx: f32, xy: f32, xz: f32, yy: f32, yz: f32, zz: f32, c: [f32; 3]) -> [f32; 3] {
    let i00 = yy * zz - yz * yz;
    let i01 = xz * yz - xy * zz;
    let i02 = xy * yz - xz * yy;
    let i11 = xx * zz - xz * xz;
    let i12 = xz * xy - xx * yz;
    let i22 = xx * yy - xy * xy;
    let det = xx * i00 + xy * i01 + xz * i02;
    if det.abs() < 1e-12 {
        return [0.0; 3];
    }
    [
        (i00 * c[0] + i01 * c[1] + i02 * c[2]) / det,
        (i01 * c[0] + i11 * c[1] + i12 * c[2]) / det,
        (i02 * c[0] + i12 * c[1] + i22 * c[2]) / det,
    ]
}

/// The colour guided filter of `mask` with `guide`, both `w`×`h` (colours and mask 0–1):
/// returns the coefficients (a: 3 planes, b) after the final box mean, and the local
/// colour variance (trace of the covariance).
#[allow(clippy::type_complexity)]
fn coefficients(
    guide: &[[f32; 3]],
    mask: &[f32],
    w: usize,
    h: usize,
    r: usize,
    eps: f32,
) -> ([Vec<f32>; 3], Vec<f32>, Vec<f32>) {
    let n = w * h;
    let plane = |f: &dyn Fn(usize) -> f32| -> Vec<f32> { (0..n).map(f).collect() };
    let mean = |v: &[f32]| box_mean(v, w, h, r);
    let ch = |c: usize| plane(&|i| guide[i][c]);
    let (ir, ig, ib) = (ch(0), ch(1), ch(2));
    let (mr, mg, mb, mp) = (mean(&ir), mean(&ig), mean(&ib), mean(mask));
    let prod = |a: &[f32], b: &[f32]| -> Vec<f32> { a.iter().zip(b).map(|(x, y)| x * y).collect() };
    let (mrp, mgp, mbp) = (
        mean(&prod(&ir, mask)),
        mean(&prod(&ig, mask)),
        mean(&prod(&ib, mask)),
    );
    let (rr, rg, rb) = (
        mean(&prod(&ir, &ir)),
        mean(&prod(&ir, &ig)),
        mean(&prod(&ir, &ib)),
    );
    let (gg, gb, bb) = (
        mean(&prod(&ig, &ig)),
        mean(&prod(&ig, &ib)),
        mean(&prod(&ib, &ib)),
    );

    let mut a = [vec![0f32; n], vec![0f32; n], vec![0f32; n]];
    let mut b = vec![0f32; n];
    let mut variance = vec![0f32; n];
    for i in 0..n {
        variance[i] = (rr[i] - mr[i] * mr[i]) + (gg[i] - mg[i] * mg[i]) + (bb[i] - mb[i] * mb[i]);
        let cov = [
            mrp[i] - mr[i] * mp[i],
            mgp[i] - mg[i] * mp[i],
            mbp[i] - mb[i] * mp[i],
        ];
        let x = solve3(
            rr[i] - mr[i] * mr[i] + eps,
            rg[i] - mr[i] * mg[i],
            rb[i] - mr[i] * mb[i],
            gg[i] - mg[i] * mg[i] + eps,
            gb[i] - mg[i] * mb[i],
            bb[i] - mb[i] * mb[i] + eps,
            cov,
        );
        for c in 0..3 {
            a[c][i] = x[c];
        }
        b[i] = mp[i] - x[0] * mr[i] - x[1] * mg[i] - x[2] * mb[i];
    }
    (
        [mean(&a[0]), mean(&a[1]), mean(&a[2])],
        mean(&b),
        mean(&variance),
    )
}

/// Refines `mask` (the coarse mask, scaled up to the photo's size) along its edges using
/// `image` as the guide.
pub fn refine_edges(image: &RgbImage, mask: &GrayImage, settings: &GuidedSettings) -> GrayImage {
    let (w, h) = image.dimensions();
    assert_eq!((w, h), mask.dimensions());
    // The band that may change: within `band` px of a pixel that isn't clearly in or out.
    let band = settings.band.round().max(1.0);
    let uncertain = GrayImage::from_fn(w, h, |x, y| {
        let v = mask.get_pixel(x, y).0[0];
        Luma([if (8..=247).contains(&v) { 255 } else { 0 }])
    });
    let band_mask = shift_mask(&uncertain, band);
    // Also include the boundary of hard masks (a step from 0 to 255).
    let hard_edge = {
        let grown = shift_mask(mask, 1.0);
        let shrunk = shift_mask(mask, -1.0);
        GrayImage::from_fn(w, h, |x, y| {
            Luma([grown.get_pixel(x, y).0[0].saturating_sub(shrunk.get_pixel(x, y).0[0])])
        })
    };
    let band_mask = {
        let edge = shift_mask(&hard_edge, band);
        GrayImage::from_fn(w, h, |x, y| {
            Luma([band_mask.get_pixel(x, y).0[0].max(edge.get_pixel(x, y).0[0])])
        })
    };
    if band_mask.pixels().all(|p| p.0[0] == 0) {
        return mask.clone();
    }

    // Fast guided filter: the coefficients on a copy with windows of ~4 px, scaled up.
    let s = (settings.radius / 4.0).max(1.0);
    let (sw, sh) = (
        ((w as f32 / s).round() as u32).max(1),
        ((h as f32 / s).round() as u32).max(1),
    );
    let small_img = imageops::resize(image, sw, sh, FilterType::Triangle);
    let small_mask = imageops::resize(mask, sw, sh, FilterType::Triangle);
    let guide: Vec<[f32; 3]> = small_img
        .pixels()
        .map(|p| {
            [
                p.0[0] as f32 / 255.0,
                p.0[1] as f32 / 255.0,
                p.0[2] as f32 / 255.0,
            ]
        })
        .collect();
    let m: Vec<f32> = small_mask.pixels().map(|p| p.0[0] as f32 / 255.0).collect();
    let r = ((settings.radius / s).round() as usize).max(1);
    let (a, b, variance) = coefficients(&guide, &m, sw as usize, sh as usize, r, settings.eps);

    // Scale the coefficients up (bilinear) and apply them to the full-size photo.
    let (fx, fy) = (sw as f32 / w as f32, sh as f32 / h as f32);
    let sample = |plane: &[f32], x: u32, y: u32| {
        let gx = ((x as f32 + 0.5) * fx - 0.5).clamp(0.0, (sw - 1) as f32);
        let gy = ((y as f32 + 0.5) * fy - 0.5).clamp(0.0, (sh - 1) as f32);
        let (x0, y0) = (gx.floor() as usize, gy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(sw as usize - 1), (y0 + 1).min(sh as usize - 1));
        let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
        let at = |xx: usize, yy: usize| plane[yy * sw as usize + xx];
        (at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx) * (1.0 - ty)
            + (at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx) * ty
    };
    let mut out = mask.clone();
    out.par_chunks_mut(w as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let y = y as u32;
            for (x, o) in row.iter_mut().enumerate() {
                let x = x as u32;
                let in_band = band_mask.get_pixel(x, y).0[0] as f32 / 255.0;
                if in_band == 0.0 {
                    continue;
                }
                let v = sample(&variance, x, y).max(0.0);
                let weight = in_band * v / (v + settings.contrast);
                let p = image.get_pixel(x, y).0;
                let q = sample(&a[0], x, y) * p[0] as f32 / 255.0
                    + sample(&a[1], x, y) * p[1] as f32 / 255.0
                    + sample(&a[2], x, y) * p[2] as f32 / 255.0
                    + sample(&b, x, y);
                let coarse = *o as f32 / 255.0;
                let v = weight * q.clamp(0.0, 1.0) + (1.0 - weight) * coarse;
                *o = (v * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::{mae, scenes};

    /// A mask predicted at `size`² and scaled back up, like U²-Net's.
    fn coarse(mask: &GrayImage, size: u32) -> GrayImage {
        let small = imageops::resize(mask, size, size, FilterType::Lanczos3);
        imageops::resize(&small, mask.width(), mask.height(), FilterType::CatmullRom)
    }

    #[test]
    fn solves_3x3_systems() {
        // [[2,1,0],[1,3,1],[0,1,4]] · [1,2,3] = [4,10,14]
        let x = solve3(2.0, 1.0, 0.0, 3.0, 1.0, 4.0, [4.0, 10.0, 14.0]);
        for (got, want) in x.iter().zip([1.0, 2.0, 3.0]) {
            assert!((got - want).abs() < 1e-4, "{x:?}");
        }
    }

    #[test]
    fn box_mean_averages_inside_the_image() {
        let v: Vec<f32> = (0..12).map(|i| i as f32).collect(); // 4×3
        let m = box_mean(&v, 4, 3, 1);
        assert!((m[0] - (0.0 + 1.0 + 4.0 + 5.0) / 4.0).abs() < 1e-5);
        assert!(
            (m[5]
                - (0..3)
                    .flat_map(|y| (0..3).map(move |x| (y * 4 + x) as f32))
                    .sum::<f32>()
                    / 9.0)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn snaps_coarse_masks_to_the_photo() {
        // A 320² mask scaled up (U²-Net's case) on the synthetic scenes: where subject and
        // background differ clearly, the refined mask is closer to the exact one; where
        // they barely differ (low-contrast), it stays about as good as before.
        for scene in scenes() {
            let rgb = scene.image.clone();
            let (w, h) = rgb.dimensions();
            let rough = coarse(&scene.mask, 320);
            let settings = GuidedSettings::for_upscale(320, w, h);
            let refined = refine_edges(&rgb, &rough, &settings);
            let (before, after) = (mae(&rough, &scene.mask), mae(&refined, &scene.mask));
            println!("{:<13} MAE {before:.4} → {after:.4}", scene.name);
            assert!(
                after <= before + 0.0005,
                "{}: {before:.4} → {after:.4}",
                scene.name
            );
            if scene.name != "low-contrast" {
                assert!(after < before, "{}: {before:.4} → {after:.4}", scene.name);
            }
        }
    }

    #[test]
    fn keeps_confident_areas() {
        // A textured subject with a hard-edged mask: far from the edge nothing changes.
        let (w, h) = (200u32, 160u32);
        let rgb = RgbImage::from_fn(w, h, |x, y| {
            let t = ((x * 37 + y * 91) % 256) as u8;
            if x < 100 {
                image::Rgb([t, 255 - t, 40])
            } else {
                image::Rgb([20, 30, 200])
            }
        });
        let mask = GrayImage::from_fn(w, h, |x, _| Luma([if x < 100 { 255 } else { 0 }]));
        let settings = GuidedSettings {
            radius: 4.0,
            eps: 1e-3,
            band: 6.0,
            contrast: 4e-3,
        };
        let refined = refine_edges(&rgb, &mask, &settings);
        for y in 0..h {
            for x in (0..80).chain(120..200) {
                assert_eq!(refined.get_pixel(x, y), mask.get_pixel(x, y), "({x},{y})");
            }
        }
    }

    #[test]
    fn grows_with_the_upscale() {
        let small = GuidedSettings::for_upscale(320, 640, 480);
        let big = GuidedSettings::for_upscale(320, 4032, 3024);
        assert!(big.radius > small.radius && big.band > small.band);
        assert_eq!(GuidedSettings::for_upscale(320, 300, 200).radius, 2.0);
    }
}
