//! AI model catalog, which file a computer uses, and the one-time model download.
//!
//! Models are downloaded only from our own mirror releases on kreokit/cutout: `models-v1`
//! (Fast, U²-Net) and `models-v2` (Best, BiRefNet_lite exported at 768 and 640 by
//! scripts/export_birefnet.py). Sources, licences and attribution are listed there and in
//! THIRD-PARTY-LICENSES.md. Files are stored in `<app data dir>/models/` and checked against
//! a pinned size and SHA-256. This download is the app's only network call.
//!
//! **Best and memory.** BiRefNet_lite peaks at about 3.4 GB at 768² and 2.5 GB at 640²
//! (5.9 GB at 1024², which isn't used). Computers with more than 8 GB use 768, others 640.
//! Before each Best cutout, [`plan`] checks the memory free right then: 768 needs
//! [`BEST_768_NEEDS`], else 640 ([`BEST_640_NEEDS`]) is used, downloaded only when first
//! needed; if even that doesn't fit, the user is warned (Use Fast / Try anyway).

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::memory::Memory;

pub const CANCELLED: &str = "cancelled";

const RELEASES: &str = "https://github.com/kreokit/cutout/releases/download";

/// Free memory Best needs at 768² (peak 3.44 GB measured, plus room for the app).
pub const BEST_768_NEEDS: u64 = 4_000_000_000;
/// Free memory Best needs at 640² (peak 2.47 GB measured, plus room for the app).
pub const BEST_640_NEEDS: u64 = 3_000_000_000;
/// Computers with more total memory than this ("more than 8 GB") use Best at 768. 8 GB
/// machines report 8 GiB (8.6 GB).
pub const LARGE_MEMORY: u64 = 9_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CutoutModel {
    /// U²-Net, 320×320 input. Apache-2.0.
    Fast,
    /// BiRefNet_lite at 768×768 or 640×640. MIT.
    Best,
}

impl CutoutModel {
    pub const ALL: [CutoutModel; 2] = [Self::Fast, Self::Best];

    /// The file this computer uses for the model (by its total memory; unknown counts as
    /// small).
    pub fn main_file(self, total_memory: Option<u64>) -> ModelFile {
        match self {
            Self::Fast => ModelFile::U2net,
            Self::Best if total_memory.is_some_and(|t| t > LARGE_MEMORY) => ModelFile::BiRefNet768,
            Self::Best => ModelFile::BiRefNet640,
        }
    }
}

/// A model file on the mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelFile {
    U2net,
    #[serde(rename = "birefnet-768")]
    BiRefNet768,
    #[serde(rename = "birefnet-640")]
    BiRefNet640,
}

impl ModelFile {
    #[cfg(test)]
    pub const ALL: [ModelFile; 3] = [Self::U2net, Self::BiRefNet768, Self::BiRefNet640];

    pub fn model(self) -> CutoutModel {
        match self {
            Self::U2net => CutoutModel::Fast,
            Self::BiRefNet768 | Self::BiRefNet640 => CutoutModel::Best,
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::U2net => "u2net.onnx",
            Self::BiRefNet768 => "birefnet_lite_768.onnx",
            Self::BiRefNet640 => "birefnet_lite_640.onnx",
        }
    }

    /// The square input size the file was exported for.
    pub fn input_size(self) -> u32 {
        match self {
            Self::U2net => 320,
            Self::BiRefNet768 => 768,
            Self::BiRefNet640 => 640,
        }
    }

    /// Size and SHA-256 of the mirrored files (see SHA256SUMS.txt in each release).
    pub fn size_bytes(self) -> u64 {
        match self {
            Self::U2net => 175_997_641,
            Self::BiRefNet768 => 205_065_982,
            Self::BiRefNet640 => 197_800_690,
        }
    }

