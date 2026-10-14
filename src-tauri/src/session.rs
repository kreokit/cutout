//! The refine session of the image being edited: keeps the decoded original, the AI mask
//! and every intermediate result, so an edit only recomputes what it changes.
//!
//! The editor works on a *working copy* of at most [`MAX_WORKING_PIXELS`] (WebKit's
//! canvas limit); larger images are scaled down for editing only. Strokes and edge
//! settings are always given in pixels of the original image.
//!
//! Renders are sent to the webview as raw RGBA after a 24-byte header (six little-endian
//! u32: working width, working height, x, y, width, height of the pixels that follow).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use image::imageops::{self, FilterType};
use image::{GrayImage, RgbaImage};
use serde::Deserialize;

use crate::images;
use crate::refine::{self, DECONTAMINATE_REACH, EdgeSettings, Rect, Stroke};

/// The largest working copy, in pixels (WebKit's canvas limit is 16 777 216).
pub const MAX_WORKING_PIXELS: u64 = 16_000_000;
/// Slider previews are rendered at most this big.
pub const PREVIEW_PIXELS: u64 = 1_000_000;

/// Everything the user changed on one image.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Edits {
    pub edges: EdgeSettings,
    pub strokes: Vec<Stroke>,
}

/// The size of a copy of `width`×`height` with at most `max_pixels` (the same size if
/// it already fits; rounded down, so the limit always holds).
pub fn scaled_size(width: u32, height: u32, max_pixels: u64) -> (u32, u32) {
    let pixels = width as u64 * height as u64;
    if pixels <= max_pixels {
        return (width, height);
    }
    let scale = (max_pixels as f64 / pixels as f64).sqrt();
    let w = ((width as f64 * scale).floor() as u32).max(1);
    let h = ((height as f64 * scale).floor() as u32).max(1);
    (w, h)
}

/// One size of the image (working or preview) with its cached results.
struct Layer {
    scale: f32,
    image: RgbaImage,
    base: GrayImage,
    /// Edge mask for (feather, shift).
    edge: Option<((f32, f32), GrayImage)>,
    /// Final mask for (feather, shift) and the strokes applied to it.
    mask: Option<((f32, f32), Vec<Stroke>, GrayImage)>,
    /// Composed output for the edge settings and strokes.
    out: Option<(EdgeSettings, Vec<Stroke>, RgbaImage)>,
}

impl Layer {
    /// A copy of `image` and `base` with at most `max_pixels`.
    fn new(image: &RgbaImage, base: &GrayImage, max_pixels: u64) -> Layer {
        let (w, h) = scaled_size(image.width(), image.height(), max_pixels);
        let scale = w as f32 / image.width() as f32;
        let (image, base) = if (w, h) != image.dimensions() {
            (
                imageops::resize(image, w, h, FilterType::Triangle),
                imageops::resize(base, w, h, FilterType::Triangle),
            )
        } else {
            (image.clone(), base.clone())
        };
        Layer {
            scale,
            image,
            base,
            edge: None,
            mask: None,
            out: None,
        }
    }

    fn size(&self) -> (u32, u32) {
        self.image.dimensions()
    }

