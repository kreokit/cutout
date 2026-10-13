//! Opening images: which files we accept, their size and format, and cached thumbnails.
//!
//! Only the file header is read to check an image (fast, even for large photos). Sizes are
//! reported as displayed, i.e. after the EXIF orientation, like the webview shows them.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use image::metadata::Orientation;
use image::{ImageDecoder, ImageFormat, ImageReader};
use serde::Serialize;

/// Image files the app opens (also used to vet dropped files).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Longest side of a Home screen thumbnail, in pixels.
const THUMBNAIL_SIZE: u32 = 480;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImageInfo {
    pub path: String,
    /// Displayed size, after the EXIF orientation.
    pub width: u32,
    pub height: u32,
    /// "png", "jpeg" or "webp", from the file contents (not the extension).
    pub format: String,
    pub bytes: u64,
}

pub fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

fn format_name(format: ImageFormat) -> Option<&'static str> {
    match format {
        ImageFormat::Png => Some("png"),
        ImageFormat::Jpeg => Some("jpeg"),
        ImageFormat::WebP => Some("webp"),
        _ => None,
    }
}

const UNSUPPORTED: &str = "Choose a PNG, JPG or WebP image.";

/// Checks that `path` is a PNG, JPG or WebP image we can read, and returns its size.
pub fn inspect(path: &Path) -> Result<ImageInfo, String> {
    if !has_image_extension(path) {
        return Err(UNSUPPORTED.into());
    }
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let bytes = fs::metadata(path)
        .map_err(|_| format!("{name} was moved or deleted."))?
        .len();
    let reader = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("cannot open {name}: {e}"))?;
    let format = reader.format().and_then(format_name).ok_or(UNSUPPORTED)?;
    let mut decoder = reader.into_decoder().map_err(|_| {
        format!(
            "{name} is damaged or not a real {} file.",
            format.to_uppercase()
        )
    })?;
    let (w, h) = decoder.dimensions();
    let rotated = matches!(
        decoder.orientation().unwrap_or(Orientation::NoTransforms),
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    );
    let (width, height) = if rotated { (h, w) } else { (w, h) };
    Ok(ImageInfo {
        path: path.to_string_lossy().into_owned(),
        width,
        height,
        format: format.into(),
        bytes,
    })
}

/// A key for caches derived from `image`: changes when the file is replaced or edited.
pub fn cache_key(image: &Path) -> Result<String, String> {
    let modified = fs::metadata(image)
        .and_then(|m| m.modified())
        .map_err(|_| "image not found")?;
    let mut hasher = DefaultHasher::new();
    (image, modified).hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

/// Decodes the whole image at `path`, turned upright by its EXIF orientation.
pub fn decode(path: &Path) -> Result<image::DynamicImage, String> {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut decoder = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("cannot open {name}: {e}"))?
        .into_decoder()
        .map_err(|e| format!("cannot read {name}: {e}"))?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("cannot read {name}: {e}"))?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// The cached thumbnail for `image` (a JPEG, longest side 480 px, EXIF orientation
/// applied), made on first use. A changed file gets a new thumbnail.
pub fn thumbnail(dir: &Path, image: &Path) -> Result<PathBuf, String> {
    let thumbs = dir.join("thumbnails");
    let out = thumbs.join(format!("{}.jpg", cache_key(image)?));
    if out.is_file() {
        return Ok(out);
    }
    let mut img = decode(image)?;
    if img.width().max(img.height()) > THUMBNAIL_SIZE {
        img = img.thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE);
    }
    // JPEG has no alpha: flatten transparent images onto the stage colour.
    let thumb = flatten(&img, [10, 10, 12]);
    fs::create_dir_all(&thumbs).map_err(|e| e.to_string())?;
    thumb
        .save_with_format(&out, ImageFormat::Jpeg)
        .map_err(|e| format!("cannot save thumbnail: {e}"))?;
    Ok(out)
}

/// `img` composited over a solid `background`, without alpha.
fn flatten(img: &image::DynamicImage, background: [u8; 3]) -> image::RgbImage {
    let rgba = img.to_rgba8();
    image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let mix = |c: u8, bg: u8| {
            ((c as u32 * a as u32 + bg as u32 * (255 - a as u32) + 127) / 255) as u8
        };
        image::Rgb([
            mix(r, background[0]),
            mix(g, background[1]),
            mix(b, background[2]),
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("kreokit-cutout-image-tests")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A 40×30 test image with a transparent half, saved as `name` (format from extension).
    fn write_image(dir: &Path, name: &str) -> PathBuf {
        let img = RgbaImage::from_fn(40, 30, |x, _| {
            if x < 20 {
                Rgba([200, 60, 30, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let path = dir.join(name);
        let img = image::DynamicImage::ImageRgba8(img);
        if name.ends_with(".jpg") {
            img.to_rgb8().save(&path).unwrap();
        } else {
            img.save(&path).unwrap();
        }
        path
    }

    #[test]
    fn reads_png_jpeg_and_webp() {
        let dir = temp_dir("formats");
        for (name, format) in [("a.png", "png"), ("b.jpg", "jpeg"), ("c.webp", "webp")] {
            let info = inspect(&write_image(&dir, name)).unwrap();
            assert_eq!((info.width, info.height), (40, 30), "{name}");
            assert_eq!(info.format, format);
            assert!(info.bytes > 0);
        }
    }

    #[test]
    fn trusts_contents_over_the_extension() {
        let dir = temp_dir("renamed");
        let png = write_image(&dir, "real.png");
        let renamed = dir.join("actually-png.jpg");
        fs::rename(&png, &renamed).unwrap();
        assert_eq!(inspect(&renamed).unwrap().format, "png");
    }

    #[test]
    fn rejects_other_files() {
        let dir = temp_dir("reject");
        let text = dir.join("notes.png");
        fs::write(&text, "not an image").unwrap();
        assert!(inspect(&text).is_err());
        let gif = dir.join("anim.gif");
        fs::write(&gif, "GIF89a").unwrap();
        assert_eq!(inspect(&gif).unwrap_err(), UNSUPPORTED);
        assert!(
            inspect(&dir.join("missing.png"))
                .unwrap_err()
                .contains("moved or deleted")
        );
        assert!(has_image_extension(Path::new("/x/Photo.JPEG")));
        assert!(!has_image_extension(Path::new("/x/photo")));
    }

    #[test]
    fn makes_and_caches_thumbnails() {
        let dir = temp_dir("thumbs");
        let image = write_image(&dir, "big.png");
        let thumb = thumbnail(&dir, &image).unwrap();
        let made = image::open(&thumb).unwrap();
        assert_eq!((made.width(), made.height()), (40, 30)); // never upscaled
        // The transparent half is flattened onto the stage colour.
        assert!(made.to_rgb8().get_pixel(35, 15).0.iter().all(|&c| c < 20));
        assert_eq!(thumbnail(&dir, &image).unwrap(), thumb); // cached
        assert!(thumbnail(&dir, &dir.join("missing.png")).is_err());
    }
}
