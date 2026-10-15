//! `cutout --self-test <image> [--reference <mask>] [--models <dir>]`: a hidden mode for
//! release CI. Inside the shipped binary (ONNX Runtime, libwebp, release build flags) it
//!
//! 1. downloads Fast (U²-Net) and Best's 640 file into `--models` if they aren't there
//!    (which also exercises the download and checksum code),
//! 2. cuts `<image>` out with Fast and with Best 640, checking each mask's IoU against
//!    `--reference`,
//! 3. runs Best's memory check and size selection (models::plan) for this computer and for
//!    an 8 GB one (which must pick 640),
//! 4. exports through the real pipeline (PNG with alpha, JPG on a colour, WebP with Fill,
//!    a blur background) and decodes each file to check it,
//!
//! and exits non-zero on any failure. It runs before Tauri starts, so it needs no window
//! or display.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use image::GrayImage;

use crate::background::Background;
use crate::encode::Format;
use crate::export::ExportSettings;
use crate::layout::{Fit, Size};
use crate::memory::{self, Memory};
use crate::models::{self, CutoutModel, ModelFile, Plan};
use crate::refine::EdgeSettings;
use crate::session::{Edits, Refiner};
use crate::{cpu, cutout};

/// The lowest IoU against the reference mask that still passes (Fast scored 0.985 on the
/// mug fixture, Best 0.996).
const MIN_IOU_FAST: f64 = 0.90;
const MIN_IOU_BEST: f64 = 0.95;

/// Runs the self-test if `args` (without the program name) start with `--self-test`.
pub fn run_if_requested(args: &[String]) -> Option<ExitCode> {
    if args.first().map(String::as_str) != Some("--self-test") {
        return None;
    }
    attach_console();
    Some(match parse(&args[1..]).and_then(|options| run(&options)) {
        Ok(()) => {
            println!("self-test: ok");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("self-test: FAILED: {e}");
            ExitCode::FAILURE
        }
    })
}

/// Release builds on Windows have no console; show the output in the one that started us.
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: a plain Win32 call; failing (no parent console) is harmless.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}

#[derive(Debug, PartialEq)]
struct SelfTestOptions {
    image: PathBuf,
    reference: Option<PathBuf>,
    models_dir: PathBuf,
}

fn parse(args: &[String]) -> Result<SelfTestOptions, String> {
    let usage = "usage: cutout --self-test <image> [--reference <mask>] [--models <dir>]";
    let mut image = None;
    let mut reference = None;
    let mut models_dir = std::env::temp_dir().join("kreokit-self-test-models");
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--reference" => reference = Some(PathBuf::from(args.next().ok_or(usage)?)),
            "--models" => models_dir = PathBuf::from(args.next().ok_or(usage)?),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}; {usage}")),
            _ if image.is_none() => image = Some(PathBuf::from(arg)),
            _ => return Err(usage.into()),
        }
    }
    Ok(SelfTestOptions {
        image: image.ok_or(usage)?,
        reference,
        models_dir,
    })
}

/// Intersection over union of the foregrounds (alpha ≥ 128) of two masks.
fn iou(a: &GrayImage, b: &GrayImage) -> f64 {
    let (mut both, mut either) = (0u64, 0u64);
    for (p, q) in a.pixels().zip(b.pixels()) {
        let (p, q) = (p.0[0] >= 128, q.0[0] >= 128);
        both += (p && q) as u64;
        either += (p || q) as u64;
    }
    if either == 0 {
        1.0
    } else {
        both as f64 / either as f64
    }
}

fn ensure_downloaded(file: ModelFile, dir: &Path) -> Result<(), String> {
    if file.is_downloaded(dir) {
        println!("model:   {} (already there)", file.file_name());
        return Ok(());
    }
    let started = Instant::now();
    models::download(file, dir, &AtomicBool::new(false), |_| {})?;
    println!(
        "model:   {} downloaded and verified in {:.1?}",
        file.file_name(),
        started.elapsed()
    );
    Ok(())
}