    /// Renders `edits` and returns the rectangle that changed since the last render
    /// (everything the first time, or when `full`) with its pixels.
    fn render(&mut self, edits: &Edits, full: bool) -> (Rect, Vec<u8>) {
        let (w, h) = self.size();
        let edges = edits.edges.scaled(self.scale);
        let strokes: Vec<Stroke> = edits.strokes.iter().map(|s| s.scaled(self.scale)).collect();
        let edge_key = (edges.feather, edges.shift);

        if self.edge.as_ref().is_none_or(|(k, _)| *k != edge_key) {
            self.edge = Some((edge_key, refine::edge_mask(&self.base, &edges)));
            self.mask = None;
            self.out = None;
        }
        let edge = &self.edge.as_ref().unwrap().1;

        // Strokes in common with the last render; the rest changed.
        let previous: &[Stroke] = self.mask.as_ref().map_or(&[], |(_, s, _)| s.as_slice());
        let common = previous
            .iter()
            .zip(&strokes)
            .take_while(|(a, b)| a == b)
            .count();
        let mut changed: Option<Rect> = None;
        for s in previous[common..].iter().chain(&strokes[common..]) {
            if let Some(b) = s.bounds(w, h) {
                changed = Some(changed.map_or(b, |c| c.union(b)));
            }
        }
        match &mut self.mask {
            // Only new strokes: paint them on.
            Some((_, prev, mask)) if common == prev.len() => {
                for s in &strokes[common..] {
                    refine::apply_stroke(mask, s);
                }
                *prev = strokes.clone();
            }
            // Strokes were undone or changed: replay them all.
            _ => {
                self.mask = Some((
                    edge_key,
                    strokes.clone(),
                    refine::final_mask(edge, &strokes),
                ))
            }
        }
        let mask = &self.mask.as_ref().unwrap().2;

        let out_valid = self.out.as_ref().is_some_and(|(e, _, _)| *e == edges);
        let region = match (full, out_valid, changed) {
            (false, true, Some(r)) => r,
            (false, true, None) => Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            _ => Rect::full(w, h),
        };
        if !out_valid || region == Rect::full(w, h) {
            self.out = Some((
                edges,
                strokes,
                refine::compose(&self.image, mask, edges.decontaminate),
            ));
        } else if region.width > 0 {
            // Recompute the changed region, with enough margin for decontamination.
            let crop = region.grown(DECONTAMINATE_REACH + 2, w, h);
            let image =
                imageops::crop_imm(&self.image, crop.x, crop.y, crop.width, crop.height).to_image();
            let sub_mask =
                imageops::crop_imm(mask, crop.x, crop.y, crop.width, crop.height).to_image();
            let part = refine::compose(&image, &sub_mask, edges.decontaminate);
            let inner = imageops::crop_imm(
                &part,
                region.x - crop.x,
                region.y - crop.y,
                region.width,
                region.height,
            )
            .to_image();
            let (_, prev_strokes, out) = self.out.as_mut().unwrap();
            imageops::replace(out, &inner, region.x as i64, region.y as i64);
            *prev_strokes = strokes;
        }
        let out = &self.out.as_ref().unwrap().2;
        let pixels = imageops::crop_imm(out, region.x, region.y, region.width, region.height)
            .to_image()
            .into_raw();
        (region, pixels)
    }
}

struct Session {
    key: (PathBuf, PathBuf),
    image: RgbaImage,
    base: GrayImage,
    working: Layer,
    preview: Option<Layer>,
}

/// The refine session of the image being edited (one at a time).
#[derive(Default)]
pub struct Refiner(Mutex<Option<Session>>);

fn header(layer_size: (u32, u32), region: Rect, pixels: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(24 + pixels.len());
    for v in [
        layer_size.0,
        layer_size.1,
        region.x,
        region.y,
        region.width,
        region.height,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&pixels);
    out
}

