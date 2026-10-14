//! Saving the result: PNG (lossless, with transparency), WebP (lossy with a quality
//! setting, with transparency) and JPG (quality setting, no transparency: transparent
//! areas become white). The original's ICC colour profile is kept, so colours don't
//! shift; other metadata (EXIF, location) is not copied.

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder, RgbImage, RgbaImage};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Png,
    Webp,
    Jpg,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Webp => "webp",
            Format::Jpg => "jpg",
        }
    }
}

/// What transparent areas become in a JPG.
pub const JPG_MATTE: [u8; 3] = [255, 255, 255];
/// The largest side libwebp can encode.
pub const WEBP_MAX_SIDE: u32 = 16_383;

fn is_opaque(image: &RgbaImage) -> bool {
    image.pixels().all(|p| p.0[3] == 255)
}

/// `image` without its alpha channel, composited over `matte`.
pub fn flatten(image: &RgbaImage, matte: [u8; 3]) -> RgbImage {
    RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let p = image.get_pixel(x, y).0;
        let a = p[3] as u32;
        let mix = |i: usize| ((p[i] as u32 * a + matte[i] as u32 * (255 - a) + 127) / 255) as u8;
        image::Rgb([mix(0), mix(1), mix(2)])
    })
}

fn drop_alpha(image: &RgbaImage) -> RgbImage {
    flatten(image, [0, 0, 0])
}

/// Encodes `image` as `format`. `quality` (1–100) is used by WebP and JPG; `icc` is the
/// colour profile to embed.
pub fn encode(
    image: &RgbaImage,
    format: Format,
    quality: u8,
    icc: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let quality = quality.clamp(1, 100);
    let (w, h) = image.dimensions();
    let mut out = Vec::new();
    let failed = |e: image::ImageError| format!("cannot encode the {}: {e}", format.extension());
    match format {
        Format::Png => {
            let mut enc = PngEncoder::new_with_quality(
                &mut out,
                CompressionType::Default,
                FilterType::Adaptive,
            );
            if let Some(icc) = icc {
                let _ = enc.set_icc_profile(icc.to_vec());
            }
            if is_opaque(image) {
                enc.write_image(drop_alpha(image).as_raw(), w, h, ExtendedColorType::Rgb8)
            } else {
                enc.write_image(image.as_raw(), w, h, ExtendedColorType::Rgba8)
            }
            .map_err(failed)?;
        }
        Format::Jpg => {
            let mut enc = JpegEncoder::new_with_quality(&mut out, quality);
            if let Some(icc) = icc {
                let _ = enc.set_icc_profile(icc.to_vec());
            }
            enc.write_image(
                flatten(image, JPG_MATTE).as_raw(),
                w,
                h,
                ExtendedColorType::Rgb8,
            )
            .map_err(failed)?;
        }
        Format::Webp => {
            out = encode_webp(image, quality)?;
            if let Some(icc) = icc {
                out = add_webp_icc(&out, icc, w, h, !is_opaque(image))?;
            }
        }
    }
    Ok(out)
}

/// Lossy WebP with libwebp's simple API; the alpha channel (if any) is stored losslessly.
fn encode_webp(image: &RgbaImage, quality: u8) -> Result<Vec<u8>, String> {
    let (w, h) = image.dimensions();
    if w > WEBP_MAX_SIDE || h > WEBP_MAX_SIDE {
        return Err(format!(
            "WebP images can be at most {WEBP_MAX_SIDE} pixels on a side; choose PNG or JPG for {w}×{h}"
        ));
    }
    let rgb;
    let (pixels, channels, encode): (&[u8], u32, _) = if is_opaque(image) {
        rgb = drop_alpha(image);
        (
            rgb.as_raw(),
            3,
            libwebp_sys::WebPEncodeRGB as unsafe extern "C" fn(_, _, _, _, _, _) -> _,
        )
    } else {
        (
            image.as_raw(),
            4,
            libwebp_sys::WebPEncodeRGBA as unsafe extern "C" fn(_, _, _, _, _, _) -> _,
        )
    };
    let mut output: *mut u8 = std::ptr::null_mut();
    // SAFETY: `pixels` holds `h` rows of `w * channels` bytes; libwebp allocates `output`,
    // which is copied and then freed with WebPFree.
    unsafe {
        let len = encode(
            pixels.as_ptr(),
            w as i32,
            h as i32,
            (w * channels) as i32,
            quality as f32,
            &raw mut output,
        );
        if output.is_null() || len == 0 {
            if !output.is_null() {
                libwebp_sys::WebPFree(output.cast());
            }
            return Err(format!("cannot encode the webp ({w}×{h})"));
        }
        let bytes = std::slice::from_raw_parts(output, len).to_vec();
        libwebp_sys::WebPFree(output.cast());
        Ok(bytes)
    }
}

fn chunk(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut c = Vec::with_capacity(payload.len() + 9);
    c.extend_from_slice(fourcc);
    c.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    c.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        c.push(0);
    }
    c
}

