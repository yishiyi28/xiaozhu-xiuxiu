# Development

Run the commands below from the repository root. For installation and editing, see the [user guide](USAGE.md).

## Prerequisites

Requires Rust **1.88+** and a C toolchain.

### Linux

Use a Linux desktop with working Vulkan drivers. Wayland and X11 are supported; Mesa software Vulkan can also run the editor. Native file dialogs use the desktop portal, so install the portal backend for your desktop if dialogs do not appear.

Typical Debian/Ubuntu prerequisites:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev \
    libvulkan1 mesa-vulkan-drivers xdg-desktop-portal
```

HEIC/HEIF import uses the bundled pure Rust `heic-rs` decoder on Linux and Windows. No `libheif` installation or `heif-convert` executable is required. Nikon NEF/NRW, Canon CR2/CR3/CRW, Fujifilm RAF, and Sony ARW import use the bundled Rawler library and also need no external converter. HEIC regression fixtures are included in `src/io/fixtures`; `cargo test --locked heif` covers decoding, orientation, limits, and project persistence, and `cargo test --locked heic_opens` covers document/layer import.

### Windows

Install the x86_64 MSVC Rust toolchain (`stable-x86_64-pc-windows-msvc`) and Visual Studio Build Tools with **Desktop development with C++** and a Windows SDK. Use Windows 10/11 with a DirectX 12 or Vulkan driver. Native file dialogs and the clipboard use Windows APIs. Packaging also requires Python **3.11+** on `PATH`.

## Build and run

```sh
cargo run --release --locked
cargo run --release --locked -- --demo
cargo run --release --locked --bin xiaozhu-xiuxiu -- photograph.png composition.xuan
```

To install a local Linux build:

```sh
cargo build --release --locked
scripts/install.sh                 # installs under ~/.local
scripts/install.sh /custom/prefix   # optional destination
```

The installer adds a desktop launcher, icons, and the `.xuan` file association. Add the installation prefix's `bin` directory to `PATH`.

## Packaging

On Windows, run from PowerShell:

```powershell
python scripts/package-windows.py
python scripts/check-packages.py
```

This builds `xiaozhu-xiuxiu-<version>-windows-x86_64.zip`, its SHA-256 checksum, and the matching source archive in `dist/`. The ZIP includes `xiaozhu-xiuxiu.exe` plus licenses and user guides under `share/`. The release executable uses the Windows GUI subsystem and statically links the MSVC runtime, so it opens without a console window and needs no Visual C++ redistributable. The script builds the explicit `x86_64-pc-windows-msvc` target on an x86_64 Windows host. The ZIP is unsigned and does not install file associations.

On Linux:

```sh
scripts/package.sh                 # portable archive (default)
scripts/package.sh deb             # Debian/Ubuntu package
scripts/package.sh rpm             # RPM package
scripts/package.sh appimage        # standalone AppImage
scripts/package.sh all             # all four binary formats and matching sources
python3 scripts/check-packages.py  # inspect binary packages and source archive
python3 scripts/check-packages.py --appimage  # inspect only AppImage and sources
```

Outputs and individual SHA-256 checksums are written to `dist/`. Both packaging scripts also produce `xiaozhu-xiuxiu-<version>-source.tar.gz`; distribute that matching source archive alongside the binaries. Linux packaging requires Python **3.11+**, binutils, and the normal build prerequisites. Debian packaging additionally needs `dpkg-deb`; RPM packaging needs `rpmbuild`. AppImage verification needs `unsquashfs` from `squashfs-tools`. On Debian/Ubuntu, install the packaging and inspection tools with `sudo apt install dpkg rpm cpio binutils desktop-file-utils squashfs-tools`. Linux packaging supports native x86_64 and aarch64 builds. The packaging scripts do not support cross-compilation.

AppImage packaging additionally requires [linuxdeploy](https://github.com/linuxdeploy/linuxdeploy/releases/tag/1-alpha-20251107-1) with its AppImage output plugin and [patchelf 0.19.1](https://github.com/NixOS/patchelf/releases/tag/0.19.1), available as `linuxdeploy` and `patchelf` on `PATH`, plus `ldconfig` and the desktop libraries listed in `scripts/package-native.py`. On Debian/Ubuntu, also install `libxkbcommon-x11-0`. The separate patchelf avoids startup crashes caused by linuxdeploy's older bundled version rewriting modern ELF libraries. CI downloads pinned releases of both tools and verifies their SHA-256 checksums before use. Packaging runs without FUSE; the AppImage output plugin downloads the runtime, so internet access is required.

The AppImage includes the shared application payload, an `AppRun` launcher, and X11/Wayland libraries (including libraries loaded through `dlopen`) with their license notices. The image also bundles glibc and its matching loader, plus their license notices, using `scripts/linuxdeploy-plugin-xuan`. This input plugin runs after linuxdeploy rewrites ELF files so it does not modify glibc or the loader. The Vulkan loader, GPU drivers, and desktop portals come from the host. `AppRun` uses the bundled libc on older hosts and the matching system libc/loader on newer hosts, whose graphics drivers may require newer glibc symbols. It passes the library search path to the loader without exporting a private glibc path to child processes. `AppRun` preserves the calling directory and arguments, so opening relative file paths works. Package verification extracts the image without FUSE, checks its contents and checksum, rejects a dynamically linked AppImage runtime, verifies that the bundled loader resolves the executable's linked libraries inside the image, and runs both the extracted launcher and the AppImage's extract-and-run mode. AppImages are named `xiaozhu-xiuxiu-<version>-<architecture>.AppImage`.

A portable Linux archive includes `bin/xiaozhu-xiuxiu`, `scripts/install.sh`, and a `share/` directory for desktop integration, icons, licenses, and user guides. It can run directly after extraction. Debian and RPM packages install the same application files under `/usr`. Binary packages omit source code, CI workflows, original artwork, development guides, and screenshots. Their `share/doc/xuan/SOURCES.md` notice points to the exact source download; links to development documentation point to the release's repository tag.

The separate source archive contains the application and build assets, the patched egui-winit sources, and the exact Rawler sources. Its manifest and lockfile use the bundled Rawler directory so `cargo build --release --locked` works after extraction. Other dependencies are downloaded from the Cargo registry. See [third-party notices](../THIRD_PARTY.md).

Package versions come from `Cargo.toml`. Prerelease versions such as `0.2.0-rc.1` become `0.2.0~rc.1` in Debian/RPM metadata so they sort before the final release. Build metadata (`+...`) is not supported. The Debian package records the executable's required glibc version; RPM derives ELF library requirements automatically. Both declare desktop libraries that are loaded at runtime.

Build on the oldest distribution you intend to support. The [CI workflow](../.github/workflows/linux.yml) builds on Ubuntu 22.04 with Python 3.12 and sets `XUAN_MAX_GLIBC=2.35`. Packaging and verification check the executable's undefined ELF symbols against that ceiling, so a newer build environment cannot silently raise the release requirement. To apply the same check locally, run `XUAN_MAX_GLIBC=2.35 scripts/package.sh all`. Local builds without this setting use the build system's glibc and may require a newer distribution.

## GitHub releases

The [release workflow](../.github/workflows/release.yml) runs when a `v*` tag is pushed. It rejects tags that do not match the package version in `Cargo.toml`, then runs the shared Linux and Windows checks. Linux builds all four x86_64 packages and the matching source archive on Ubuntu 22.04, tests AppImage startup without FUSE under Xvfb, and tests Debian installation, native startup, and removal. Additional container jobs launch the AppImage on Ubuntu 20.04, 22.04, and 24.04 and the native archive on 22.04 and 24.04. The 20.04 check verifies that the bundled libc is actually loaded before capturing a demo screenshot. All compatibility jobs must pass before publication. The [Windows workflow](../.github/workflows/windows.yml) runs on Windows Server 2022, builds the portable MSVC x86_64 ZIP, verifies its payload and checksums, and launches the packaged application with DirectX 12 to capture a demo screenshot. Both jobs must pass before publishing. Package checks reject development files in binary payloads and verify the source archive's vendored dependencies and resolved lockfile.

To release, update the version in `Cargo.toml` and the `xiaozhu-xiuxiu` entry in `Cargo.lock`, commit the changes, then create and push the matching tag. For example, for version `0.2.0`:

```sh
git tag -a v0.2.0 -m 'Release v0.2.0'
git push origin v0.2.0
```

After validation succeeds, [changelogithub](https://github.com/antfu-collective/changelogithub) generates release notes from conventional commits. The GitHub CLI uploads the four Linux binary packages, Windows ZIP, matching source archive, and all six checksums. Missing artifacts fail the workflow before publication; retries replace matching assets and upload failures fail the workflow. The workflow fetches the full Git history and uses pinned changelogithub **15.0.5** with Node.js 24. Only the publishing job receives `contents: write`; it uses the built-in `GITHUB_TOKEN` and needs no separate release secret. Prerelease tags such as `v0.2.0-rc.1` are marked as GitHub prereleases.

Preview release notes locally without publishing:

```sh
npx --yes changelogithub@15.0.5 --dry --to HEAD --github yishiyi28/xiaozhu-xiuxiu
```

The first release uses the available commit history; subsequent notes start after the preceding release tag. To retry a failed release, rerun its workflow in GitHub Actions.

## Application icons

Application and desktop icons are generated from [`assets/小猪修修.png`](../assets/小猪修修.png). After changing the logo, run:

```sh
scripts/generate-icons.sh
```

This requires ImageMagick 7. The generated transparent PNGs cover sizes from 16 to 1024 pixels and are committed, so building, packaging, and installing do not require ImageMagick. Rebuild the application after regenerating the icons.

## Tool icons

Toolbar and layer controls embed the icons in [`assets/svg`](../assets/svg)
with `egui::include_image!` in `src/app/icons.rs`. The Gradient icon is drawn in Rust.
SVG image loaders are registered when the editor starts; egui caches the rendered
image at the display's pixel scale.
Monochrome SVGs use `color="white"` with `stroke="currentColor"` so the icon can be
tinted with the toolbar's text color. Rebuild the app after editing an embedded SVG.

## Checks

```sh
scripts/check.sh          # formatting, Clippy, engine and UI tests
scripts/check.sh --gpu    # also compares wgpu output against the CPU reference
```

On Windows, run the equivalent checks in PowerShell:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --package egui-winit --lib clipboard_paste
```

