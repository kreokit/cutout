//! Refining a cutout: the maths behind the Edges controls and the brush.
//!
//! The final mask is built in a fixed order, so what the user paints is exactly what they
//! get: the AI mask → **shift** (grow or shrink) → **feather** (soften) → **brush strokes**
//! (restore / erase, in order). The colours of semi-transparent edge pixels can then be
//! **decontaminated** ("Remove dark fringe"): the old background mixed into them is
//! estimated and taken out.
//!
//! Every function here works on whole images or crops and is used for both the preview
//! and the export. Lengths (brush radius, shift, feather) are in pixels of the image they
//! are applied to.

use image::{GrayImage, Luma, RgbaImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushMode {
    /// Make pixels transparent.
    Erase,
    /// Bring pixels back (fully opaque, original colour).
    Restore,
}

/// One brush stroke: a polyline swept by a round brush.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub mode: BrushMode,
    /// Brush radius in image pixels.
    pub radius: f32,
    /// 0 = hard edge (1 px anti-aliasing), 1 = soft from the centre.
    pub softness: f32,
    /// Points in image pixels; pixel (x, y) covers x…x+1, y…y+1.
    pub points: Vec<[f32; 2]>,
}

impl Stroke {
    /// The same stroke in an image scaled by `scale` (e.g. a smaller working copy).
    pub fn scaled(&self, scale: f32) -> Stroke {
        Stroke {
            radius: self.radius * scale,
            points: self
                .points
                .iter()
                .map(|[x, y]| [x * scale, y * scale])
                .collect(),
            ..self.clone()
        }
    }

    /// Pixel bounds (x0, y0, x1, y1), exclusive end, clipped to `width`×`height`.
    pub fn bounds(&self, width: u32, height: u32) -> Option<Rect> {
        let r = self.radius + 1.0;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &[x, y] in &self.points {
            x0 = x0.min(x - r);
            y0 = y0.min(y - r);
            x1 = x1.max(x + r);
            y1 = y1.max(y + r);
        }
        Rect::clipped(x0.floor(), y0.floor(), x1.ceil(), y1.ceil(), width, height)
    }
}

/// A pixel rectangle: `x`, `y` and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn full(width: u32, height: u32) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    /// The rectangle x0…x1, y0…y1 (exclusive) inside `width`×`height`; None if empty.
    fn clipped(x0: f32, y0: f32, x1: f32, y1: f32, width: u32, height: u32) -> Option<Rect> {
        let x0 = x0.max(0.0) as u32;
        let y0 = y0.max(0.0) as u32;
        let x1 = (x1.max(0.0) as u32).min(width);
        let y1 = (y1.max(0.0) as u32).min(height);
        (x1 > x0 && y1 > y0).then_some(Rect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    /// This rectangle grown by `margin` on every side, inside `width`×`height`.
    pub fn grown(self, margin: u32, width: u32, height: u32) -> Rect {
        let x = self.x.saturating_sub(margin);
        let y = self.y.saturating_sub(margin);
        let x1 = (self.x + self.width + margin).min(width);
        let y1 = (self.y + self.height + margin).min(height);
        Rect {
            x,
            y,
            width: x1 - x,
            height: y1 - y,
        }
    }

    pub fn union(self, other: Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let x1 = (self.x + self.width).max(other.x + other.width);
        let y1 = (self.y + self.height).max(other.y + other.height);
        Rect {
            x,
            y,
            width: x1 - x,
            height: y1 - y,
        }
    }
}

/// How much a brush of `radius` and `softness` covers a pixel whose centre is `distance`
/// from the stroke: 1 inside the hard core, 0 outside the radius, a smoothstep between.
/// The soft band is at least 1 px wide, so a hard brush is still anti-aliased.
/// Shared with the frontend's live preview (fixtures/brush-cases.json).
pub fn brush_alpha(distance: f32, radius: f32, softness: f32) -> f32 {
    let band = (radius * softness.clamp(0.0, 1.0))
        .max(1.0)
        .min(radius.max(1e-6));
    let inner = radius - band;
    if distance <= inner {
        1.0
    } else if distance >= radius {
        0.0
    } else {
        let t = (radius - distance) / band;
        t * t * (3.0 - 2.0 * t)
    }
}

/// Distance from `p` to the segment a–b.
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (a[0] + t * dx - p[0], a[1] + t * dy - p[1]);
    (cx * cx + cy * cy).sqrt()
}

