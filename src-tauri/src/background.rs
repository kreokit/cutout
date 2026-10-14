//! Backgrounds behind the cutout: transparent, a solid colour, the original photo
//! blurred, or an image of the user's. Each one covers the whole output frame; the
//! cutout is placed on top separately (see export).
//!
//! **Blur without a halo.** Blurring the photo as it is would spread the subject's
//! colours into the background around it. Instead the blur is a *normalized
//! convolution* that only sees the background: blur(original × background weight) /
//! blur(background weight), with the weight eroded first so the subject's soft edge
//! doesn't count. Where no background is within reach (behind a large subject), wider
//! blurs fill in from the surrounding background.
//!
//! **Speed.** The blur is computed on a small grid (cells of about σ/4 pixels, so the
//! blur there is only a few cells wide) and scaled up to the output. The grid depends on
//! the original and the strength only, so the preview and the export of any size show
//! the same background.

use image::imageops::{self, FilterType};
use image::{GrayImage, Rgba, RgbaImage};
use rayon::prelude::*;
use serde::{Deserialize, Deserializer};

use crate::refine::gaussian_blur;

/// The background chosen in the inspector (shared by all open images).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Background {
    Transparent,
    Color {
        #[serde(deserialize_with = "hex_colour")]
        color: [u8; 3],
    },
    /// `strength` from [`BLUR_MIN`] to 100.
    Blur {
        strength: f32,
    },
    Image {
        path: String,
    },
}

/// The weakest blur strength (keeps the blur grid small on huge photos).
pub const BLUR_MIN: f32 = 5.0;
/// σ of the strongest blur (strength 100), as a share of the original's short side.
const BLUR_MAX_SIGMA: f32 = 0.06;
/// σ of the blur on the grid, in cells.
const GRID_SIGMA: f32 = 4.0;
/// The grid never has more cells than this.
const GRID_MAX_CELLS: u64 = 2_000_000;
/// Cells that are less background than this don't count as background at all.
const CLEAN_FROM: f32 = 0.9;
/// A blur level is trusted where at least this share of its reach is background;
/// below that it fades into the next, wider level.
const TRUST_FROM: f32 = 0.05;

/// Parses `#RRGGBB` (the colour pickers' format).
pub fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn hex_colour<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 3], D::Error> {
    let hex = String::deserialize(d)?;
    parse_hex(&hex).ok_or_else(|| serde::de::Error::custom(format!("not a colour: {hex}")))
}

/// A `width`×`height` frame of one opaque colour.
pub fn solid(colour: [u8; 3], width: u32, height: u32) -> RgbaImage {
    let [r, g, b] = colour;
    RgbaImage::from_pixel(width, height, Rgba([r, g, b, 255]))
}

/// How a `src_w`×`src_h` picture is scaled and moved to cover a `width`×`height` frame,
/// centred: frame pixel centre `(x + 0.5, y + 0.5)` shows source point
/// `((x + 0.5 - width/2) / scale + src_w/2, …)`. Returns `scale`.
fn cover_scale(src_w: u32, src_h: u32, width: u32, height: u32) -> f32 {
    (width as f32 / src_w as f32).max(height as f32 / src_h as f32)
}

/// `src` scaled to cover a `width`×`height` frame, centred and cropped (a custom image
/// background), fully opaque.
pub fn cover(src: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (sw, sh) = src.dimensions();
    let scale = cover_scale(sw, sh, width, height);
    // The part of the source that is visible, rounded to whole pixels.
    let cw = ((width as f32 / scale).round() as u32).clamp(1, sw);
    let ch = ((height as f32 / scale).round() as u32).clamp(1, sh);
    let crop = imageops::crop_imm(src, (sw - cw) / 2, (sh - ch) / 2, cw, ch).to_image();
    let mut out = imageops::resize(&crop, width, height, FilterType::CatmullRom);
    for p in out.pixels_mut() {
        p.0[3] = 255;
    }
    out
}

/// σ of the blur at `strength`, in pixels of a `width`×`height` original.
pub fn blur_sigma(strength: f32, width: u32, height: u32) -> f32 {
    strength.clamp(BLUR_MIN, 100.0) / 100.0 * BLUR_MAX_SIGMA * width.min(height) as f32
}

