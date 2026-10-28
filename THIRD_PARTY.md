# Third-party components

小猪修修's own source code is MIT licensed. Dependency licenses remain applicable to their respective code.

- **Rawler 0.7.2** provides Nikon and Canon RAW decoding, camera calibration data, and PPG demosaicing. Copyright Daniel Vogelbacher, Pedro Côrte-Real, and the Rawler contributors. Licensed under the GNU LGPL 2.1; see [the license](licenses/rawler-LGPL-2.1.txt) and [upstream source](https://crates.io/crates/rawler/0.7.2).
- **heic-rs 0.1.1** provides HEIC/HEIF decoding in Rust. The decoder and synthetic regression fixtures are used under the [MIT license](licenses/heic-rs-MIT.txt); see [upstream source](https://crates.io/crates/heic-rs/0.1.1) and [fixture provenance](src/io/fixtures/README.md).
- **MediaPipe BlazeFace and Face Mesh ONNX models**: local 478-point face landmark analysis. Copyright Google LLC. Licensed under [Apache License 2.0](licenses/mediapipe-apache-2.0.txt); see [model provenance](licenses/mediapipe-onnx-models.md).
- **Droid Sans Fallback Full**: Chinese UI and filename glyphs, Copyright 2005–2008 The Android Open Source Project, [Apache License 2.0](assets/fonts/DroidSansFallback-LICENSE.txt). Bundled unmodified from the Android Droid font family.
- **Inter Variable**: [SIL Open Font License](assets/fonts/Inter-LICENSE.txt).
- **Tabler Icons**: toolbar and layer-control SVGs use [Tabler Icons](https://tabler.io/icons), licensed under the [MIT license](licenses/tabler-icons-MIT.txt).
- **egui-winit**: [MIT](vendor/egui-winit/LICENSE-MIT) / [Apache 2.0](vendor/egui-winit/LICENSE-APACHE), with local patches retained in the accompanying source archive.
- **AppImage runtime libraries**: Linux AppImages also bundle X11/Wayland client libraries, libgcc, glibc, and its matching dynamic loader. Their copyright and license notices are included under `usr/share/doc/` and `usr/share/licenses/` inside the image, with glibc notices under `usr/share/licenses/glibc/`; extract it with `--appimage-extract` to inspect them.
- **sRGB ICC profile**: generated with Little CMS through Pillow's `ImageCms.createProfile("sRGB")`. The profile is embedded in Develop's 16-bit TIFF exports; generating it is not a build or runtime requirement.

Every release provides a matching `xiaozhu-xiuxiu-<version>-source.tar.gz` alongside the binary packages on [GitHub Releases](https://github.com/yishiyi28/xiaozhu-xiuxiu/releases). It contains 小猪修修 build sources, the patched egui-winit sources, and the exact Rawler sources used in the executable. Binary packages include a `SOURCES.md` notice with the exact archive name and download link. Distribute this source archive alongside the binaries and retain equivalent access to it when redistributing them.

To rebuild or relink with a modified Rawler, extract the source archive into a writable directory, enter `xiaozhu-xiuxiu-<version>-source`, edit `vendor/rawler` if desired, then run `cargo build --release --locked`. A Rust toolchain, a C toolchain, the platform prerequisites listed in the [development guide](docs/DEVELOPMENT.md#prerequisites), and access to the Cargo registry for other dependencies are required. The source manifest patches Rawler to the included directory, and its lockfile is resolved for that path dependency. GitHub's automatically generated source archives do not include these vendored Rawler sources.
