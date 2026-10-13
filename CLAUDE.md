# KreoKit Cutout: build spec for Claude Code

Read this before every task. It is the source of truth for scope and architecture.
**Status (2026-10-13):** Milestone 1 (Scaffold) is done and tested. Next: Milestone 2 (Cutout).

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
2. **Cutout:** `models-v1` mirror release; model download with progress + ONNX inference → mask → RGBA on a checkerboard; Before/After compare
3. **Refine:** feather/shift edges, erase/restore brush with undo for brush strokes, zoom/pan
4. **Backgrounds + export:** transparent/color/blur/image backgrounds; PNG/WebP/JPG export; social size presets
5. **Batch + second model:** multi-image grid, export all; add the Fast/Best choice
6. **Release:** reuse the Captions release workflow (no FFmpeg), licenses incl. model weights, draft pre-release v0.1.0-alpha

## Tests (our oracle)

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
