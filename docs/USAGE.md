# Using 小猪修修

## Install and launch

小猪修修 runs on Windows 10/11 (x86_64) with DirectX 12 or Vulkan, and on Linux with Wayland or X11 and working Vulkan drivers. Mesa software Vulkan can also run the editor. On Linux, native file dialogs use the desktop portal; install the portal backend for your desktop if dialogs do not appear.

Download the package for your operating system from [Releases](https://github.com/yishiyi28/xiaozhu-xiuxiu/releases). GitHub releases provide x86_64 builds.

### Windows

Extract `xiaozhu-xiuxiu-<version>-windows-x86_64.zip` into a writable folder and double-click `xiaozhu-xiuxiu.exe`. To open a sample composition or specific files from PowerShell:

```powershell
.\xiaozhu-xiuxiu.exe --demo
.\xiaozhu-xiuxiu.exe photograph.png composition.xuan
```

The portable package needs no installer or administrator access. Keep the accompanying `share` directory for documentation and licenses. To remove the application, delete the extracted folder. Releases are unsigned.

To verify a download in PowerShell, compare the hash to the matching `.sha256` file:

```powershell
Get-FileHash .\xiaozhu-xiuxiu-<version>-windows-x86_64.zip -Algorithm SHA256
Get-Content .\xiaozhu-xiuxiu-<version>-windows-x86_64.zip.sha256
```

### Linux

For a standalone AppImage, make the downloaded file executable and launch it:

```sh
chmod +x xiaozhu-xiuxiu-<version>-x86_64.AppImage
./xiaozhu-xiuxiu-<version>-x86_64.AppImage --demo
```

Replace `<version>` with the downloaded version. The AppImage bundles 小猪修修, its X11/Wayland client libraries, glibc, and a matching dynamic loader; it needs no installation or administrator access. It can run on systems with older glibc, including Ubuntu 20.04. Your system still supplies the Vulkan loader, graphics drivers, and desktop portal. On systems with a newer glibc than the bundled copy, the launcher uses the system's matching libc and loader to support newer graphics drivers. If FUSE is unavailable, run `./xiaozhu-xiuxiu-<version>-x86_64.AppImage --appimage-extract-and-run --demo`. To remove it, delete the AppImage. It does not install a launcher or file associations.

Native Linux releases (`.deb`, `.rpm`, and `.tar.gz`) require glibc **2.35** or newer, as provided by Ubuntu 22.04. Use the AppImage on older distributions.

On Debian or Ubuntu, install the downloaded `.deb` with APT so runtime dependencies are installed too:

```sh
sudo apt install ./xiaozhu-xiuxiu-*-linux-x86_64.deb
```

On Fedora or another compatible RPM distribution using DNF:

```sh
sudo dnf install ./xiaozhu-xiuxiu-*-linux-x86_64.rpm
```

Install one version at a time. These packages add 小猪修修 to the application menu and provide the `xuan` command. Remove them with `sudo apt remove xuan` or `sudo dnf remove xuan`.

For a portable `xiaozhu-xiuxiu-<version>-linux-<architecture>.tar.gz` archive, extract it and run:

```sh
scripts/install.sh                 # installs under ~/.local
~/.local/bin/xiaozhu-xiuxiu
```

The installer adds a desktop launcher, icons, and the `.xuan` file association. Use `scripts/install.sh /custom/prefix` to choose another location, and add the installation prefix's `bin` directory to `PATH`. You can also run `bin/xiaozhu-xiuxiu` directly from the extracted archive.

Each Linux download has a matching `.sha256` file. From the download directory, verify it with `sha256sum --check <package-file>.sha256`.

### Sources and documentation

The separate `xiaozhu-xiuxiu-<version>-source.tar.gz` download is for rebuilding the application. The binary packages include a `SOURCES.md` notice under `share/doc/xuan` (`/usr/share/doc/xuan` for Debian/RPM installations) with a link to the matching source release.

Once `xiaozhu-xiuxiu` is on `PATH`:

```sh
xiaozhu-xiuxiu --demo
xiaozhu-xiuxiu photograph.png composition.xuan
```

To compile the application or produce a release archive, see the [development guide](DEVELOPMENT.md). Linux archives built on newer distributions may require a newer glibc; build from source on your target distribution if needed.

## Workspace

小猪修修 has a charcoal theme, contextual controls above the canvas, a vertical tool rail, document tabs, and a Layers panel. The menu bar shares the titlebar with the window controls. Drag the titlebar to move the window, double-click to maximize, or drag an edge to resize.

## Editing tools

- **Layers:** folders, 13 blend modes, opacity, visibility, locks, drag reordering/nesting, duplication, merge, clipping masks, linked or independent raster masks, and copying layers to another project tab.
- **Transforms:** move, scale, rotate, flip, free perspective distortion, numeric controls, shared transforms for several layers or folders, and snapping to edges and centers. Original source pixels remain available during transforms. Move / Transform has **Ignore Transparent Pixels** checked by default to select only at visible pixels; uncheck it to select and drag anywhere inside a layer's bounds.
- **Selections:** rectangle, ellipse, freehand/polygonal lasso, contiguous/global magic wand, add/subtract/intersect, inverse, feather, outline movement, and moving or duplicating selected pixels.
- **Paint:** brush, eraser, aligned/unaligned clone stamp with layer/all-layer sampling, spot healing, blur/smudge, gradients, rectangles, rounded rectangles, ellipses, and eyedropper. Live shapes redraw at the new size until their pixels are edited.
- **Text:** editable multiline text layers, searchable installed font families, size and color, bold, italic, underline, and strikethrough. Click with the Text tool (T) to place or edit text; double-click a text layer to reopen its live preview. Move and transform text with the Move tool.
- **Adjustments:** editable Hue/Saturation color ranges, per-channel Levels and Curves, Exposure, Gradient Map, Grain, and Invert. Apply directly or add an adjustment layer, with live preview and selection coverage.
- **Filters:** Gaussian and Motion Blur with expanded bounds, Add Noise, Lens Correction, Unsharp Mask, High Pass, Posterize, Threshold, Pixelate, Vignette, content-aware fill, and edge-color background removal. Filter layers preview the resident composite on the GPU and fall back to the CPU reference for materialized renders. Filtering and expensive retouching run in cancellable workers.
- **RAW Develop:** Nikon NEF/NRW, Canon CR2/CR3/CRW, Fujifilm RAF, and Sony ARW open in a dedicated Develop workspace. Adjust white balance, exposure, tone curves, HSL, monochrome and split toning, noise reduction, sharpening, manual lens correction, crop, and brush/gradient masks. Compare before/after and inspect clipping or full-resolution detail. Develop creates an embedded RAW layer; double-click it to edit the original RAW again. Save `.xuan` to retain the source and adjustments, or export a 16-bit sRGB TIFF directly from Develop. See [RAW workflow and limits](RAW.md).
- **Documents:** independent tab histories, crop, canvas/image size, high-quality downsampling, pixel grid, pasting copied images or image files as layers, Copy Merged, and save-on-close prompts. Undo retains up to 64 steps with a 512 MiB asset budget, keeping at least one step.

Double-click a layer name to rename it inline. Press Enter or click elsewhere to
save, or Escape to cancel. **Rename…** in the layer's context menu opens the same
inline editor. Double-click elsewhere on a text, RAW, filter, or adjustment row
to reopen its settings.

See [keyboard shortcuts](SHORTCUTS.md) for tool and command bindings.

### Brush stroke smoothing

Select a painting tool and increase **Smoothing** in its toolbar to reduce small
shakes in mouse or pen strokes. **0%** (the default) turns smoothing off; higher
values produce steadier curves with more distance between the pointer and the
painted tip. The brush outline follows the painted tip, and the smoothing distance
stays consistent on screen when you zoom. Release the mouse or lift the pen to
finish the stroke at its final input position, as a single undo step.

Smoothing applies to the brush, eraser, clone stamp, blur/smudge, spot healing,
and painting on layer masks. Pressure and tilt continue to control the brush.
Shift-click straight lines bypass smoothing. RAW Develop's mask brushes are
unchanged. **Hardness** controls edge softness independently of stroke smoothing.

### Drawing tablets

Wacom, Parblo, and other tablets supported by your system's driver can draw and
operate the interface with the pen. Linux uses native Wayland tablet-v2 or
XInput2 on X11/XWayland. Windows uses Windows Ink; enable **Windows Ink** in the
tablet driver's settings for 小猪修修. A driver that supplies only mouse events
still works as a mouse, without pressure, tilt, or eraser identification.

Pressure controls brush size by default. Open **Pen dynamics** in the painting
toolbar to control these independently:

- **Pressure: size** scales the selected brush size with pen pressure.
- **Pressure: opacity** scales the selected opacity with pen pressure.
- **Tilt: shape** flattens and rotates the brush footprint with the pen's tilt.
  The cursor outline previews the footprint. This option starts disabled.

These controls apply to raster painting, erasing, clone stamp, blur/smudge,
healing, and layer masks. Missing pressure data uses the selected size and
opacity; missing tilt data gives a circular brush. Flipping a pen with an eraser
tip temporarily erases without changing the selected tool. Hovering does not
paint, and lifting the tip ends the stroke as one undo step. Fast strokes retain
intermediate pen samples and interpolate their size, opacity, and tilt.

Pen buttons use the platform's secondary and middle pointer actions;
middle-drag pans the canvas. Configure express keys and touch rings as keyboard
shortcuts in your driver or compositor; 小猪修修 uses its normal shortcut bindings.
There is no separate tablet-button mapping editor. RAW Develop's local adjustment
brushes retain their existing fixed-size behavior.

No root access or direct access to `/dev/input` is needed. The tablet must first
work in the desktop session. When reporting a problem, include the tablet model,
driver, desktop/compositor, and whether 小猪修修 is using Wayland, X11, or Windows Ink.

### Attached image effects

An image can contain multiple masks, filters, and adjustment layers. Use its
chevron to expand or collapse the attached layers. Each child affects only that
image; effects run from the bottom child upward. Drag children to reorder them,
use their eye icons to bypass them, and double-click a filter or adjustment to
edit its settings. The original image pixels remain unchanged.

**Layer → New Adjustment Layer** and **Layer → New Filter Layer** create standalone
layers that affect the stack below. Drag an effect onto the middle of an image
row to attach it. Drag it beside an outside row or use **Move Out of Parent** to
detach it. The existing **Filter** menu still applies raster edits directly.

**Add Layer Mask** adds a new mask child to the selected image (or the image of
the selected child); it can be used repeatedly. **New Mask Layer** creates a
standalone mask. Select a mask child to paint it, and use **Link / Unlink** to
control whether it follows its image's transforms. Older single image masks are
shown as children when a project is opened. Save as `.xuan` to preserve the stack.

## Portrait retouch and presets

Choose **Edit → Portrait Retouch…** (or press `P`) with a pixel layer selected. The first portrait session loads the embedded local models and detects up to eight faces. Select a face chip, adjust skin, eye, teeth, lip and face-shape controls, then use **Apply**. The result is a non-destructive **Portrait Retouch** adjustment layer; the source image pixels remain untouched.

The preview supports a manual liquify brush: choose Push, Pinch or Bloat, set size, strength and hardness, then drag directly on the preview. Hold the original-view checkbox to compare the untouched source. Portrait layers persist face landmarks, settings and liquify strokes in `.xuan` projects. Double-click the layer, or use **Edit portrait…** in its context menu, to reopen the recipe. Hide, delete or mask the layer to control its contribution.

Presets contain color grade and portrait settings. Apply a built-in preset from **File → Presets**, or use **Manage Presets…** to import/export `.xzp` files. User presets are stored under `%APPDATA%\xiaozhu-xiuxiu\presets\` on Windows or the matching XDG configuration directory on Linux. Applying a preset is one history transaction; invalid files are rejected without changing the document.

Portrait analysis is local-only. Photos, landmarks and presets are never uploaded. RAW batch processing and automatic body landmarks are not part of 0.6.2; use manual liquify for body adjustments.

### Batch export

Use **File → Batch Export…** or the CLI:

```powershell
xiaozhu-xiuxiu --batch .\photos --out .\output --preset .\look.xzp --format jpeg --quality 92 --max-edge 2048 --suffix _edit
```

Add `--recursive` to include subdirectories. Existing outputs can be skipped, replaced with `--overwrite`, or automatically numbered with `--auto-number`. With a preset, the default suffix is `_xiaozhu`; without `--preset`, the original batch behavior is preserved. Batch failures are reported per file and do not stop the remaining images.

## Files and export

Use **File → Open Compositor Project…** to import an original `.comp` folder package. Save it as `.xuan` to keep editing in 小猪修修. Image export supports PNG, JPEG, TIFF, and WebP; JPEG has a quality preview and PNG/JPEG carry print resolution.

HEIC/HEIF photos (`.heic`, `.heif`, and `.hif`, including uppercase extensions) open directly on Linux and Windows using the bundled decoder. Use File → Open, import as a layer, or drag a photo into the editor. The primary still image is imported, including tiled images and container rotation/mirroring; sequences and unsupported HEVC coding features report an error. Images use the editor's 8-bit raster pipeline and are limited to 512 MiB per file, 30,000 pixels per side, and 100 megapixels. Saved `.xuan` projects embed the decoded pixels, so the original HEIC file is no longer required. HEIC export is not supported. Nikon NEF/NRW, Canon CR2/CR3/CRW, Fujifilm RAF, and Sony ARW import use the bundled Rawler library. No external converter is required for these formats. See the [project format](FORMAT.md) for details about saved documents.

## Current limits

The photo editor uses an 8-bit sRGB raster pipeline. RAW Develop uses floating-point camera data and offers direct 16-bit TIFF output with an sRGB profile; its photo-layer render uses the existing 8-bit pipeline. Imported raster ICC profiles are not converted or preserved. `.comp` versions 1–7 can be imported; 小猪修修 does not write the original macOS format. Selections and undo history are session state and are not saved in project archives.

Remove Background uses a border-color matte, intended for simple backgrounds, instead of Apple's foreground-recognition service. Content-aware fill and healing use a portable texture-matching implementation, so results differ from Compositor. Initial zoomed-out canvas previews are capped at 4096 pixels per side. At 100% zoom and above, the preview uses full document resolution up to the device's texture limit; the pixel grid appears from 800% when individual document pixels can be displayed. Filter Apply uses full layer dimensions and export uses full document dimensions. Imports, saves, and raster adjustments can temporarily occupy the UI thread. Vulkan is the verified rendering path; OpenGL surface availability depends on the driver.

## Language and settings

Open **Edit → Settings…** (Ctrl+,). The sidebar's **General** category contains the
language selector: **English** or **简体中文**. Changes apply immediately and are
saved automatically. Chinese glyphs are bundled with the application. The
**Autosave interval** slider controls crash-recovery snapshots (default 120
seconds; 0 disables them).

While a project has unsaved changes, 小猪修修 writes an atomic snapshot under
`%LOCALAPPDATA%\xuan\recovery` on Windows, or `$XDG_DATA_HOME/xuan/recovery`
(falling back to `~/.local/share/xuan/recovery`) on Linux. If the app exits
unexpectedly, the next launch offers to restore or discard each snapshot.
Saving or discarding a project removes its snapshot. Snapshots older than two
weeks are pruned automatically.

Preferences are separate from projects and window layout:

- Linux: `$XDG_CONFIG_HOME/xiaozhu-xiuxiu/config.toml`, or `~/.config/xiaozhu-xiuxiu/config.toml` when
  `XDG_CONFIG_HOME` is unset or not an absolute path.
- Windows: `%APPDATA%\xiaozhu-xiuxiu\config.toml`.

The file is created when a preference changes. For example:

```toml
language = "zh-CN" # Use "en" for English (the default).
autosave_seconds = 120 # Crash-recovery interval; 0 disables snapshots.
```

Invalid or unreadable configuration is reported and the app starts with defaults.
Saving uses an atomic replacement and preserves other TOML options. An invalid
existing TOML file must be corrected before settings can be saved. Operating-system
file dialogs and technical diagnostics may follow the system language or English.