/// The blur grid of an original: pixels per cell (both axes) and its size in cells.
fn grid_size(width: u32, height: u32, sigma: f32) -> (u32, u32) {
    let mut cell = (sigma / GRID_SIGMA).max(1.0);
    let pixels = width as f64 * height as f64;
    cell = cell.max((pixels / GRID_MAX_CELLS as f64).sqrt() as f32);
    let size = |cell: f32| {
        (
            ((width as f32 / cell).ceil() as u32).clamp(1, width),
            ((height as f32 / cell).ceil() as u32).clamp(1, height),
        )
    };
    // Rounding up can go just over the limit.
    while size(cell).0 as u64 * size(cell).1 as u64 > GRID_MAX_CELLS {
        cell *= 1.01;
    }
    size(cell)
}

/// One row of pooled grid cells: background-weighted colour sums, weight sums and
/// pixel counts.
type CellRow = (Vec<[f32; 3]>, Vec<f32>, Vec<u32>);

/// RGB planes on the blur grid.
struct Planes {
    w: usize,
    h: usize,
    rgb: [Vec<f32>; 3],
}

/// The original blurred without its subject, on the blur grid. `mask` is the cutout's
/// alpha (the subject is opaque).
fn blur_grid(original: &RgbaImage, mask: &GrayImage, strength: f32) -> Planes {
    let (width, height) = original.dimensions();
    let sigma = blur_sigma(strength, width, height);
    let (gw, gh) = grid_size(width, height, sigma);
    let (gw, gh) = (gw as usize, gh as usize);
    let cell_sigma = sigma * gw as f32 / width as f32;
    let col_of: Vec<usize> = (0..width as usize)
        .map(|x| x * gw / width as usize)
        .collect();

    // Pool each cell.
    let rows: Vec<CellRow> = (0..gh)
        .into_par_iter()
        .map(|gy| {
            let (mut sum, mut weight, mut count) =
                (vec![[0f32; 3]; gw], vec![0f32; gw], vec![0u32; gw]);
            let y0 = (gy * height as usize).div_ceil(gh);
            let y1 = ((gy + 1) * height as usize).div_ceil(gh);
            for y in y0..y1.min(height as usize) {
                for (x, &c) in col_of.iter().enumerate() {
                    let p = original.get_pixel(x as u32, y as u32).0;
                    let w = 1.0 - mask.get_pixel(x as u32, y as u32).0[0] as f32 / 255.0;
                    for i in 0..3 {
                        sum[c][i] += w * p[i] as f32;
                    }
                    weight[c] += w;
                    count[c] += 1;
                }
            }
            (sum, weight, count)
        })
        .collect();

    // Each cell's background colour and how clean it is (1 = all background).
    let n = gw * gh;
    let mut colour = [vec![0f32; n], vec![0f32; n], vec![0f32; n]];
    let mut clean = vec![0u8; n];
    for (gy, (sum, weight, count)) in rows.iter().enumerate() {
        for gx in 0..gw {
            let i = gy * gw + gx;
            if weight[gx] > 0.0 {
                for c in 0..3 {
                    colour[c][i] = sum[gx][c] / weight[gx];
                }
            }
            let share = if count[gx] > 0 {
                weight[gx] / count[gx] as f32
            } else {
                0.0
            };
            let e = ((share - CLEAN_FROM) / (1.0 - CLEAN_FROM)).clamp(0.0, 1.0);
            clean[i] = (e * 255.0).round() as u8;
        }
    }
    // Erode by one cell: anything next to the subject doesn't count either.
    let clean = crate::refine::shift_mask(
        &GrayImage::from_raw(gw as u32, gh as u32, clean).unwrap(),
        -1.0,
    );
    let mut e: Vec<f32> = clean.as_raw().iter().map(|&v| v as f32 / 255.0).collect();
    if e.iter().all(|&v| v == 0.0) {
        // No background anywhere: blur the whole photo.
        e.iter_mut().for_each(|v| *v = 1.0);
        plain_means(original, gw, gh, &col_of, &mut colour);
    }
    let weighted: [Vec<f32>; 3] =
        std::array::from_fn(|c| colour[c].iter().zip(&e).map(|(v, w)| v * w).collect());

    // Normalized convolution at σ, then wider levels where too little background is near.
    let blur = |src: &[f32], s: f32| {
        let mut d = src.to_vec();
        gaussian_blur(&mut d, gw, gh, s);
        d
    };
    let mut levels = Vec::new();
    let mut s = cell_sigma;
    loop {
        let d = blur(&e, s);
        let n3: [Vec<f32>; 3] = std::array::from_fn(|c| blur(&weighted[c], s));
        let done = d.iter().all(|&v| v >= TRUST_FROM) || s > 2.0 * gw.max(gh) as f32;
        levels.push((d, n3));
        if done {
            break;
        }
        s *= 3.0;
    }
    // The mean background colour, for anything even the widest level doesn't reach.
    let total: f32 = e.iter().sum();
    let mean: [f32; 3] =
        std::array::from_fn(|c| weighted[c].iter().sum::<f32>() / total.max(f32::EPSILON));

    let mut rgb = [vec![0f32; n], vec![0f32; n], vec![0f32; n]];
    for i in 0..n {
        let mut v = mean;
        for (d, n3) in levels.iter().rev() {
            if d[i] > 0.0 {
                let a = (d[i] / TRUST_FROM).min(1.0);
                for c in 0..3 {
                    v[c] = a * (n3[c][i] / d[i]) + (1.0 - a) * v[c];
                }
            }
        }
        for c in 0..3 {
            rgb[c][i] = v[c];
        }
    }
    Planes { w: gw, h: gh, rgb }
}

