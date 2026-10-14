//! Where the cutout goes in the output frame: the original size, or a social preset
//! with **Fit** (the whole photo inside the frame) or **Fill** (the frame covered, the
//! crop following the subject, which is never cut off).
//!
//! The background is replaced, so the photo's own edges never show; only the subject
//! must stay whole. Where the subject touches an edge of the photo (a portrait cut off
//! at the chest), Fill keeps it on that edge of the frame instead of letting it float.

use image::GrayImage;
use serde::Deserialize;

use crate::refine::Rect;

/// The output size: `None` is the original's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// The largest output side we accept.
pub const MAX_SIDE: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    #[default]
    Fit,
    Fill,
}

/// Fill keeps this share of the frame's short side free around the subject (except on
/// sides where the subject touches the photo's edge).
pub const FILL_MARGIN: f32 = 0.04;
/// Mask values at least this high count as subject for its bounds.
const SUBJECT_FROM: u8 = 32;

/// The photo drawn into the frame: scaled by `scale`, its top-left corner at (`x`, `y`)
/// in frame pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
}

/// The frame for an original of `width`×`height` and the chosen `size`.
pub fn frame_size(width: u32, height: u32, size: Option<Size>) -> Result<(u32, u32), String> {
    match size {
        None => Ok((width, height)),
        Some(s) if (1..=MAX_SIDE).contains(&s.width) && (1..=MAX_SIDE).contains(&s.height) => {
            Ok((s.width, s.height))
        }
        Some(s) => Err(format!("unsupported size {}×{}", s.width, s.height)),
    }
}

