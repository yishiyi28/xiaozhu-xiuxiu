# 小猪修修（原 XUAN）更新日志

## 0.7.0

- Add a task-first welcome screen with single-portrait, batch-export and professional-edit entry points.
- Add one-click Natural, Clear Portrait and Warm portrait templates that open an image and apply the recipe immediately.
- Add a template review dialog with a local low-resolution preview, original comparison, photo-based recommendation, and recommendation reasoning.
- Add one-click retry for only the failed files in a batch report.
- Add selectable batch results with source/output tracking and equal-size before/after previews inside a scrollable report.
- Add per-file batch preset overrides so individual photos can use a different built-in or user recipe while keeping the global default for the rest of the folder.
- Add a portrait-workspace sync action that opens a pre-filled batch job for applying the current face settings to a whole folder, with automatic face detection per photo.
- Make the welcome screen switch to compact single-column action cards in the minimum window size.
- Add English and Simplified Chinese coverage plus interaction and workflow regression tests for the new start screen.

## 0.6.9

- Fix batch processing of ordinary image files so the imported image layer becomes active before a portrait preset analyzes it.
- Restore batch portrait presets on JPEG, PNG, TIFF, WebP, BMP, GIF and HEIF inputs.
- Add a regression test ensuring loaded batch images expose an active pixel layer.

## 0.6.8

- Complete the GPU portrait-layer compositing path for runtime masks, including face reshaping and manual liquify.
- Compose portrait color, GPU warp, blur, layer opacity, coverage and blending without an intermediate CPU readback for supported previews and exports.
- Preserve CPU fallback for legacy projects whose runtime masks are not available.
- Add GPU/CPU parity coverage for portrait composition with smoothing, reshaping and liquify.

## 0.6.7

- Move color-only portrait adjustment layers into the native GPU compositor instead of returning the full document to CPU compositing.
- Upload adaptive face masks and portrait settings as GPU resources, apply them to the resident composite texture, and preserve layer opacity, coverage and blending.
- Reuse GPU Gaussian filtering for portrait skin and eye detail during composition.
- Keep CPU fallback for portrait layers with missing runtime masks, legacy project data or unsupported GPU conditions.
- Add ignored GPU/CPU parity coverage for full portrait-layer composition.

## 0.6.6

- Add a GPU warp pass for portrait face shaping and manual liquify.
- Keep warp-grid construction on the CPU for deterministic landmark and brush behavior, while moving per-pixel displacement interpolation and bilinear sampling to the GPU.
- Preserve edge clamping and transparent-pixel handling in the GPU warp shader.
- Fall back to the CPU warp implementation when no GPU context or pipeline is available.
- Add GPU/CPU parity coverage for liquify warping.

## 0.6.5

- Add a native GPU portrait retouch pass for skin smoothing, texture, tone, blemish reduction, eye enhancement, dark circles, teeth whitening and lip tint.
- Use the existing GPU separable blur pipeline for skin and eye detail before the portrait color pass.
- Keep the CPU portrait implementation as the fallback for small images, missing GPU context, unsupported adapters and shader failures.
- Add an ignored GPU/CPU parity test for the portrait shader.

## 0.6.4

- Add an adaptive skin-colour model estimated from each detected face instead of relying only on fixed YCbCr thresholds.
- Preserve deep skin tones and warm or mixed lighting more reliably while reducing false skin coverage on hair and backgrounds.
- Keep the feathered face and feature masks intact while applying confidence-weighted skin masking.
- Add regression coverage for adaptive skin classification.

## 0.6.3

- Add persistent preset favorites and a recent-preset list, with quick access sections in the preset manager.
- Add color-aware preset swatches so built-in and user presets are visually distinguishable before application.
- Add user-preset rename and delete actions with conflict protection.
- Add import conflict policies for preset files: skip, overwrite, or automatically assign a unique name.
- Keep preset preferences and management state local to the user configuration directory.

## 0.6.2

- Add non-destructive Portrait Retouch adjustment layers that persist face landmarks, per-face settings, and liquify strokes while leaving source pixels untouched.
- Reopen portrait layers by double-clicking the layer or using the layer context menu; portrait adjustments can be hidden, deleted, masked, and saved in `.xuan` projects.
- Support portrait recipe rendering in both 8-bit and 16-bit CPU paths, with a safe GPU fallback while the compositor gains dedicated portrait acceleration.
- Add batch conflict policies for skipping, overwriting, or automatically numbering existing outputs, including the CLI `--auto-number` option.
- Localize the new portrait-layer and batch-conflict controls.

## 0.6.1

- Improve portrait detection on small and distant faces with overlapping 2x2 crops and a conditional dense 3x3 pass, merged through global weighted NMS.
- Cache the local ONNX models process-wide and reuse the analyzed face set when applying portrait edits and batch presets; each image is detected once.
- Add canvas face boxes, numbered selections, confidence-aware colors and the selected face's landmark outline.
- Add hold-to-compare, `Shift+P` temporary original preview, and empty-operation history protection.
- Compose manual-liquify strokes into one warp pass before previewing to reduce drag latency.
- Add bilingual labels for portrait, preset and batch dialogs.
- Add live batch progress, elapsed/ETA estimates, and copy/export actions for failed files.
- Keep color presets working for images with no detected faces and avoid loading portrait models for color-only work.

## 0.6.0

- Add fully local face detection and 478-point face landmark analysis using embedded Apache-2.0 ONNX models.
- Add portrait skin smoothing and texture, even tone, blemish reduction, eye enhancement, teeth whitening, lip tint and face/feature reshaping.
- Add a manual liquify brush for face and body adjustments.
- Add `.xzp` presets with eight built-ins, import/export, and one-transaction application.
- Add GUI and CLI batch preset processing with resizing, suffix, overwrite and cancellation controls.
- Keep portrait analysis offline; no image or biometric data is uploaded.
- Known limits: automatic body landmarks, RAW batch processing and separate per-face batch recipes are deferred to 0.6.x.

## 0.5.1

- 修复部分 Windows 环境首次启动时沿用异常过小的持久化窗口尺寸，导致仅显示顶部菜单和缩放栏的问题。
- 启动时若检测到恢复后的客户区低于界面最小可用尺寸，会自动恢复到适配当前显示器的安全尺寸；正常窗口尺寸和用户后续调整不受影响。
- 增加窗口尺寸不超过当前显示器的约束，避免跨显示器或分辨率变化后恢复到不可完整显示的尺寸。

## 0.5.0

- 品牌更名为“小猪修修”，独立发布；工程格式继续使用 `.xuan`，兼容原 XUAN 项目。
- 永久免费：无订阅、无广告、无导出付费墙、无强制水印。

### High-depth editing

- Preserve 16-bit PNG, TIFF, and WebP layer sources through project save/load.
- Composite high-depth documents with GPU float targets or a CPU float fallback.
- Keep 16-bit samples through adjustment layers, pointwise and spatial filters, non-destructive filter layers, masks, coverage, painting, retouching, gradient/fill, vector refresh, merging, and clip-aware copying.
- Export 16-bit PNG and TIFF with sRGB ICC profiles; single-layer exports pass samples through without requantizing.
- Add 16-bit mask assets and high-depth mask sampling while retaining the 8-bit editing preview.
- Preserve high-depth data through text/shape refresh and transform-only operations.

### Compatibility

- Existing 8-bit projects and `.comp` imports remain supported.
- New text/shape layers continue to default to 8-bit; high-depth sources retain their depth when edited.
- Clipboard exchange remains 8-bit, and some mixed low-opacity attachment stacks still materialize an 8-bit intermediate.