    fn sha256(self) -> &'static str {
        match self {
            Self::U2net => "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491",
            Self::BiRefNet768 => "7677ee31fde9788af5ca553e1221c948b5fbc5053be204a8e715dba60f421e29",
            Self::BiRefNet640 => "44432bf3426945bd06d6c75a1ab41d791205141bc4a101fb63bb62543bafdb5a",
        }
    }

    fn url(self) -> String {
        let release = match self {
            Self::U2net => "models-v1",
            Self::BiRefNet768 | Self::BiRefNet640 => "models-v2",
        };
        format!("{RELEASES}/{release}/{}", self.file_name())
    }

    pub fn path_in(self, models_dir: &Path) -> PathBuf {
        models_dir.join(self.file_name())
    }

    /// A file counts as downloaded once it is complete. Downloads land in a `.part` file
    /// and are only renamed after the checksum matches.
    pub fn is_downloaded(self, models_dir: &Path) -> bool {
        fs::metadata(self.path_in(models_dir)).is_ok_and(|m| m.len() == self.size_bytes())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: CutoutModel,
    /// The file this computer uses for it.
    pub file: ModelFile,
    pub size_bytes: u64,
    pub downloaded: bool,
}

pub fn list(models_dir: &Path, total_memory: Option<u64>) -> Vec<ModelInfo> {
    CutoutModel::ALL
        .iter()
        .map(|&id| {
            let file = id.main_file(total_memory);
            ModelInfo {
                id,
                file,
                size_bytes: file.size_bytes(),
                downloaded: file.is_downloaded(models_dir),
            }
        })
        .collect()
}

/// What to do for a cutout with a model, given the files and the memory right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Plan {
    Run {
        file: ModelFile,
    },
    /// Download `file` first (the model's own file, or Best's smaller fallback).
    Download {
        file: ModelFile,
        size_bytes: u64,
    },
    /// Not enough free memory even for the smallest file: warn (Use Fast / Try anyway).
    LowMemory {
        available_bytes: u64,
        needed_bytes: u64,
    },
}

