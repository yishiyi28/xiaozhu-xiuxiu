use std::{
    collections::HashSet,
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use image::RgbaImage;

use super::MAX_ASSET;
use crate::{
    blend::BlendMode,
    document::{Document, Layer, validate_size},
};

fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).with_context(|| format!("Cannot read {}", path.display()))?;
    ensure!(metadata.len() <= MAX_ASSET, "PSD exceeds 512 MiB");
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_ASSET + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_ASSET, "PSD exceeds 512 MiB");
    Ok(bytes)
}

fn parse(path: &Path) -> Result<psd::Psd> {
    let bytes = read_bytes(path)?;
    let psd =
        psd::Psd::from_bytes(&bytes).map_err(|error| anyhow::anyhow!("Invalid PSD: {error}"))?;
    ensure!(
        psd.width() > 0 && psd.height() > 0,
        "PSD has an empty canvas"
    );
    ensure!(
        matches!(psd.color_mode(), psd::ColorMode::Rgb),
        "Only RGB PSD files are supported"
    );
    ensure!(
        matches!(psd.depth(), psd::PsdDepth::Eight),
        "Only 8-bit PSD files are supported"
    );
    validate_size(psd.width(), psd.height())?;
    Ok(psd)
}

/// Map the on-disk Photoshop blend mode code to the editor's blend mode.
fn convert_blend(mode: u32) -> BlendMode {
    match mode {
        3 => BlendMode::Darken,
        4 => BlendMode::Multiply,
        5 => BlendMode::ColorBurn,
        6 => BlendMode::LinearBurn,
        8 => BlendMode::Lighten,
        9 => BlendMode::Screen,
        10 => BlendMode::ColorDodge,
        11 => BlendMode::LinearDodge,
        13 => BlendMode::Overlay,
        14 => BlendMode::SoftLight,
        15 => BlendMode::HardLight,
        16 => BlendMode::VividLight,
        20 => BlendMode::Difference,
        21 => BlendMode::Exclusion,
        24 => BlendMode::Hue,
        25 => BlendMode::Saturation,
        26 => BlendMode::Color,
        27 => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

/// Import a PSD as a layered document. Layers are stored bottom to top.
///
/// The parser returns every layer as a canvas-sized RGBA buffer; layers are
/// cropped to their own bounds so the document stores compact layer pixels.
pub(super) fn import(path: &Path) -> Result<Document> {
    let psd = parse(path)?;
    let width = psd.width();
    let height = psd.height();
    let canvas_w = width as usize;
    let canvas_h = height as usize;
    let mut document = Document::new(width, height)?;
    document.layers.clear();
    let mut clip_base = None;
    // The parser stores layers top-to-bottom; the document stores them bottom-to-top.
    for source in psd.layers().iter().rev() {
        let layer_w = i64::from(source.width());
        let layer_h = i64::from(source.height());
        if layer_w <= 0 || layer_h <= 0 {
            continue;
        }
        let left = i64::from(source.layer_left());
        let top = i64::from(source.layer_top());
        let x0 = left.max(0);
        let y0 = top.max(0);
        let x1 = (left + layer_w).min(canvas_w as i64);
        let y1 = (top + layer_h).min(canvas_h as i64);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        let pixels = source.rgba();
        ensure!(
            pixels.len() == canvas_w * canvas_h * 4,
            "PSD layer '{}' has an unexpected pixel buffer",
            source.name()
        );
        let cropped_w = (x1 - x0) as u32;
        let cropped_h = (y1 - y0) as u32;
        let mut image = RgbaImage::new(cropped_w, cropped_h);
        let stride = cropped_w as usize * 4;
        {
            let dst: &mut [u8] = &mut image;
            for row in 0..cropped_h as usize {
                let source_y = y0 as usize + row;
                let source_start = (source_y * canvas_w + x0 as usize) * 4;
                let target_start = row * stride;
                dst[target_start..target_start + stride]
                    .copy_from_slice(&pixels[source_start..source_start + stride]);
            }
        }
        let mut layer = Layer::image(source.name(), image);
        // The parser exposes the raw layer flag, where a set bit means hidden.
        layer.visible = !source.visible();
        layer.opacity = f32::from(source.opacity()) / 255.0;
        layer.blend = convert_blend(source.blend_mode() as u32);
        layer.transform.x = x0 as f32;
        layer.transform.y = y0 as f32;
        // `is_clipping_mask` means the layer is a clipping base; layers above it
        // are clipped to it.
        if source.is_clipping_mask() {
            clip_base = Some(layer.id);
        } else {
            layer.clip_to = clip_base;
        }
        document.layers.push(layer);
    }
    if document.layers.is_empty() {
        let pixels = psd.rgba();
        ensure!(
            pixels.len() == canvas_w * canvas_h * 4,
            "PSD composite has an unexpected pixel buffer"
        );
        let image = RgbaImage::from_raw(width, height, pixels)
            .context("PSD composite pixels are invalid")?;
        document.layers.push(Layer::image("Background", image));
    }
    let top = document.layers.last().unwrap().id;
    document.active = Some(top);
    document.selected = HashSet::from([top]);
    Ok(document)
}

/// Import a PSD as a flattened image, for Import as Layer.
pub(super) fn import_image(path: &Path) -> Result<RgbaImage> {
    let psd = parse(path)?;
    let width = psd.width();
    let height = psd.height();
    let pixels = psd.rgba();
    ensure!(
        pixels.len() == width as usize * height as usize * 4,
        "PSD composite has an unexpected pixel buffer"
    );
    RgbaImage::from_raw(width, height, pixels).context("PSD composite pixels are invalid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photoshop_blend_codes_map_to_editor_modes() {
        assert_eq!(convert_blend(1), BlendMode::Normal);
        assert_eq!(convert_blend(4), BlendMode::Multiply);
        assert_eq!(convert_blend(15), BlendMode::HardLight);
        assert_eq!(convert_blend(16), BlendMode::VividLight);
        assert_eq!(convert_blend(21), BlendMode::Exclusion);
        assert_eq!(convert_blend(22), BlendMode::Normal);
    }
}
