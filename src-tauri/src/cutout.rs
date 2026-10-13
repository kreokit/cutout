//! The cutout pipeline: image → model input → ONNX Runtime → mask → transparent PNG.
//!
//! Pre- and post-processing follow rembg's reference implementation for each model, so
//! results match it: the image is stretched to the model's square input, scaled and
//! normalised with the ImageNet mean/std, and the predicted mask is mapped to 0–255 and
//! resized back to the original size. Results are cached per image (path + modified
//! time) and model in `<app cache dir>/cutouts/`.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::{self, FilterType};
use image::{GrayImage, ImageEncoder, RgbImage, RgbaImage};
use ndarray::Array4;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use serde::Serialize;

use crate::images;
use crate::models::CutoutModel;

const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// How a model wants its input and how to read its output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    /// Square input size in pixels.
    pub size: u32,
    /// Scale pixels by the image's brightest value (U²-Net in rembg) instead of by 255.
    pub scale_by_max: bool,
    /// The output is logits (apply a sigmoid) instead of a map to stretch to 0–1.
    pub sigmoid: bool,
}

pub fn spec(model: CutoutModel) -> Spec {
    match model {
        CutoutModel::Fast => Spec {
            size: 320,
            scale_by_max: true,
            sigmoid: false,
        },
        CutoutModel::Best => Spec {
            size: 1024,
            scale_by_max: false,
            sigmoid: true,
        },
    }
}

/// The model input: `img` stretched to `spec.size`², normalised, as 1×3×H×W.
pub fn preprocess(img: &RgbImage, spec: Spec) -> Array4<f32> {
    let n = spec.size;
    let small = imageops::resize(img, n, n, FilterType::Lanczos3);
    let scale = if spec.scale_by_max {
        small.as_raw().iter().copied().max().unwrap_or(0).max(1) as f32
    } else {
        255.0
    };
    let mut input = Array4::<f32>::zeros((1, 3, n as usize, n as usize));
    for (x, y, pixel) in small.enumerate_pixels() {
        for c in 0..3 {
            input[[0, c, y as usize, x as usize]] = (pixel.0[c] as f32 / scale - MEAN[c]) / STD[c];
        }
    }
    input
}

/// The model's raw `spec.size`² output as a 0–255 mask.
pub fn postprocess(raw: &[f32], spec: Spec) -> GrayImage {
    let n = spec.size;
    let values: Vec<f32> = if spec.sigmoid {
        raw.iter().map(|v| 1.0 / (1.0 + (-v).exp())).collect()
    } else {
        let (lo, hi) = raw
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });
        let range = if hi > lo { hi - lo } else { 1.0 };
        raw.iter().map(|v| (v - lo) / range).collect()
    };
    let pixels = values
        .iter()
        .take((n * n) as usize)
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    GrayImage::from_raw(n, n, pixels).expect("model output has the input's size")
}

/// The original with the mask as its alpha (multiplied with any alpha it already had).
pub fn compose(original: &RgbaImage, mask: &GrayImage) -> RgbaImage {
    let mut out = original.clone();
    for (pixel, m) in out.pixels_mut().zip(mask.pixels()) {
        pixel.0[3] = ((pixel.0[3] as u32 * m.0[0] as u32 + 127) / 255) as u8;
    }
    out
}

/// Seconds spent in each step, for the log and the performance notes.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Timings {
    pub load_model: f32,
    pub decode: f32,
    pub preprocess: f32,
    pub inference: f32,
    pub postprocess: f32,
    pub save: f32,
    pub total: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CutoutResult {
    /// Transparent PNG at the original size.
    pub cutout: String,
    /// Greyscale PNG mask at the original size (255 = keep).
    pub mask: String,
    pub model: CutoutModel,
    /// Came from the cache; `timings` is then empty.
    pub cached: bool,
    pub timings: Timings,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    LoadingModel,
    CuttingOut,
    Saving,
}

/// Keeps the last used model loaded (loading U²-Net takes about a second). One cutout
/// runs at a time.
#[derive(Default)]
pub struct Engine(Mutex<Option<(CutoutModel, Session)>>);

fn load_session(path: &Path) -> Result<Session, String> {
    Session::builder()
        .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
        .and_then(|b| b.commit_from_file(path))
        .map_err(|e| format!("cannot load the AI model: {e}"))
}

fn secs(since: Instant) -> f32 {
    since.elapsed().as_secs_f32()
}