/// Applies `stroke` to `mask` (0–255) within `area`. Within one stroke, coverage is the
/// maximum over its segments (overlaps don't build up); restore then blends towards 255
/// and erase towards 0.
pub fn apply_stroke(mask: &mut GrayImage, stroke: &Stroke) {
    let (w, h) = mask.dimensions();
    let Some(bounds) = stroke.bounds(w, h) else {
        return;
    };
    let segments: Vec<([f32; 2], [f32; 2])> = match stroke.points.as_slice() {
        [] => return,
        [p] => vec![(*p, *p)],
        points => points.windows(2).map(|s| (s[0], s[1])).collect(),
    };
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let d = segments
                .iter()
                .map(|&(a, b)| segment_distance(p, a, b))
                .fold(f32::INFINITY, f32::min);
            let a = brush_alpha(d, stroke.radius, stroke.softness);
            if a <= 0.0 {
                continue;
            }
            let m = mask.get_pixel(x, y).0[0] as f32;
            let out = match stroke.mode {
                BrushMode::Restore => m + (255.0 - m) * a,
                BrushMode::Erase => m * (1.0 - a),
            };
            mask.put_pixel(x, y, Luma([out.round().clamp(0.0, 255.0) as u8]));
        }
    }
}

/// Settings of the Edges controls.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeSettings {
    /// Gaussian softening of the mask edge, as a blur radius in pixels (σ = feather / 2).
    pub feather: f32,
    /// Grow (> 0) or shrink (< 0) the mask by this many pixels.
    pub shift: f32,
    /// Remove the old background's colour from semi-transparent edge pixels.
    pub decontaminate: bool,
}

impl Default for EdgeSettings {
    fn default() -> Self {
        EdgeSettings {
            feather: 0.0,
            shift: 0.0,
            decontaminate: true,
        }
    }
}

impl EdgeSettings {
    /// The same settings for an image scaled by `scale`.
    pub fn scaled(self, scale: f32) -> EdgeSettings {
        EdgeSettings {
            feather: self.feather * scale,
            shift: self.shift * scale,
            ..self
        }
    }
}

/// Running min or max over a window of `2r+1` samples (van Herk / Gil-Werman, O(n)).
/// Samples beyond the ends are treated as absent.
fn running_extreme(line: &[u8], r: usize, out: &mut [u8], max: bool) {
    let n = line.len();
    let pick = |a: u8, b: u8| if max { a.max(b) } else { a.min(b) };
    let w = 2 * r + 1;
    let ident = if max { 0u8 } else { 255u8 };
    // Pad with the identity so the window never reads past the ends.
    let padded_len = n + 2 * r;
    let at = |i: usize| -> u8 {
        if i < r || i >= r + n {
            ident
        } else {
            line[i - r]
        }
    };
    let blocks = padded_len.div_ceil(w);
    let total = blocks * w;
    let mut g = vec![ident; total]; // prefix extremes within each block
    let mut hh = vec![ident; total]; // suffix extremes within each block
    for i in 0..total {
        let v = if i < padded_len { at(i) } else { ident };
        g[i] = if i % w == 0 { v } else { pick(g[i - 1], v) };
    }
    for i in (0..total).rev() {
        let v = if i < padded_len { at(i) } else { ident };
        hh[i] = if i % w == w - 1 || i == total - 1 {
            v
        } else {
            pick(hh[i + 1], v)
        };
    }
    for (i, o) in out.iter_mut().enumerate().take(n) {
        // Window over padded indices i … i + 2r.
        *o = pick(hh[i], g[i + 2 * r]);
    }
}

/// Grows (`shift` > 0, grayscale dilation) or shrinks (erosion) the mask by `shift`
/// pixels with a square window. The soft edge profile is kept, just moved.
pub fn shift_mask(mask: &GrayImage, shift: f32) -> GrayImage {
    let r = shift.abs().round() as usize;
    if r == 0 {
        return mask.clone();
    }
    let max = shift > 0.0;
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let mut tmp = vec![0u8; w * h];
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        running_extreme(
            &mask.as_raw()[y * w..(y + 1) * w],
            r,
            &mut tmp[y * w..(y + 1) * w],
            max,
        );
    }
    let (mut col, mut res) = (vec![0u8; h], vec![0u8; h]);
    for x in 0..w {
        for y in 0..h {
            col[y] = tmp[y * w + x];
        }
        running_extreme(&col, r, &mut res, max);
        for y in 0..h {
            out[y * w + x] = res[y];
        }
    }
    GrayImage::from_raw(w as u32, h as u32, out).unwrap()
}