impl Refiner {
    /// Renders `edits` for `image_path` with its AI mask at `mask_path`. With `preview`,
    /// renders the whole image small (for live slider changes); otherwise renders the
    /// working copy and returns only what changed since the last render (all of it with
    /// `full`). The result has the header described in the module docs.
    pub fn render(
        &self,
        image_path: &Path,
        mask_path: &Path,
        edits: &Edits,
        preview: bool,
        full: bool,
    ) -> Result<Vec<u8>, String> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let key = (image_path.to_path_buf(), mask_path.to_path_buf());
        if slot.as_ref().is_none_or(|s| s.key != key) {
            *slot = None; // free the previous image first
            let image = images::decode(image_path)?.to_rgba8();
            let mut base = image::open(mask_path)
                .map_err(|e| format!("cannot read the cutout mask: {e}"))?
                .to_luma8();
            if base.dimensions() != image.dimensions() {
                base = imageops::resize(&base, image.width(), image.height(), FilterType::Triangle);
            }
            let working = Layer::new(&image, &base, MAX_WORKING_PIXELS);
            *slot = Some(Session {
                key,
                image,
                base,
                working,
                preview: None,
            });
        }
        let Session {
            image,
            base,
            working,
            preview: preview_layer,
            ..
        } = slot.as_mut().unwrap();
        let start = Instant::now();
        let (layer, full) = if preview {
            (
                preview_layer.get_or_insert_with(|| Layer::new(image, base, PREVIEW_PIXELS)),
                true,
            )
        } else {
            (working, full)
        };
        let (region, pixels) = layer.render(edits, full);
        let size = layer.size();
        if !preview {
            eprintln!(
                "[refine] {}×{} region {}×{}: {:.2}s",
                size.0,
                size.1,
                region.width,
                region.height,
                start.elapsed().as_secs_f32()
            );
        }
        Ok(header(size, region, pixels))
    }

    /// Forgets the session (e.g. when going back to Home), freeing its memory.
    pub fn clear(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refine::BrushMode;
    use image::{Luma, Rgba};
    use std::fs;

    fn scene(dir: &Path, w: u32, h: u32) -> (PathBuf, PathBuf) {
        fs::create_dir_all(dir).unwrap();
        let image = RgbaImage::from_fn(w, h, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
        });
        let (cx, cy, r) = (w as f32 / 2.0, h as f32 / 2.0, w.min(h) as f32 / 3.0);
        let mask = GrayImage::from_fn(w, h, |x, y| {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            Luma([((r - d + 0.5).clamp(0.0, 1.0) * 255.0) as u8])
        });
        let (ip, mp) = (dir.join("image.png"), dir.join("mask.png"));
        image.save(&ip).unwrap();
        mask.save(&mp).unwrap();
        (ip, mp)
    }

    fn parse(bytes: &[u8]) -> ([u32; 6], Vec<u8>) {
        let mut h = [0u32; 6];
        for (i, v) in h.iter_mut().enumerate() {
            *v = u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        }
        (h, bytes[24..].to_vec())
    }

    /// The expected full output, computed directly with the refine functions.
    fn expected(ip: &Path, mp: &Path, edits: &Edits) -> RgbaImage {
        let image = image::open(ip).unwrap().to_rgba8();
        let base = image::open(mp).unwrap().to_luma8();
        let mask = refine::final_mask(&refine::edge_mask(&base, &edits.edges), &edits.strokes);
        refine::compose(&image, &mask, edits.edges.decontaminate)
    }

    fn stroke(mode: BrushMode, points: Vec<[f32; 2]>) -> Stroke {
        Stroke {
            mode,
            radius: 6.0,
            softness: 0.5,
            points,
        }
    }

    #[test]
    fn scales_only_huge_images() {
        assert_eq!(scaled_size(4032, 3024, MAX_WORKING_PIXELS), (4032, 3024));
        // A 48-megapixel photo: about 0.57 of each side, never over the limit.
        let (w, h) = scaled_size(8064, 6048, MAX_WORKING_PIXELS);
        assert!(w as u64 * h as u64 <= MAX_WORKING_PIXELS);
        assert_eq!((w, h), (4618, 3464));
        assert_eq!(scaled_size(1600, 1200, PREVIEW_PIXELS), (1154, 866));
    }

    #[test]
    fn incremental_renders_match_a_full_render() {
        let dir = std::env::temp_dir().join("kreokit-cutout-session");
        let (ip, mp) = scene(&dir, 160, 120);
        let refiner = Refiner::default();
        let mut edits = Edits {
            edges: EdgeSettings {
                feather: 2.0,
                shift: -1.0,
                decontaminate: true,
            },
            strokes: vec![],
        };
        let (h, px) = parse(&refiner.render(&ip, &mp, &edits, false, false).unwrap());
        assert_eq!(h, [160, 120, 0, 0, 160, 120]);
        assert_eq!(px, *expected(&ip, &mp, &edits).as_raw());

        // A new stroke: only its bounds come back, and they match a full render.
        edits
            .strokes
            .push(stroke(BrushMode::Erase, vec![[70.0, 60.0], [90.0, 62.0]]));
        let (h, px) = parse(&refiner.render(&ip, &mp, &edits, false, false).unwrap());
        let bounds = edits.strokes[0].bounds(160, 120).unwrap();
        assert_eq!(h[2..], [bounds.x, bounds.y, bounds.width, bounds.height]);
        let full = expected(&ip, &mp, &edits);
        let want =
            imageops::crop_imm(&full, bounds.x, bounds.y, bounds.width, bounds.height).to_image();
        assert_eq!(px, *want.as_raw());

        // Undo: the removed stroke's bounds are recomputed.
        edits.strokes.pop();
        let (h, px) = parse(&refiner.render(&ip, &mp, &edits, false, false).unwrap());
        assert_eq!(h[2..], [bounds.x, bounds.y, bounds.width, bounds.height]);
        let full = expected(&ip, &mp, &edits);
        let want =
            imageops::crop_imm(&full, bounds.x, bounds.y, bounds.width, bounds.height).to_image();
        assert_eq!(px, *want.as_raw());

        // Nothing changed: an empty region. Edge settings changed: everything.
        let (h, px) = parse(&refiner.render(&ip, &mp, &edits, false, false).unwrap());
        assert_eq!((h[4], h[5], px.len()), (0, 0, 0));
        edits.edges.feather = 0.0;
        let (h, _) = parse(&refiner.render(&ip, &mp, &edits, false, false).unwrap());
        assert_eq!(h[4..], [160, 120]);
        // `full` always sends everything.
        let (h, px) = parse(&refiner.render(&ip, &mp, &edits, false, true).unwrap());
        assert_eq!(h[4..], [160, 120]);
        assert_eq!(px, *expected(&ip, &mp, &edits).as_raw());
    }

    #[test]
    fn previews_are_small_and_whole() {
        let dir = std::env::temp_dir().join("kreokit-cutout-session-preview");
        let (ip, mp) = scene(&dir, 1600, 1200);
        let refiner = Refiner::default();
        let edits = Edits {
            edges: EdgeSettings::default(),
            strokes: vec![stroke(BrushMode::Restore, vec![[10.0, 10.0]])],
        };
        let (h, px) = parse(&refiner.render(&ip, &mp, &edits, true, false).unwrap());
        assert!(h[0] as u64 * h[1] as u64 <= PREVIEW_PIXELS);
        assert_eq!(h[2..4], [0, 0]);
        assert_eq!(h[4..], h[..2]);
        assert_eq!(px.len(), (h[0] * h[1] * 4) as usize);
        // Then the working copy is still full size.
        let (h, _) = parse(&refiner.render(&ip, &mp, &edits, false, false).unwrap());
        assert_eq!(h[..2], [1600, 1200]);
    }

    /// Times edits on a 12-megapixel image: `cargo test --release refine_timings --
    /// --ignored --nocapture`.
    #[test]
    #[ignore]
    fn refine_timings() {
        let dir = std::env::temp_dir().join("kreokit-cutout-session-12mp");
        let (ip, mp) = scene(&dir, 4032, 3024);
        let refiner = Refiner::default();
        let mut edits = Edits {
            edges: EdgeSettings::default(),
            strokes: vec![],
        };
        let time = |label: &str, edits: &Edits, preview: bool| {
            let t = Instant::now();
            let bytes = refiner.render(&ip, &mp, edits, preview, false).unwrap();
            println!(
                "{label:<34} {:.2}s ({} MB)",
                t.elapsed().as_secs_f32(),
                bytes.len() / 1_000_000
            );
        };
        time("first render (load + decontaminate)", &edits, false);
        edits.strokes.push(Stroke {
            mode: BrushMode::Erase,
            radius: 60.0,
            softness: 0.3,
            points: vec![[1000.0, 1000.0], [1600.0, 1200.0]],
        });
        time("brush stroke (region)", &edits, false);
        edits.edges.feather = 6.0;
        time("slider preview, first (1 MP)", &edits, true);
        edits.edges.feather = 7.0;
        time("slider preview, next frame", &edits, true);
        time("feather 6 px (full)", &edits, false);
        edits.edges.shift = -3.0;
        time("shift -3 px (full)", &edits, false);
        edits.edges.decontaminate = false;
        time("fringe removal off (full)", &edits, false);
    }
}
