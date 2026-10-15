mod background;
mod batch;
mod cpu;
mod cutout;
mod encode;
mod export;
mod images;
mod layout;
mod matting;
mod memory;
mod models;
mod recent;
mod refine;
mod session;
#[cfg(test)]
mod synthetic;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use images::ImageInfo;
use models::{CutoutModel, DownloadProgress, ModelFile, ModelInfo, Plan};

/// Set by `cancel`; checked by model downloads.
#[derive(Default)]
struct CancelFlag(Arc<AtomicBool>);

/// Set by `stop_batch`; checked between the images of "Export all".
#[derive(Default)]
struct BatchStop(Arc<AtomicBool>);

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

/// The models with the file this computer uses for each (by its total memory).
#[tauri::command]
fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    let total = memory::memory().map(|m| m.total_bytes);
    Ok(models::list(&models_dir(&app)?, total))
}

/// Downloads a model file (the model's own, or Best's smaller fallback).
#[tauri::command]
async fn download_model(
    app: AppHandle,
    cancel: State<'_, CancelFlag>,
    file: ModelFile,
    on_progress: Channel<DownloadProgress>,
) -> Result<(), String> {
    let dir = models_dir(&app)?;
    let cancel = cancel.0.clone();
    cancel.store(false, Ordering::Relaxed);
    blocking(move || {
        models::download(file, &dir, &cancel, |p| {
            let _ = on_progress.send(p);
        })
        .map(|_| ())
    })
    .await
}

/// What `cut_out` did: the result, or why it didn't run yet.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum CutOutcome {
    Done {
        result: cutout::CutoutResult,
    },
    /// Download this file first (Best's first download, or its smaller fallback).
    Download {
        file: ModelFile,
        size_bytes: u64,
    },
    /// Not enough free memory for Best: the user picks Use Fast or Try anyway (`force`).
    LowMemory {
        available_bytes: u64,
        needed_bytes: u64,
    },
}

