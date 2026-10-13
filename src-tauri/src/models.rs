//! AI model catalog and the one-time model download.
//!
//! Models are downloaded only from our own mirror release `models-v1` on kreokit/cutout
//! (sources, licences and attribution are listed there and in THIRD-PARTY-LICENSES.md),
//! stored in `<app data dir>/models/` and checked against a pinned size and SHA-256.
//! This download is the app's only network call.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CANCELLED: &str = "cancelled";

const MIRROR: &str = "https://github.com/kreokit/cutout/releases/download/models-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CutoutModel {
    /// U²-Net, 320×320 input. Apache-2.0.
    Fast,
    /// BiRefNet_lite, 1024×1024 input. MIT.
    Best,
}

impl CutoutModel {
    pub const ALL: [CutoutModel; 2] = [Self::Fast, Self::Best];

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Fast => "u2net.onnx",
            Self::Best => "birefnet_lite.onnx",
        }
    }

    /// Size and SHA-256 of the mirrored files (see SHA256SUMS.txt in the release).
    fn size_bytes(self) -> u64 {
        match self {
            Self::Fast => 175_997_641,
            Self::Best => 224_005_088,
        }
    }

    fn sha256(self) -> &'static str {
        match self {
            Self::Fast => "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491",
            Self::Best => "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333",
        }
    }

    fn url(self) -> String {
        format!("{MIRROR}/{}", self.file_name())
    }

    pub fn path_in(self, models_dir: &Path) -> PathBuf {
        models_dir.join(self.file_name())
    }

    /// A model counts as downloaded once the complete file is in place. Downloads land
    /// in a `.part` file and are only renamed after the checksum matches.
    pub fn is_downloaded(self, models_dir: &Path) -> bool {
        fs::metadata(self.path_in(models_dir)).is_ok_and(|m| m.len() == self.size_bytes())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: CutoutModel,
    pub size_bytes: u64,
    pub downloaded: bool,
}

pub fn list(models_dir: &Path) -> Vec<ModelInfo> {
    CutoutModel::ALL
        .iter()
        .map(|&id| ModelInfo {
            id,
            size_bytes: id.size_bytes(),
            downloaded: id.is_downloaded(models_dir),
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// Downloads `model` into `models_dir`, verifying size and SHA-256.
/// `on_progress` is called roughly every 1 MB. Returns [`CANCELLED`] if `cancel` is set.
pub fn download(
    model: CutoutModel,
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
            CutoutModel::Fast.url(),
            "https://github.com/kreokit/cutout/releases/download/models-v1/u2net.onnx"
        );
        assert!(CutoutModel::Best.url().starts_with(MIRROR));
    }

    #[test]
    fn partial_file_is_not_downloaded() {
        let dir = temp_dir("partial");
        fs::write(CutoutModel::Fast.path_in(&dir), b"truncated").unwrap();
        assert!(!CutoutModel::Fast.is_downloaded(&dir));
        assert!(!list(&dir)[0].downloaded);
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
        let path = download(CutoutModel::Fast, &dev_models_dir(), &cancel, |_| {}).unwrap();
        assert!(CutoutModel::Fast.is_downloaded(path.parent().unwrap()));
    }
}
