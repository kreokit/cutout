//! The exported picture: the background covering the output frame, the refined cutout
//! placed on it (layout.rs), then encoded (encode.rs) and written atomically.
//!
//! The same [`render`] makes the full-size export and the small preview in the Export
//! dialog; only the sources (full size or a scaled copy) and the output scale differ.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use image::imageops::{self, FilterType};
use image::{GrayImage, Rgba, RgbaImage};
use serde::Deserialize;

use crate::background::{self, Background};
use crate::encode::Format;
use crate::layout::{self, Fit, Size};
use crate::refine::Rect;

/// The Export dialog's settings.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ExportSettings {
    pub format: Format,
    /// 1–100, for WebP and JPG.
    pub quality: u8,
    /// `None`: the original's size.
    pub size: Option<Size>,
    pub fit: Fit,
}

/// What a picture is made from: the original, the refined cutout and its final mask, all
/// the same size, which is `scale` × the original's `full` size.
pub struct Sources<'a> {
    pub original: &'a RgbaImage,
    pub cutout: &'a RgbaImage,
    pub mask: &'a GrayImage,
    pub scale: f32,
    pub full: (u32, u32),
}

/// The output frame and where the photo goes in it, in pixels of the full-size output.
fn layout_of(
    src: &Sources,
    settings: &ExportSettings,
) -> Result<((u32, u32), layout::Placement), String> {
    let (w, h) = src.full;
    let frame = layout::frame_size(w, h, settings.size)?;
    // The original size has nothing to fill.
    let fit = if settings.size.is_none() {
        Fit::Fit
    } else {
        settings.fit
    };
    let subject = layout::subject_bounds(src.mask).map(|r| {
        let x0 = (r.x as f32 / src.scale).floor() as u32;
        let y0 = (r.y as f32 / src.scale).floor() as u32;
        let x1 = (((r.x + r.width) as f32 / src.scale).ceil() as u32).min(w);
        let y1 = (((r.y + r.height) as f32 / src.scale).ceil() as u32).min(h);
        Rect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        }
    });
    Ok((frame, layout::place(w, h, frame, fit, subject)))
}

/// The size of the full output for `settings`.
pub fn output_size(full: (u32, u32), settings: &ExportSettings) -> Result<(u32, u32), String> {
    layout::frame_size(full.0, full.1, settings.size)
}

/// Renders the picture, `factor` × the output size (1 for the export). `custom` is the
/// decoded image of an image background.
pub fn render(
    src: &Sources,
    background: &Background,
    custom: Option<&RgbaImage>,
    settings: &ExportSettings,
    factor: f32,
) -> Result<RgbaImage, String> {
    let ((fw, fh), place) = layout_of(src, settings)?;
    let ow = ((fw as f32 * factor).round() as u32).max(1);
    let oh = ((fh as f32 * factor).round() as u32).max(1);
    let mut out = match background {
        Background::Transparent => RgbaImage::new(ow, oh),
        Background::Color { color } => background::solid(*color, ow, oh),
        Background::Blur { strength } => {
            background::blurred(src.original, src.mask, *strength, ow, oh)
        }
        Background::Image { .. } => {
            let custom = custom.ok_or("the background image is missing")?;
            background::cover(custom, ow, oh)
        }
    };
    let s = place.scale * (ow as f32 / fw as f32) / src.scale;
    let (ox, oy) = (
        place.x * ow as f32 / fw as f32,
        place.y * oh as f32 / fh as f32,
    );
    draw(&mut out, src.cutout, s, ox, oy);
    Ok(out)
}

/// Composites straight-alpha `top` over `bottom` (both straight alpha).
fn over(bottom: &mut Rgba<u8>, top: [u8; 4]) {
    let at = top[3] as f32 / 255.0;
    if at >= 1.0 {
        bottom.0 = top;
        return;
    }
    if at <= 0.0 {
        return;
    }
    let ab = bottom.0[3] as f32 / 255.0;
    let ao = at + ab * (1.0 - at);
    for (b, t) in bottom.0.iter_mut().zip(top).take(3) {
        let c = (t as f32 * at + *b as f32 * ab * (1.0 - at)) / ao;
        *b = c.round().clamp(0.0, 255.0) as u8;
    }
    bottom.0[3] = (ao * 255.0).round() as u8;
}

