# 小猪修修（原 XUAN）更新日志

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