/// The plan for a cutout with `model`. With `force` ("Try anyway"), runs the smallest
/// downloaded file even when memory is short.
pub fn plan(model: CutoutModel, models_dir: &Path, memory: Option<Memory>, force: bool) -> Plan {
    let main = model.main_file(memory.map(|m| m.total_bytes));
    let download = |file: ModelFile| Plan::Download {
        file,
        size_bytes: file.size_bytes(),
    };
    if !main.is_downloaded(models_dir) {
        return download(main);
    }
    if model == CutoutModel::Fast {
        return Plan::Run { file: main };
    }
    let available = memory.map_or(u64::MAX, |m| m.available_bytes);
    let small = ModelFile::BiRefNet640;
    if main == ModelFile::BiRefNet768 && available >= BEST_768_NEEDS {
        return Plan::Run { file: main };
    }
    if available >= BEST_640_NEEDS {
        return if small.is_downloaded(models_dir) {
            Plan::Run { file: small }
        } else {
            download(small)
        };
    }
    if force {
        let file = if small.is_downloaded(models_dir) {
            small
        } else {
            main
        };
        return Plan::Run { file };
    }
    Plan::LowMemory {
        available_bytes: available,
        needed_bytes: BEST_640_NEEDS,
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// Downloads `model` (a file) into `models_dir`, verifying size and SHA-256.
/// `on_progress` is called roughly every 1 MB. Returns [`CANCELLED`] if `cancel` is set.
pub fn download(
    model: ModelFile,
    models_dir: &Path,
    cancel: &AtomicBool,
    on_progress: impl FnMut(DownloadProgress),
) -> Result<PathBuf, String> {
    let target = model.path_in(models_dir);
    if model.is_downloaded(models_dir) {
        return Ok(target);
    }
    fs::create_dir_all(models_dir).map_err(|e| format!("cannot create models folder: {e}"))?;
    let part = target.with_extension("onnx.part");

    let result = (|| {
        let response = ureq::get(&model.url())
            .call()
            .map_err(|e| format!("model download failed: {e}"))?;
        let reader = response.into_body().into_reader();
        let file = File::create(&part).map_err(|e| format!("cannot write model file: {e}"))?;
        save_verified(
            reader,
            file,
            model.size_bytes(),
            model.sha256(),
            cancel,
            on_progress,
        )
        .map_err(|e| match e.as_str() {
            CORRUPT => format!(
                "the downloaded {} is damaged; please try again",
                model.file_name()
            ),
            _ => e,
        })?;
        fs::rename(&part, &target).map_err(|e| format!("cannot save model file: {e}"))?;
        Ok(target.clone())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

const CORRUPT: &str = "corrupt";

/// Copies `reader` into `file`, reporting progress, and checks the size and SHA-256 at
/// the end ([`CORRUPT`] if they don't match).
fn save_verified(
    mut reader: impl Read,
    file: File,
    total_bytes: u64,
    sha256: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(DownloadProgress),
) -> Result<(), String> {
    let mut file = BufWriter::new(file);
    let mut hasher = Sha256::new();
    let mut downloaded_bytes = 0u64;
    let mut last_reported = 0u64;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.to_string());
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("model download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| format!("cannot write model file: {e}"))?;
        downloaded_bytes += n as u64;
        if downloaded_bytes - last_reported >= 1 << 20 || downloaded_bytes == total_bytes {
            last_reported = downloaded_bytes;
            on_progress(DownloadProgress {
                downloaded_bytes,
                total_bytes,
            });
        }
    }
    file.flush()
        .map_err(|e| format!("cannot write model file: {e}"))?;
    if downloaded_bytes != total_bytes || to_hex(&hasher.finalize()) != sha256 {
        return Err(CORRUPT.into());
    }
    Ok(())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The app data dir on macOS, used by the ignored tests that need a real model.
    /// Override with `KREOKIT_MODELS_DIR`.
    pub(crate) fn dev_models_dir() -> PathBuf {
        std::env::var_os("KREOKIT_MODELS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap())
                    .join("Library/Application Support/com.kreokit.cutout/models")
            })
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("kreokit-cutout-models-tests")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn model_ids_match_the_frontend() {
        let ids: Vec<String> = CutoutModel::ALL
            .iter()
            .map(|m| serde_json::to_string(m).unwrap())
            .collect();
        assert_eq!(ids, [r#""fast""#, r#""best""#]);
    }

    #[test]
    fn downloads_only_from_our_mirror() {
        assert_eq!(
            ModelFile::U2net.url(),
            "https://github.com/kreokit/cutout/releases/download/models-v1/u2net.onnx"
        );
        assert_eq!(
            ModelFile::BiRefNet768.url(),
            "https://github.com/kreokit/cutout/releases/download/models-v2/birefnet_lite_768.onnx"
        );
        assert_eq!(
            ModelFile::BiRefNet640.url(),
            "https://github.com/kreokit/cutout/releases/download/models-v2/birefnet_lite_640.onnx"
        );
        let ids: Vec<String> = ModelFile::ALL
            .iter()
            .map(|f| serde_json::to_string(f).unwrap())
            .collect();
        assert_eq!(
            ids,
            [r#""u2net""#, r#""birefnet-768""#, r#""birefnet-640""#]
        );
    }

    #[test]
    fn partial_file_is_not_downloaded() {
        let dir = temp_dir("partial");
        fs::write(ModelFile::U2net.path_in(&dir), b"truncated").unwrap();
        assert!(!ModelFile::U2net.is_downloaded(&dir));
        assert!(!list(&dir, None)[0].downloaded);
    }

    const GB: u64 = 1_000_000_000;

    fn mem(total_gb: f64, available_gb: f64) -> Option<Memory> {
        Some(Memory {
            total_bytes: (total_gb * GB as f64) as u64,
            available_bytes: (available_gb * GB as f64) as u64,
        })
    }

    /// A stand-in file of the right size (`is_downloaded` checks the size).
    fn fake(dir: &Path, file: ModelFile) {
        let f = File::create(file.path_in(dir)).unwrap();
        f.set_len(file.size_bytes()).unwrap();
    }

    #[test]
    fn best_uses_768_above_8_gb_and_640_otherwise() {
        let eight = 8.0 * 1.073_741_824; // 8 GiB, as 8 GB machines report it
        assert_eq!(
            CutoutModel::Best.main_file(mem(16.0, 1.0).map(|m| m.total_bytes)),
            ModelFile::BiRefNet768
        );
        assert_eq!(
            CutoutModel::Best.main_file(mem(12.0, 1.0).map(|m| m.total_bytes)),
            ModelFile::BiRefNet768
        );
        assert_eq!(
            CutoutModel::Best.main_file(mem(eight, 1.0).map(|m| m.total_bytes)),
            ModelFile::BiRefNet640
        );
        assert_eq!(CutoutModel::Best.main_file(None), ModelFile::BiRefNet640);
        assert_eq!(CutoutModel::Fast.main_file(Some(4 * GB)), ModelFile::U2net);
        // The list shows the file this computer downloads.
        let dir = temp_dir("list");
        let best = &list(&dir, Some(16 * GB))[1];
        assert_eq!(
            (best.file, best.size_bytes, best.downloaded),
            (ModelFile::BiRefNet768, 205_065_982, false)
        );
    }

    #[test]
    fn plans_best_by_free_memory() {
        use ModelFile::*;
        let dir = temp_dir("plan");
        let best = |m, force| plan(CutoutModel::Best, &dir, m, force);
        let dl = |file: ModelFile| Plan::Download {
            file,
            size_bytes: file.size_bytes(),
        };

        // Nothing downloaded: the computer's own file first.
        assert_eq!(best(mem(16.0, 10.0), false), dl(BiRefNet768));
        assert_eq!(best(mem(8.0, 5.0), false), dl(BiRefNet640));

        // A big computer with 768: plenty free runs it; less free wants 640 first.
        fake(&dir, BiRefNet768);
        assert_eq!(best(mem(16.0, 5.0), false), Plan::Run { file: BiRefNet768 });
        assert_eq!(best(mem(16.0, 3.5), false), dl(BiRefNet640));
        assert_eq!(
            best(mem(16.0, 2.0), false),
            Plan::LowMemory {
                available_bytes: 2 * GB,
                needed_bytes: BEST_640_NEEDS
            }
        );
        // Try anyway with only 768 downloaded runs 768.
        assert_eq!(best(mem(16.0, 2.0), true), Plan::Run { file: BiRefNet768 });

        // With 640 there too: it's the fallback, and what Try anyway runs.
        fake(&dir, BiRefNet640);
        assert_eq!(best(mem(16.0, 3.5), false), Plan::Run { file: BiRefNet640 });
        assert_eq!(best(mem(16.0, 2.0), true), Plan::Run { file: BiRefNet640 });

        // An 8 GB computer only ever uses 640.
        assert_eq!(best(mem(8.0, 6.0), false), Plan::Run { file: BiRefNet640 });
        assert!(matches!(best(mem(8.0, 2.5), false), Plan::LowMemory { .. }));
        assert_eq!(best(mem(8.0, 2.5), true), Plan::Run { file: BiRefNet640 });

        // Unknown memory: run (nothing to go by).
        assert_eq!(best(None, false), Plan::Run { file: BiRefNet640 });

        // Fast never checks memory.
        assert_eq!(
            plan(CutoutModel::Fast, &dir, mem(8.0, 0.5), false),
            dl(U2net)
        );
        fake(&dir, U2net);
        assert_eq!(
            plan(CutoutModel::Fast, &dir, mem(8.0, 0.5), false),
            Plan::Run { file: U2net }
        );
    }

    #[test]
    fn verifies_size_and_checksum() {
        let dir = temp_dir("verify");
        let data = b"hello model";
        let sha = "a1c8ae6c0f2b2dd3b6f5c64a4eb24a8d1c6c3bd8a2bf1a2e4d1ddc6b0b5c6f45"; // wrong
        let good = to_hex(&Sha256::digest(data));
        let cancel = AtomicBool::new(false);
        let mut reports = Vec::new();
        let file = || File::create(dir.join("m.part")).unwrap();
        save_verified(&data[..], file(), 11, &good, &cancel, |p| {
            reports.push(p.downloaded_bytes)
        })
        .unwrap();
        assert_eq!(reports, [11]);
        assert_eq!(fs::read(dir.join("m.part")).unwrap(), data);
        assert_eq!(
            save_verified(&data[..], file(), 11, sha, &cancel, |_| {}).unwrap_err(),
            CORRUPT
        );
        assert_eq!(
            save_verified(&data[..], file(), 12, &good, &cancel, |_| {}).unwrap_err(),
            CORRUPT
        );
        cancel.store(true, Ordering::Relaxed);
        assert_eq!(
            save_verified(&data[..], file(), 11, &good, &cancel, |_| {}).unwrap_err(),
            CANCELLED
        );
    }

    #[test]
    fn hex_encodes() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xab]), "000fab");
    }

    /// Downloads the Fast model (176 MB) from the mirror into the app data dir:
    /// `cargo test downloads_fast_model -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_fast_model() {
        let cancel = AtomicBool::new(false);
        let path = download(ModelFile::U2net, &dev_models_dir(), &cancel, |_| {}).unwrap();
        assert!(ModelFile::U2net.is_downloaded(path.parent().unwrap()));
    }
}