/// Draws `image` scaled by `s` with its top-left corner at (`ox`, `oy`) over `out`.
fn draw(out: &mut RgbaImage, image: &RgbaImage, s: f32, ox: f32, oy: f32) {
    let (ow, oh) = out.dimensions();
    let (iw, ih) = image.dimensions();
    if (s - 1.0).abs() < 1e-6 && ox == ox.round() && oy == oy.round() {
        // Same size: no resampling.
        for (x, y, p) in image.enumerate_pixels() {
            let (tx, ty) = (x as i64 + ox as i64, y as i64 + oy as i64);
            if tx >= 0 && ty >= 0 && (tx as u32) < ow && (ty as u32) < oh {
                over(out.get_pixel_mut(tx as u32, ty as u32), p.0);
            }
        }
        return;
    }
    // The part of the image that lands in the frame, in image pixels (with a pixel of
    // margin for the filter).
    let sx0 = ((-ox / s).floor() - 1.0).clamp(0.0, iw as f32) as u32;
    let sy0 = ((-oy / s).floor() - 1.0).clamp(0.0, ih as f32) as u32;
    let sx1 = (((ow as f32 - ox) / s).ceil() + 1.0).clamp(0.0, iw as f32) as u32;
    let sy1 = (((oh as f32 - oy) / s).ceil() + 1.0).clamp(0.0, ih as f32) as u32;
    if sx1 <= sx0 || sy1 <= sy0 {
        return;
    }
    let tw = (((sx1 - sx0) as f32 * s).round() as u32).max(1);
    let th = (((sy1 - sy0) as f32 * s).round() as u32).max(1);
    // Premultiplied, so transparent pixels don't bleed their colour into the edges.
    let mut part = imageops::crop_imm(image, sx0, sy0, sx1 - sx0, sy1 - sy0).to_image();
    for p in part.pixels_mut() {
        let a = p.0[3] as u32;
        for i in 0..3 {
            p.0[i] = ((p.0[i] as u32 * a + 127) / 255) as u8;
        }
    }
    let filter = if s < 1.0 {
        FilterType::Triangle
    } else {
        FilterType::CatmullRom
    };
    let scaled = imageops::resize(&part, tw, th, filter);
    let tx0 = (ox + sx0 as f32 * s).round() as i64;
    let ty0 = (oy + sy0 as f32 * s).round() as i64;
    for (x, y, p) in scaled.enumerate_pixels() {
        let (tx, ty) = (tx0 + x as i64, ty0 + y as i64);
        if tx < 0 || ty < 0 || tx as u32 >= ow || ty as u32 >= oh || p.0[3] == 0 {
            continue;
        }
        let a = p.0[3];
        let unpremultiply = |c: u8| ((c.min(a) as u32 * 255 + a as u32 / 2) / a as u32) as u8;
        let top = [
            unpremultiply(p.0[0]),
            unpremultiply(p.0[1]),
            unpremultiply(p.0[2]),
            a,
        ];
        over(out.get_pixel_mut(tx as u32, ty as u32), top);
    }
}

/// `dest` with the extension of `format` if it has none; an error if it has another.
pub fn destination(dest: &Path, format: Format) -> Result<PathBuf, String> {
    let ext = dest.extension().map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        None | Some("") => Ok(dest.with_extension(format.extension())),
        Some(e) if e == format.extension() || (format == Format::Jpg && e == "jpeg") => {
            Ok(dest.to_path_buf())
        }
        Some(e) => Err(format!(
            "the file name ends in .{e}, but the format is {}",
            format.extension().to_uppercase()
        )),
    }
}

