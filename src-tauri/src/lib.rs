mod images;
mod models;
mod recent;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use images::ImageInfo;
use models::{CutoutModel, DownloadProgress, ModelInfo};

/// Set by `cancel`; checked by model downloads.
#[derive(Default)]
struct CancelFlag(Arc<AtomicBool>);

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("cannot find app data folder: {e}"))
}

fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("models"))
}

/// Runs blocking work (decoding, file I/O) off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct Opened {
    images: Vec<ImageInfo>,
    /// One message per file that couldn't be opened.
    errors: Vec<String>,
}

/// Opens the images at `paths` (dropped or picked): checks each one, lets the webview
/// show the good ones (the asset protocol only serves files the user opened) and puts
/// them at the top of the recent list.
#[tauri::command]
async fn open_images(app: AppHandle, paths: Vec<String>) -> Result<Opened, String> {
    let dir = data_dir(&app)?;
    let opened = blocking(move || {
        let (mut images, mut errors) = (Vec::new(), Vec::new());
        for path in &paths {
            match images::inspect(Path::new(path)) {
                Ok(info) => images.push(info),
                Err(e) => errors.push(e),
            }
        }
        let good: Vec<String> = images.iter().map(|i| i.path.clone()).collect();
        if !good.is_empty() {
            recent::touch(&dir, &good, recent::now_ms())?;
        }
        Ok(Opened { images, errors })
    })
    .await?;
    for image in &opened.images {
        app.asset_protocol_scope()
            .allow_file(&image.path)
            .map_err(|e| e.to_string())?;
    }
    Ok(opened)
}

#[tauri::command]
fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    Ok(models::list(&models_dir(&app)?))
}

#[tauri::command]
async fn download_model(
    app: AppHandle,
    cancel: State<'_, CancelFlag>,
    model: CutoutModel,
    on_progress: Channel<DownloadProgress>,
) -> Result<(), String> {
    let dir = models_dir(&app)?;
    let cancel = cancel.0.clone();
    cancel.store(false, Ordering::Relaxed);
    blocking(move || {
        models::download(model, &dir, &cancel, |p| {
            let _ = on_progress.send(p);
        })
        .map(|_| ())
    })
    .await
}

/// Stops a running model download.
#[tauri::command]
fn cancel(cancel: State<'_, CancelFlag>) {
    cancel.0.store(true, Ordering::Relaxed);
}

/// Recent images for the Home screen, newest first, with thumbnails where possible.
#[tauri::command]
async fn list_recent(app: AppHandle) -> Result<Vec<recent::RecentEntry>, String> {
    let dir = data_dir(&app)?;
    let entries = blocking(move || {
        Ok(recent::load(&dir)
            .into_iter()
            .map(|file| {
                let path = Path::new(&file.path);
                recent::RecentEntry {
                    exists: path.is_file(),
                    thumbnail: images::thumbnail(&dir, path)
                        .ok()
                        .map(|p| p.to_string_lossy().into_owned()),
                    file,
                }
            })
            .collect::<Vec<_>>())
    })
    .await?;
    for thumb in entries.iter().filter_map(|e| e.thumbnail.as_ref()) {
        let _ = app.asset_protocol_scope().allow_file(thumb);
    }
    Ok(entries)
}

#[tauri::command]
fn remove_recent(app: AppHandle, path: String) -> Result<(), String> {
    recent::remove(&data_dir(&app)?, &path)
}

/// Where testers report problems: the bug-report form, with the app version filled in.
fn report_problem_url() -> String {
    format!(
        "https://github.com/kreokit/cutout/issues/new?template=bug_report.yml&version={}",
        env!("CARGO_PKG_VERSION")
    )
}

/// Opens the bug-report page in the default browser. Only this fixed page can be opened;
/// the webview gets no general permission to open links.
#[tauri::command]
fn report_problem(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(report_problem_url(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Development aid: images to open at startup, from `KREOKIT_DEV_OPEN` (paths separated
/// like `PATH`). Always empty in release builds.
#[tauri::command]
fn dev_startup_images() -> Vec<String> {
    match std::env::var_os("KREOKIT_DEV_OPEN") {
        Some(paths) if cfg!(debug_assertions) => std::env::split_paths(&paths)
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(CancelFlag::default())
        .invoke_handler(tauri::generate_handler![
            open_images,
            list_models,
            download_model,
            cancel,
            list_recent,
            remove_recent,
            report_problem,
            dev_startup_images
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn report_problem_opens_the_bug_form() {
        assert_eq!(
            super::report_problem_url(),
            format!(
                "https://github.com/kreokit/cutout/issues/new?template=bug_report.yml&version={}",
                env!("CARGO_PKG_VERSION")
            )
        );
    }
}
