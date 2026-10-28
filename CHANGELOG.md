# 小猪修修（原 XUAN）更新日志

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
