# Third-party licences

KreoKit Cutout is free software, dual-licensed under MIT or Apache-2.0. It includes or
uses the third-party components below, each under its own licence.

## AI models (downloaded on first use, not bundled)

Only models whose **weights** are MIT, Apache-2.0 or BSD licensed are used. The app
downloads them from this project's own GitHub releases
[`models-v1`](https://github.com/kreokit/cutout/releases/tag/models-v1) (Fast) and
[`models-v2`](https://github.com/kreokit/cutout/releases/tag/models-v2) (Best), where each
file sits next to its licence and an attribution note, and checks each download against a
pinned SHA-256 checksum.

| Model | Use | Licence | Original source |
|---|---|---|---|
| U²-Net (`u2net`), © Xuebin Qin et al. | Fast cutout | Apache-2.0 | Weights: <https://github.com/xuebinqin/U-2-Net>. ONNX conversion: <https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2net.onnx> (rembg, MIT) |
| BiRefNet_lite (general, Swin-T), © Peng Zheng et al. | Best cutout | MIT | Weights: <https://huggingface.co/ZhengPeng7/BiRefNet_lite> (revision `aa62cd8`; code and licence <https://github.com/ZhengPeng7/BiRefNet>). Exported to ONNX at 768×768 and 640×640 by this project's `scripts/export_birefnet.py` (GitHub Actions), published in the [`models-v2`](https://github.com/kreokit/cutout/releases/tag/models-v2) release |

## ONNX Runtime (compiled into the app)

The AI models run with **ONNX Runtime 1.22.0**, © Microsoft Corporation, **MIT** licence
(<https://github.com/microsoft/onnxruntime>), statically linked through the `ort` crate
(2.0.0-rc.10, MIT OR Apache-2.0) from pyke's prebuilt libraries. ONNX Runtime contains
third-party components (for example protobuf, Abseil, Eigen, XNNPACK), each under its own
licence; their notices ship with the app as `licenses/ONNXRUNTIME-ThirdPartyNotices.txt`,
next to `licenses/ONNXRUNTIME-LICENSE.txt`.

On Windows, ONNX Runtime is linked against `DirectML.dll`. KreoKit Cutout does **not**
ship that file: the copy that comes with Windows 10 (version 1903 or later) and Windows 11
is used, and the app runs on the CPU only.

## libwebp (compiled into the app)

WebP export uses **libwebp 1.6.0**, © Google Inc., **BSD-3-Clause** licence
(<https://chromium.googlesource.com/webm/libwebp>), built from source and statically
linked through the `libwebp-sys` crate (0.14, MIT). Its licence and the additional patent
grant ship with the app as `licenses/LIBWEBP-COPYING.txt` and `licenses/LIBWEBP-PATENTS.txt`.

## Fonts (bundled)

Inter (interface font), © The Inter Project Authors, **SIL Open Font License 1.1**; the
licence ships with the app as `licenses/Inter-OFL.txt`.

## Icons

Lucide icons, © Lucide contributors, **ISC** licence: <https://lucide.dev>.
