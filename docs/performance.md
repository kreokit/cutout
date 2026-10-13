# Performance and quality log

Measured on the dev machine: MacBook Pro, **Intel Core i5-6267U** (2 cores / 4 threads,
Skylake), 16 GB RAM, macOS 12.7.6, CPU only. Release builds, ONNX Runtime 1.22.0 (ort
2.0.0-rc.10).

## 2026-10-13: Fast model (U²-Net), milestone 2

### Seconds per image

From `cargo test --release timings_12_megapixels` and `photo_regression` (see
`src-tauri/src/cutout.rs`). The first cutout after starting the app also loads the model
(about 0.8 s).

| Image | Load model | Decode | Preprocess | Inference | Postprocess | Save | **Total** |
|---|---|---|---|---|---|---|---|
| 12 MP photo (4032×3024), first run | 0.82 | 0.09 | 0.27 | 1.10 | 0.49 | 0.15 | **2.92** |
| 12 MP photo, model already loaded | – | 0.06 | 0.24 | 0.79–0.85 | 0.52–0.59 | 0.15 | **1.82–1.85** |
| Photos up to 1600 px, model loaded | – | | | 0.77–0.86 | | | **0.90–1.01** |

Inference is a fixed ~0.8 s (the model always sees 320×320). The rest grows with the
image: shrinking it for the model (preprocess) and enlarging the mask plus compositing
(postprocess) take about 0.8 s together at 12 MP.

Debug builds (`pnpm tauri dev`) are slower in the steps outside ONNX Runtime: an 800×600
image took 3.0 s in the dev app, against about 0.9 s in release.

### For comparison (2026-10-13, same machine)

| Runtime | U²-Net 320² | BiRefNet_lite 1024² |
|---|---|---|
| ONNX Runtime 1.22 via ort rc.10 (used) | 0.9–1.1 s | 13–22 s, peak 8.5 GB RAM |
| ONNX Runtime 1.19.2 (Python) | 0.8–1.1 s | 16–25 s |
| tract 0.23.8, 4 threads | 2.2 s | 51 s, peak 4.5 GB RAM |

### Mask quality: synthetic scenes (exact masks)

`cargo test --release cuts_out_synthetic_scenes`, which fails below the minimums in the test.

| Scene | IoU | MAE |
|---|---|---|
| ball | 0.996 | 0.0020 |
| product | 0.997 | 0.0021 |
| figure | 0.996 | 0.0034 |
| logo (ring with a hole, plus a bar) | **0.767** | 0.0666 |
| low-contrast | 0.995 | 0.0016 |

U²-Net fills the ring's hole: it predicts whole "salient objects".

### Mask quality: photos against provisional masks

`cargo test --release photo_regression`. The reference masks were made by BiRefNet (full)
and are **provisional, not ground truth**, so low numbers mean "differs from BiRefNet".
Each case was checked by eye.

| Photo | IoU | MAE | Seen by eye |
|---|---|---|---|
| graphic-logo.png | 0.996 | 0.0025 | clean |
| product-mug.jpg | 0.985 | 0.0020 | clean |
| dog-grass.jpg | 0.978 | 0.0075 | clean, softer fur edge |
| low-contrast-polar-bear.jpg | 0.974 | 0.0018 | clean |
| person-curly-hair.jpg | **0.743** | 0.1765 | dark hair against the dark background is cut off |
| cat-tabby.jpg | **0.596** | 0.2590 | head and whiskers good; the lower body fades out |
| small-subject-snow-dog.jpg | **0.283** | 0.0225 | the dog's dark body is faint, only the head is solid |

These are the cases for the refine brush (milestone 3) and the Best model (milestone 5).