/// Where the results for `image` with `model` are cached.
fn result_paths(
    cache_dir: &Path,
    image: &Path,
    model: CutoutModel,
) -> Result<(PathBuf, PathBuf), String> {
    let dir = cache_dir.join("cutouts");
    let key = images::cache_key(image)?;
    let id = serde_json::to_value(model).unwrap();
    let id = id.as_str().unwrap();
    Ok((
        dir.join(format!("{key}-{id}.png")),
        dir.join(format!("{key}-{id}-mask.png")),
    ))
}

/// Saves a PNG via a temporary file, so a crash never leaves half a file in the cache.
fn save_png(
    path: &Path,
    data: &[u8],
    width: u32,
    height: u32,
    color: image::ExtendedColorType,
) -> Result<(), String> {
    let tmp = path.with_extension("png.tmp");
    let result = (|| {
        let file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        PngEncoder::new_with_quality(
            BufWriter::new(file),
            CompressionType::Fast,
            PngFilter::Adaptive,
        )
        .write_image(data, width, height, color)
        .map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|e| format!("cannot save the cutout: {e}"))
}

/// Cuts out the image at `path` with `model` (which must be downloaded), reporting each
/// stage. Returns the cached result if there is one.
pub fn cut_out(
    engine: &Engine,
    model_path: &Path,
    cache_dir: &Path,
    model: CutoutModel,
    path: &Path,
    mut on_stage: impl FnMut(Stage),
) -> Result<CutoutResult, String> {
    let (cutout_path, mask_path) = result_paths(cache_dir, path, model)?;
    let result = |timings, cached| CutoutResult {
        cutout: cutout_path.to_string_lossy().into_owned(),
        mask: mask_path.to_string_lossy().into_owned(),
        model,
        cached,
        timings,
    };
    if cutout_path.is_file() && mask_path.is_file() {
        return Ok(result(Timings::default(), true));
    }

    let start = Instant::now();
    let mut t = Timings::default();
    let mut slot = engine.0.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_none_or(|(m, _)| *m != model) {
        on_stage(Stage::LoadingModel);
        *slot = None; // free the old model first
        let step = Instant::now();
        *slot = Some((model, load_session(model_path)?));
        t.load_model = secs(step);
    }
    let session = &mut slot.as_mut().unwrap().1;

    on_stage(Stage::CuttingOut);
    let step = Instant::now();
    let original = images::decode(path)?.to_rgba8();
    t.decode = secs(step);

    let step = Instant::now();
    let spec = spec(model);
    let rgb = image::DynamicImage::ImageRgba8(original.clone()).to_rgb8();
    let input = Tensor::from_array(preprocess(&rgb, spec)).map_err(|e| e.to_string())?;
    t.preprocess = secs(step);

    let step = Instant::now();
    let outputs = session
        .run(ort::inputs![input])
        .map_err(|e| format!("the AI model failed: {e}"))?;
    let (_, raw) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("unexpected model output: {e}"))?;
    t.inference = secs(step);

    let step = Instant::now();
    let small = postprocess(raw, spec);
    drop(outputs);
    let (w, h) = original.dimensions();
    let mask = imageops::resize(&small, w, h, FilterType::CatmullRom);
    let cutout = compose(&original, &mask);
    t.postprocess = secs(step);

    on_stage(Stage::Saving);
    let step = Instant::now();
    fs::create_dir_all(cutout_path.parent().unwrap()).map_err(|e| e.to_string())?;
    save_png(
        &mask_path,
        mask.as_raw(),
        w,
        h,
        image::ExtendedColorType::L8,
    )?;
    save_png(
        &cutout_path,
        cutout.as_raw(),
        w,
        h,
        image::ExtendedColorType::Rgba8,
    )?;
    t.save = secs(step);
    t.total = secs(start);

    eprintln!(
        "[cutout] {} {w}×{h} {model:?}: load model {:.2}s, decode {:.2}s, preprocess {:.2}s, \
         inference {:.2}s, postprocess {:.2}s, save {:.2}s, total {:.2}s",
        path.file_name().unwrap_or_default().to_string_lossy(),
        t.load_model,
        t.decode,
        t.preprocess,
        t.inference,
        t.postprocess,
        t.save,
        t.total
    );
    Ok(result(t, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb, Rgba};

    const FAST: Spec = Spec {
        size: 4,
        scale_by_max: true,
        sigmoid: false,
    };

    #[test]
    fn specs_match_the_models() {
        assert_eq!(spec(CutoutModel::Fast).size, 320);
        assert_eq!(spec(CutoutModel::Best).size, 1024);
        assert!(spec(CutoutModel::Fast).scale_by_max && !spec(CutoutModel::Best).scale_by_max);
    }

    #[test]
    fn normalises_into_planes() {
        // A flat image keeps its value after resizing; the brightest value is 200.
        let img = RgbImage::from_pixel(8, 8, Rgb([200, 100, 0]));
        let input = preprocess(&img, FAST);
        assert_eq!(input.shape(), [1, 3, 4, 4]);
        let expect = |c: usize, v: f32| (v - MEAN[c]) / STD[c];
        assert!((input[[0, 0, 2, 1]] - expect(0, 1.0)).abs() < 1e-5);
        assert!((input[[0, 1, 0, 3]] - expect(1, 0.5)).abs() < 1e-5);
        assert!((input[[0, 2, 3, 0]] - expect(2, 0.0)).abs() < 1e-5);
        let by_255 = preprocess(
            &img,
            Spec {
                scale_by_max: false,
                ..FAST
            },
        );
        assert!((by_255[[0, 0, 0, 0]] - expect(0, 200.0 / 255.0)).abs() < 1e-5);
    }

    #[test]
    fn stretches_the_output_to_the_full_range() {
        let mut raw = vec![0.25f32; 16];
        raw[5] = 0.75;
        raw[6] = 0.5;
        let mask = postprocess(&raw, FAST);
        assert_eq!(mask.dimensions(), (4, 4));
        assert_eq!(mask.get_pixel(1, 1).0[0], 255);
        assert_eq!(mask.get_pixel(2, 1).0[0], 128);
        assert_eq!(mask.get_pixel(0, 0).0[0], 0);
        // A flat output must not divide by zero.
        assert!(postprocess(&[0.3; 16], FAST).pixels().all(|p| p.0[0] == 0));
    }

    #[test]
    fn applies_a_sigmoid_to_logits() {
        let mask = postprocess(
            &[-20.0, 0.0, 20.0, 0.0],
            Spec {
                size: 2,
                scale_by_max: false,
                sigmoid: true,
            },
        );
        assert_eq!(mask.as_raw(), &[0, 128, 255, 128]);
    }

    #[test]
    fn mask_becomes_alpha() {
        let mut original = RgbaImage::from_pixel(2, 1, Rgba([10, 20, 30, 255]));
        original.put_pixel(1, 0, Rgba([10, 20, 30, 128]));
        let mask = GrayImage::from_fn(2, 1, |_, _| Luma([128]));
        let out = compose(&original, &mask);
        assert_eq!(out.get_pixel(0, 0).0, [10, 20, 30, 128]);
        assert_eq!(out.get_pixel(1, 0).0, [10, 20, 30, 64]);
    }

    /// Runs the Fast model on the synthetic scenes and checks the masks against the exact
    /// ones. Needs the model: `KREOKIT_MODELS_DIR=<dir with u2net.onnx>
    /// cargo test --release cuts_out_synthetic_scenes -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cuts_out_synthetic_scenes() {
        use crate::synthetic::{iou, mae, scenes};
        let model = CutoutModel::Fast;
        let model_path = model.path_in(&crate::models::tests::dev_models_dir());
        let dir = std::env::temp_dir().join("kreokit-cutout-synthetic");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let engine = Engine::default();
        // Minimum IoU per scene: the first measured baseline (2026-10-13) minus a margin.
        // U²-Net fills the logo ring's hole (IoU 0.767): a known limit of the model, for
        // the refine brush (M3) or the Best model (M5) to fix.
        let minimum = |name: &str| if name == "logo" { 0.74 } else { 0.97 };
        let mut failures = Vec::new();
        for scene in scenes() {
            let path = dir.join(format!("{}.png", scene.name));
            scene.image.save(&path).unwrap();
            let result = cut_out(&engine, &model_path, &dir, model, &path, |_| {}).unwrap();
            let mask = image::open(&result.mask).unwrap().to_luma8();
            let (iou, mae) = (iou(&mask, &scene.mask), mae(&mask, &scene.mask));
            println!(
                "{:<13} IoU {iou:.3}  MAE {mae:.4}  inference {:.2}s  total {:.2}s",
                scene.name, result.timings.inference, result.timings.total
            );
            let cutout = image::open(&result.cutout).unwrap();
            assert_eq!(cutout.color(), image::ColorType::Rgba8);
            if iou < minimum(scene.name) {
                failures.push(format!("{}: IoU {iou:.3}", scene.name));
            }
        }
        assert!(failures.is_empty(), "below the minimum: {failures:?}");
    }

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/photos")
    }

    /// Regression check on the free-licensed photos in fixtures/photos against their
    /// *provisional* masks (made with BiRefNet, not ground truth): prints IoU, MAE and
    /// timings, and warns (never fails) when IoU drops more than 0.05 below
    /// `baseline-fast.json`. `KREOKIT_UPDATE_BASELINE=1` rewrites the baseline. Needs the
    /// model: `KREOKIT_MODELS_DIR=<dir> cargo test --release photo_regression -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore]
    fn photo_regression() {
        use crate::synthetic::{iou, mae};
        use std::collections::BTreeMap;
        let model = CutoutModel::Fast;
        let model_path = model.path_in(&crate::models::tests::dev_models_dir());
        let fixtures = fixtures_dir();
        let baseline_path = fixtures.join("baseline-fast.json");
        let baseline: BTreeMap<String, (f64, f64)> = fs::read_to_string(&baseline_path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let cache = std::env::temp_dir().join("kreokit-cutout-photos");
        let _ = fs::remove_dir_all(&cache);
        let engine = Engine::default();
        let mut photos: Vec<PathBuf> = fs::read_dir(&fixtures)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| images::has_image_extension(p))
            .collect();
        photos.sort();
        assert!(
            photos.len() >= 5,
            "expected the fixture photos in {}",
            fixtures.display()
        );

        let mut measured = BTreeMap::new();
        let mut warnings = Vec::new();
        println!("| Photo | Size | IoU | MAE | Inference | Total |\n|---|---|---|---|---|---|");
        for photo in &photos {
            let name = photo.file_name().unwrap().to_string_lossy().into_owned();
            let stem = photo.file_stem().unwrap().to_string_lossy();
            let reference = fixtures.join(format!("masks/{stem}.provisional-mask.png"));
            let reference = image::open(&reference).unwrap().to_luma8();
            let result = cut_out(&engine, &model_path, &cache, model, photo, |_| {}).unwrap();
            let mask = image::open(&result.mask).unwrap().to_luma8();
            let (i, m) = (iou(&mask, &reference), mae(&mask, &reference));
            let (w, h) = mask.dimensions();
            println!(
                "| {name} | {w}×{h} | {i:.3} | {m:.4} | {:.2} s | {:.2} s |",
                result.timings.inference, result.timings.total
            );
            if let Some(&(base, _)) = baseline.get(&name)
                && i < base - 0.05
            {
                warnings.push(format!("{name}: IoU {i:.3}, baseline {base:.3}"));
            }
            measured.insert(
                name,
                (
                    (i * 1000.0).round() / 1000.0,
                    (m * 10000.0).round() / 10000.0,
                ),
            );
        }
        for warning in &warnings {
            println!("WARNING: big drop against the provisional masks: {warning}");
        }
        if std::env::var("KREOKIT_UPDATE_BASELINE").as_deref() == Ok("1") {
            let json = serde_json::to_string_pretty(&measured).unwrap();
            fs::write(&baseline_path, json + "\n").unwrap();
            println!("wrote {}", baseline_path.display());
        }
    }

    /// Timings for a 12-megapixel photo (4032×3024, a common phone size), three runs.
    /// `KREOKIT_MODELS_DIR=<dir> cargo test --release timings_12_megapixels -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore]
    fn timings_12_megapixels() {
        let model = CutoutModel::Fast;
        let model_path = model.path_in(&crate::models::tests::dev_models_dir());
        let dir = std::env::temp_dir().join("kreokit-cutout-12mp");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let photo = image::open(fixtures_dir().join("product-mug.jpg")).unwrap();
        let big = dir.join("mug-12mp.jpg");
        photo
            .resize_exact(4032, 3024, FilterType::Triangle)
            .to_rgb8()
            .save(&big)
            .unwrap();
        let engine = Engine::default();
        for run in 1..=3 {
            let cache = dir.join(format!("cache-{run}"));
            let t = cut_out(&engine, &model_path, &cache, model, &big, |_| {})
                .unwrap()
                .timings;
            println!(
                "run {run}: load model {:.2}s, decode {:.2}s, preprocess {:.2}s, inference {:.2}s, \
                 postprocess {:.2}s, save {:.2}s, total {:.2}s",
                t.load_model, t.decode, t.preprocess, t.inference, t.postprocess, t.save, t.total
            );
        }
    }
}