/// Cuts out the image at `path` with `model` and lets the webview show the result. Stages
/// go to `on_progress`. Best first checks the free memory (models::plan); with `force`
/// ("Try anyway") it runs even when memory is short.
#[tauri::command]
async fn cut_out(
    app: AppHandle,
    engine: State<'_, Arc<cutout::Engine>>,
    path: String,
    model: CutoutModel,
    force: bool,
    on_progress: Channel<cutout::Stage>,
) -> Result<CutOutcome, String> {
    let support = cpu::check();
    if !support.supported {
        return Err(cpu::unsupported_message(&support.missing));
    }
    let models = models_dir(&app)?;
    let cache = cache_dir(&app)?;
    let engine = engine.inner().clone();
    let outcome = blocking(move || {
        let image = Path::new(&path);
        // A result from before needs no model and no memory.
        if let Some(result) = cutout::cached(&cache, image, model)? {
            return Ok(CutOutcome::Done { result });
        }
        let file = match models::plan(model, &models, memory::memory(), force) {
            Plan::Run { file } => file,
            Plan::Download { file, size_bytes } => {
                return Ok(CutOutcome::Download { file, size_bytes });
            }
            Plan::LowMemory {
                available_bytes,
                needed_bytes,
            } => {
                return Ok(CutOutcome::LowMemory {
                    available_bytes,
                    needed_bytes,
                });
            }
        };
        let result = cutout::cut_out(&engine, &models, &cache, file, image, |stage| {
            let _ = on_progress.send(stage);
        })?;
        Ok(CutOutcome::Done { result })
    })
    .await?;
    if let CutOutcome::Done { result } = &outcome {
        for file in [&result.cutout, &result.mask] {
            app.asset_protocol_scope()
                .allow_file(file)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(outcome)
}

/// Renders the refined cutout of `path` (already cut out with `model`) with `edits`, as
/// raw RGBA after a 24-byte header (see session.rs). `preview` renders the whole image
/// small for live slider changes; otherwise only what changed comes back (everything
/// with `full`).
#[tauri::command]
async fn render_refined(
    app: AppHandle,
    refiner: State<'_, Arc<session::Refiner>>,
    path: String,
    model: CutoutModel,
    edits: session::Edits,
    preview: bool,
    full: bool,
) -> Result<tauri::ipc::Response, String> {
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("cannot find the cache folder: {e}"))?;
    let refiner = refiner.inner().clone();
    let bytes = blocking(move || {
        let image = Path::new(&path);
        let (_, mask) = cutout::result_paths(&cache, image, model)?;
        refiner.render(image, &mask, &edits, preview, full)
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// The original of `path` at the editor's working size (same header as a render).
#[tauri::command]
async fn working_original(
    app: AppHandle,
    refiner: State<'_, Arc<session::Refiner>>,
    path: String,
    model: CutoutModel,
) -> Result<tauri::ipc::Response, String> {
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("cannot find the cache folder: {e}"))?;
    let refiner = refiner.inner().clone();
    let bytes = blocking(move || {
        let image = Path::new(&path);
        let (_, mask) = cutout::result_paths(&cache, image, model)?;
        refiner.original(image, &mask)
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map_err(|e| format!("cannot find the cache folder: {e}"))
}

/// Exports `path` (cut out with `model`) with `edits` on `background` to `dest`, at full
/// size. Returns the path written (with the format's extension added if it had none).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn export_image(
    app: AppHandle,
    refiner: State<'_, Arc<session::Refiner>>,
    path: String,
    model: CutoutModel,
    edits: session::Edits,
    background: background::Background,
    settings: export::ExportSettings,
    dest: String,
) -> Result<String, String> {
    let cache = cache_dir(&app)?;
    let refiner = refiner.inner().clone();
    blocking(move || {
        let image = Path::new(&path);
        let (_, mask) = cutout::result_paths(&cache, image, model)?;
        let dest = export::destination(Path::new(&dest), settings.format)?;
        let bytes = refiner.export(image, &mask, &edits, &background, &settings)?;
        export::write_atomic(&dest, &bytes)?;
        Ok(dest.to_string_lossy().into_owned())
    })
    .await
}

/// A small preview of the export (raw RGBA after the render header).
#[tauri::command]
async fn export_preview(
    app: AppHandle,
    refiner: State<'_, Arc<session::Refiner>>,
    path: String,
    model: CutoutModel,
    edits: session::Edits,
    background: background::Background,
    settings: export::ExportSettings,
) -> Result<tauri::ipc::Response, String> {
    let cache = cache_dir(&app)?;
    let refiner = refiner.inner().clone();
    let bytes = blocking(move || {
        let image = Path::new(&path);
        let (_, mask) = cutout::result_paths(&cache, image, model)?;
        refiner.export_preview(image, &mask, &edits, &background, &settings)
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// The background layer under the cutout on the stage (raw RGBA after the render header).
#[tauri::command]
async fn stage_background(
    app: AppHandle,
    refiner: State<'_, Arc<session::Refiner>>,
    path: String,
    model: CutoutModel,
    edits: session::Edits,
    background: background::Background,
) -> Result<tauri::ipc::Response, String> {
    let cache = cache_dir(&app)?;
    let refiner = refiner.inner().clone();
    let bytes = blocking(move || {
        let image = Path::new(&path);
        let (_, mask) = cutout::result_paths(&cache, image, model)?;
        refiner.stage_background(image, &mask, &edits, &background)
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// The cached thumbnail of the image at `path` (opened before), for the batch grid.
#[tauri::command]
async fn image_thumbnail(app: AppHandle, path: String) -> Result<String, String> {
    let dir = data_dir(&app)?;
    let thumb = blocking(move || images::thumbnail(&dir, Path::new(&path))).await?;
    app.asset_protocol_scope()
        .allow_file(&thumb)
        .map_err(|e| e.to_string())?;
    Ok(thumb.to_string_lossy().into_owned())
}

/// The cached thumbnail of the cutout of `path` with `model` (a PNG with alpha).
#[tauri::command]
async fn cutout_thumbnail(
    app: AppHandle,
    path: String,
    model: CutoutModel,
) -> Result<String, String> {
    let cache = cache_dir(&app)?;
    let thumb = blocking(move || cutout::cutout_thumbnail(&cache, Path::new(&path), model)).await?;
    app.asset_protocol_scope()
        .allow_file(&thumb)
        .map_err(|e| e.to_string())?;
    Ok(thumb.to_string_lossy().into_owned())
}

/// The files "Export all" of `paths` as `format` to `folder` would replace.
#[tauri::command]
async fn batch_conflicts(
    folder: String,
    paths: Vec<String>,
    format: encode::Format,
) -> Result<Vec<String>, String> {
    blocking(move || {
        let images: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        Ok(export::conflicts(Path::new(&folder), &images, format)
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect())
    })
    .await
}

/// "Export all": `items` to `folder` with the same background and settings. Without
/// `replace`, existing files are never touched (new names get -2, -3…).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn export_batch(
    app: AppHandle,
    refiner: State<'_, Arc<session::Refiner>>,
    stop: State<'_, BatchStop>,
    items: Vec<batch::BatchItem>,
    background: background::Background,
    settings: export::ExportSettings,
    folder: String,
    replace: bool,
    on_progress: Channel<batch::Progress>,
) -> Result<Vec<batch::ItemResult>, String> {
    let cache = cache_dir(&app)?;
    let refiner = refiner.inner().clone();
    let stop = stop.0.clone();
    stop.store(false, Ordering::Relaxed);
    blocking(move || {
        Ok(batch::export_all(
            &refiner,
            &cache,
            &items,
            &background,
            &settings,
            Path::new(&folder),
            replace,
            &stop,
            |p| {
                let _ = on_progress.send(p);
            },
        ))
    })
    .await
}

/// Stops "Export all" before its next image.
#[tauri::command]
fn stop_batch(stop: State<'_, BatchStop>) {
    stop.0.store(true, Ordering::Relaxed);
}

/// Shows an exported file in the file manager. Only files with an image extension.
#[tauri::command]
fn reveal_export(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = PathBuf::from(path);
    if !images::has_image_extension(&path) || !path.is_file() {
        return Err("not an exported image".into());
    }
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}

/// Frees the refine session (when leaving the editor).
#[tauri::command]
fn close_refine(refiner: State<'_, Arc<session::Refiner>>) {
    refiner.clear();
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

/// Whether this CPU can run cutouts (see cpu.rs).
#[tauri::command]
fn cpu_support() -> cpu::CpuSupport {
    cpu::check()
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
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
        .manage(BatchStop::default())
        .manage(Arc::new(cutout::Engine::default()))
        .manage(Arc::new(session::Refiner::default()))
        .invoke_handler(tauri::generate_handler![
            open_images,
            list_models,
            download_model,
            cut_out,
            render_refined,
            working_original,
            close_refine,
            export_image,
            export_preview,
            stage_background,
            reveal_export,
            batch_conflicts,
            image_thumbnail,
            cutout_thumbnail,
            export_batch,
            stop_batch,
            cancel,
            list_recent,
            remove_recent,
            report_problem,
            cpu_support,
            quit_app,
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