/// Cuts out `options.image` with `file` and checks the mask; returns the mask's path.
fn check_cutout(
    engine: &cutout::Engine,
    options: &SelfTestOptions,
    cache: &Path,
    file: ModelFile,
    min_iou: f64,
) -> Result<PathBuf, String> {
    let started = Instant::now();
    let result = cutout::cut_out(
        engine,
        &options.models_dir,
        cache,
        file,
        &options.image,
        |_| {},
    )?;
    if result.cached {
        return Err("the cutout came from the cache; the self-test must run the model".into());
    }
    let mask = image::open(&result.mask)
        .map_err(|e| format!("cannot read the mask: {e}"))?
        .to_luma8();
    let covered = mask.pixels().filter(|p| p.0[0] >= 128).count() as f64 / mask.len() as f64;
    println!(
        "{:<8} {}×{} in {:.1?} (inference {:.2}s), foreground {:.0}%",
        format!("{:?}:", file.model()),
        mask.width(),
        mask.height(),
        started.elapsed(),
        result.timings.inference,
        covered * 100.0
    );
    if !(0.01..0.99).contains(&covered) {
        return Err(format!("{}: the mask is empty or full", file.file_name()));
    }
    if let Some(reference) = &options.reference {
        let reference = image::open(reference)
            .map_err(|e| format!("cannot read {}: {e}", reference.display()))?
            .to_luma8();
        let i = iou(&mask, &reference);
        println!("         IoU {i:.3} against the reference (min {min_iou})");
        if i < min_iou {
            return Err(format!(
                "{}: IoU {i:.3} is below {min_iou}",
                file.file_name()
            ));
        }
    }
    Ok(PathBuf::from(result.mask))
}

/// Best's memory check and size selection: for this computer (printed), and for an
/// 8 GB computer with this computer's free memory, which must run the 640 file (with
/// "Try anyway" if the free memory is short).
fn check_best_plan(models_dir: &Path) -> Result<(), String> {
    let mem = memory::memory();
    match mem {
        Some(m) => println!(
            "memory:  {:.1} GB total, {:.1} GB free",
            m.total_bytes as f64 / 1e9,
            m.available_bytes as f64 / 1e9
        ),
        None => println!("memory:  unknown on this system"),
    }
    let main = CutoutModel::Best.main_file(mem.map(|m| m.total_bytes));
    println!("Best:    this computer uses {}", main.file_name());
    println!(
        "plan:    {:?}",
        models::plan(CutoutModel::Best, models_dir, mem, false)
    );
    let eight = Some(Memory {
        total_bytes: 8 << 30,
        available_bytes: mem.map_or(u64::MAX, |m| m.available_bytes),
    });
    let plan = match models::plan(CutoutModel::Best, models_dir, eight, false) {
        Plan::LowMemory {
            available_bytes, ..
        } => {
            println!(
                "plan:    8 GB computer: low memory ({:.1} GB free) → Try anyway",
                available_bytes as f64 / 1e9
            );
            models::plan(CutoutModel::Best, models_dir, eight, true)
        }
        plan => plan,
    };
    println!("plan:    8 GB computer: {plan:?}");
    if plan
        != (Plan::Run {
            file: ModelFile::BiRefNet640,
        })
    {
        return Err(format!(
            "an 8 GB computer should run Best at 640, not {plan:?}"
        ));
    }
    Ok(())
}