The GPU checks require a working graphics environment. CI also validates the desktop entry, builds the release archive, and runs native screenshot and clipboard checks under Xvfb. See [implementation and verification notes](PORTING.md) for the architecture and recorded results.

## Tablet input checks

Tablet regression checks run with `cargo test --locked tablet`. They cover native
Wayland protocol frames through an in-process compositor, XInput valuator decoding,
pointer/pressure conversion, focus loss and device removal, multi-sample strokes,
eraser switching, tilt footprints, and undo. The GPU brush checks compare varying
pressure and tilt against the CPU implementation. The Linux tablet workers use
event-driven input and stop before eframe destroys its window; Windows intercepts
Ink pen messages before winit's touch conversion.

For hardware verification on both Linux backends and Windows, check hover, light
and heavy strokes, fast curves, tilted strokes with **Tilt: shape** enabled, eraser
tips, barrel-button panning, toolbar clicks, mouse switching, hot-unplug during a
stroke, and driver-configured shortcuts. Repeat at non-default display and UI
scales. Automated tests do not certify individual Wacom or Parblo models.

## RAW sample checks

The regular test suite uses synthetic camera-linear data and small embedded-asset fixtures. Camera files are not committed to the repository. Optional tests use a local Nikon, Canon, Fujifilm, or Sony RAW file:

