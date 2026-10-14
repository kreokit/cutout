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
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use image::imageops::{self, FilterType};
use image::{GrayImage, RgbaImage};
use serde::Deserialize;

use crate::background::{self, Background};
use crate::encode;
use crate::export::{self, ExportSettings, Sources};
use crate::images;
use crate::refine::{self, DECONTAMINATE_REACH, EdgeSettings, Rect, Stroke};

/// The largest working copy, in pixels (WebKit's canvas limit is 16 777 216).
pub const MAX_WORKING_PIXELS: u64 = 16_000_000;
/// Slider previews are rendered at most this big.
pub const PREVIEW_PIXELS: u64 = 1_000_000;
/// The Export dialog's preview is at most this big.
pub const EXPORT_PREVIEW_PIXELS: u64 = 400_000;
/// An image background is sent to the stage at most this big.
pub const STAGE_BACKGROUND_PIXELS: u64 = 4_000_000;

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

    /// The cached output and final mask, if they are for `edits`.
    fn cached(&self, edits: &Edits) -> Option<(&RgbaImage, &GrayImage)> {
        let edges = edits.edges.scaled(self.scale);
        let strokes: Vec<Stroke> = edits.strokes.iter().map(|s| s.scaled(self.scale)).collect();
        let (out_edges, out_strokes, out) = self.out.as_ref()?;
        let (mask_key, mask_strokes, mask) = self.mask.as_ref()?;
        (*out_edges == edges
            && *out_strokes == strokes
            && *mask_key == (edges.feather, edges.shift)
            && *mask_strokes == strokes)
            .then_some((out, mask))
    }

    /// This layer's image with its output and final mask for `edits` (rendering them if
    /// needed).
    fn results(&mut self, edits: &Edits) -> (&RgbaImage, &RgbaImage, &GrayImage) {
        if self.cached(edits).is_none() {
            self.render(edits, true);
        }
        let (out, mask) = self.cached(edits).expect("just rendered");
        (&self.image, out, mask)
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
    /// The original's ICC colour profile, kept in exports.
    profile: Option<Vec<u8>>,
    base: GrayImage,
    working: Layer,
    preview: Option<Layer>,
    /// The last full-size export render (when the working copy is smaller): its edits,
    /// cutout and final mask, so exporting again with other settings is quick.
    full: Option<(Edits, RgbaImage, GrayImage)>,
}

impl Session {
    fn preview(&mut self) -> &mut Layer {
        let (image, base) = (&self.image, &self.base);
        self.preview
            .get_or_insert_with(|| Layer::new(image, base, PREVIEW_PIXELS))
    }
}

/// A decoded image background: its path, modification time and pixels.
type CustomImage = (PathBuf, Option<SystemTime>, Arc<RgbaImage>);

/// The refine session of the image being edited (one at a time), and the decoded image
/// of an image background.
pub struct Refiner {
    session: Mutex<Option<Session>>,
    custom: Mutex<Option<CustomImage>>,
    /// The largest working copy ([`MAX_WORKING_PIXELS`]; smaller in tests).
    working_limit: u64,
}

impl Default for Refiner {
    fn default() -> Refiner {
        Refiner::with_working_limit(MAX_WORKING_PIXELS)
    }
}

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

/// Makes `slot` hold the session for `image_path` and `mask_path`, loading it if needed.
fn load(
    slot: &mut Option<Session>,
    image_path: &Path,
    mask_path: &Path,
    working_limit: u64,
) -> Result<(), String> {
    let key = (image_path.to_path_buf(), mask_path.to_path_buf());
    if slot.as_ref().is_some_and(|s| s.key == key) {
        return Ok(());
    }
    *slot = None; // free the previous image first
    let (image, profile) = images::decode_with_profile(image_path)?;
    let image = image.to_rgba8();
    let mut base = image::open(mask_path)
        .map_err(|e| format!("cannot read the cutout mask: {e}"))?
        .to_luma8();
    if base.dimensions() != image.dimensions() {
        base = imageops::resize(&base, image.width(), image.height(), FilterType::Triangle);
    }
    let working = Layer::new(&image, &base, working_limit);
    *slot = Some(Session {
        key,
        image,
        profile,
        base,
        working,
        preview: None,
        full: None,
    });
    Ok(())
}

