<p align="center">
  <img src="assets/brand/kreokit-logo-wide.png" alt="KreoKit" width="420">
</p>

# KreoKit Cutout

**Free, offline background removal for your images.** Drop in a photo, get a clean cutout in seconds, refine the edges, pick a new background, and export, with no watermark, no account and no upload.

Part of [KreoKit](https://github.com/kreokit), free open-source creator tools.

> 🚧 **Work in progress.** There is no release yet. Follow along or watch the repo to hear about the first alpha.

## Features (planned for v0.1)

- ✂️ **AI cutout** that runs locally on your computer: a **Fast** model for quick results and a **Best** model for hair, fur and fine details
- 🖌️ **Refine**: feather or shift the edges, and erase or restore areas with a brush
- 🎨 **Backgrounds**: transparent, a solid color, a blurred version of the original, or your own image
- 📤 **Export** PNG, WebP or JPG at the original size or a social preset (1080×1080, 1080×1350, 1080×1920, 1280×720)
- 🗂️ **Batch**: cut out a whole folder of images with the same settings
- 🔒 **Private**: your images never leave your computer

## Build from source

Requirements: Rust (stable), Node.js 20+, pnpm.

```bash
git clone https://github.com/kreokit/cutout
cd cutout
pnpm install
pnpm tauri dev
```

## Roadmap

- [ ] v0.1 (alpha): cutout, refine, backgrounds, export, batch
- [ ] v0.2: click to keep or remove an object
- [ ] v0.3: GPU acceleration (CoreML / DirectML)
- [ ] Later: video background removal

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option. The AI models are downloaded on first use and are permissively licensed too (MIT / Apache 2.0). Third-party components will be listed in `THIRD-PARTY-LICENSES.md`.

Implemented from public specifications, open-source libraries and observed behaviour only. Not affiliated with any company whose products are mentioned as alternatives.
