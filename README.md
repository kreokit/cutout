<p align="center">
  <img src="assets/brand/kreokit-logo-wide.png" alt="KreoKit" width="420">
</p>

# KreoKit Cutout

**Free, offline background removal for your images.** Drop in a photo, get a clean cutout in seconds, refine the edges, pick a new background, and export, with no watermark, no account and no upload.

Part of [KreoKit](https://github.com/kreokit), free open-source creator tools.

🌐 **Website: [kreokit.vercel.app/cutout](https://kreokit.vercel.app/cutout)**: download for your computer and see how to install it.

## Download

**[Get the latest version from Releases →](https://github.com/kreokit/cutout/releases)** (currently [v0.1.0-alpha](https://github.com/kreokit/cutout/releases/tag/v0.1.0-alpha), an early test version)

| Your computer | Download |
|---|---|
| Mac with Apple Silicon (M1, M2, M3, M4…) | [KreoKit.Cutout_0.1.0_aarch64.dmg](https://github.com/kreokit/cutout/releases/download/v0.1.0-alpha/KreoKit.Cutout_0.1.0_aarch64.dmg) |
| Mac with Intel processor | [KreoKit.Cutout_0.1.0_x64.dmg](https://github.com/kreokit/cutout/releases/download/v0.1.0-alpha/KreoKit.Cutout_0.1.0_x64.dmg) |
| Windows 10 / 11 | [KreoKit.Cutout_0.1.0_x64-setup.exe](https://github.com/kreokit/cutout/releases/download/v0.1.0-alpha/KreoKit.Cutout_0.1.0_x64-setup.exe) |
| Linux (any distribution) | [KreoKit.Cutout_0.1.0_amd64.AppImage](https://github.com/kreokit/cutout/releases/download/v0.1.0-alpha/KreoKit.Cutout_0.1.0_amd64.AppImage) |
| Linux (Debian / Ubuntu) | [KreoKit.Cutout_0.1.0_amd64.deb](https://github.com/kreokit/cutout/releases/download/v0.1.0-alpha/KreoKit.Cutout_0.1.0_amd64.deb) |

Not sure which Mac you have? Apple menu → **About This Mac**: "Chip: Apple M…" means Apple Silicon, "Processor: Intel…" means Intel.

### System requirements

- **macOS 12 Monterey** or later, **Windows 10** (version 1903 or later) **/ 11** (64-bit), or **Linux** with glibc 2.35+ (e.g. Ubuntu 22.04+)
- On Intel / AMD computers, a processor with **SSE4.2 and POPCNT** (almost every Intel CPU since 2009 and AMD CPU since 2011). The app tells you if yours isn't supported.
- Disk space for the AI models, downloaded once on first use: Fast 176 MB; Best about 200 MB

### Installing (the alpha isn't signed yet)

**macOS:** open the `.dmg` and drag **KreoKit Cutout** to Applications, then open it. If macOS says it can't verify the app, go to **System Settings → Privacy & Security** (macOS 12: **System Preferences → Security & Privacy → General**) and click **Open Anyway**. If it says the app "is damaged", run this once in Terminal and open it again:

```bash
xattr -dr com.apple.quarantine "/Applications/KreoKit Cutout.app"
```

**Windows:** run the installer. If SmartScreen says "Windows protected your PC", click **More info → Run anyway**.

**Linux:** make the AppImage executable and run it, or install the `.deb`:

```bash
chmod +x KreoKit.Cutout_*.AppImage
./KreoKit.Cutout_*.AppImage
# or: sudo apt install ./KreoKit.Cutout_*_amd64.deb
```

Found a problem? [Open an issue](https://github.com/kreokit/cutout/issues/new/choose) or use **⋯ → Report a problem** in the app.

## Features

- ✂️ **AI cutout** that runs locally on your computer: a **Fast** model for quick results and a **Best** model for hair, fur and fine details (it adapts to how much memory your computer has)
- 🖌️ **Refine**: feather or shift the edges, and erase or restore areas with a brush
- 🎨 **Backgrounds**: transparent, a solid color, a blurred version of the original, or your own image
- 📤 **Export** PNG, WebP or JPG at the original size or a social preset (1080×1080, 1080×1350, 1080×1920, 1280×720), with Fit or Fill framing that never cuts off the subject
- 🗂️ **Batch**: cut out many images, check each one in a grid, and export them all to a folder, never overwriting files without asking
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

- [x] v0.1 (alpha): cutout, refine, backgrounds, export, batch
- [ ] v0.2: click to keep or remove an object
- [ ] v0.3: GPU acceleration (CoreML / DirectML)
- [ ] Later: video background removal

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option. The AI models (U²-Net, Apache 2.0; BiRefNet_lite, MIT) are downloaded on first use from this project's model releases. Third-party components and their licences are listed in [`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md) and included in the app.

Implemented from public specifications, open-source libraries and observed behaviour only. Not affiliated with any company whose products are mentioned as alternatives.