/// Writes `bytes` to `dest` through a temporary file in the same folder, so a failed or
/// interrupted export never leaves a broken file behind.
pub fn write_atomic(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = dest
        .file_name()
        .ok_or("the destination has no file name")?
        .to_string_lossy();
    let tmp = dest.with_file_name(format!(".{name}.kreokit-tmp"));
    fs::write(&tmp, bytes)
        .and_then(|_| fs::rename(&tmp, dest))
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("cannot save {name}: {e}")
        })
}

/// What happened when writing without replacing.
#[derive(Debug, PartialEq)]
pub enum WriteNew {
    Written,
    /// `dest` exists; nothing was written.
    Exists,
}

/// Writes `bytes` to `dest` only if no file is there: through a temporary file that is
/// hard-linked into place, which fails if `dest` exists, so even a file created a moment
/// ago is never replaced. Where hard links aren't supported (FAT, exFAT), checks first
/// and renames.
pub fn write_new(dest: &Path, bytes: &[u8]) -> Result<WriteNew, String> {
    let name = dest
        .file_name()
        .ok_or("the destination has no file name")?
        .to_string_lossy()
        .into_owned();
    let tmp = dest.with_file_name(format!(".{name}.kreokit-tmp"));
    let failed = |e: std::io::Error| format!("cannot save {name}: {e}");
    fs::write(&tmp, bytes).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        failed(e)
    })?;
    let result = match fs::hard_link(&tmp, dest) {
        Ok(()) => Ok(WriteNew::Written),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(WriteNew::Exists),
        Err(_) if dest.exists() => Ok(WriteNew::Exists),
        Err(_) => fs::rename(&tmp, dest)
            .map(|_| WriteNew::Written)
            .map_err(failed),
    };
    let _ = fs::remove_file(&tmp);
    result
}

/// `<stem>-cutout.<ext>`, or with `-<n>` before the extension.
fn cutout_name(image: &Path, format: Format, n: u32) -> String {
    let stem = image
        .file_stem()
        .map_or_else(|| "image".into(), |s| s.to_string_lossy().into_owned());
    match n {
        1 => format!("{stem}-cutout.{}", format.extension()),
        n => format!("{stem}-cutout-{n}.{}", format.extension()),
    }
}