```sh
XUAN_TEST_RAW=/path/to/photo.CR3 cargo test --locked sample_raw -- --ignored --nocapture
cargo run --locked -- /path/to/photo.CR3 --screenshot /tmp/develop.png
```

`XUAN_TEST_RAW_PREVIEW=/tmp/preview.png` optionally writes the engine test's default preview. The sample checks cover full-resolution rendering, project save/load, reopening Develop and cancellation. Verified samples include Nikon Z6 III NEF (4032 × 6048 after orientation) and Canon EOS M50 Mark II CR3 (4000 × 6000 after orientation). `XUAN_TEST_NEF` remains accepted as a fallback for existing local test commands.

Set `XUAN_TEST_RAW_QUARTER_TURNS=1` (or 2/3) to include clockwise quarter turns
in the engine sample roundtrip and its optional preview. Rotation regressions
also compare exact CPU pixel placement at 8/16 bits, GPU output and resident
previews, crop/picker coordinates, before/after alignment, and Develop Undo/Redo.

RAF/ARW verification also covers these seven local samples through both the engine
roundtrip and the Develop commit/reopen/cancel tests:

| Camera | Samples | Oriented dimensions |
| --- | --- | --- |
| Fujifilm X70 (X-Trans) | `DSCF0062.raf`, `DSCF1276.raf`, `DSCF9609.raf` | 4896 × 3264 |
| Fujifilm X100S (X-Trans) | `DSCF9791.raf` | 4896 × 3264 |
| Sony ILCE-7M2 | `3307522568.arw` | 4000 × 6000 |
| Sony DSC-RX100M2 | `RAW_SONY_DSC-RX100M2.arw` | 3648 × 5472 |
| Sony DSC-RX100 | `RAW_SONY_RX100.arw` | 5472 × 3648 |

Synthetic X-Trans checks exercise all 36 pattern phases, image borders, repeating
black levels, values above sensor white, and active/default crops with unaligned
origins. They also verify that demosaicing retains each measured color sample.

