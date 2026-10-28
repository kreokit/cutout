# KreoKit Cutout: build spec for Claude Code

Read this before every task. It is the source of truth for scope and architecture.
**Status (2026-10-15):** Milestones 1–6 are done. v0.1.0-alpha is published as a pre-release (not "latest"; tag at 555c03b, the commit the files were built from): <https://github.com/kreokit/cutout/releases/tag/v0.1.0-alpha>. README and the org profile link to it. Next work comes from the backlog and tester feedback (issues). **No release shows "Latest"** (2026-10-21, user, as in Clips): `models-v1` and `models-v2` were marked pre-releases too, after checking the app (main and the v0.1.0-alpha tag) downloads models only by tag (`…/releases/download/models-vN/<file>`); both downloads re-checked against their pinned SHA-256 afterwards. Create new `models-vN` mirrors as pre-releases, and never link `/releases/latest`.

App #2 of the KreoKit suite. App #1 is KreoKit Captions (`kreokit/captions`); reuse its foundations wherever possible.

## What we are building

A cross-platform desktop app that removes image backgrounds **fully offline**:
drop images → AI cutout → refine edges → choose a new background → export PNG/WebP/JPG, one image or a whole batch.

Target users: creators worldwide (thumbnails, product photos, social posts, profile pictures). English-first UI.

## Hard rules (same as Captions)