/// Lower-cased file names in `folder` (macOS and Windows compare names without case).
fn names_in(folder: &Path) -> HashSet<String> {
    fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

/// Where each image of a batch export goes in `folder`: `<stem>-cutout.<ext>`, with -2,
/// -3… for names that repeat within the batch, and (with `keep_existing`) for names that
/// are already in the folder.
pub fn batch_destinations(
    folder: &Path,
    images: &[PathBuf],
    format: Format,
    keep_existing: bool,
) -> Vec<PathBuf> {
    let mut taken = if keep_existing {
        names_in(folder)
    } else {
        HashSet::new()
    };
    images
        .iter()
        .map(|image| {
            let name = (1..)
                .map(|n| cutout_name(image, format, n))
                .find(|name| !taken.contains(&name.to_lowercase()))
                .unwrap();
            taken.insert(name.to_lowercase());
            folder.join(name)
        })
        .collect()
}

/// The files a batch export to `folder` would replace (if the user chooses Replace).
pub fn conflicts(folder: &Path, images: &[PathBuf], format: Format) -> Vec<PathBuf> {
    let existing = names_in(folder);
    batch_destinations(folder, images, format, false)
        .into_iter()
        .filter(|d| {
            d.file_name()
                .is_some_and(|n| existing.contains(&n.to_string_lossy().to_lowercase()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn over_blends_straight_alpha() {
        let mut p = Rgba([0, 0, 255, 255]);
        over(&mut p, [255, 0, 0, 128]);
        assert_eq!(p.0, [128, 0, 127, 255]);
        let mut clear = Rgba([9, 9, 9, 0]);
        over(&mut clear, [10, 20, 30, 64]);
        assert_eq!(clear.0, [10, 20, 30, 64]);
    }

    #[test]
    fn scaled_drawing_has_no_dark_fringe() {
        // A white disc on transparent *black* pixels: scaled down, its edge must stay white.
        let img = RgbaImage::from_fn(200, 200, |x, y| {
            let d = ((x as f32 - 100.0).powi(2) + (y as f32 - 100.0).powi(2)).sqrt();
            if d < 60.0 {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let mut out = RgbaImage::new(50, 50);
        draw(&mut out, &img, 0.25, 0.0, 0.0);
        for p in out.pixels().filter(|p| p.0[3] > 0) {
            assert!(p.0[0] >= 250, "{p:?}");
        }
        assert_eq!(out.get_pixel(25, 25).0, [255, 255, 255, 255]);
    }

    #[test]
    fn destinations() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(destination(&p("/a/cat"), Format::Png), Ok(p("/a/cat.png")));
        assert_eq!(
            destination(&p("/a/cat.PNG"), Format::Png),
            Ok(p("/a/cat.PNG"))
        );
        assert_eq!(
            destination(&p("/a/cat.jpeg"), Format::Jpg),
            Ok(p("/a/cat.jpeg"))
        );
        assert!(destination(&p("/a/cat.png"), Format::Jpg).is_err());
    }

    #[test]
    fn writes_atomically() {
        let dir = std::env::temp_dir().join("kreokit-cutout-write");
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("out.png");
        write_atomic(&dest, b"one").unwrap();
        write_atomic(&dest, b"two").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"two");
        assert!(!dir.join(".out.png.kreokit-tmp").exists());
        assert!(write_atomic(&dir.join("missing/out.png"), b"x").is_err());
    }

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_new_never_replaces() {
        let dir = fresh_dir("kreokit-cutout-write-new");
        let dest = dir.join("cat-cutout.png");
        assert_eq!(write_new(&dest, b"first"), Ok(WriteNew::Written));
        assert_eq!(write_new(&dest, b"second"), Ok(WriteNew::Exists));
        assert_eq!(fs::read(&dest).unwrap(), b"first");
        // No temporary files are left behind.
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("cat-cutout.png")]);
        assert!(write_new(&dir.join("missing/x.png"), b"x").is_err());
    }

    #[test]
    fn batch_names_are_unique() {
        let dir = fresh_dir("kreokit-cutout-batch-names");
        let images: Vec<PathBuf> = ["/a/cat.jpg", "/b/cat.png", "/c/Cat.jpeg", "/d/dog.jpg"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let names = |dests: Vec<PathBuf>| -> Vec<String> {
            dests
                .iter()
                .map(|d| d.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            names(batch_destinations(&dir, &images, Format::Png, true)),
            [
                "cat-cutout.png",
                "cat-cutout-2.png",
                "Cat-cutout-3.png",
                "dog-cutout.png"
            ]
        );
        assert!(conflicts(&dir, &images, Format::Png).is_empty());

        // Files already there: Replace would overwrite them; Keep both skips their names.
        fs::write(dir.join("CAT-cutout.png"), b"old").unwrap();
        fs::write(dir.join("dog-cutout.png"), b"old").unwrap();
        fs::write(dir.join("dog-cutout.jpg"), b"other format").unwrap();
        assert_eq!(
            names(conflicts(&dir, &images, Format::Png)),
            ["cat-cutout.png", "dog-cutout.png"]
        );
        assert_eq!(
            names(batch_destinations(&dir, &images, Format::Png, true)),
            [
                "cat-cutout-2.png",
                "cat-cutout-3.png",
                "Cat-cutout-4.png",
                "dog-cutout-2.png"
            ]
        );
        assert_eq!(
            names(batch_destinations(&dir, &images, Format::Png, false)),
            [
                "cat-cutout.png",
                "cat-cutout-2.png",
                "Cat-cutout-3.png",
                "dog-cutout.png"
            ]
        );
    }
}