/// The plain (unweighted) mean colour of each grid cell.
fn plain_means(
    original: &RgbaImage,
    gw: usize,
    gh: usize,
    col_of: &[usize],
    colour: &mut [Vec<f32>; 3],
) {
    let height = original.height();
    let mut count = vec![0u32; gw * gh];
    colour
        .iter_mut()
        .for_each(|p| p.iter_mut().for_each(|v| *v = 0.0));
    for y in 0..height as usize {
        let gy = y * gh / height as usize;
        for (x, &gx) in col_of.iter().enumerate() {
            let i = gy * gw + gx;
            let p = original.get_pixel(x as u32, y as u32).0;
            for c in 0..3 {
                colour[c][i] += p[c] as f32;
            }
            count[i] += 1;
        }
    }
    for (i, k) in count.iter().enumerate() {
        for plane in colour.iter_mut() {
            plane[i] /= (*k).max(1) as f32;
        }
    }
}

/// The original without its subject, blurred with `strength`, covering a
/// `width`×`height` frame. `mask` is the cutout's alpha at the original's size.
pub fn blurred(
    original: &RgbaImage,
    mask: &GrayImage,
    strength: f32,
    width: u32,
    height: u32,
) -> RgbaImage {
    assert_eq!(original.dimensions(), mask.dimensions());
    let grid = blur_grid(original, mask, strength);
    let (sw, sh) = original.dimensions();
    let scale = cover_scale(sw, sh, width, height);
    // Frame pixel → grid coordinates (cell centres at integers).
    let gx_of = |x: u32| {
        let sx = (x as f32 + 0.5 - width as f32 / 2.0) / scale + sw as f32 / 2.0;
        sx * grid.w as f32 / sw as f32 - 0.5
    };
    let gy_of = |y: u32| {
        let sy = (y as f32 + 0.5 - height as f32 / 2.0) / scale + sh as f32 / 2.0;
        sy * grid.h as f32 / sh as f32 - 0.5
    };
    let taps = |g: f32, n: usize| {
        let g = g.clamp(0.0, (n - 1) as f32);
        let i0 = g.floor() as usize;
        (i0, (i0 + 1).min(n - 1), g - i0 as f32)
    };
    let xs: Vec<(usize, usize, f32)> = (0..width).map(|x| taps(gx_of(x), grid.w)).collect();
    let mut out = RgbaImage::new(width, height);
    out.par_chunks_mut(width as usize * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let (y0, y1, ty) = taps(gy_of(y as u32), grid.h);
            for (x, &(x0, x1, tx)) in xs.iter().enumerate() {
                for c in 0..3 {
                    let p = &grid.rgb[c];
                    let top = p[y0 * grid.w + x0] * (1.0 - tx) + p[y0 * grid.w + x1] * tx;
                    let bottom = p[y1 * grid.w + x0] * (1.0 - tx) + p[y1 * grid.w + x1] * tx;
                    row[x * 4 + c] =
                        (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
                }
                row[x * 4 + 3] = 255;
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    const BG: [u8; 3] = [40, 110, 200];
    const SUBJECT: [u8; 3] = [255, 0, 0];

    /// A blue-ish background with a soft-edged red disc; edge pixels mix both colours,
    /// as in a real photo.
    fn scene(w: u32, h: u32, r: f32) -> (RgbaImage, GrayImage) {
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let alpha = |x: u32, y: u32| {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            ((r - d) / 3.0 + 0.5).clamp(0.0, 1.0)
        };
        let image = RgbaImage::from_fn(w, h, |x, y| {
            let a = alpha(x, y);
            let mix = |i: usize| (SUBJECT[i] as f32 * a + BG[i] as f32 * (1.0 - a)).round() as u8;
            Rgba([mix(0), mix(1), mix(2), 255])
        });
        let mask = GrayImage::from_fn(w, h, |x, y| Luma([(alpha(x, y) * 255.0).round() as u8]));
        (image, mask)
    }

    #[test]
    fn parses_colours() {
        assert_eq!(parse_hex("#C8F135"), Some([0xC8, 0xF1, 0x35]));
        assert_eq!(parse_hex("#0e0e10"), Some([14, 14, 16]));
        assert_eq!(parse_hex("C8F135"), None);
        assert_eq!(parse_hex("#C8F13"), None);
        assert_eq!(parse_hex("#C8F1ZZ"), None);
        let bg: Background =
            serde_json::from_str(r##"{"kind":"color","color":"#FFFFFF"}"##).unwrap();
        assert_eq!(
            bg,
            Background::Color {
                color: [255, 255, 255]
            }
        );
        let bg: Background = serde_json::from_str(r#"{"kind":"blur","strength":40}"#).unwrap();
        assert_eq!(bg, Background::Blur { strength: 40.0 });
        assert!(serde_json::from_str::<Background>(r#"{"kind":"color","color":"red"}"#).is_err());
    }

    #[test]
    fn solid_fills_the_frame() {
        let img = solid([1, 2, 3], 7, 5);
        assert_eq!(img.dimensions(), (7, 5));
        assert!(img.pixels().all(|p| p.0 == [1, 2, 3, 255]));
    }

    #[test]
    fn cover_fills_and_keeps_the_centre() {
        // Left third red, middle green, right third blue; covering a square keeps the middle.
        let src = RgbaImage::from_fn(300, 100, |x, _| match x / 100 {
            0 => Rgba([255, 0, 0, 128]),
            1 => Rgba([0, 255, 0, 128]),
            _ => Rgba([0, 0, 255, 128]),
        });
        let out = cover(&src, 50, 50);
        assert_eq!(out.dimensions(), (50, 50));
        assert!(out.pixels().all(|p| p.0 == [0, 255, 0, 255]));
        // A tall frame from a wide picture: the full height is used, stretched evenly.
        let out = cover(&src, 30, 90);
        assert_eq!(out.dimensions(), (30, 90));
        assert_eq!(out.get_pixel(15, 45).0, [0, 255, 0, 255]);
    }

    #[test]
    fn blur_has_no_halo_around_the_subject() {
        let (image, mask) = scene(300, 200, 50.0);
        let out = blurred(&image, &mask, 100.0, 300, 200);
        assert_eq!(out.dimensions(), (300, 200));
        // Everywhere, also right next to the subject and behind it, the background colour.
        let worst = out
            .pixels()
            .map(|p| {
                (0..3)
                    .map(|i| (p.0[i] as i32 - BG[i] as i32).abs())
                    .max()
                    .unwrap()
            })
            .max()
            .unwrap();
        assert!(worst <= 2, "background off by {worst}");

        // The test is sensitive: a plain blur of the photo is clearly red near the subject.
        let plain = blurred(&image, &GrayImage::new(300, 200), 100.0, 300, 200);
        let near = plain.get_pixel(150 + 55, 100).0;
        assert!(near[0] as i32 - BG[0] as i32 > 30, "plain blur {near:?}");
    }

    #[test]
    fn blur_keeps_the_layout_of_the_background() {
        // Dark left half, bright right half, a subject in the middle: far from the
        // boundary each side keeps its colour; the boundary itself is soft.
        let (w, h) = (400, 200);
        let image = RgbaImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                Rgba([20, 20, 20, 255])
            } else {
                Rgba([230, 230, 230, 255])
            }
        });
        let mask = GrayImage::from_fn(w, h, |x, y| {
            Luma([
                if (x as i32 - 100).abs() < 20 && (y as i32 - 100).abs() < 20 {
                    255
                } else {
                    0
                },
            ])
        });
        let out = blurred(&image, &mask, 30.0, w, h);
        assert!(out.get_pixel(20, 100).0[0] <= 22);
        assert!(out.get_pixel(380, 100).0[0] >= 228);
        let mid = out.get_pixel(200, 100).0[0];
        assert!(mid > 60 && mid < 190, "boundary {mid}");
        // Behind the subject: filled from the dark background around it.
        assert!(out.get_pixel(100, 100).0[0] <= 30);
    }

    #[test]
    fn blur_looks_the_same_at_every_size() {
        let (image, mask) = scene(600, 400, 80.0);
        let image = RgbaImage::from_fn(600, 400, |x, y| {
            let p = image.get_pixel(x, y).0;
            Rgba([p[0], (x / 3) as u8, (y / 2) as u8, 255])
        });
        let big = blurred(&image, &mask, 40.0, 600, 400);
        let small = blurred(&image, &mask, 40.0, 150, 100);
        let shrunk = imageops::resize(&big, 150, 100, FilterType::Triangle);
        let diff: f64 = small
            .as_raw()
            .iter()
            .zip(shrunk.as_raw())
            .map(|(a, b)| (*a as f64 - *b as f64).abs())
            .sum::<f64>()
            / small.as_raw().len() as f64;
        assert!(diff < 1.5, "mean difference {diff}");
        // Another aspect ratio: covers the frame exactly.
        assert_eq!(
            blurred(&image, &mask, 40.0, 1080, 1920).dimensions(),
            (1080, 1920)
        );
    }

    #[test]
    fn blur_of_an_all_subject_photo() {
        let (image, _) = scene(120, 80, 30.0);
        let full = GrayImage::from_pixel(120, 80, Luma([255]));
        let out = blurred(&image, &full, 50.0, 120, 80);
        assert_eq!(out.dimensions(), (120, 80));
        assert!(out.pixels().all(|p| p.0[3] == 255));
        // It's a plain blur then: some red from the disc shows in the middle.
        assert!(out.get_pixel(60, 40).0[0] > BG[0]);
    }

    #[test]
    fn grid_stays_small() {
        // A 48-megapixel photo at the weakest blur.
        let sigma = blur_sigma(0.0, 8064, 6048);
        let (gw, gh) = grid_size(8064, 6048, sigma);
        assert!(gw as u64 * gh as u64 <= GRID_MAX_CELLS);
        assert!(blur_sigma(100.0, 1000, 500) == 30.0);
    }

    /// Times the blur on a 48-megapixel photo: `cargo test --release background_timings
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn background_timings() {
        let (image, mask) = scene(8064, 6048, 2000.0);
        for strength in [BLUR_MIN, 40.0, 100.0] {
            let t = std::time::Instant::now();
            let out = blurred(&image, &mask, strength, 8064, 6048);
            println!(
                "blur {strength:>5} at 8064×6048: {:.2}s",
                t.elapsed().as_secs_f32()
            );
            drop(out);
        }
        let t = std::time::Instant::now();
        blurred(&image, &mask, 40.0, 1080, 1920);
        println!("blur 40 to 1080×1920: {:.2}s", t.elapsed().as_secs_f32());
    }
}