- **No proprietary code or assets.** Build only from public docs, open-source libraries and observed behaviour. Never copy another product's name, logo, icons or exact UI.
- **Offline first.** No servers, no accounts, no telemetry. The only network call is the one-time model download.
- **Licences must stay MIT/Apache-compatible**, and this includes **model weights**. Only use models whose weights are MIT, Apache-2.0 or BSD. **Never** use non-commercial or "research only" weights (for example RMBG by BRIA is non-commercial, so it's not allowed). Verify each model's license from its official source before adding it, and record it in `THIRD-PARTY-LICENSES.md`.
- Small, testable steps. After each milestone: `cargo test`, `pnpm test`, and run the app. Small topic-based commits, **each one buildable and passing tests on its own**.
- Only screenshot the KreoKit window, never the full screen.

## Reuse from KreoKit Captions

Copy (don't reinvent) what `kreokit/captions` CLAUDE.md lists under "Reusable for the next KreoKit apps":
- Design tokens, Inter font, flat dark theme, lime accent `#C8F135` on `#0E0E10`
- UI kit components (Button, Menu, Select, Slider, ColorPicker, Switch, Tabs, Dialog, Toast, Tooltip), Lucide icons
- App shell: macOS overlay title bar, 48px top bar with logo mark, `⋯` menu, single lime primary button (Export)
- Home screen pattern: drop zone + recent items
- Release workflow (GitHub Actions matrix, draft pre-release), license checks, `cargo-deny`, feedback issue templates, "Report a problem" menu item
- Logo files: `assets/brand/`

Copy the files into this repo for now. A shared package can come later, once there are 3 apps.

## Stack

- **Shell:** Tauri 2 (Rust) + React + TypeScript + Vite, pnpm. Dev server on port **1430** (HMR 1431), so it runs next to Captions (1420)
- **Inference:** ONNX Runtime from Rust (`ort` crate), CPU by default, CoreML/DirectML later
  - **Pinned: `ort` 2.0.0-rc.10 = ONNX Runtime 1.22.0, statically linked, on all platforms.** Reason: Intel Mac + macOS 12 support. rc.10 is the last `ort` with a prebuilt `x86_64-apple-darwin` library (rc.11+ and Microsoft's ONNX Runtime ≥ 1.24 dropped Intel Macs; Microsoft's 1.20–1.23 Intel builds need macOS 13.3). pyke's 1.22 library is built for macOS 13.3 too, but it was tested to link and run on macOS 12 (2026-10-13). tract (pure Rust) was measured as the fallback: 2.4× slower
  - **Upgrade path:** when Intel Macs / macOS 12 are dropped, move to the newest `ort` + ONNX Runtime. To keep them, build ONNX Runtime ourselves for `x86_64-apple-darwin` (deployment target 12.0) and link it via `ORT_LIB_LOCATION`, or switch Intel Macs only to tract. Re-run the macOS 12 test and the CPU-baseline check (`cpu.rs`) after any upgrade
  - **macOS 12 binary check:** ONNX Runtime 1.22's objects are built for macOS 13.3, and the release runners' SDK is newer than this Mac's, so the release app must pass `scripts/check_macos_binary.py` (minimum macOS ≤ 12.0, no weak imports; it runs in the release workflow). A weak import of something newer than macOS 12 is NULL there and crashes when called (KreoKit Captions 0.1.0/0.1.1 crashed on macOS 12 this way). If it fails after an `ort` upgrade, don't allow-list the symbol: fix the build or re-test on macOS 12
  - Measured on the dev Mac (i5-6267U, 2 cores / 4 threads): U²-Net 0.9–1.1 s per image; BiRefNet_lite at 1024² 13–30 s with a peak of 8.2 GB (5.9 GB lean), at 768² about 14 s / 3.4 GB, at 640² about 9 s / 2.5 GB
  - **CPU baseline:** pyke's ONNX Runtime 1.22 libraries need x86-64-v2 (SSSE3, SSE4.1, SSE4.2, POPCNT); AVX/FMA only in runtime-dispatched kernels (disassembled on macOS, Windows and Linux, 2026-10-13). `src-tauri/src/cpu.rs` checks this at startup
  - **Windows:** pyke's library links `DirectML.dll`. We never ship it (proprietary redistributable); Windows 10 1903+ / 11 has its own copy. CI (`ci.yml`) runs the Rust tests on Windows to prove the app loads
  - **Licences:** ONNX Runtime's LICENSE and ThirdPartyNotices ship in `src-tauri/resources/licenses/` (bundled as `licenses/`)
  - **Pipeline** (`cutout.rs`): matches rembg (U²-Net: Lanczos to 320², scale by the brightest value, ImageNet mean/std, min-max output; BiRefNet: /255 and sigmoid). Results are cached in `<app cache>/cutouts/<path+mtime hash>-<model>.png` plus `-mask.png`. Every step is timed and logged as `[cutout] …`
  - Known U²-Net limits: fills holes (ring logo), loses dark hair on dark backgrounds and faint/dark bodies (see `docs/performance.md`)
  - **Best on 8 GB machines (M5, done):** BiRefNet_lite runs lean (arena and memory pattern off) at 768² (more than 8 GB total) or 640² (8 GB or less); before each cutout `models::plan` checks free memory (`memory.rs`): 768 needs 4 GB free, else 640 (3 GB, downloaded only when first needed), else a "Use Fast / Try anyway" warning
- **Models** (downloaded on first use into the app data dir, checksum-verified, never committed). Licences verified 2026-10-13:
  - **Fast: U²-Net (`u2net`, 320×320, 176 MB)**, Apache-2.0 (`xuebinqin/U-2-Net` repo licence covers the published weights). ONNX conversion from `danielgatis/rembg` (MIT) release `v0.0.0`
  - **Best: BiRefNet_lite (general, Swin-T backbone)**, MIT (`ZhengPeng7/BiRefNet` repo + HF card `ZhengPeng7/BiRefNet_lite`). Exported from the HF weights (revision `aa62cd8`) at fixed **768×768 (205 MB) and 640×640 (198 MB)** by `scripts/export_birefnet.py` in the manual "Export BiRefNet" workflow (opset 17, PyTorch 2.5.1, the author's deform_conv2d exporter; checked against PyTorch in ONNX Runtime 1.22). A dynamic-size export failed (the deform conv decomposition has the size built in). The author's 1024² ONNX (`models-v1`) is no longer used
  - **Excluded:** IS-Net / `isnet-general-use` (DIS repo licenses only "code and evaluation metric" as Apache-2.0; weights unclear; the `onnx-community/ISNet-ONNX` copy is AGPL-3.0), RMBG 1.4/2.0 (non-commercial), BEN2 full (commercial)
  - **Mirror:** the app downloads only from our own GitHub releases on `kreokit/cutout`, pinned by SHA-256: `models-v1` (U²-Net; also the unused 1024² BiRefNet) and `models-v2` (BiRefNet_lite 768 and 640). Each model file sits there with its licence file and a source/attribution note. Never change a published release's files; publish a new `models-vN` instead. The original source URLs stay documented in `THIRD-PARTY-LICENSES.md`
- **Image I/O:** the `image` crate (PNG, JPG, WebP, HEIC if feasible later). Lossy WebP export uses **libwebp 1.6.0** (BSD-3-Clause) via `libwebp-sys` 0.14, built from source; **not** the `webp` crate (it pulls libwebp 1.3.1, CVE-2023-4863). libwebp's COPYING and PATENTS ship in `licenses/`
- **No FFmpeg** in v0.1 (images only). Video cutout is a later milestone.
- **State:** Zustand. Projects are not needed in v0.1; recent files list only.

## Core pipeline

1. Load image → keep the original at full resolution.
2. Resize to the model's input size, normalize, run inference → alpha mask.
3. Upscale the mask to the original size; optional edge refinement (guided filter or similar) for hair and fur.
4. Composite: original RGB + mask → RGBA.
5. Apply the chosen background, then export.

## Refine (milestone 3)

- **Order** (`src-tauri/src/refine.rs`): AI mask → **shift** (grayscale erode/dilate, square window, ±10 px) → **feather** (Gaussian, σ = feather / 2, up to 20 px) → **brush strokes** in order (restore blends towards opaque, erase towards transparent; no build-up within one stroke) → colours: **Remove dark fringe** (blur-fusion foreground estimation, Forte & Pitié 2021; on by default) for semi-transparent pixels. Lengths are in pixels of the original image.
- **Rust is the only implementation** of the mask maths; preview and (M4) export use the same functions. `session.rs` caches per image (original, edge mask, final mask, output) and renders only what changed; renders go to the webview as raw RGBA after a 24-byte header. Working copy ≤ 16 MP (WebKit's canvas limit); export is full size.
- **Frontend:** `CutoutCanvas` shows renders ("latest wins" queue; slider drags send 1 MP previews, the full render follows on release). The brush paints live into the canvas with the same maths (`src/lib/brush.ts`, `paint.ts`; parity via `fixtures/brush-cases.json`), then the backend renders the stroke's bounds. Restore reads the working original from the backend (`working_original`), never from a cross-origin image.
- **Undo/redo** (`src/store/edits.ts`): strokes and edge changes, one step per slider drag (`Slider` `onCommit`); ⌘Z, ⇧⌘Z, Ctrl+Y. Edits are kept for the session only.
- **Zoom/pan** (`src/lib/view.ts`, Figma style): scroll pans; ⌘/Ctrl + scroll and pinch zoom; Space + drag and middle-drag pan; zoom bar, ⌘0 / ⌘1 / ⌘+ / ⌘−. Pixels are sharp from 200%.
- **Tools:** H / E / R, brush size in screen pixels (`[` `]`), softness (Shift + `[` `]`).

## Backgrounds + export (milestone 4)

- **Backgrounds** (`background.rs`): transparent, solid colour, blur of the original, or an image (cover-fitted). **One background for all open images**, with undo: every step in `src/store/edits.ts` has a global number, so ⌘Z undoes the newest step of the current image *or* of the background.
- **Blur without halo:** normalized convolution over the background only, blur(original × background weight) / blur(weight); the weight is made strict (cells < 90 % background count as 0) and eroded by one cell, so subject colours never bleed in. Wider levels fill in behind the subject (it can show there after Fill). Computed on a small grid (cells ≈ σ/4) and sampled up, so preview and export of any size look the same; strength 5–100 = σ up to 6 % of the short side. 48 MP: 0.6 s (2.1 s at strength 5)
- **Sizes** (`layout.rs`): original, 1080×1080, 1080×1350, 1080×1920, 1280×720. **Fit**: the whole photo inside the frame. **Fill**: covers the frame centred on the subject (final-mask bounds ≥ 32), zooming out just enough that it is never cut off (4 % margin); a subject touching the photo's edge stays on that frame edge
- **Export** (`export.rs`, `session.rs`): every refine edit at **full size** (the working copy's render is reused when it is full size), the cutout scaled premultiplied (no dark fringe), straight-alpha "over" the background, fully transparent pixels cleared (nothing erased stays hidden in the file), written atomically (temp file + rename). Over 16 MP, the last full-size render is kept per edits, so exporting again is quick (48 MP: first export ≈ 8 s, then 1–2 s; WebP encode ≈ 7 s). PNG keeps alpha (RGB when opaque); WebP is lossy with lossless alpha (max 16383 px a side); JPG goes on white when transparent. The original's ICC profile is embedded (ICCP chunk for WebP); EXIF is not copied
- **Stage:** a colour is CSS; blur (from the 1 MP copy) and image (≤ 4 MP) come from `stage_background`, under the cutout canvas. The Export dialog's preview (`export_preview`, ≤ 0.4 MP) uses the same `export::render`
- **Export dialog** (⌘E): format, quality 40–100 (WebP/JPG), size, Fit/Fill, preview; settings remembered in localStorage (`store/exportSettings.ts`, sanitized on load); save dialog defaults to `<name>-cutout.<ext>` next to the original; "Show in folder" after saving
- **macOS 12 binary check:** `check_macos_binary.py` allows exactly two extra weak imports from libSystem (user decisions, 2026-10-15): `_dispatch_once_f` (since macOS 10.6; the linker adds it; also present before libwebp, A/B at 2d51254) and `__availability_version_check` (since macOS 11; compiler-rt's OS-version checks import it weakly and check for NULL; the runners' newer SDK adds it to both macOS builds; verified present at runtime on macOS 12.7.6). Allow nothing else without asking
- **Dark outline on solid colours (checked 2026-10-15):** the export composites edges with the decontaminated colours (test `exports_composite_the_decontaminated_edge`). A dark line that stays is in the photo under a near-opaque mask (e.g. a dark rim U²-Net counts as subject); fixes are Shift −1…−3, Erase, or Best
- **UI kit:** `ColorPicker` has a saturation/brightness area and hue bar (the only gradients in the kit: they are the colour space), custom `swatches` and `onCommit`. Editor shortcuts ignore keys from inside dialogs and popovers

## Batch + Best (milestone 5)

- **Batch grid** (`Batch.tsx`): opening more than one image starts on the grid (`session.view`); tiles show the cutout thumbnail (480 px, cached next to the cutout, `cutout_thumbnail`) or the original's while waiting, the status, Retry, and Best / Edited badges. Click opens the editor; "Back to batch" / Esc returns. Cutouts run one after another as each image's model is downloaded
- **Model per image:** `useCutouts.modelOf[path]` (falls back to the Home choice); the editor's Background removal section switches Fast / Best per image, and each model keeps its own edits (keys are `model:path`). After a Fast cutout, a quiet "Better edges for hair → Try Best" button switches the image to Best
- **Export all** (`BatchExportDialog.tsx`, `batch.rs`): the Export dialog's settings for every image into one folder as `<name>-cutout.<ext>`, each with its own model and edits and the shared background. Missing cutouts are cut out first. **No silent overwrites:** existing files are listed first (Replace / Keep both / Cancel); names repeated in the batch get -2, -3…; without Replace, `export::write_new` hard-links a temporary file into place, which fails if the name exists (so even a file created during the export is kept; the next free name is used). Progress per image, Stop, summary with failures
- **Best model files** (`models.rs`): `ModelFile` (`u2net`, `birefnet-768`, `birefnet-640`); `CutoutModel::main_file` picks 768 above 9 GB total (8 GB machines report 8.6 GB) and 640 otherwise; `plan()` decides per cutout (run / download the fallback / low memory). `cut_out` returns `done`, `download` or `low-memory`; the UI shows a download card or the "Use Fast / Try anyway" card (stage and batch tiles). Cached results skip the plan
- **Measuring tools:** `model_memory` (ignored test; each configuration in its own process; `KREOKIT_PROBE_FILE`, `KREOKIT_PROBE_FILES`, `KREOKIT_PROBE_EXTRA`); the model tests take `KREOKIT_MODEL_FILE`
- **Guided-filter refinement for Fast: tried, not adopted** (2026-10-15, `docs/performance.md`): every setting worsened Fast's masks against the provisional masks, and strong settings smear haze into the background. Code kept on branch `experiment/guided-filter-fast` (user: keep it). For hair, use Best

## Release (milestone 6)

- **Workflow** (`release.yml`, manual with a tag or on a `v*` tag push): checks job (tests, cargo-deny, THIRD-PARTY check) → draft pre-release (notes from `docs/release-notes/<tag>.md`) → builds for macOS Apple Silicon (macos-15), macOS Intel (macos-15-intel), Windows x64 (NSIS), Linux x64 (AppImage + .deb, Ubuntu 22.04 / glibc 2.35), `MACOSX_DEPLOYMENT_TARGET=12.0`. Each build: bundle check (all licence files, ad-hoc signature) → `check_macos_binary.py` → **`--self-test`** → upload (`--clobber`). A failed check uploads nothing; re-running the workflow for the same tag replaces the files
- **`cutout --self-test <image> [--reference <mask>] [--models <dir>]`** (`selftest.rs`, before Tauri starts, no display): downloads Fast and Best 640 if missing (models cached in CI by checksum), cuts out `fixtures/photos/product-mug.jpg` with Fast (IoU ≥ 0.90) and Best 640 (≥ 0.95), runs Best's plan for the machine and for an 8 GB one (must pick 640; Try anyway if memory is short), exports PNG / JPG / WebP / blur and decodes them. On Windows it attaches to the parent console. Results 2026-10-15: Fast 0.985, Best 0.997 on every runner; the Apple Silicon runner (7.5 GB) chose 640 by itself
- **Licences in the app** (`licenses/`): ONNX Runtime (+ ThirdPartyNotices), libwebp (COPYING, PATENTS), the AI models (`MODEL-U2NET-LICENSE.txt` Apache-2.0, `MODEL-U2NET-ONNX-REMBG-LICENSE.txt` MIT, `MODEL-BIREFNET-LICENSE.txt` MIT), Inter (frontend), and `THIRD-PARTY-LICENSES.md`
- **macOS 12 check on the dev Mac:** after each release run, download the Intel `.app.tar.gz` from the draft and run `check_macos_binary.py`, the self-test (empty `--models`, so the download runs too) and open the app once. Done for v0.1.0-alpha on macOS 12.7.6

## Release checklist

- After publishing, update the kreokit/.github profile README row.
- After publishing, update the tag and file names in kreokit/website data/apps.json.

## Screens (v0.1)

1. **Home:** big drop zone (one or many images), recent files, model picker (Fast / Best)
2. **Editor:**
   - Center: canvas on a checkerboard; toggle Before / After (slider compare)
   - Zoom and pan
   - Right inspector: **Background** (transparent, solid color, blur original, image), **Edges** (feather, shift/erode-dilate), **Refine brush** (erase / restore, brush size)
   - Top bar: Export (PNG / WebP / JPG, size: original or social presets 1080×1080, 1080×1350, 1080×1920, 1280×720)
3. **Batch:** a grid of thumbnails with per-image status, "Export all" to a folder with the same settings

## Milestones

1. ✅ **Scaffold:** repo from the Captions foundations (tokens, UI kit, shell, icon, release config skeleton); Home screen with drop zone; load and display an image
2. ✅ **Cutout:** `models-v1` mirror release; model download with progress + ONNX inference → mask → RGBA on a checkerboard; Before/After compare
3. ✅ **Refine:** feather/shift edges, erase/restore brush with undo for brush strokes, zoom/pan
4. ✅ **Backgrounds + export:** transparent/color/blur/image backgrounds; PNG/WebP/JPG export; social size presets
5. ✅ **Batch + second model:** multi-image grid, export all; add the Fast/Best choice
6. ✅ **Release:** reuse the Captions release workflow (no FFmpeg), licenses incl. model weights, pre-release v0.1.0-alpha (published 2026-10-15)

## Reusable for the next KreoKit apps

Copy these into a new app instead of rebuilding them (Captions' list covers the shared tokens, UI kit, shell, fonts and FFmpeg):

- **ONNX Runtime:** the pinned `ort` 2.0.0-rc.10 / ONNX Runtime 1.22 static setup (`Cargo.toml`), its licence files in `src-tauri/resources/licenses/`, and the CPU baseline check `src-tauri/src/cpu.rs` (x86-64-v2; merge it with other engines' baselines)
- **Model download + mirror:** `src-tauri/src/models.rs` (download with progress to the app data dir, `.part` file, SHA-256 check against pinned checksums, only from our own `models-vN` releases) and the mirror layout (model + licence + source note + SHA256SUMS; never change a published release)
- **Reproducible model export in Actions:** `.github/workflows/export-birefnet.yml` + `scripts/export_birefnet.py` (pinned tool versions, output checked against the original framework in ONNX Runtime 1.22)
- **Memory check:** `src-tauri/src/memory.rs` (total and available memory on macOS, Windows, Linux) and the plan-before-run pattern in `models::plan` (pick a smaller variant or warn)
- **Batch:** the grid (`Batch.tsx`, `batch.css`), "Export all" (`BatchExportDialog.tsx`, `batch.rs`: conflicts listed first, Replace / Keep both / Cancel, progress, Stop, summary) and safe writing (`export::write_new`: hard-link a temporary file into place, so nothing is ever overwritten silently)
- **Release:** `release.yml` with the bundle check, `scripts/check_macos_binary.py` (two documented allow-listed libSystem symbols; allow nothing else without asking) and `--self-test` (`selftest.rs`, attaches to the console on Windows); CI on Windows to prove the app loads (`ci.yml`)
- **Home:** `src/styles/home.css` (drop zone + recent grid; mind the macOS 12 WebKit aspect-ratio note) and `relativeTime` with tests (`src/lib/recent.ts`)
- **Dev aid:** `KREOKIT_DEV_OPEN` opens files at startup in debug builds
- **Lesson (from Clips M1):** cache files made on request (thumbnails) need a unique temporary name per request: React's StrictMode and two screens can ask at once

## Tests (our oracle)

- **Synthetic fixtures** (generated, exact masks) are the strict mask tests for now.
- **Provisional reference masks:** the free-licensed photos get masks made with BiRefNet (full) and are labelled **"provisional, not ground truth"**. They're used only as a regression check (warn on big drops, never fail the build) until hand-made masks exist.
- Synthetic scenes: `src-tauri/src/synthetic.rs`; real-model checks are ignored tests that need the model (`KREOKIT_MODELS_DIR=<dir> [KREOKIT_MODEL_FILE=birefnet-768] cargo test --release <name> -- --ignored --nocapture`): `cuts_out_synthetic_scenes` (strict), `photo_regression` (warn only; `KREOKIT_UPDATE_BASELINE=1` rewrites `fixtures/photos/baseline-<model>.json`), `timings_12_megapixels`, `model_memory`. Record results in `docs/performance.md`
- `fixtures/photos/`: 7 photos (CC0 / public domain from Wikimedia Commons, plus our logo) with sources in `SOURCES.md`
- `fixtures/`: 5–8 images that are our own or free-licensed (Pexels/Pixabay/Unsplash license noted per file), covering a person with hair, a product on a table, a pet, a logo/flat graphic, and a low-contrast background. Each has a **hand-made reference mask**.
- Mask quality per model: IoU and mean absolute error vs the reference, logged per image, with a minimum threshold enforced once the baseline is known.
- Export round-trip (`session.rs` tests): the exported PNG keeps the alpha channel and equals the refine functions run at full size, also when the working copy is smaller; the JPG export has the chosen background, no alpha; preset sizes are exact; blur and image backgrounds; the ICC profile is kept. `export_timings` and `background_timings` (ignored) time 12 and 48 MP.
- Batch: N inputs → N outputs, names preserved, nothing overwritten without confirmation.
- Performance: log seconds per image per model on the dev machine.

## Dev environments to support

- macOS 12 Monterey on Intel Core i5 (CPU only): keep the default model fast enough here; show progress for anything over 1 s
- Windows 10/11, Linux (AppImage + .deb), macOS Apple Silicon

## Backlog (not v0.1)

- Video background removal (with FFmpeg sidecar, like Captions)
- Object/area selection by click ("keep this")
- GPU acceleration (CoreML / DirectML)
- Shared `@kreokit/ui` package across apps