/// Exports `image` with its `mask` through the real pipeline and checks each file.
fn check_exports(image: &Path, mask: &Path) -> Result<(), String> {
    let refiner = Refiner::default();
    let edits = Edits {
        edges: EdgeSettings::default(),
        strokes: vec![],
    };
    let (w, h) = image::image_dimensions(image).map_err(|e| e.to_string())?;
    let cases = [
        (
            "PNG, transparent",
            Format::Png,
            None,
            Fit::Fit,
            Background::Transparent,
        ),
        (
            "JPG 1080×1080, colour",
            Format::Jpg,
            Some((1080, 1080)),
            Fit::Fit,
            Background::Color {
                color: [61, 139, 255],
            },
        ),
        (
            "WebP 1080×1350, Fill",
            Format::Webp,
            Some((1080, 1350)),
            Fit::Fill,
            Background::Transparent,
        ),
        (
            "PNG, blur",
            Format::Png,
            None,
            Fit::Fit,
            Background::Blur { strength: 40.0 },
        ),
    ];
    for (label, format, size, fit, background) in cases {
        let settings = ExportSettings {
            format,
            quality: 90,
            size: size.map(|(width, height)| Size { width, height }),
            fit,
        };
        let started = Instant::now();
        let bytes = refiner.export(image, mask, &edits, &background, &settings)?;
        let out = image::load_from_memory(&bytes)
            .map_err(|e| format!("{label}: the exported file doesn't decode: {e}"))?;
        let want = size.unwrap_or((w, h));
        if (out.width(), out.height()) != want {
            return Err(format!(
                "{label}: {}×{}, expected {}×{}",
                out.width(),
                out.height(),
                want.0,
                want.1
            ));
        }
        let alpha = out.color().has_alpha();
        let want_alpha = matches!(background, Background::Transparent) && format != Format::Jpg;
        if alpha != want_alpha {
            return Err(format!(
                "{label}: alpha channel {alpha}, expected {want_alpha}"
            ));
        }
        println!(
            "export:  {label:<24} {}×{} {} KB in {:.1?}",
            out.width(),
            out.height(),
            bytes.len() / 1000,
            started.elapsed()
        );
    }
    Ok(())
}

fn run(options: &SelfTestOptions) -> Result<(), String> {
    let support = cpu::check();
    if !support.supported {
        return Err(cpu::unsupported_message(&support.missing));
    }
    println!("image:   {}", options.image.display());
    // A fresh cache, so both models really run.
    let cache =
        std::env::temp_dir().join(format!("kreokit-self-test-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let result = (|| {
        let engine = cutout::Engine::default();
        ensure_downloaded(ModelFile::U2net, &options.models_dir)?;
        let fast_mask = check_cutout(&engine, options, &cache, ModelFile::U2net, MIN_IOU_FAST)?;

        ensure_downloaded(ModelFile::BiRefNet640, &options.models_dir)?;
        check_best_plan(&options.models_dir)?;
        check_cutout(
            &engine,
            options,
            &cache,
            ModelFile::BiRefNet640,
            MIN_IOU_BEST,
        )?;
        drop(engine); // free Best's memory before exporting

        check_exports(&options.image, &fast_mask)
    })();
    let _ = std::fs::remove_dir_all(&cache);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_the_arguments() {
        assert_eq!(
            parse(&args(&[
                "mug.jpg",
                "--reference",
                "mug.png",
                "--models",
                "/m"
            ])),
            Ok(SelfTestOptions {
                image: "mug.jpg".into(),
                reference: Some("mug.png".into()),
                models_dir: "/m".into(),
            })
        );
        let default = parse(&args(&["mug.jpg"])).unwrap();
        assert_eq!(default.reference, None);
        assert!(default.models_dir.ends_with("kreokit-self-test-models"));
        assert!(parse(&args(&[])).is_err());
        assert!(parse(&args(&["a.jpg", "b.jpg"])).is_err());
        assert!(
            parse(&args(&["a.jpg", "--fast"]))
                .unwrap_err()
                .contains("unknown flag")
        );
        assert!(parse(&args(&["a.jpg", "--models"])).is_err());
    }

    #[test]
    fn only_the_flag_starts_it() {
        assert!(run_if_requested(&args(&[])).is_none());
        assert!(run_if_requested(&args(&["photo.jpg"])).is_none());
    }

    #[test]
    fn iou_of_masks() {
        let a = GrayImage::from_fn(4, 1, |x, _| image::Luma([if x < 2 { 255 } else { 0 }]));
        let b = GrayImage::from_fn(4, 1, |x, _| image::Luma([if x < 3 { 255 } else { 0 }]));
        assert!((iou(&a, &b) - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(iou(&GrayImage::new(2, 2), &GrayImage::new(2, 2)), 1.0);
    }
}