/// The bounds of the subject in `mask` (pixels of at least [`SUBJECT_FROM`]), or `None`
/// if there is no subject.
pub fn subject_bounds(mask: &GrayImage) -> Option<Rect> {
    let (w, h) = mask.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (y, row) in mask.as_raw().chunks_exact(w as usize).enumerate() {
        let Some(first) = row.iter().position(|&v| v >= SUBJECT_FROM) else {
            continue;
        };
        let last = row.iter().rposition(|&v| v >= SUBJECT_FROM).unwrap();
        x0 = x0.min(first as u32);
        x1 = x1.max(last as u32 + 1);
        y0 = y0.min(y as u32);
        y1 = y as u32 + 1;
    }
    (x1 > x0).then(|| Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// One axis of Fill: the subject spans `a`..`b` of a photo `n` long, in a frame `len`
/// long with `margin`. Returns the largest scale that keeps the subject whole.
fn axis_limit(len: f32, n: f32, a: f32, b: f32, margin: f32) -> f32 {
    let (ml, mr) = margins(n, a, b, margin);
    (len - ml - mr).max(1.0) / (b - a).max(1.0)
}

/// Margins before and after the subject: none on a side where it touches the photo.
fn margins(n: f32, a: f32, b: f32, margin: f32) -> (f32, f32) {
    (
        if a <= 1.0 { 0.0 } else { margin },
        if b >= n - 1.0 { 0.0 } else { margin },
    )
}

/// One axis of Fill: the offset of the photo (scaled by `s`) in the frame.
fn axis_offset(len: f32, n: f32, a: f32, b: f32, margin: f32, s: f32) -> f32 {
    let (ml, mr) = margins(n, a, b, margin);
    let (touch_start, touch_end) = (a <= 1.0, b >= n - 1.0);
    // Centre on the subject, or keep it on the edge it touches.
    let mut x = match (touch_start, touch_end) {
        (true, false) => 0.0,
        (false, true) => len - s * n,
        _ => len / 2.0 - s * (a + b) / 2.0,
    };
    // Cover the frame with the photo where it's big enough.
    if s * n >= len {
        x = x.clamp(len - s * n, 0.0);
    }
    // Never cut the subject (this range is never empty at the scale from `axis_limit`).
    let (lo, hi) = (ml - s * a, len - mr - s * b);
    if lo <= hi {
        x.clamp(lo, hi)
    } else {
        (lo + hi) / 2.0
    }
}

/// Places a `width`×`height` photo whose subject is `subject` in a `frame`.
pub fn place(
    width: u32,
    height: u32,
    frame: (u32, u32),
    fit: Fit,
    subject: Option<Rect>,
) -> Placement {
    let (w, h) = (width as f32, height as f32);
    let (fw, fh) = (frame.0 as f32, frame.1 as f32);
    let contain = (fw / w).min(fh / h);
    let cover = (fw / w).max(fh / h);
    let centred = |scale: f32| Placement {
        scale,
        x: (fw - w * scale) / 2.0,
        y: (fh - h * scale) / 2.0,
    };
    if frame == (width, height) && fit == Fit::Fit {
        return Placement {
            scale: 1.0,
            x: 0.0,
            y: 0.0,
        };
    }
    let (Fit::Fill, Some(r)) = (fit, subject) else {
        return centred(if fit == Fit::Fit { contain } else { cover });
    };
    let margin = FILL_MARGIN * fw.min(fh);
    let (ax, bx) = (r.x as f32, (r.x + r.width) as f32);
    let (ay, by) = (r.y as f32, (r.y + r.height) as f32);
    let scale = cover
        .min(axis_limit(fw, w, ax, bx, margin))
        .min(axis_limit(fh, h, ay, by, margin));
    Placement {
        scale,
        x: axis_offset(fw, w, ax, bx, margin, scale),
        y: axis_offset(fh, h, ay, by, margin, scale),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    const PRESETS: [(u32, u32); 4] = [(1080, 1080), (1080, 1350), (1080, 1920), (1280, 720)];

    fn rect(x: u32, y: u32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// The subject's rectangle in frame pixels.
    fn in_frame(p: Placement, r: Rect) -> (f32, f32, f32, f32) {
        (
            p.x + p.scale * r.x as f32,
            p.y + p.scale * r.y as f32,
            p.x + p.scale * (r.x + r.width) as f32,
            p.y + p.scale * (r.y + r.height) as f32,
        )
    }

    #[test]
    fn frame_sizes() {
        assert_eq!(frame_size(4032, 3024, None), Ok((4032, 3024)));
        for (w, h) in PRESETS {
            let size = Size {
                width: w,
                height: h,
            };
            assert_eq!(frame_size(4032, 3024, Some(size)), Ok((w, h)));
        }
        let bad = Size {
            width: 0,
            height: 10,
        };
        assert!(frame_size(10, 10, Some(bad)).is_err());
    }

    #[test]
    fn finds_the_subject() {
        let mut mask = GrayImage::new(100, 80);
        assert_eq!(subject_bounds(&mask), None);
        for (x, y) in [(10, 20), (60, 30), (35, 70)] {
            mask.put_pixel(x, y, Luma([255]));
        }
        mask.put_pixel(90, 5, Luma([SUBJECT_FROM - 1])); // faint noise doesn't count
        assert_eq!(subject_bounds(&mask), Some(rect(10, 20, 51, 51)));
    }

    #[test]
    fn original_size_is_unchanged() {
        let p = place(4032, 3024, (4032, 3024), Fit::Fit, None);
        assert_eq!(
            p,
            Placement {
                scale: 1.0,
                x: 0.0,
                y: 0.0
            }
        );
    }

    #[test]
    fn fit_shows_the_whole_photo() {
        for (fw, fh) in PRESETS {
            let p = place(4032, 3024, (fw, fh), Fit::Fit, Some(rect(100, 100, 50, 50)));
            let (x1, y1) = (p.x + 4032.0 * p.scale, p.y + 3024.0 * p.scale);
            assert!(p.x >= -0.01 && p.y >= -0.01, "{p:?}");
            assert!(x1 <= fw as f32 + 0.01 && y1 <= fh as f32 + 0.01, "{p:?}");
            // And it touches two sides.
            assert!((x1 - p.x - fw as f32).abs() < 0.01 || (y1 - p.y - fh as f32).abs() < 0.01);
        }
    }

    #[test]
    fn fill_covers_the_frame_around_a_small_subject() {
        // A small subject off-centre in a landscape photo, into a story frame.
        let subject = rect(2600, 1200, 400, 900);
        let p = place(4032, 3024, (1080, 1920), Fit::Fill, Some(subject));
        assert!(
            (p.scale - 1920.0 / 3024.0).abs() < 1e-4,
            "cover scale, {p:?}"
        );
        assert!(p.x <= 0.0 && p.x + 4032.0 * p.scale >= 1080.0);
        let (x0, _, x1, _) = in_frame(p, subject);
        // Centred on the subject.
        assert!(((x0 + x1) / 2.0 - 540.0).abs() < 1.0, "{x0} {x1}");
    }

    #[test]
    fn fill_never_cuts_the_subject() {
        let photos = [(4032, 3024), (3024, 4032), (1920, 1080), (1000, 1000)];
        for (w, h) in photos {
            for (fw, fh) in PRESETS {
                // Wide, tall, corner and full-height-ish subjects, none touching an edge.
                let subjects = [
                    rect(w / 10, h / 3, w * 8 / 10, h / 3),
                    rect(w / 3, h / 20, w / 3, h * 18 / 20),
                    rect(5, 5, w / 4, h / 4),
                    rect(w / 2, h / 2, w / 2 - 3, h / 2 - 3),
                ];
                for s in subjects {
                    let p = place(w, h, (fw, fh), Fit::Fill, Some(s));
                    let (x0, y0, x1, y1) = in_frame(p, s);
                    let m = FILL_MARGIN * fw.min(fh) as f32 - 0.01;
                    assert!(
                        x0 >= m && y0 >= m && x1 <= fw as f32 - m && y1 <= fh as f32 - m,
                        "{w}×{h} → {fw}×{fh}, subject {s:?}: {p:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn fill_keeps_a_cut_off_portrait_on_the_bottom_edge() {
        // Head and shoulders, cut off by the bottom of a landscape photo.
        let subject = rect(1500, 800, 1000, 3024 - 800);
        for (fw, fh) in PRESETS {
            let p = place(4032, 3024, (fw, fh), Fit::Fill, Some(subject));
            let (x0, y0, x1, y1) = in_frame(p, subject);
            assert!((y1 - fh as f32).abs() < 0.01, "{fw}×{fh}: bottom at {y1}");
            let m = FILL_MARGIN * fw.min(fh) as f32 - 0.01;
            assert!(
                x0 >= m && x1 <= fw as f32 - m && y0 >= m,
                "{fw}×{fh}: {p:?}"
            );
        }
    }

    #[test]
    fn fill_without_a_subject_covers_centred() {
        let p = place(2000, 1000, (1080, 1080), Fit::Fill, None);
        assert!((p.scale - 1.08).abs() < 1e-4);
        assert!((p.x - (1080.0 - 2160.0) / 2.0).abs() < 1e-3 && p.y.abs() < 1e-3);
    }
}
