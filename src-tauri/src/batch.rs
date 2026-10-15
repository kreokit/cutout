//! "Export all": every image of a batch to one folder, with the same background and
//! export settings, each with its own model and edits. Nothing is overwritten unless the
//! user chose Replace; names that repeat get -2, -3… (export.rs).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::background::Background;
use crate::cutout;
use crate::export::{self, ExportSettings, WriteNew};
use crate::models::CutoutModel;
use crate::session::{Edits, Refiner};

/// One image to export.
#[derive(Debug, Clone, Deserialize)]
pub struct BatchItem {
    pub path: String,
    pub model: CutoutModel,
    pub edits: Edits,
}

/// What happened to one image.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ItemResult {
    Saved { path: String, dest: String },
    Failed { path: String, error: String },
    Skipped { path: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Progress {
    /// Image `index` (of `total`) is being exported.
    Started {
        index: usize,
        total: usize,
    },
    Finished {
        index: usize,
        result: ItemResult,
    },
}

/// Writes `bytes` to `dest`; without `replace`, an existing file is never touched: the
/// next free name (-2, -3…) is used instead.
fn write(dest: &Path, bytes: &[u8], replace: bool) -> Result<PathBuf, String> {
    if replace {
        export::write_atomic(dest, bytes)?;
        return Ok(dest.to_path_buf());
    }
    let stem = dest
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let ext = dest
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    for n in 1.. {
        let candidate = match n {
            1 => dest.to_path_buf(),
            n => dest.with_file_name(format!("{stem}-{n}.{ext}")),
        };
        if export::write_new(&candidate, bytes)? == WriteNew::Written {
            return Ok(candidate);
        }
    }
    unreachable!()
}

/// Exports `items` into `folder`. Stops before the next image once `cancel` is set (the
/// rest are reported as skipped).
#[allow(clippy::too_many_arguments)]
pub fn export_all(
    refiner: &Refiner,
    cache_dir: &Path,
    items: &[BatchItem],
    background: &Background,
    settings: &ExportSettings,
    folder: &Path,
    replace: bool,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Vec<ItemResult> {
    let paths: Vec<PathBuf> = items.iter().map(|i| PathBuf::from(&i.path)).collect();
    let dests = export::batch_destinations(folder, &paths, settings.format, !replace);
    let total = items.len();
    let mut results = Vec::with_capacity(total);
    for (index, (item, dest)) in items.iter().zip(&dests).enumerate() {
        let path = item.path.clone();
        let result = if cancel.load(Ordering::Relaxed) {
            ItemResult::Skipped { path }
        } else {
            on_progress(Progress::Started { index, total });
            let image = Path::new(&item.path);
            let saved = cutout::result_paths(cache_dir, image, item.model)
                .and_then(|(_, mask)| {
                    if mask.is_file() {
                        Ok(mask)
                    } else {
                        Err("this image is not cut out yet".to_string())
                    }
                })
                .and_then(|mask| refiner.export(image, &mask, &item.edits, background, settings))
                .and_then(|bytes| write(dest, &bytes, replace));
            match saved {
                Ok(dest) => ItemResult::Saved {
                    path,
                    dest: dest.to_string_lossy().into_owned(),
                },
                Err(error) => ItemResult::Failed { path, error },
            }
        };
        on_progress(Progress::Finished {
            index,
            result: result.clone(),
        });
        results.push(result);
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::Format;
    use crate::layout::Fit;
    use crate::refine::{BrushMode, EdgeSettings, Stroke};
    use image::{GrayImage, Luma, Rgba, RgbaImage};
    use std::fs;

    /// Images (two share a file name) with their masks where the app caches them.
    fn setup(name: &str) -> (PathBuf, PathBuf, Vec<BatchItem>) {
        let root = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&root);
        let cache = root.join("cache");
        let out = root.join("out");
        fs::create_dir_all(&out).unwrap();
        let mut items = Vec::new();
        for (i, rel) in ["a/cat.jpg", "b/cat.png", "c/dog.png"].iter().enumerate() {
            let path = root.join("in").join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let (w, h) = (60 + i as u32 * 10, 40);
            let fmt = if rel.ends_with(".jpg") {
                image::ImageFormat::Jpeg
            } else {
                image::ImageFormat::Png
            };
            image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, Rgba([200, 50, 50, 255])))
                .to_rgb8()
                .save_with_format(&path, fmt)
                .unwrap();
            let (_, mask_path) = cutout::result_paths(&cache, &path, CutoutModel::Fast).unwrap();
            fs::create_dir_all(mask_path.parent().unwrap()).unwrap();
            GrayImage::from_fn(w, h, |x, _| Luma([if x < w / 2 { 255 } else { 0 }]))
                .save(&mask_path)
                .unwrap();
            items.push(BatchItem {
                path: path.to_string_lossy().into_owned(),
                model: CutoutModel::Fast,
                edits: Edits {
                    edges: EdgeSettings::default(),
                    strokes: vec![],
                },
            });
        }
        // The dog has an erase stroke over its left edge.
        items[2].edits.strokes.push(Stroke {
            mode: BrushMode::Erase,
            radius: 6.0,
            softness: 0.0,
            points: vec![[3.0, 20.0]],
        });
        (cache, out, items)
    }

    fn png() -> ExportSettings {
        ExportSettings {
            format: Format::Png,
            quality: 90,
            size: None,
            fit: Fit::Fit,
        }
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn n_images_give_n_files_with_their_names_and_edits() {
        let (cache, out, items) = setup("kreokit-cutout-batch");
        let mut events = Vec::new();
        let results = export_all(
            &Refiner::default(),
            &cache,
            &items,
            &Background::Transparent,
            &png(),
            &out,
            false,
            &AtomicBool::new(false),
            |p| events.push(p),
        );
        assert_eq!(results.len(), 3);
        assert!(
            results
                .iter()
                .all(|r| matches!(r, ItemResult::Saved { .. })),
            "{results:?}"
        );
        assert_eq!(
            file_names(&out),
            ["cat-cutout-2.png", "cat-cutout.png", "dog-cutout.png"]
        );
        assert_eq!(events.len(), 6);

        // Each file is its own image, with its own edits.
        let cat = image::open(out.join("cat-cutout.png")).unwrap().to_rgba8();
        assert_eq!(cat.dimensions(), (60, 40));
        let dog = image::open(out.join("dog-cutout.png")).unwrap().to_rgba8();
        assert_eq!(dog.dimensions(), (80, 40));
        assert_eq!(dog.get_pixel(3, 20).0[3], 0, "the erase stroke");
        assert_eq!(dog.get_pixel(30, 20).0[3], 255);
        assert_eq!(cat.get_pixel(3, 20).0[3], 255, "no stroke on the cat");
    }

    #[test]
    fn existing_files_are_kept_unless_replace() {
        let (cache, out, items) = setup("kreokit-cutout-batch-existing");
        fs::write(out.join("dog-cutout.png"), b"keep me").unwrap();
        let run = |replace: bool| {
            export_all(
                &Refiner::default(),
                &cache,
                &items,
                &Background::Transparent,
                &png(),
                &out,
                replace,
                &AtomicBool::new(false),
                |_| {},
            )
        };
        assert_eq!(
            export::conflicts(
                &out,
                &items
                    .iter()
                    .map(|i| PathBuf::from(&i.path))
                    .collect::<Vec<_>>(),
                Format::Png
            ),
            [out.join("dog-cutout.png")]
        );
        // Keep both: the old file is untouched, the new one gets -2.
        let results = run(false);
        assert_eq!(fs::read(out.join("dog-cutout.png")).unwrap(), b"keep me");
        assert!(
            matches!(&results[2], ItemResult::Saved { dest, .. } if dest.ends_with("dog-cutout-2.png"))
        );
        // Replace: now it is overwritten.
        run(true);
        assert_ne!(fs::read(out.join("dog-cutout.png")).unwrap(), b"keep me");
        assert!(image::open(out.join("dog-cutout.png")).is_ok());
    }

    #[test]
    fn stops_and_reports_missing_cutouts() {
        let (cache, out, mut items) = setup("kreokit-cutout-batch-stop");
        items[1].model = CutoutModel::Best; // never cut out with Best
        let cancel = AtomicBool::new(false);
        let results = export_all(
            &Refiner::default(),
            &cache,
            &items,
            &Background::Transparent,
            &png(),
            &out,
            false,
            &cancel,
            |p| {
                if let Progress::Finished { index: 1, .. } = p {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );
        assert!(matches!(&results[0], ItemResult::Saved { .. }));
        assert!(
            matches!(&results[1], ItemResult::Failed { error, .. } if error.contains("not cut out"))
        );
        assert!(matches!(&results[2], ItemResult::Skipped { .. }));
        assert_eq!(file_names(&out), ["cat-cutout.png"]);
    }
}