## Screenshots

Refresh the README screenshot from the current release build:

```sh
cargo run --release --locked -- --demo --screenshot docs/screenshots/editor.png
```

The screenshot helper captures the real native window and exits. Inspect the resulting image before committing it. To capture a panel:

```sh
cargo run --release --locked -- --demo --screenshot /tmp/levels.png --screenshot-panel levels
```

The helper also supports `hue`, `curves`, `export`, `new`, `brush`, `selection`, `gradient`, `shape`, `text`, and `settings`. On a Wayland desktop with XWayland available, prefix the command with `env -u WAYLAND_DISPLAY` to capture the X11 path.

## Benchmarks

To benchmark large-image zoom and editing updates on a GPU:

```sh
cargo test --release --locked --bin xiaozhu-xiuxiu benchmark_large_image -- --ignored --nocapture --test-threads=1
```

These measure UI updates, tessellation, and compositor completion on a 3000×3000 image: 48 zoom steps and 24 pointer updates each for moving a layer, marquee, lasso, brush, and eraser, plus gesture release. The Levels benchmark also measures 24 pointer updates and 24 live preview changes with its adjustment-layer dialog open. Set `XUAN_ZOOM_BENCH_IMAGE` to use a local image instead of the generated image. Window presentation is not included.

Verify pixel-grid alignment at 6400% zoom with GPU-rendered frames at 1× and 1.7× display scale:

```sh
cargo test --locked --bin xiaozhu-xiuxiu magnified_gpu_preview -- --ignored
```

This uses a 5712-pixel-wide test image by default. Set `XUAN_ZOOM_BENCH_IMAGE` to an opaque photo to check its original pixels, and optionally set `XUAN_PIXEL_GRID_SCREENSHOT=/tmp/pixel-grid.png` to save the rendered frame.

The Motion Blur benchmarks measure live GPU preview updates and full-resolution Apply, including GPU readback, against CPU filtering at distances 15 and 200. Apply uses original layer pixels even when the preview texture is downsampled. See the [GPU processing audit](GPU_PROCESSING.md) for backend routing, CPU exceptions, and transfer-inclusive benchmarks.

To measure standalone filter-layer slider updates on a local image:

```sh
XUAN_ZOOM_BENCH_IMAGE=/path/to/photo.jpg cargo test --release --locked --bin xiaozhu-xiuxiu benchmark_large_image_filter_layers -- --ignored --nocapture --test-threads=1
```

The image benchmarks also accept RAW files, which are developed before timing.
Filter-layer measurements cover 12 changing settings after three warm-up frames,
including UI updates, tessellation, and GPU completion. They use the native app's
GPU resource limits and preview resolution cap; decoding, initial uploads, shader
compilation, and window presentation are excluded.


### RAW preview performance

Benchmark the worker's resident GPU path and UI texture registration separately:

```sh
XUAN_TEST_RAW=/path/to/photo.CR3 cargo test --release --locked --bin xiaozhu-xiuxiu benchmark_raw_develop_preview -- --ignored --nocapture --test-threads=1
```

Without a camera file, this generates a 24-megapixel linear RGB image. It measures
1600-pixel, 3200-pixel, and full-resolution previews, with and without clipping
textures. Exposure changes between runs; reported medians exclude the first
allocation/compilation run and include GPU completion plus histogram readback.
Registration timings exclude window presentation. See the [GPU processing
audit](GPU_PROCESSING.md) for sample measurements and the remaining decode costs.

### Localization and negative conversion checks

UI text uses `xuan::i18n::tr` with the English text as its key. Simplified Chinese
translations are UTF-8 tab-separated pairs in `assets/locales/zh-CN.tsv`. Keep UI
IDs, command identifiers and user document content independent of translated labels.
The UI test checks the settings shortcut, live language changes and bundled glyph
coverage. Configuration tests use temporary directories.

```sh
cargo test --locked negative
cargo test --locked settings_shortcut
cargo test --locked --lib gpu::processing_tests::processing_raw_matches_cpu_at_both_depths -- --ignored
XUAN_TEST_RAW=/path/to/negative.CR2 XUAN_TEST_RAW_PREVIEW=/tmp/positive.png \
  cargo test --locked --lib sample_negative_raw -- --ignored --nocapture
```

The optional sample check writes positive and original previews, develops the full
resolution, compares 8/16-bit output and verifies project persistence. Camera files
are kept outside the repository.

For scans containing a film holder, set `XUAN_TEST_RAW_CROP` to normalized
`left,top,right,bottom` bounds when running the sample check. This models the
**crop → Analyze crop** workflow and excludes the holder from calibration.
