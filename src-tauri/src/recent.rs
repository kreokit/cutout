//! Recent images for the Home screen.
//!
//! The list is `recent.json` in the app data dir, newest first. Thumbnails are made by
//! `images::thumbnail`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How many images the Home screen keeps.
pub const MAX_RECENT: usize = 12;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: String,
    /// Last opened, in Unix milliseconds.
    pub opened_at: u64,
}

/// A list entry plus what the Home screen needs to show it.
#[derive(Debug, Clone, Serialize)]
pub struct RecentEntry {
    #[serde(flatten)]
    pub file: RecentFile,
    /// The image still exists.
    pub exists: bool,
    /// Path of a cached thumbnail, if one could be made.
    pub thumbnail: Option<String>,
}

fn list_file(dir: &Path) -> PathBuf {
    dir.join("recent.json")
}

/// The stored list, newest first. A missing or damaged file is an empty list.
pub fn load(dir: &Path) -> Vec<RecentFile> {
    fs::read_to_string(list_file(dir))
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn store(dir: &Path, list: &[RecentFile]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    fs::write(list_file(dir), json).map_err(|e| format!("cannot save recent files: {e}"))
}

/// Puts `paths` at the top of the list in the given order (adding or moving them),
/// capped at MAX_RECENT.
pub fn touch(dir: &Path, paths: &[String], now_ms: u64) -> Result<(), String> {
    let mut list = load(dir);
    list.retain(|f| !paths.contains(&f.path));
    let added = paths.iter().map(|path| RecentFile {
        path: path.clone(),
        opened_at: now_ms,
    });
    list.splice(0..0, added);
    list.truncate(MAX_RECENT);
    store(dir, &list)
}

pub fn remove(dir: &Path, path: &str) -> Result<(), String> {
    let mut list = load(dir);
    list.retain(|f| f.path != path);
    store(dir, &list)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("kreokit-cutout-recent-tests")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn paths(dir: &Path) -> Vec<String> {
        load(dir).into_iter().map(|f| f.path).collect()
    }

    #[test]
    fn newest_first_without_duplicates() {
        let dir = temp_dir("order");
        touch(&dir, &["/a.png".into()], 1).unwrap();
        touch(&dir, &["/b.png".into()], 2).unwrap();
        touch(&dir, &["/a.png".into()], 3).unwrap();
        assert_eq!(paths(&dir), ["/a.png", "/b.png"]);
        assert_eq!(load(&dir)[0].opened_at, 3);
    }

    #[test]
    fn a_batch_keeps_its_order_at_the_top() {
        let dir = temp_dir("batch");
        touch(&dir, &["/old.png".into(), "/b.png".into()], 1).unwrap();
        touch(
            &dir,
            &["/a.png".into(), "/b.png".into(), "/c.png".into()],
            2,
        )
        .unwrap();
        assert_eq!(paths(&dir), ["/a.png", "/b.png", "/c.png", "/old.png"]);
    }

    #[test]
    fn keeps_at_most_twelve() {
        let dir = temp_dir("cap");
        for i in 0..20 {
            touch(&dir, &[format!("/{i}.png")], i).unwrap();
        }
        let list = load(&dir);
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].path, "/19.png");
    }

    #[test]
    fn removes_and_survives_damage() {
        let dir = temp_dir("remove");
        touch(&dir, &["/a.png".into()], 1).unwrap();
        remove(&dir, "/a.png").unwrap();
        assert!(load(&dir).is_empty());
        fs::write(list_file(&dir), "not json").unwrap();
        assert!(load(&dir).is_empty());
    }
}
