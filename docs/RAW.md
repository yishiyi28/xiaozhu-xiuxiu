# RAW Develop

Open a Nikon `.nef`/`.nrw`, Canon `.cr2`/`.cr3`/`.crw`, Fujifilm `.raf`, or Sony `.arw` through File → Open, Import Image as Layer, the command line, a file-manager paste, or drag-and-drop. The image first opens in Develop. Files selected together are queued, so each RAW receives its own Develop session. Import-as-layer remembers its destination project.

Use **Develop** to create a photo layer, or **Cancel** to leave the destination document unchanged. Double-click the RAW layer row or the image with Move selected to return to Develop. Layer → Develop RAW and the layer context menu also reopen it. A committed redevelopment is one document undo step; changes inside Develop have their own Undo/Redo. Cancelling redevelopment retains the last committed pixels and settings.

Develop shares the editor's menu bar and document tabs. Each open RAW session has a **RAW** tab; switching to another tab keeps its pending adjustments, undo history, zoom, and pan in memory. Return to that tab to continue. Undo/Redo and the Fit, 100%, and zoom commands act on the active tab. Closing a RAW tab, its destination project, or the application asks before discarding pending development. Use **Develop** and save the project to retain adjustments after quitting.

Save as `.xuan` to embed the original RAW bytes, shooting metadata, and all Develop adjustments. The original camera file is never modified and is not needed to reopen a saved project. Duplicates share the in-memory source but have independent settings. Moving, scaling, rotating, masking, grouping, and blending a RAW layer retain editability. Reopening a RAW processes that layer in isolation; the result returns to its existing position in the composition.

Direct pixel painting and destructive filters require **Rasterize RAW Layer**, which is undoable. Paint on a separate pixel layer and use adjustment layers when you want to retain RAW editing. Merging or flattening produces ordinary pixel layers.

## Controls

- **Basic:** as-shot white balance, temperature/tint and lighting presets, neutral picker, auto exposure, ±10 EV exposure, brightness, contrast, highlights/shadows, white/black points, clarity, texture, dehaze, vibrance, saturation.
- **Negative:** negative-to-positive conversion, automatic crop analysis, film-base picker, black point, film gamma, RGB balance, and film calibration.
- **Tone:** draggable five-knot master and RGB curves; eight-band hue/saturation/lightness; monochrome RGB mix; shadow/highlight split toning and balance.
- **Detail:** edge-aware luminance/chroma noise reduction; luminance sharpening with radius and threshold. Previews start with a 1,600-pixel proxy and select progressively larger resolutions to cover the displayed image, including on high-density displays. At 100% and above they use full source resolution. During adjustment drags, a quick proxy updates continuously and refines after a short pause. Edited and Original comparisons use the same resolution. Choose **100%** for one image pixel per screen pixel to evaluate sharpening and noise reduction, or **Always use full-resolution preview** to process at full resolution even in Fit view. The full-resolution override also applies while adjusting controls. Choosing 100% does not enable that override permanently; returning to Fit can use a smaller preview. Smaller previews are downsampled and can differ in fine detail. Full-resolution preview is available when the image fits the GPU's texture-size limit; larger images still develop/export at full resolution.
- **Rotation:** use the rotate-left/right buttons beside the TIFF export button to turn the image by 90°. These buttons are also under **Lens → Geometry**. **Straighten** provides ±45° fine rotation. Quarter turns preserve the whole image, swap landscape/portrait dimensions, and are included in Develop Undo/Redo, saved projects, presets, and 16-bit TIFF exports. Existing crops and local masks rotate with the image; crop bounds refer to the displayed orientation.
- **Lens:** manual radial distortion, red/cyan and blue/yellow aberration, purple defringing, vignette compensation, horizontal/vertical perspective, and normalized crop bounds. A crop preserves the placement of surviving pixels when redeveloping an existing layer.
- **Masks:** linear and radial gradients or soft brush masks with exposure, warmth, saturation, visibility and inversion. Select a mask, enable Draw mask, and drag on the image. The brush radius/feather applies to the entire stroke collection in that mask. Up to 32 masks and 8,192 total brush points are saved with the RAW.
- **Info:** camera, lens, oriented dimensions, decoded depth, ISO, aperture, shutter, focal length and embedded source size.

The RGB histogram and clipping indicators describe the developed output. Compare Edited, Original (default development), Split, or Side by side. Drag to pan; in Split view, drag near the divider to move it. Space-drag or middle-button drag pans in every view, including while using the white-balance picker or drawing a mask. Alt-drag also pans in Split view. The wheel zooms around the pointer in single-image views. In Side by side, both images zoom around their respective pane centers and pan together. Horizontal wheel input or Shift+wheel pans horizontally. Presets can save/load validated JSON settings, including crops and masks.

## Precision and output