/// Box blur of one channel (`w`×`h`, row-major) with a window of `2r+1`, edges clamped.
fn box_blur(data: &mut [f32], w: usize, h: usize, r: usize) {
    if r == 0 || w == 0 || h == 0 {
        return;
    }
    let mut line = vec![0f32; w.max(h)];
    let norm = 1.0 / (2 * r + 1) as f32;
    let blur_line = |src: &[f32], dst: &mut [f32]| {
        let n = src.len();
        let get = |i: isize| src[i.clamp(0, n as isize - 1) as usize];
        let mut sum: f32 = (-(r as isize)..=r as isize).map(get).sum();
        for (i, d) in dst.iter_mut().enumerate() {
            *d = sum * norm;
            sum += get(i as isize + r as isize + 1) - get(i as isize - r as isize);
        }
    };
    for y in 0..h {
        let row = &mut data[y * w..(y + 1) * w];
        line[..w].copy_from_slice(row);
        blur_line(&line[..w], row);
    }
    let mut col = vec![0f32; h];
    for x in 0..w {
        for y in 0..h {
            col[y] = data[y * w + x];
        }
        blur_line(&col, &mut line[..h]);
        for y in 0..h {
            data[y * w + x] = line[y];
        }
    }
}

/// Three box blurs approximating a Gaussian with standard deviation `sigma`.
pub(crate) fn gaussian_blur(data: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma < 0.3 {
        return;
    }
    // Box radii for 3 passes (Kovesi, "Fast almost-Gaussian filtering").
    let n = 3.0;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n)
        / (-4.0 * wl as f32 - 4.0))
        .round() as i32;
    for i in 0..3 {
        let size = if i < m { wl } else { wu };
        box_blur(data, w, h, ((size - 1) / 2).max(0) as usize);
    }
}

/// Softens the mask edge: a Gaussian blur with σ = `feather` / 2.
pub fn feather_mask(mask: &GrayImage, feather: f32) -> GrayImage {
    if feather <= 0.0 {
        return mask.clone();
    }
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let mut data: Vec<f32> = mask.as_raw().iter().map(|&v| v as f32).collect();
    gaussian_blur(&mut data, w, h, feather / 2.0);
    let pixels = data
        .iter()
        .map(|v| v.round().clamp(0.0, 255.0) as u8)
        .collect();
    GrayImage::from_raw(w as u32, h as u32, pixels).unwrap()
}

/// The AI mask with the Edges settings applied (shift, then feather).
pub fn edge_mask(base: &GrayImage, edges: &EdgeSettings) -> GrayImage {
    feather_mask(&shift_mask(base, edges.shift), edges.feather)
}

/// How far decontamination looks around a pixel (see [`decontaminate`]), in pixels. A
/// region rendered with at least this margin is exact.
pub const DECONTAMINATE_REACH: u32 = (BLUR_LARGE / 2 + BLUR_SMALL / 2) as u32;
const BLUR_LARGE: usize = 90;
const BLUR_SMALL: usize = 6;

/// Alpha values treated as fully transparent (≤) or fully opaque (≥) by
/// decontamination: their colour is kept, and tiles with only such pixels are skipped.
const SOLID_LOW: u8 = 2;
const SOLID_HIGH: u8 = 253;
/// Decontamination works in tiles of this size (plus [`DECONTAMINATE_REACH`] of margin).
const TILE: u32 = 256;

fn is_edge(a: u8) -> bool {
    a > SOLID_LOW && a < SOLID_HIGH
}

