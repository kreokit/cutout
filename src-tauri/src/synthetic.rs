//! Synthetic test scenes with exact masks, plus mask quality metrics (tests only).
//!
//! Each scene is drawn from shapes whose coverage is known per pixel (4×4 supersampled),
//! so the reference mask is exact, unlike masks of real photos.

use image::{GrayImage, Luma, Rgb, RgbImage};

pub struct Scene {
    pub name: &'static str,
    pub image: RgbImage,
    /// 255 = foreground.
    pub mask: GrayImage,
}

type Inside = dyn Fn(f32, f32) -> bool;

/// Renders `w`×`h`: `background(x, y)` everywhere, `color(x, y)` where `inside`, with
/// 4×4 supersampled edges. Returns the image and the exact coverage mask.
fn render(
    w: u32,
    h: u32,
    background: impl Fn(f32, f32) -> [f32; 3],
    color: impl Fn(f32, f32) -> [f32; 3],
    inside: &Inside,
) -> (RgbImage, GrayImage) {
    let mut image = RgbImage::new(w, h);
    let mut mask = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut hits = 0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let (px, py) = (
                        x as f32 + (sx as f32 + 0.5) / 4.0,
                        y as f32 + (sy as f32 + 0.5) / 4.0,
                    );
                    hits += inside(px, py) as u32;
                }
            }
            let a = hits as f32 / 16.0;
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (fg, bg) = (color(cx, cy), background(cx, cy));
            let mix = |c: usize| (fg[c] * a + bg[c] * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
            image.put_pixel(x, y, Rgb([mix(0), mix(1), mix(2)]));
            mask.put_pixel(x, y, Luma([(a * 255.0).round() as u8]));
        }
    }
    (image, mask)
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> impl Fn(f32, f32) -> bool {
    move |x, y| ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
}

