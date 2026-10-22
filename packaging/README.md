# 小猪修修

A native image editor for layered compositions, photo retouching, and camera RAW development on Linux and Windows.

On Windows, extract the entire ZIP and double-click `xiaozhu-xiuxiu.exe`, or run `.\xiaozhu-xiuxiu.exe --demo` in PowerShell to explore a sample composition.

On Linux, launch Xuan from your application menu or run `xiaozhu-xiuxiu --demo`. In a portable Linux archive, run `bin/xiaozhu-xiuxiu` directly or use `scripts/install.sh` to install under `~/.local`.

Native Linux releases require glibc 2.35 or newer. The AppImage includes glibc and its loader for older systems.

For an AppImage, make the downloaded file executable with `chmod +x xiaozhu-xiuxiu-*.AppImage`, then run `./xiaozhu-xiuxiu-*.AppImage --demo`. If FUSE is unavailable, add `--appimage-extract-and-run` before `--demo`.

- [Installation and editing](../docs/USAGE.md)
- [Keyboard shortcuts](../docs/SHORTCUTS.md)
- [RAW workflow](../docs/RAW.md)
- [Source download and rebuilding](SOURCES.md)
- [Third-party notices](../THIRD_PARTY.md)

小猪修修 is [MIT licensed](../LICENSE), with separately licensed dependencies. Original Compositor copyright © 2026 Wonder Assembly LLC.