/// Estimates the true foreground colour of semi-transparent pixels and replaces their
/// colour with it, so the old background no longer shows at the edges ("dark fringe").
/// Implements blur-fusion foreground estimation from Forte & Pitié, "Approximate Fast
/// Foreground Colour Estimation" (ICIP 2021): two passes with box blurs of 90 and 6 px.
/// `alpha` is the final mask; (nearly) transparent and opaque pixels keep their colour.
///
/// Only tiles that contain edge pixels are computed, each with enough margin to be
/// exact, in parallel.
pub fn decontaminate(image: &RgbaImage, alpha: &GrayImage) -> RgbaImage {
    use rayon::prelude::*;
    let (w, h) = image.dimensions();
    let mut tiles = Vec::new();
    for ty in (0..h).step_by(TILE as usize) {
        for tx in (0..w).step_by(TILE as usize) {
            let tile = Rect {
                x: tx,
                y: ty,
                width: TILE.min(w - tx),
                height: TILE.min(h - ty),
            };
            let edge = (tile.y..tile.y + tile.height).any(|y| {
                (tile.x..tile.x + tile.width).any(|x| is_edge(alpha.get_pixel(x, y).0[0]))
            });
            if edge {
                tiles.push(tile);
            }
        }
    }
    let results: Vec<(Rect, RgbaImage)> = tiles
        .into_par_iter()
        .map(|tile| {
            let crop = tile.grown(DECONTAMINATE_REACH + 2, w, h);
            let sub = image::imageops::crop_imm(image, crop.x, crop.y, crop.width, crop.height)
                .to_image();
            let sub_a = image::imageops::crop_imm(alpha, crop.x, crop.y, crop.width, crop.height)
                .to_image();
            let done = decontaminate_whole(&sub, &sub_a);
            let inner = image::imageops::crop_imm(
                &done,
                tile.x - crop.x,
                tile.y - crop.y,
                tile.width,
                tile.height,
            )
            .to_image();
            (tile, inner)
        })
        .collect();
    let mut out = image.clone();
    for (tile, inner) in results {
        image::imageops::replace(&mut out, &inner, tile.x as i64, tile.y as i64);
    }
    out
}

/// [`decontaminate`] computed over the whole image at once.
fn decontaminate_whole(image: &RgbaImage, alpha: &GrayImage) -> RgbaImage {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let n = w * h;
    let a: Vec<f32> = alpha.as_raw().iter().map(|&v| v as f32 / 255.0).collect();
    let mut out = image.clone();
    if !alpha.as_raw().iter().any(|&v| is_edge(v)) {
        return out;
    }
    // Blurred alpha per pass, shared by the three colour channels.
    let blurred_alpha = |r: usize| {
        let mut b = a.clone();
        box_blur(&mut b, w, h, r / 2);
        b
    };
    let (ba_large, ba_small) = (blurred_alpha(BLUR_LARGE), blurred_alpha(BLUR_SMALL));
    for c in 0..3 {
        let img: Vec<f32> = image
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[c] as f32 / 255.0)
            .collect();
        let (mut f, mut b) = (img.clone(), img.clone());
        for (r, ba) in [(BLUR_LARGE, &ba_large), (BLUR_SMALL, &ba_small)] {
            let mut fa: Vec<f32> = (0..n).map(|i| f[i] * a[i]).collect();
            let mut b1a: Vec<f32> = (0..n).map(|i| b[i] * (1.0 - a[i])).collect();
            box_blur(&mut fa, w, h, r / 2);
            box_blur(&mut b1a, w, h, r / 2);
            for i in 0..n {
                let bf = fa[i] / (ba[i] + 1e-5);
                let bb = b1a[i] / ((1.0 - ba[i]) + 1e-5);
                f[i] = (bf + a[i] * (img[i] - a[i] * bf - (1.0 - a[i]) * bb)).clamp(0.0, 1.0);
                b[i] = bb;
            }
        }
        for (i, p) in out.as_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if is_edge(alpha.as_raw()[i]) {
                p[c] = (f[i] * 255.0).round() as u8;
            }
        }
    }
    out
}

/// The final mask: the edge mask with all `strokes` applied in order.
pub fn final_mask(edge: &GrayImage, strokes: &[Stroke]) -> GrayImage {
    let mut mask = edge.clone();
    for stroke in strokes {
        apply_stroke(&mut mask, stroke);
    }
    mask
}