/// Adds an ICCP chunk to a WebP file (RIFF container, extended format "VP8X").
fn add_webp_icc(file: &[u8], icc: &[u8], w: u32, h: u32, alpha: bool) -> Result<Vec<u8>, String> {
    const ICC_FLAG: u8 = 0x20;
    const ALPHA_FLAG: u8 = 0x10;
    let bad = || "the WebP encoder wrote an unexpected file".to_string();
    if file.len() < 20 || &file[0..4] != b"RIFF" || &file[8..12] != b"WEBP" {
        return Err(bad());
    }
    let body = &file[12..];
    let mut out = b"RIFF\0\0\0\0WEBP".to_vec();
    if &body[0..4] == b"VP8X" {
        let size = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
        let end = 8 + size + size % 2;
        if body.len() < end || size < 10 {
            return Err(bad());
        }
        let mut vp8x = body[..end].to_vec();
        vp8x[8] |= ICC_FLAG;
        out.extend_from_slice(&vp8x);
        out.extend_from_slice(&chunk(b"ICCP", icc));
        out.extend_from_slice(&body[end..]);
    } else {
        // Simple format: make it extended, with the canvas size.
        let mut payload = [0u8; 10];
        payload[0] = ICC_FLAG | if alpha { ALPHA_FLAG } else { 0 };
        payload[4..7].copy_from_slice(&(w - 1).to_le_bytes()[..3]);
        payload[7..10].copy_from_slice(&(h - 1).to_le_bytes()[..3]);
        out.extend_from_slice(&chunk(b"VP8X", &payload));
        out.extend_from_slice(&chunk(b"ICCP", icc));
        out.extend_from_slice(body);
    }
    let size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ColorType, GenericImageView, ImageDecoder, ImageReader, Rgba};
    use std::io::Cursor;

    /// A colourful, slightly noisy picture with a transparent left part and a soft edge.
    fn picture(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let noise = ((x * 7919 + y * 104_729) % 23) as u8;
            let a = (((x as i32 - w as i32 / 3) * 32).clamp(0, 255)) as u8;
            Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 120 + noise, a])
        })
    }

    fn decode(bytes: &[u8]) -> image::DynamicImage {
        image::load_from_memory(bytes).unwrap()
    }

    fn icc_of(bytes: &[u8]) -> Option<Vec<u8>> {
        let mut decoder = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        decoder.icc_profile().unwrap()
    }

    #[test]
    fn png_keeps_alpha_exactly() {
        let img = picture(64, 40);
        let out = decode(&encode(&img, Format::Png, 90, None).unwrap());
        assert_eq!(out.color(), ColorType::Rgba8);
        assert_eq!(out.to_rgba8(), img);
        // Opaque pictures are saved without an alpha channel.
        let opaque = RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255]));
        let out = decode(&encode(&opaque, Format::Png, 90, None).unwrap());
        assert_eq!(out.color(), ColorType::Rgb8);
        assert_eq!(out.to_rgba8(), opaque);
    }

    #[test]
    fn jpg_has_no_alpha_and_transparent_becomes_white() {
        let mut img = RgbaImage::from_pixel(64, 64, Rgba([200, 40, 40, 255]));
        for y in 0..64 {
            for x in 0..32 {
                img.put_pixel(x, y, Rgba([0, 255, 0, 0]));
            }
        }
        let out = decode(&encode(&img, Format::Jpg, 95, None).unwrap());
        assert_eq!(out.color(), ColorType::Rgb8);
        let out = out.to_rgb8();
        let near = |a: [u8; 3], b: [u8; 3]| (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= 6);
        assert!(
            near(out.get_pixel(8, 32).0, JPG_MATTE),
            "{:?}",
            out.get_pixel(8, 32)
        );
        assert!(
            near(out.get_pixel(56, 32).0, [200, 40, 40]),
            "{:?}",
            out.get_pixel(56, 32)
        );
    }

    #[test]
    fn webp_is_lossy_and_keeps_alpha_exactly() {
        let img = picture(64, 40);
        let out = decode(&encode(&img, Format::Webp, 90, None).unwrap());
        assert!(out.color().has_alpha());
        let out = out.to_rgba8();
        assert!(
            out.pixels()
                .zip(img.pixels())
                .all(|(a, b)| a.0[3] == b.0[3])
        );
        // Colours are close where the picture is opaque.
        let (mut sum, mut n) = (0.0, 0);
        for (a, b) in out
            .pixels()
            .zip(img.pixels())
            .filter(|(_, b)| b.0[3] == 255)
        {
            sum += (0..3)
                .map(|i| (a.0[i] as f64 - b.0[i] as f64).abs())
                .sum::<f64>();
            n += 3;
        }
        assert!(sum / n as f64 <= 4.0, "mean error {}", sum / n as f64);
        // Opaque pictures have no alpha channel.
        let opaque = RgbaImage::from_pixel(16, 16, Rgba([9, 99, 199, 255]));
        assert!(
            !decode(&encode(&opaque, Format::Webp, 90, None).unwrap())
                .color()
                .has_alpha()
        );
    }

    #[test]
    fn lower_quality_makes_smaller_files() {
        let img = picture(256, 256);
        for format in [Format::Jpg, Format::Webp] {
            let high = encode(&img, format, 95, None).unwrap().len();
            let low = encode(&img, format, 40, None).unwrap().len();
            assert!(low < high, "{format:?}: {low} >= {high}");
        }
    }

    #[test]
    fn keeps_the_colour_profile() {
        let icc: Vec<u8> = (0..=255u8).cycle().take(557).collect(); // odd length on purpose
        let transparent = picture(32, 20);
        let opaque = RgbaImage::from_pixel(32, 20, Rgba([5, 6, 7, 255]));
        for format in [Format::Png, Format::Jpg, Format::Webp] {
            for img in [&transparent, &opaque] {
                let bytes = encode(img, format, 90, Some(&icc)).unwrap();
                assert_eq!(icc_of(&bytes).as_deref(), Some(&icc[..]), "{format:?}");
                // Still a valid image of the right size.
                assert_eq!(decode(&bytes).dimensions(), (32, 20));
            }
            assert_eq!(icc_of(&encode(&opaque, format, 90, None).unwrap()), None);
        }
    }

    #[test]
    fn webp_too_wide_is_a_clear_error() {
        let img = RgbaImage::new(WEBP_MAX_SIDE + 1, 1);
        let err = encode(&img, Format::Webp, 90, None).unwrap_err();
        assert!(err.contains("16383"), "{err}");
    }
}
