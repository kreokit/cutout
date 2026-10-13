# Third-party licences

KreoKit Cutout is free software, dual-licensed under MIT or Apache-2.0. It includes or
uses the third-party components below, each under its own licence.

## AI models (downloaded on first use, not bundled)

Only models whose **weights** are MIT, Apache-2.0 or BSD licensed are used. The app
downloads them from this project's own GitHub release
[`models-v1`](https://github.com/kreokit/cutout/releases/tag/models-v1), where each file
sits next to its licence and an attribution note, and checks each download against a
pinned SHA-256 checksum.

| Model | Use | Licence | Original source |
|---|---|---|---|
| U²-Net (`u2net`), © Xuebin Qin et al. | Fast cutout | Apache-2.0 | Weights: <https://github.com/xuebinqin/U-2-Net>. ONNX conversion: <https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2net.onnx> (rembg, MIT) |
| BiRefNet_lite (general, Swin-T), © Peng Zheng et al. | Best cutout | MIT | <https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/BiRefNet-general-bb_swin_v1_tiny-epoch_232.onnx>, model card <https://huggingface.co/ZhengPeng7/BiRefNet_lite> |

## Fonts (bundled)

Inter (interface font), © The Inter Project Authors, **SIL Open Font License 1.1**; the
licence ships with the app as `licenses/Inter-OFL.txt`.

## Icons

Lucide icons, © Lucide contributors, **ISC** licence: <https://lucide.dev>.