/// The cutout: `image` with `mask` as alpha (multiplied with the image's own alpha),
/// colours decontaminated when asked.
pub fn compose(image: &RgbaImage, mask: &GrayImage, decontaminate_colours: bool) -> RgbaImage {
    let mut out = if decontaminate_colours {
        decontaminate(image, mask)
    } else {
        image.clone()
    };
    for (p, m) in out.pixels_mut().zip(mask.pixels()) {
        p.0[3] = ((p.0[3] as u32 * m.0[0] as u32 + 127) / 255) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn disc(w: u32, h: u32, cx: f32, cy: f32, r: f32) -> GrayImage {
        GrayImage::from_fn(w, h, |x, y| {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            Luma([if d <= r { 255 } else { 0 }])
        })
    }

    fn count_on(mask: &GrayImage) -> usize {
        mask.pixels().filter(|p| p.0[0] >= 128).count()
    }

    #[test]
    fn brush_profile() {
        // Hard brush: full inside, 1 px anti-aliased rim, nothing outside.
        assert_eq!(brush_alpha(0.0, 10.0, 0.0), 1.0);
        assert_eq!(brush_alpha(9.0, 10.0, 0.0), 1.0);
        assert!((brush_alpha(9.5, 10.0, 0.0) - 0.5).abs() < 1e-6);
        assert_eq!(brush_alpha(10.0, 10.0, 0.0), 0.0);
        // Fully soft: smoothstep from the centre.
        assert!((brush_alpha(5.0, 10.0, 1.0) - 0.5).abs() < 1e-6);
        assert!(brush_alpha(1.0, 10.0, 1.0) > 0.95);
        // Half soft: hard core to radius 5.
        assert_eq!(brush_alpha(5.0, 10.0, 0.5), 1.0);
        assert!(brush_alpha(7.5, 10.0, 0.5) > 0.4 && brush_alpha(7.5, 10.0, 0.5) < 0.6);
    }

    #[test]
    fn brush_matches_the_shared_fixture() {
        #[derive(Deserialize)]
        struct Case {
            distance: f32,
            radius: f32,
            softness: f32,
            alpha: f32,
        }
        let json = include_str!("../../fixtures/brush-cases.json");
        let cases: Vec<Case> = serde_json::from_str(json).unwrap();
        assert!(cases.len() >= 10);
        for c in cases {
            let got = brush_alpha(c.distance, c.radius, c.softness);
            assert!(
                (got - c.alpha).abs() < 1e-4,
                "d={} r={} s={}: {got} != {}",
                c.distance,
                c.radius,
                c.softness,
                c.alpha
            );
        }
    }

    #[test]
    fn erase_and_restore_follow_the_stroke() {
        let mut mask = GrayImage::from_pixel(100, 60, Luma([255]));
        let stroke = Stroke {
            mode: BrushMode::Erase,
            radius: 8.0,
            softness: 0.0,
            points: vec![[20.0, 30.0], [80.0, 30.0]],
        };
        apply_stroke(&mut mask, &stroke);
        assert_eq!(mask.get_pixel(50, 30).0[0], 0); // on the line
        assert_eq!(mask.get_pixel(50, 45).0[0], 255); // well outside
        assert_eq!(mask.get_pixel(5, 30).0[0], 255); // past the end cap
        assert_eq!(mask.get_pixel(14, 29).0[0], 0); // inside the round end cap
        let erased = mask.clone();
        apply_stroke(
            &mut mask,
            &Stroke {
                mode: BrushMode::Restore,
                ..stroke.clone()
            },
        );
        // The hard core comes back fully; the 1 px rim ends at 255·(1 − a + a²) ≥ 191.
        assert_eq!(mask.get_pixel(50, 30).0[0], 255);
        assert!(
            mask.pixels().all(|p| p.0[0] >= 191),
            "restore brings back what was erased"
        );
        // Overlapping segments of one stroke don't build up: going back and forth equals once.
        let mut twice = GrayImage::from_pixel(100, 60, Luma([255]));
        let soft = Stroke {
            softness: 1.0,
            points: vec![[20.0, 30.0], [80.0, 30.0], [20.0, 30.0]],
            ..stroke.clone()
        };
        apply_stroke(&mut twice, &soft);
        let mut once = GrayImage::from_pixel(100, 60, Luma([255]));
        apply_stroke(
            &mut once,
            &Stroke {
                points: vec![[20.0, 30.0], [80.0, 30.0]],
                ..soft
            },
        );
        assert_eq!(twice, once);
        assert_ne!(erased, mask);
    }

    #[test]
    fn single_point_stroke_is_a_dot() {
        let mut mask = GrayImage::from_pixel(20, 20, Luma([0]));
        apply_stroke(
            &mut mask,
            &Stroke {
                mode: BrushMode::Restore,
                radius: 3.0,
                softness: 0.0,
                points: vec![[10.0, 10.0]],
            },
        );
        assert_eq!(mask.get_pixel(9, 9).0[0], 255);
        assert_eq!(mask.get_pixel(15, 10).0[0], 0);
        // π·3² ≈ 28 px area; pixels at ≥ 50% coverage are those within ~2.5 px.
        assert!((16..=32).contains(&count_on(&mask)), "{}", count_on(&mask));
    }

    #[test]
    fn stroke_bounds_and_scaling() {
        let s = Stroke {
            mode: BrushMode::Erase,
            radius: 5.0,
            softness: 0.5,
            points: vec![[10.0, 10.0], [30.0, 12.0]],
        };
        assert_eq!(
            s.bounds(100, 100),
            Some(Rect {
                x: 4,
                y: 4,
                width: 32,
                height: 14
            })
        );
        assert_eq!(s.bounds(20, 100).unwrap().width, 16);
        let half = s.scaled(0.5);
        assert_eq!(half.radius, 2.5);
        assert_eq!(half.points[1], [15.0, 6.0]);
        let off = Stroke {
            points: vec![[-50.0, -50.0]],
            ..s
        };
        assert_eq!(off.bounds(100, 100), None);
    }

    #[test]
    fn running_extreme_matches_brute_force() {
        let line: Vec<u8> = (0..37u32).map(|i| ((i * 73 + 11) % 256) as u8).collect();
        for r in [1usize, 2, 5, 40] {
            for max in [true, false] {
                let mut out = vec![0; line.len()];
                running_extreme(&line, r, &mut out, max);
                for (i, &got) in out.iter().enumerate() {
                    let lo = i.saturating_sub(r);
                    let hi = (i + r).min(line.len() - 1);
                    let w = &line[lo..=hi];
                    let expect = if max {
                        *w.iter().max().unwrap()
                    } else {
                        *w.iter().min().unwrap()
                    };
                    assert_eq!(got, expect, "r={r} max={max} i={i}");
                }
            }
        }
    }

    #[test]
    fn shift_grows_and_shrinks_by_whole_pixels() {
        let mask = disc(80, 80, 40.0, 40.0, 20.0);
        let grown = shift_mask(&mask, 3.0);
        let shrunk = shift_mask(&mask, -3.0);
        assert_eq!(shift_mask(&mask, 0.0), mask);
        // The edge moves 3 px along the axes.
        assert_eq!(grown.get_pixel(62, 40).0[0], 255);
        assert_eq!(grown.get_pixel(63, 40).0[0], 0);
        assert_eq!(shrunk.get_pixel(56, 40).0[0], 255);
        assert_eq!(shrunk.get_pixel(57, 40).0[0], 0);
        assert!(count_on(&grown) > count_on(&mask) && count_on(&mask) > count_on(&shrunk));
        // Soft values move with the edge instead of being thresholded.
        let soft = GrayImage::from_fn(10, 1, |x, _| Luma([(x * 25) as u8]));
        assert_eq!(shift_mask(&soft, 1.0).as_raw()[..3], [25, 50, 75]);
    }

    #[test]
    fn feather_softens_without_moving_the_edge() {
        let mask = disc(100, 100, 50.0, 50.0, 25.0);
        let soft = feather_mask(&mask, 6.0);
        assert_eq!(feather_mask(&mask, 0.0), mask);
        // The centre and far outside are unchanged; the edge is now a ramp.
        assert_eq!(soft.get_pixel(50, 50).0[0], 255);
        assert_eq!(soft.get_pixel(2, 2).0[0], 0);
        let ramp: Vec<u8> = (70..80).map(|x| soft.get_pixel(x, 50).0[0]).collect();
        assert!(
            ramp.windows(2).all(|p| p[0] >= p[1]),
            "monotonic ramp: {ramp:?}"
        );
        assert!(ramp.iter().filter(|&&v| v > 10 && v < 245).count() >= 4);
        // Roughly the same area (the 50% contour stays near the old edge).
        let (a, b) = (count_on(&mask) as f64, count_on(&soft) as f64);
        assert!((a - b).abs() / a < 0.03, "{a} vs {b}");
    }

    /// A red disc over a blue background, with a soft (mixed) 6 px edge: the edge pixels
    /// are red/blue mixes, i.e. a fringe of the old background.
    fn fringed_scene() -> (RgbaImage, GrayImage) {
        let (w, h) = (120, 120);
        let alpha = feather_mask(&disc(w, h, 60.0, 60.0, 30.0), 6.0);
        let image = RgbaImage::from_fn(w, h, |x, y| {
            let a = alpha.get_pixel(x, y).0[0] as f32 / 255.0;
            let mix = |f: f32, b: f32| (f * a + b * (1.0 - a)).round() as u8;
            Rgba([mix(220.0, 20.0), mix(30.0, 40.0), mix(30.0, 200.0), 255])
        });
        (image, alpha)
    }

    #[test]
    fn decontamination_removes_the_background_colour() {
        let (image, alpha) = fringed_scene();
        let clean = decontaminate(&image, &alpha);
        let (mut before, mut after, mut edge) = (0.0, 0.0, 0);
        for (i, (p, q)) in image.pixels().zip(clean.pixels()).enumerate() {
            let a = alpha.as_raw()[i];
            if a == 255 {
                assert_eq!(p, q, "opaque pixels keep their colour");
            }
            if a > 20 && a < 235 {
                // Blue in a red subject = contamination.
                before += p.0[2] as f64;
                after += q.0[2] as f64;
                edge += 1;
            }
        }
        assert!(edge > 50);
        let (before, after) = (before / edge as f64, after / edge as f64);
        assert!(
            after < before * 0.4,
            "mean blue at the edge: {before:.1} → {after:.1}"
        );
        // Nothing to do without semi-transparent pixels.
        let hard = disc(120, 120, 60.0, 60.0, 30.0);
        assert_eq!(decontaminate(&image, &hard), image);
    }

    #[test]
    fn region_with_margin_matches_the_full_result() {
        let (image, alpha) = fringed_scene();
        let full = decontaminate(&image, &alpha);
        // A region near the edge, cropped with the documented margin.
        let region = Rect {
            x: 80,
            y: 40,
            width: 20,
            height: 30,
        };
        let crop = region.grown(DECONTAMINATE_REACH + 2, 120, 120);
        let sub_img =
            image::imageops::crop_imm(&image, crop.x, crop.y, crop.width, crop.height).to_image();
        let sub_a =
            image::imageops::crop_imm(&alpha, crop.x, crop.y, crop.width, crop.height).to_image();
        let part = decontaminate(&sub_img, &sub_a);
        for y in region.y..region.y + region.height {
            for x in region.x..region.x + region.width {
                let (p, q) = (full.get_pixel(x, y), part.get_pixel(x - crop.x, y - crop.y));
                for c in 0..3 {
                    assert!(p.0[c].abs_diff(q.0[c]) <= 1, "({x},{y}) {p:?} vs {q:?}");
                }
            }
        }
    }

    #[test]
    fn compose_follows_the_order() {
        let base = disc(60, 60, 30.0, 30.0, 15.0);
        let edges = EdgeSettings {
            feather: 0.0,
            shift: 2.0,
            decontaminate: false,
        };
        let edge = edge_mask(&base, &edges);
        let erase_centre = Stroke {
            mode: BrushMode::Erase,
            radius: 4.0,
            softness: 0.0,
            points: vec![[30.0, 30.0]],
        };
        let mask = final_mask(&edge, &[erase_centre]);
        let image = RgbaImage::from_pixel(60, 60, Rgba([10, 20, 30, 255]));
        let out = compose(&image, &mask, false);
        assert_eq!(out.get_pixel(30, 30).0, [10, 20, 30, 0]); // erased last, so it stays erased
        assert_eq!(out.get_pixel(46, 30).0[3], 255); // grown by the shift
        assert_eq!(out.get_pixel(5, 5).0[3], 0);
        assert!(EdgeSettings::default().decontaminate);
        assert_eq!(edges.scaled(0.5).shift, 1.0);
    }

    #[test]
    fn rect_helpers() {
        let r = Rect {
            x: 5,
            y: 5,
            width: 10,
            height: 10,
        };
        assert_eq!(
            r.grown(10, 18, 100),
            Rect {
                x: 0,
                y: 0,
                width: 18,
                height: 25
            }
        );
        assert_eq!(
            r.union(Rect {
                x: 20,
                y: 0,
                width: 2,
                height: 2
            }),
            Rect {
                x: 5,
                y: 0,
                width: 17,
                height: 15
            }
        );
        assert_eq!(
            Rect::full(4, 3),
            Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 3
            }
        );
    }
}