impl Refiner {
    pub fn with_working_limit(working_limit: u64) -> Refiner {
        Refiner {
            session: Mutex::new(None),
            custom: Mutex::new(None),
            working_limit,
        }
    }

    /// The session for `image_path` and `mask_path`, loaded if needed.
    fn session(
        &self,
        image_path: &Path,
        mask_path: &Path,
    ) -> Result<std::sync::MutexGuard<'_, Option<Session>>, String> {
        let mut slot = self.session.lock().unwrap_or_else(|e| e.into_inner());
        load(&mut slot, image_path, mask_path, self.working_limit)?;
        Ok(slot)
    }

    /// The decoded picture of an image background (cached while the file is unchanged).
    fn custom_image(&self, background: &Background) -> Result<Option<Arc<RgbaImage>>, String> {
        let Background::Image { path } = background else {
            return Ok(None);
        };
        let path = PathBuf::from(path);
        let modified = std::fs::metadata(&path)
            .map_err(|e| format!("cannot open the background image: {e}"))?
            .modified()
            .ok();
        let mut slot = self.custom.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((p, m, img)) = slot.as_ref()
            && *p == path
            && *m == modified
        {
            return Ok(Some(img.clone()));
        }
        *slot = None;
        let img = Arc::new(images::decode(&path)?.to_rgba8());
        *slot = Some((path, modified, img.clone()));
        Ok(Some(img))
    }

    /// The exported file's bytes: `edits` applied to the full-size original, on
    /// `background`, as `settings` say.
    pub fn export(
        &self,
        image_path: &Path,
        mask_path: &Path,
        edits: &Edits,
        background: &Background,
        settings: &ExportSettings,
    ) -> Result<Vec<u8>, String> {
        let custom = self.custom_image(background)?;
        let mut slot = self.session(image_path, mask_path)?;
        let Session {
            image,
            profile,
            base,
            working,
            full,
            ..
        } = slot.as_mut().unwrap();
        let start = Instant::now();
        // The working copy's results when it is the full size and up to date, else the
        // last full-size render, else a new one.
        let (cutout, mask) = match working.cached(edits) {
            Some(results) if working.scale == 1.0 => results,
            _ => {
                if full.as_ref().is_none_or(|(e, _, _)| e != edits) {
                    *full = None; // free the old one first
                    let mask =
                        refine::final_mask(&refine::edge_mask(base, &edits.edges), &edits.strokes);
                    let cutout = refine::compose(image, &mask, edits.edges.decontaminate);
                    *full = Some((edits.clone(), cutout, mask));
                }
                let (_, cutout, mask) = full.as_ref().unwrap();
                (cutout, mask)
            }
        };
        let refined = start.elapsed().as_secs_f32();
        let sources = Sources {
            original: image,
            cutout,
            mask,
            scale: 1.0,
            full: image.dimensions(),
        };
        let picture = export::render(&sources, background, custom.as_deref(), settings, 1.0)?;
        let rendered = start.elapsed().as_secs_f32();
        let bytes = encode::encode(
            &picture,
            settings.format,
            settings.quality,
            profile.as_deref(),
        )?;
        eprintln!(
            "[export] {}×{} {:?}: refine {refined:.2}s, compose {:.2}s, encode {:.2}s",
            picture.width(),
            picture.height(),
            settings.format,
            rendered - refined,
            start.elapsed().as_secs_f32() - rendered
        );
        Ok(bytes)
    }

    /// A small preview of the export (same header as a render), made from the 1 MP copy.
    pub fn export_preview(
        &self,
        image_path: &Path,
        mask_path: &Path,
        edits: &Edits,
        background: &Background,
        settings: &ExportSettings,
    ) -> Result<Vec<u8>, String> {
        let custom = self.custom_image(background)?;
        let mut slot = self.session(image_path, mask_path)?;
        let session = slot.as_mut().unwrap();
        let full = session.image.dimensions();
        let (fw, fh) = export::output_size(full, settings)?;
        let factor = ((EXPORT_PREVIEW_PIXELS as f64 / (fw as f64 * fh as f64)).sqrt()).min(1.0);
        let layer = session.preview();
        let scale = layer.scale;
        let (original, cutout, mask) = layer.results(edits);
        let sources = Sources {
            original,
            cutout,
            mask,
            scale,
            full,
        };
        let picture = export::render(
            &sources,
            background,
            custom.as_deref(),
            settings,
            factor as f32,
        )?;
        let size = picture.dimensions();
        Ok(header(size, Rect::full(size.0, size.1), picture.into_raw()))
    }

    /// The background layer under the cutout on the stage (same header as a render), the
    /// shape of the image: the blur from the 1 MP copy, an image at up to 4 MP.
    pub fn stage_background(
        &self,
        image_path: &Path,
        mask_path: &Path,
        edits: &Edits,
        background: &Background,
    ) -> Result<Vec<u8>, String> {
        let custom = self.custom_image(background)?;
        let mut slot = self.session(image_path, mask_path)?;
        let session = slot.as_mut().unwrap();
        let (w, h) = session.image.dimensions();
        let picture = match background {
            Background::Transparent => RgbaImage::new(1, 1),
            Background::Color { color } => background::solid(*color, 1, 1),
            Background::Blur { strength } => {
                let layer = session.preview();
                let (lw, lh) = layer.size();
                let (original, _, mask) = layer.results(edits);
                background::blurred(original, mask, *strength, lw, lh)
            }
            Background::Image { .. } => {
                let (bw, bh) = scaled_size(w, h, STAGE_BACKGROUND_PIXELS);
                background::cover(
                    custom.as_deref().ok_or("the background image is missing")?,
                    bw,
                    bh,
                )
            }
        };
        let size = picture.dimensions();
        Ok(header(size, Rect::full(size.0, size.1), picture.into_raw()))
    }

    /// The original image at the working size (fully opaque where the photo is), with
    /// the same header as a render: the brush's live preview restores from it.
    pub fn original(&self, image_path: &Path, mask_path: &Path) -> Result<Vec<u8>, String> {
        let slot = self.session(image_path, mask_path)?;
        let image = &slot.as_ref().unwrap().working.image;
        let size = image.dimensions();
        Ok(header(
            size,
            Rect::full(size.0, size.1),
            image.as_raw().clone(),
        ))
    }

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
        let mut slot = self.session(image_path, mask_path)?;
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
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.custom.lock().unwrap_or_else(|e| e.into_inner()) = None;
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
    fn sends_the_working_original() {
        let dir = std::env::temp_dir().join("kreokit-cutout-session-original");
        let (ip, mp) = scene(&dir, 40, 30);
        let (h, px) = parse(&Refiner::default().original(&ip, &mp).unwrap());
        assert_eq!(h, [40, 30, 0, 0, 40, 30]);
        assert_eq!(px, *image::open(&ip).unwrap().to_rgba8().as_raw());
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

    use crate::encode::Format;
    use crate::layout::{Fit, Size};

    fn settings(format: Format, size: Option<(u32, u32)>, fit: Fit) -> ExportSettings {
        ExportSettings {
            format,
            quality: 95,
            size: size.map(|(width, height)| Size { width, height }),
            fit,
        }
    }

    fn some_edits() -> Edits {
        Edits {
            edges: EdgeSettings {
                feather: 3.0,
                shift: 2.0,
                decontaminate: true,
            },
            strokes: vec![
                stroke(BrushMode::Erase, vec![[100.0, 80.0], [130.0, 95.0]]),
                stroke(BrushMode::Restore, vec![[20.0, 20.0], [40.0, 30.0]]),
            ],
        }
    }

    fn near(a: [u8; 3], b: [u8; 3], tolerance: i32) -> bool {
        (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= tolerance)
    }

    const BLUE: [u8; 3] = [0x3D, 0x8B, 0xFF];

    /// The expected export: fully transparent pixels carry no colour (nothing of the
    /// erased photo is hidden in the file).
    fn expected_export(ip: &Path, mp: &Path, edits: &Edits) -> RgbaImage {
        let mut want = expected(ip, mp, edits);
        for p in want.pixels_mut().filter(|p| p.0[3] == 0) {
            p.0 = [0; 4];
        }
        want
    }

    #[test]
    fn png_export_keeps_alpha_and_every_edit_at_full_size() {
        let dir = std::env::temp_dir().join("kreokit-cutout-export-png");
        let (ip, mp) = scene(&dir, 240, 180);
        let edits = some_edits();
        let want = expected_export(&ip, &mp, &edits);
        assert!(want.pixels().any(|p| p.0[3] == 0) && want.pixels().any(|p| p.0[3] == 255));
        let png = settings(Format::Png, None, Fit::Fit);

        // A working copy smaller than the image: the export is still made at full size.
        let small = Refiner::with_working_limit(10_000);
        let (h, _) = parse(&small.render(&ip, &mp, &edits, false, false).unwrap());
        assert!(h[0] < 240, "working copy {}×{}", h[0], h[1]);
        let bytes = small
            .export(&ip, &mp, &edits, &Background::Transparent, &png)
            .unwrap();
        let out = image::load_from_memory(&bytes).unwrap();
        assert_eq!(out.color(), image::ColorType::Rgba8);
        assert_eq!(out.to_rgba8(), want);

        // Exporting again (another format) reuses the full-size render.
        assert!(
            small
                .session
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .full
                .is_some()
        );
        let again = small
            .export(
                &ip,
                &mp,
                &edits,
                &Background::Transparent,
                &settings(Format::Webp, None, Fit::Fit),
            )
            .unwrap();
        assert_eq!(
            {
                let i = image::load_from_memory(&again).unwrap();
                (i.width(), i.height())
            },
            (240, 180)
        );

        // A full-size working copy: its cached render is reused, with the same result.
        let full = Refiner::default();
        full.render(&ip, &mp, &edits, false, false).unwrap();
        let bytes = full
            .export(&ip, &mp, &edits, &Background::Transparent, &png)
            .unwrap();
        assert_eq!(image::load_from_memory(&bytes).unwrap().to_rgba8(), want);
        // And with edits it hasn't rendered yet.
        let mut later = edits.clone();
        later.strokes.pop();
        let bytes = full
            .export(&ip, &mp, &later, &Background::Transparent, &png)
            .unwrap();
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().to_rgba8(),
            expected_export(&ip, &mp, &later)
        );
    }

    #[test]
    fn jpg_export_has_the_chosen_background_and_no_alpha() {
        let dir = std::env::temp_dir().join("kreokit-cutout-export-jpg");
        let (ip, mp) = scene(&dir, 240, 180);
        let refiner = Refiner::default();
        let edits = Edits {
            edges: EdgeSettings::default(),
            strokes: vec![],
        };
        let jpg = settings(Format::Jpg, Some((1080, 1080)), Fit::Fit);
        let colour = Background::Color { color: BLUE };
        let out =
            image::load_from_memory(&refiner.export(&ip, &mp, &edits, &colour, &jpg).unwrap())
                .unwrap();
        assert_eq!(out.color(), image::ColorType::Rgb8);
        assert_eq!((out.width(), out.height()), (1080, 1080));
        let out = out.to_rgb8();
        // The corners (background, and the band Fit leaves) are the colour.
        for (x, y) in [(5, 5), (1074, 5), (5, 1074), (1074, 1074), (540, 150)] {
            assert!(
                near(out.get_pixel(x, y).0, BLUE, 6),
                "({x},{y}) {:?}",
                out.get_pixel(x, y)
            );
        }
        // The subject (the disc in the middle) is the photo, scaled 4.5×.
        assert!(
            near(out.get_pixel(540, 540).0, [120, 90, 90], 10),
            "{:?}",
            out.get_pixel(540, 540)
        );

        // A transparent background in a JPG becomes white.
        let out = image::load_from_memory(
            &refiner
                .export(&ip, &mp, &edits, &Background::Transparent, &jpg)
                .unwrap(),
        )
        .unwrap()
        .to_rgb8();
        assert!(near(out.get_pixel(5, 5).0, [255, 255, 255], 4));
    }

    #[test]
    fn export_sizes_are_exact() {
        let dir = std::env::temp_dir().join("kreokit-cutout-export-sizes");
        let (ip, mp) = scene(&dir, 300, 200);
        let refiner = Refiner::default();
        let edits = some_edits();
        let presets = [(1080, 1080), (1080, 1350), (1080, 1920), (1280, 720)];
        for size in presets {
            for fit in [Fit::Fit, Fit::Fill] {
                let png = settings(Format::Png, Some(size), fit);
                let bytes = refiner
                    .export(&ip, &mp, &edits, &Background::Transparent, &png)
                    .unwrap();
                let out = image::load_from_memory(&bytes).unwrap();
                assert_eq!((out.width(), out.height()), size, "{fit:?}");
            }
        }
        for format in [Format::Webp, Format::Jpg] {
            let s = settings(format, Some((1080, 1350)), Fit::Fill);
            let bytes = refiner
                .export(&ip, &mp, &edits, &Background::Color { color: BLUE }, &s)
                .unwrap();
            let out = image::load_from_memory(&bytes).unwrap();
            assert_eq!((out.width(), out.height()), (1080, 1350), "{format:?}");
        }
        // The original size.
        let png = settings(Format::Png, None, Fit::Fill);
        let out = image::load_from_memory(
            &refiner
                .export(&ip, &mp, &edits, &Background::Transparent, &png)
                .unwrap(),
        )
        .unwrap();
        assert_eq!((out.width(), out.height()), (300, 200));
    }

    #[test]
    fn blur_and_image_backgrounds_in_the_export() {
        let dir = std::env::temp_dir().join("kreokit-cutout-export-backgrounds");
        let (ip, mp) = scene(&dir, 240, 180);
        let refiner = Refiner::default();
        let edits = Edits {
            edges: EdgeSettings::default(),
            strokes: vec![],
        };
        let jpg = settings(Format::Jpg, Some((1280, 720)), Fit::Fit);

        let blur = Background::Blur { strength: 30.0 };
        let out = image::load_from_memory(&refiner.export(&ip, &mp, &edits, &blur, &jpg).unwrap())
            .unwrap();
        assert_eq!(out.color(), image::ColorType::Rgb8);
        // Far from the subject, the photo's blue channel (90) shows through the blur.
        let p = out.to_rgb8().get_pixel(20, 360).0;
        assert!((p[2] as i32 - 90).abs() <= 6, "{p:?}");

        let green = dir.join("green.png");
        RgbaImage::from_pixel(50, 40, image::Rgba([20, 200, 60, 255]))
            .save(&green)
            .unwrap();
        let picture = Background::Image {
            path: green.to_string_lossy().into_owned(),
        };
        let out =
            image::load_from_memory(&refiner.export(&ip, &mp, &edits, &picture, &jpg).unwrap())
                .unwrap()
                .to_rgb8();
        assert!(
            near(out.get_pixel(10, 10).0, [20, 200, 60], 6),
            "{:?}",
            out.get_pixel(10, 10)
        );
        let missing = Background::Image {
            path: dir.join("nope.png").to_string_lossy().into_owned(),
        };
        assert!(refiner.export(&ip, &mp, &edits, &missing, &jpg).is_err());
    }

    #[test]
    fn export_keeps_the_colour_profile() {
        let dir = std::env::temp_dir().join("kreokit-cutout-export-icc");
        let (ip, mp) = scene(&dir, 60, 40);
        let icc: Vec<u8> = (0..200u8).collect();
        let original = image::open(&ip).unwrap().to_rgba8();
        std::fs::write(
            &ip,
            encode::encode(&original, Format::Png, 90, Some(&icc)).unwrap(),
        )
        .unwrap();
        let refiner = Refiner::default();
        let edits = Edits {
            edges: EdgeSettings::default(),
            strokes: vec![],
        };
        let bytes = refiner
            .export(
                &ip,
                &mp,
                &edits,
                &Background::Transparent,
                &settings(Format::Jpg, None, Fit::Fit),
            )
            .unwrap();
        use image::ImageDecoder;
        let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        assert_eq!(decoder.icc_profile().unwrap(), Some(icc));
    }

    #[test]
    fn previews_have_the_frame_shape() {
        let dir = std::env::temp_dir().join("kreokit-cutout-export-preview");
        let (ip, mp) = scene(&dir, 1600, 1200);
        let refiner = Refiner::default();
        let edits = some_edits();
        let s = settings(Format::Png, Some((1080, 1920)), Fit::Fill);
        let (h, px) = parse(
            &refiner
                .export_preview(&ip, &mp, &edits, &Background::Blur { strength: 40.0 }, &s)
                .unwrap(),
        );
        assert!(h[0] as u64 * h[1] as u64 <= EXPORT_PREVIEW_PIXELS);
        assert!(
            (h[0] as f32 / h[1] as f32 - 1080.0 / 1920.0).abs() < 0.01,
            "{h:?}"
        );
        assert_eq!(px.len(), (h[0] * h[1] * 4) as usize);

        // The stage background is the shape of the image.
        let (h, _) = parse(
            &refiner
                .stage_background(&ip, &mp, &edits, &Background::Blur { strength: 40.0 })
                .unwrap(),
        );
        assert!(h[0] as u64 * h[1] as u64 <= PREVIEW_PIXELS);
        assert!(
            (h[0] as f32 / h[1] as f32 - 4.0 / 3.0).abs() < 0.01,
            "{h:?}"
        );
    }

    /// Times exports of 12- and 48-megapixel images: `cargo test --release export_timings
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn export_timings() {
        let edits = some_edits();
        for (w, h) in [(4032, 3024), (8064, 6048)] {
            let dir = std::env::temp_dir().join(format!("kreokit-cutout-export-{w}"));
            let (ip, mp) = scene(&dir, w, h);
            let refiner = Refiner::default();
            let t = Instant::now();
            refiner.render(&ip, &mp, &edits, false, false).unwrap();
            println!("{w}×{h} first render: {:.2}s", t.elapsed().as_secs_f32());
            let cases = [
                (
                    "PNG original, transparent",
                    Format::Png,
                    None,
                    Background::Transparent,
                ),
                (
                    "JPG original, colour",
                    Format::Jpg,
                    None,
                    Background::Color { color: BLUE },
                ),
                (
                    "WebP original, transparent",
                    Format::Webp,
                    None,
                    Background::Transparent,
                ),
                (
                    "JPG 1080×1350 Fill, blur",
                    Format::Jpg,
                    Some((1080, 1350)),
                    Background::Blur { strength: 40.0 },
                ),
                (
                    "PNG original, blur",
                    Format::Png,
                    None,
                    Background::Blur { strength: 40.0 },
                ),
            ];
            for (label, format, size, background) in cases {
                let t = Instant::now();
                let bytes = refiner
                    .export(
                        &ip,
                        &mp,
                        &edits,
                        &background,
                        &settings(format, size, Fit::Fill),
                    )
                    .unwrap();
                println!(
                    "  {label:<28} {:.2}s ({:.1} MB)",
                    t.elapsed().as_secs_f32(),
                    bytes.len() as f64 / 1e6
                );
            }
        }
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
