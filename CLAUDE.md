# KreoKit Cutout: build spec for Claude Code

Read this before every task. It is the source of truth for scope and architecture.
**Status (2026-10-14):** Milestones 1–2 are done and user-tested. Milestone 3 (Refine) is built and waiting for the user's test in the app.

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

- **Shell:** Tauri 2 (Rust) + React + TypeScript + Vite, pnpm
- **Inference:** ONNX Runtime from Rust (`ort` crate), CPU by default, CoreML/DirectML later
  - **Pinned: `ort` 2.0.0-rc.10 = ONNX Runtime 1.22.0, statically linked, on all platforms.** Reason: Intel Mac + macOS 12 support. rc.10 is the last `ort` with a prebuilt `x86_64-apple-darwin` library (rc.11+ and Microsoft's ONNX Runtime ≥ 1.24 dropped Intel Macs; Microsoft's 1.20–1.23 Intel builds need macOS 13.3). pyke's 1.22 library is built for macOS 13.3 too, but it was tested to link and run on macOS 12 (2026-10-13). tract (pure Rust) was measured as the fallback: 2.4× slower
  - **Upgrade path:** when Intel Macs / macOS 12 are dropped, move to the newest `ort` + ONNX Runtime. To keep them, build ONNX Runtime ourselves for `x86_64-apple-darwin` (deployment target 12.0) and link it via `ORT_LIB_LOCATION`, or switch Intel Macs only to tract. Re-run the macOS 12 test and the CPU-baseline check (`cpu.rs`) after any upgrade
  - Measured on the dev Mac (i5-6267U, 2 cores / 4 threads): U²-Net 0.9–1.1 s per image, BiRefNet_lite 13–22 s with a **peak of 8.5 GB RAM**
  - **CPU baseline:** pyke's ONNX Runtime 1.22 libraries need x86-64-v2 (SSSE3, SSE4.1, SSE4.2, POPCNT); AVX/FMA only in runtime-dispatched kernels (disassembled on macOS, Windows and Linux, 2026-10-13). `src-tauri/src/cpu.rs` checks this at startup
  - **Windows:** pyke's library links `DirectML.dll`. We never ship it (proprietary redistributable); Windows 10 1903+ / 11 has its own copy. CI (`ci.yml`) runs the Rust tests on Windows to prove the app loads
  - **Licences:** ONNX Runtime's LICENSE and ThirdPartyNotices ship in `src-tauri/resources/licenses/` (bundled as `licenses/`)
  - **Pipeline** (`cutout.rs`): matches rembg (U²-Net: Lanczos to 320², scale by the brightest value, ImageNet mean/std, min-max output; BiRefNet: /255 and sigmoid). Results are cached in `<app cache>/cutouts/<path+mtime hash>-<model>.png` plus `-mask.png`. Every step is timed and logged as `[cutout] …`
  - Known U²-Net limits: fills holes (ring logo), loses dark hair on dark backgrounds and faint/dark bodies (see `docs/performance.md`)
  - **Best must work on 8 GB RAM machines without crashing (M5):** use ONNX Runtime memory settings (arena / memory pattern off), a lower input resolution, or check free memory first and show a clear warning. Test this before Best ships
- **Models** (downloaded on first use into the app data dir, checksum-verified, never committed). Licences verified 2026-10-13:
  - **Fast: U²-Net (`u2net`, 320×320, 176 MB)**, Apache-2.0 (`xuebinqin/U-2-Net` repo licence covers the published weights). ONNX conversion from `danielgatis/rembg` (MIT) release `v0.0.0`
  - **Best: BiRefNet_lite (general, Swin-T backbone, 1024×1024, 224 MB)**, MIT (`ZhengPeng7/BiRefNet` repo + HF card `ZhengPeng7/BiRefNet_lite`); ONNX `BiRefNet-general-bb_swin_v1_tiny-epoch_232.onnx` from the author's GitHub release `v1`
  - **Excluded:** IS-Net / `isnet-general-use` (DIS repo licenses only "code and evaluation metric" as Apache-2.0; weights unclear; the `onnx-community/ISNet-ONNX` copy is AGPL-3.0), RMBG 1.4/2.0 (non-commercial), BEN2 full (commercial)
  - **Mirror:** the app downloads only from our own GitHub release `models-v1` on `kreokit/cutout`, pinned by SHA-256. Each model file is uploaded there with its licence file and a source/attribution note. The original source URLs stay documented in `THIRD-PARTY-LICENSES.md`
  - Start with Fast (U²-Net) in M2; add Best in M5.
- **Image I/O:** the `image` crate (PNG, JPG, WebP, HEIC if feasible later)
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
3. **Refine:** feather/shift edges, erase/restore brush with undo for brush strokes, zoom/pan
4. **Backgrounds + export:** transparent/color/blur/image backgrounds; PNG/WebP/JPG export; social size presets
5. **Batch + second model:** multi-image grid, export all; add the Fast/Best choice
6. **Release:** reuse the Captions release workflow (no FFmpeg), licenses incl. model weights, draft pre-release v0.1.0-alpha

## Tests (our oracle)

- **Synthetic fixtures** (generated, exact masks) are the strict mask tests for now.
- **Provisional reference masks:** the free-licensed photos get masks made with BiRefNet (full) and are labelled **"provisional, not ground truth"**. They're used only as a regression check (warn on big drops, never fail the build) until hand-made masks exist.
- Synthetic scenes: `src-tauri/src/synthetic.rs`; real-model checks are ignored tests that need the model (`KREOKIT_MODELS_DIR=<dir> cargo test --release <name> -- --ignored --nocapture`): `cuts_out_synthetic_scenes` (strict), `photo_regression` (warn only; `KREOKIT_UPDATE_BASELINE=1` rewrites `fixtures/photos/baseline-fast.json`), `timings_12_megapixels`. Record results in `docs/performance.md`
- `fixtures/photos/`: 7 photos (CC0 / public domain from Wikimedia Commons, plus our logo) with sources in `SOURCES.md`
- `fixtures/`: 5–8 images that are our own or free-licensed (Pexels/Pixabay/Unsplash license noted per file), covering a person with hair, a product on a table, a pet, a logo/flat graphic, and a low-contrast background. Each has a **hand-made reference mask**.
- Mask quality per model: IoU and mean absolute error vs the reference, logged per image, with a minimum threshold enforced once the baseline is known.
- Export round-trip: the exported PNG keeps the alpha channel; the JPG export has the chosen background, no alpha.
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