Rawler decodes the actual sensor data. Bayer files use its black/white-level correction and PPG demosaicing. Fujifilm X-Trans files use 小猪修修’s 6×6 demosaicing path, which applies the sensor’s repeating black levels, reconstructs green, and interpolates red/blue differences from green. Both paths preserve sensor crop alignment and camera orientation. 小猪修修 preserves oriented camera RGB in 32-bit floating point, applies white balance and exposure before the camera-to-sRGB matrix and display transfer function, and retains values above 1 until tone processing. Exposure reduction can therefore recover encoded highlight differences that are absent in an 8-bit preview. Sensor-saturated channels contain no recoverable detail.

**Develop** renders a full-resolution 8-bit sRGB photo layer, matching the existing compositor. **16-bit TIFF…** renders directly from the floating-point pipeline, without an intermediate 8-bit conversion, and embeds an sRGB ICC profile. It exports the current RAW development alone, including crop and local masks. To export the whole composition, use the normal photo editor's File → Export. Both outputs are independent of preview zoom and comparison/clipping overlays.

Decoding, preview processing and analysis, full-resolution development, and TIFF encoding run in background workers. Interactive previews are throttled to at most one new request every 33 ms, using the latest settings; full detail refines after 150 ms without edits. Obsolete refinements are cancelled, and interactive results only advance to newer compatible snapshots. On supported GPUs, preview pixels stay on the device and only histogram bins return to the CPU. Clipping pixels are generated when their display is enabled, and Original comparisons are cached and rendered at larger resolutions only when needed. Inactive RAW tabs release their compute buffers while retaining their displayed previews. Cancelling never commits a worker's late result. RAW import/development is limited to the editor's 100-megapixel image limit; project RAW sources have an aggregate 512 MiB budget. Full-resolution processing uses substantially more memory than the fit preview.

## Current limits

This implements the Develop → embedded RAW layer → Develop workflow and the controls listed above. It is not full Affinity feature parity. Camera support follows Rawler 0.7.2's Nikon NEF/NRW, Canon CR2/CR3/CRW, Fujifilm RAF, and Sony ARW decoders. RGB Bayer and 6×6 X-Trans sensors are supported; individual camera models and compression modes must be supported by that decoder. Canon CR3 RAW and C-RAW use the same Develop workflow. Reduced-resolution Canon sRAW/mRAW and older non-RGB sensor layouts are not supported. Unsupported/damaged files produce an error. The camera's embedded JPEG is not used as the development source.

Lens correction is manual; there is no automatic lens-profile database. Noise reduction is a conventional local filter, not a learned denoiser. Defringing suppresses purple excess and can affect purple objects. There is no reconstruction of saturated sensor channels, dual-illuminant profile interpolation, custom camera/ICC output profiles, wide-gamut/HDR compositor, RAW spot-healing tool, automatic subject masks, or batch preset development. Use the photo editor's healing tools after developing/rasterizing. RAW metadata remains in the project; the TIFF export currently includes the output color profile but does not copy shooting EXIF.

## Film negatives

For camera scans of color negative film, open the RAW and enable **Negative →
Convert negative to positive**. 小猪修修 estimates the orange film base and
channel density ranges, then converts the scan to a positive before applying the
usual exposure, tone, color, and detail controls. This is a native workflow inspired
by tools such as Grain2Pixel; Photoshop plugins are not loaded.

1. Use **Lens → Crop** to exclude the film holder, light source and unwanted borders.
   Click **Analyze crop** to estimate fresh endpoints from that area. Automatic
   analysis is a starting point; scenes without neutral shadows/highlights can need
   manual color adjustments.
2. For a measured orange mask, leave an unexposed film edge visible and enable
   **Pick film base**. The preview switches to Original. Click a clear section of
   that edge; 小猪修修 averages a small patch of linear camera RGB and returns to Edited.
   Crop the edge away afterward. Escape cancels the picker.
3. Tune **Black point**, **Film gamma**, and the red/green/blue balance sliders.
   **Film calibration** exposes the measured base RGB and each channel's density
   range. Increasing density range lowers that channel's white point in the output;
   increasing gamma darkens midtones. Balance is expressed in positive-image EV.
4. Finish with the usual positive-image exposure, curves, saturation and detail
   adjustments. Camera white balance and camera auto exposure are disabled during
   conversion; use the film color balance instead. Turn conversion off to inspect
   the normal RAW rendering without losing calibration.
5. **Develop** and save the `.xuan` project to retain the source and conversion
   settings, or export a **16-bit TIFF** directly. Preset save/load and Develop
   Undo/Redo include the conversion parameters, so a measured base can be reused
   for the same film stock and scanning light.

Inversion uses optical density (`log10(film base / transmission)`) in camera-linear
RGB, with bounded black/white points and an adjustable transfer curve. It bypasses
the scanner camera's positive-image color matrix and white balance, and runs in
both the CPU and GPU pipelines. Preview and full-resolution output use the same
saved calibration; zooming never reanalyzes the image. This is not a calibrated
film-stock profile or an exact reproduction of Grain2Pixel. It cannot recover a
clipped scan, and colored lighting or unusual emulsions can require manual balance.