fn rounded_rect(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> impl Fn(f32, f32) -> bool {
    move |x, y| {
        let dx = (x0 + r - x).max(x - (x1 - r)).max(0.0);
        let dy = (y0 + r - y).max(y - (y1 - r)).max(0.0);
        x >= x0 && x <= x1 && y >= y0 && y <= y1 && dx * dx + dy * dy <= r * r
    }
}

/// A little deterministic noise (no RNG dependency).
fn noise(x: f32, y: f32) -> f32 {
    let v = ((x * 12.9898 + y * 78.233).sin() * 43_758.547).fract();
    v * 2.0 - 1.0
}

pub fn scenes() -> Vec<Scene> {
    let mut out = Vec::new();
    let mut add = |name, (image, mask)| out.push(Scene { name, image, mask });

    // A red ball on a plain light background: the easy case.
    add(
        "ball",
        render(
            640,
            480,
            |_, _| [232.0, 232.0, 228.0],
            |x, y| {
                let shade = 1.0 - ((x - 290.0).hypot(y - 200.0) / 260.0).min(1.0) * 0.45;
                [210.0 * shade, 40.0 * shade, 35.0 * shade]
            },
            &ellipse(320.0, 240.0, 150.0, 150.0),
        ),
    );

    // A dark product box on a soft gradient "table".
    add(
        "product",
        render(
            800,
            600,
            |_, y| {
                let t = y / 600.0;
                [245.0 - 40.0 * t, 245.0 - 30.0 * t, 250.0 - 20.0 * t]
            },
            |x, _| {
                let t = (x - 260.0) / 280.0;
                [40.0 + 30.0 * t, 60.0 + 30.0 * t, 90.0 + 20.0 * t]
            },
            &rounded_rect(260.0, 170.0, 540.0, 470.0, 24.0),
        ),
    );

    // A simple figure (head and body) on a pale green wall.
    let head = ellipse(240.0, 210.0, 70.0, 85.0);
    let body = rounded_rect(130.0, 300.0, 350.0, 720.0, 60.0);
    add(
        "figure",
        render(
            480,
            720,
            |x, y| [200.0 + 8.0 * noise(x, y), 222.0 + 8.0 * noise(y, x), 196.0],
            |_, y| {
                if y < 300.0 {
                    [222.0, 180.0, 150.0]
                } else {
                    [50.0, 90.0, 160.0]
                }
            },
            &move |x, y| head(x, y) || body(x, y),
        ),
    );

    // A flat logo: a ring (with a hole) and a bar, on white.
    let outer = ellipse(300.0, 260.0, 160.0, 160.0);
    let inner = ellipse(300.0, 260.0, 85.0, 85.0);
    let bar = rounded_rect(140.0, 470.0, 460.0, 530.0, 12.0);
    add(
        "logo",
        render(
            600,
            600,
            |_, _| [255.0, 255.0, 255.0],
            |_, y| {
                if y < 450.0 {
                    [20.0, 120.0, 220.0]
                } else {
                    [240.0, 120.0, 20.0]
                }
            },
            &move |x, y| (outer(x, y) && !inner(x, y)) || bar(x, y),
        ),
    );

    // Low contrast: a grey stone on a slightly lighter, noisy grey ground.
    add(
        "low-contrast",
        render(
            640,
            480,
            |x, y| {
                let v = 150.0 + 10.0 * noise(x, y);
                [v, v, v + 4.0]
            },
            |x, y| {
                let v = 118.0 + 6.0 * noise(y, x);
                [v + 4.0, v, v]
            },
            &ellipse(330.0, 250.0, 170.0, 120.0),
        ),
    );
    out
}

/// Intersection over union of the foregrounds (alpha ≥ 128) of two masks.
pub fn iou(predicted: &GrayImage, reference: &GrayImage) -> f64 {
    let (mut both, mut either) = (0u64, 0u64);
    for (p, r) in predicted.pixels().zip(reference.pixels()) {
        let (p, r) = (p.0[0] >= 128, r.0[0] >= 128);
        both += (p && r) as u64;
        either += (p || r) as u64;
    }
    if either == 0 {
        1.0
    } else {
        both as f64 / either as f64
    }
}

/// Mean absolute difference of two masks, 0–1.
pub fn mae(predicted: &GrayImage, reference: &GrayImage) -> f64 {
    let sum: u64 = predicted
        .pixels()
        .zip(reference.pixels())
        .map(|(p, r)| p.0[0].abs_diff(r.0[0]) as u64)
        .sum();
    sum as f64 / (predicted.len() as f64 * 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_match_the_drawing() {
        for scene in scenes() {
            assert_eq!(
                scene.image.dimensions(),
                scene.mask.dimensions(),
                "{}",
                scene.name
            );
            let fg = scene.mask.pixels().filter(|p| p.0[0] >= 128).count();
            let share = fg as f64 / scene.mask.len() as f64;
            assert!(
                (0.05..0.6).contains(&share),
                "{}: foreground share {share}",
                scene.name
            );
        }
        let ball = &scenes()[0];
        assert_eq!(ball.mask.get_pixel(320, 240).0[0], 255);
        assert_eq!(ball.mask.get_pixel(5, 5).0[0], 0);
        assert_eq!(ball.image.get_pixel(5, 5).0, [232, 232, 228]);
        // The logo's ring has a real hole.
        assert_eq!(scenes()[3].mask.get_pixel(300, 260).0[0], 0);
    }

    #[test]
    fn metrics() {
        let a = GrayImage::from_fn(4, 1, |x, _| Luma([if x < 2 { 255 } else { 0 }]));
        let b = GrayImage::from_fn(4, 1, |x, _| Luma([if x < 3 { 255 } else { 0 }]));
        assert!((iou(&a, &b) - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(iou(&a, &a), 1.0);
        assert!((mae(&a, &b) - 0.25).abs() < 1e-9);
        assert_eq!(mae(&a, &a), 0.0);
    }
}
