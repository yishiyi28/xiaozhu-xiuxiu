use std::{
    borrow::Cow,
    collections::HashSet,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::Path,
    sync::Arc,
};

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageFormat, ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    blend::BlendMode,
    document::{
        Adjustment, Document, Layer, Luma16Image, MAX_PIXELS, Mask, PixelStorage, Point,
        Rgba16Image, Transform, validate_size,
    },
    icc, render,
};

const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
const MAX_ASSET: u64 = 512 * 1024 * 1024;

mod heif;
mod psd;

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    document: Document,
    pixel_layers: HashSet<Uuid>,
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>> {
    let mut encoded = Cursor::new(Vec::new());
    image.write_to(&mut encoded, ImageFormat::Png)?;
    Ok(encoded.into_inner())
}

fn encode_png16(image: &Rgba16Image) -> Result<Vec<u8>> {
    let mut encoded = Cursor::new(Vec::new());
    let mut info = icc::png_info(image.width(), image.height());
    info.bit_depth = png::BitDepth::Sixteen;
    let encoder = png::Encoder::with_info(&mut encoded, info)?;
    let mut writer = encoder.write_header()?;
    let mut bytes = Vec::with_capacity(image.as_raw().len() * 2);
    for value in image.as_raw() {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    writer.write_image_data(&bytes)?;
    drop(writer);
    Ok(encoded.into_inner())
}

fn encode_png_luma16(image: &Luma16Image) -> Result<Vec<u8>> {
    let mut encoded = Cursor::new(Vec::new());
    let mut info = png::Info::with_size(image.width(), image.height());
    info.color_type = png::ColorType::Grayscale;
    info.bit_depth = png::BitDepth::Sixteen;
    let encoder = png::Encoder::with_info(&mut encoded, info)?;
    let mut writer = encoder.write_header()?;
    let mut bytes = Vec::with_capacity(image.as_raw().len() * 2);
    for value in image.as_raw() {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    writer.write_image_data(&bytes)?;
    drop(writer);
    Ok(encoded.into_inner())
}

fn is_high_depth(color: image::ColorType) -> bool {
    matches!(
        color,
        image::ColorType::Rgba16
            | image::ColorType::Rgb16
            | image::ColorType::L16
            | image::ColorType::La16
    )
}

fn decode_storage(bytes: Vec<u8>, used: &mut u64) -> Result<PixelStorage> {
    let image = decode_image(bytes, used)?;
    if is_high_depth(image.color()) {
        Ok(PixelStorage::from_rgba16(image.to_rgba16()))
    } else {
        Ok(PixelStorage::from_rgba8(image.to_rgba8()))
    }
}

fn decode_image(bytes: Vec<u8>, used: &mut u64) -> Result<DynamicImage> {
    ensure!(bytes.len() as u64 <= MAX_ASSET, "Image file is too large");
    if heic_rs::ftyp::parse(&bytes).is_ok() {
        return heif::decode(&bytes, used).map(DynamicImage::ImageRgba8);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(30_000);
    limits.max_image_height = Some(30_000);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    use image::ImageDecoder;
    let (width, height) = decoder.dimensions();
    reserve_pixels(width, height, used)?;
    let orientation = decoder.orientation()?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

fn reserve_pixels(width: u32, height: u32, used: &mut u64) -> Result<()> {
    validate_size(width, height)?;
    let total = used.saturating_add(u64::from(width) * u64::from(height));
    ensure!(
        total <= MAX_PIXELS,
        "Project exceeds 100 megapixels of source images"
    );
    *used = total;
    Ok(())
}

fn read_image_file(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).with_context(|| format!("Cannot read {}", path.display()))?;
    ensure!(metadata.len() <= MAX_ASSET, "Image exceeds 512 MiB");
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_ASSET + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_ASSET, "Image exceeds 512 MiB");
    Ok(bytes)
}

/// Import an image while retaining 16-bit samples when the source provides them.
pub fn import_pixels(path: &Path) -> Result<PixelStorage> {
    let bytes = read_image_file(path)?;
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "heic" | "heif" | "hif") {
        return Ok(PixelStorage::from_rgba8(heif::decode(&bytes, &mut 0)?));
    }
    let profile = icc::extract(&bytes);
    let image = decode_image(bytes, &mut 0)?;
    if is_high_depth(image.color()) {
        let storage = PixelStorage::from_rgba16(image.to_rgba16());
        if let Some(profile) = profile
            && !icc::is_srgb_equivalent(&profile)
        {
            // The transform engine is 8-bit; wide-gamut high-depth sources are
            // converted through their preview and stored at 8 bits for now.
            let mut preview = storage.into_rgba8();
            icc::convert_to_srgb(&mut preview, &profile);
            return Ok(PixelStorage::from_rgba8(preview));
        }
        Ok(storage)
    } else {
        let mut image = image.to_rgba8();
        if let Some(profile) = profile {
            icc::convert_to_srgb(&mut image, &profile);
        }
        Ok(PixelStorage::from_rgba8(image))
    }
}

pub fn import_image(path: &Path) -> Result<RgbaImage> {
    Ok(import_pixels(path)?.into_rgba8())
}

/// Import a Photoshop document as a layered composition.
pub fn import_psd(path: &Path) -> Result<Document> {
    psd::import(path)
}

/// Import a Photoshop document as a flattened image, for Import as Layer.
pub fn import_psd_image(path: &Path) -> Result<RgbaImage> {
    psd::import_image(path)
}

/// Persist a complete sibling temporary file, then atomically replace the destination.
pub fn save(document: &Document, path: &Path) -> Result<()> {
    document.validate()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut archive = ZipWriter::new(temporary.as_file_mut());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let manifest = Manifest {
            format: "me.silverl.xuan".into(),
            version: if document.layers.iter().any(|l| {
                l.filter.is_some()
                    || l.parent.is_some_and(|id| {
                        document
                            .layers
                            .iter()
                            .any(|p| p.id == id && p.can_attach_effects())
                    })
            }) {
                4
            } else if document.layers.iter().any(|l| l.standalone_mask) {
                3
            } else if document.layers.iter().any(|l| l.raw.is_some()) {
                2
            } else {
                1
            },
            document: document.clone(),
            pixel_layers: document
                .layers
                .iter()
                .filter(|l| l.pixels.is_some())
                .map(|l| l.id)
                .collect(),
        };
        let json = serde_json::to_vec_pretty(&manifest)?;
        ensure!(
            json.len() as u64 <= MAX_MANIFEST,
            "Project metadata exceeds 4 MiB"
        );
        archive.start_file("manifest.json", options)?;
        archive.write_all(&json)?;
        for layer in &document.layers {
            if let Some(raw) = &layer.raw {
                // The legacy archive suffix is shared by all RAW formats.
                archive.start_file(format!("raw/{}.nef", layer.id), options)?;
                archive.write_all(&raw.bytes)?;
            }
            if let Some(pixels) = &layer.pixels {
                archive.start_file(format!("images/{}.png", layer.id), options)?;
                if let Some(pixels16) = pixels.rgba16() {
                    archive.write_all(&encode_png16(pixels16)?)?;
                } else {
                    archive.write_all(&encode_png(&DynamicImage::ImageRgba8(
                        pixels.rgba8().clone(),
                    ))?)?;
                }
            }
            if let Some(mask) = &layer.mask {
                archive.start_file(format!("images/{}.mask.png", layer.id), options)?;
                if let Some(high) = &mask.high {
                    archive.write_all(&encode_png_luma16(high)?)?;
                } else {
                    archive.write_all(&encode_png(&DynamicImage::ImageLuma8(
                        (*mask.pixels).clone(),
                    ))?)?;
                }
            }
        }
        archive.finish()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;

    // Sync the rename on Unix; Windows cannot open directories with File::open.
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn zip_read(archive: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let file = archive
        .by_name(name)
        .with_context(|| format!("Missing project asset: {name}"))?;
    ensure!(file.size() <= limit, "Project asset exceeds size limit");
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Project asset exceeds size limit"
    );
    Ok(bytes)
}

pub fn load(path: &Path) -> Result<Document> {
    if path.is_dir() {
        return load_compositor(path);
    }
    let mut archive = ZipArchive::new(File::open(path)?)?;
    ensure!(archive.len() <= 30_001, "Too many project assets");
    let mut manifest: Manifest =
        serde_json::from_slice(&zip_read(&mut archive, "manifest.json", MAX_MANIFEST)?)?;
    ensure!(
        manifest.format == "me.silverl.xuan" && (1..=4).contains(&manifest.version),
        "Unsupported xuan project version"
    );
    let mut used_pixels = 0;
    let mut used_masks = 0;
    let mut used_raw = 0;
    ensure!(manifest.document.layers.len() <= 10_000, "Too many layers");
    validate_size(manifest.document.width, manifest.document.height)?;
    for layer in &mut manifest.document.layers {
        if let Some(raw) = &mut layer.raw {
            let bytes = zip_read(&mut archive, &format!("raw/{}.nef", layer.id), MAX_ASSET)?;
            used_raw += bytes.len() as u64;
            ensure!(
                used_raw <= crate::raw::MAX_RAW_BYTES,
                "Project exceeds 512 MiB of RAW assets"
            );
            raw.bytes = Arc::new(bytes);
            raw.validate()?;
        }
        if manifest.pixel_layers.contains(&layer.id) {
            let bytes = zip_read(&mut archive, &format!("images/{}.png", layer.id), MAX_ASSET)?;
            layer.pixels = Some(Arc::new(decode_storage(bytes, &mut used_pixels)?));
        }
        if let Some(mask) = &mut layer.mask {
            let bytes = zip_read(
                &mut archive,
                &format!("images/{}.mask.png", layer.id),
                MAX_ASSET,
            )?;
            let image = decode_image(bytes, &mut used_masks)?;
            mask.high = is_high_depth(image.color()).then(|| Arc::new(image.to_luma16()));
            mask.pixels = Arc::new(image.to_luma8());
        }
    }
    ensure!(
        manifest
            .pixel_layers
            .iter()
            .all(|id| manifest.document.layers.iter().any(|l| l.id == *id)),
        "Unreferenced pixel layer metadata"
    );
    manifest.document.selected = manifest.document.active.into_iter().collect();
    manifest.document.validate()?;
    Ok(manifest.document)
}

fn package_read(root: &Path, relative: &Path, limit: u64) -> Result<Vec<u8>> {
    let root = root.canonicalize()?;
    let file = root.join(relative);
    let metadata = fs::symlink_metadata(&file)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= limit,
        "Unsafe or oversized project asset"
    );
    ensure!(
        file.canonicalize()?.starts_with(&root),
        "Project asset escapes its package"
    );
    let mut bytes = Vec::new();
    File::open(file)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Project asset exceeds size limit"
    );
    Ok(bytes)
}

fn number(value: &Value, key: &str, default: f32) -> f32 {
    value[key].as_f64().map_or(default, |v| v as f32)
}
fn identifier(value: &Value) -> Result<Option<Uuid>> {
    value
        .as_str()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(Into::into)
}

fn comp_transform(value: &Value) -> Result<Transform> {
    let pair = |value: &Value, a: &str, b: &str| -> Result<(f32, f32)> {
        let (x, y) = if let Some(values) = value.as_array() {
            ensure!(values.len() == 2, "Invalid transform coordinates");
            (values[0].as_f64(), values[1].as_f64())
        } else {
            (value[a].as_f64(), value[b].as_f64())
        };
        Ok((
            x.context("Missing transform coordinate")? as f32,
            y.context("Missing transform coordinate")? as f32,
        ))
    };
    let (x, y) = pair(&value["origin"], "x", "y")?;
    let (width, height) = pair(&value["size"], "width", "height")?;
    let t = Transform {
        x,
        y,
        width,
        height,
        rotation: number(value, "rotation", 0.0),
        flip_x: value["flipX"].as_bool().unwrap_or(false),
        flip_y: value["flipY"].as_bool().unwrap_or(false),
        warp: None,
    };
    ensure!(t.valid(), "Invalid Compositor layer transform");
    Ok(t)
}

// Swift dictionaries with enum keys are encoded as alternating key/value arrays.
fn swift_dictionary_get<'a>(value: &'a Value, key: &str) -> &'a Value {
    if let Some(array) = value.as_array() {
        for pair in array.as_chunks::<2>().0 {
            if pair[0].as_str() == Some(key) {
                return &pair[1];
            }
        }
        &Value::Null
    } else {
        &value[key]
    }
}

fn comp_adjustment(value: &Value) -> Result<Adjustment> {
    let kind = value["kind"]
        .as_str()
        .context("Adjustment kind is missing")?;
    let result = match kind {
        "Hue/Saturation" => {
            let hsv = &value["hsvSettings"];
            if hsv.is_null() {
                Adjustment::HueSaturation {
                    hue: number(value, "hue", 0.0),
                    saturation: number(value, "saturation", 0.0),
                    lightness: number(value, "lightness", 0.0),
                    colorize: value["colorize"].as_bool().unwrap_or(false),
                }
            } else {
                let mut settings = crate::color::HueSettings {
                    range: crate::color::HueSettings::RANGES
                        .iter()
                        .position(|name| Some(*name) == hsv["range"].as_str())
                        .unwrap_or(0),
                    colorize: hsv["colorize"].as_bool().unwrap_or(false),
                    invert_range: hsv["invertRange"].as_bool().unwrap_or(false),
                    ..Default::default()
                };
                for (index, name) in crate::color::HueSettings::RANGES.iter().enumerate() {
                    let adjustment = swift_dictionary_get(&hsv["adjustments"], name);
                    settings.adjustments[index] = [
                        number(adjustment, "hue", 0.0),
                        number(adjustment, "saturation", 0.0),
                        number(adjustment, "lightness", 0.0),
                    ];
                    let band = swift_dictionary_get(&hsv["bands"], name);
                    if !band.is_null() {
                        settings.bands[index] = [
                            number(band, "falloffStart", 0.0),
                            number(band, "rangeStart", 0.0),
                            number(band, "rangeEnd", 360.0),
                            number(band, "falloffEnd", 360.0),
                        ];
                    }
                }
                Adjustment::HueRanges {
                    settings: Box::new(settings),
                }
            }
        }
        "Levels" => {
            let input = value["levels"]["ranges"]
                .as_array()
                .context("Missing levels ranges")?;
            ensure!(input.len() == 4, "Invalid levels ranges");
            let ranges = std::array::from_fn(|i| {
                let r = &input[i];
                [
                    number(r, "black", 0.0),
                    number(r, "gamma", 1.0),
                    number(r, "white", 255.0),
                    number(r, "outputBlack", 0.0),
                    number(r, "outputWhite", 255.0),
                ]
            });
            Adjustment::LevelsChannels { ranges }
        }
        "Curves" => {
            let input = value["curves"]["channels"]
                .as_array()
                .context("Missing curve channels")?;
            ensure!(
                input.len() == 4 && input.iter().all(|c| c.is_array()),
                "Invalid curve channels"
            );
            let channels = std::array::from_fn(|i| {
                input[i]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| Point::new(number(p, "x", 0.0) / 255.0, number(p, "y", 0.0) / 255.0))
                    .collect()
            });
            Adjustment::CurvesChannels { channels }
        }
        "Exposure" => {
            let settings = &value["exposureSettings"];
            Adjustment::Exposure {
                exposure: number(settings, "exposure", 0.0),
                offset: number(settings, "offset", 0.0),
                gamma: number(settings, "gamma", 1.0),
            }
        }
        "Gradient Map" => {
            let color = |key| {
                let c = &value["gradientMapSettings"][key];
                [
                    number(c, "red", if key == "shadows" { 0.0 } else { 1.0 }),
                    number(c, "green", if key == "shadows" { 0.0 } else { 1.0 }),
                    number(c, "blue", if key == "shadows" { 0.0 } else { 1.0 }),
                    1.0,
                ]
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            };
            Adjustment::GradientMap {
                shadows: color(
                    if value["gradientMapSettings"]["reversed"]
                        .as_bool()
                        .unwrap_or(false)
                    {
                        "highlights"
                    } else {
                        "shadows"
                    },
                ),
                highlights: color(
                    if value["gradientMapSettings"]["reversed"]
                        .as_bool()
                        .unwrap_or(false)
                    {
                        "shadows"
                    } else {
                        "highlights"
                    },
                ),
            }
        }
        "Grain" => {
            let settings = &value["grainSettings"];
            Adjustment::FilmGrain {
                amount: number(settings, "amount", 0.0),
                size: number(settings, "size", 1.0),
                roughness: number(settings, "roughness", 50.0),
                seed: settings["seed"].as_u64().unwrap_or(1) as u32,
            }
        }
        _ => bail!("Unsupported Compositor adjustment: {kind}"),
    };
    crate::effects::validate_adjustment(&result)?;
    Ok(result)
}

pub fn load_compositor(path: &Path) -> Result<Document> {
    let manifest: Value = serde_json::from_slice(&package_read(
        path,
        Path::new("manifest.json"),
        MAX_MANIFEST,
    )?)?;
    ensure!(
        manifest["format"] == "com.compositor.project",
        "Not a Compositor project"
    );
    let version = manifest["version"]
        .as_u64()
        .context("Project version missing")?;
    ensure!(
        (1..=7).contains(&version),
        "Unsupported Compositor project version {version}"
    );
    ensure!(
        manifest["colorSpace"].as_str().unwrap_or("sRGB") == "sRGB",
        "Unsupported color space"
    );
    let width = u32::try_from(manifest["width"].as_u64().context("Missing canvas width")?)?;
    let height = u32::try_from(
        manifest["height"]
            .as_u64()
            .context("Missing canvas height")?,
    )?;
    let mut document = Document::new(width, height)?;
    document.id = identifier(&manifest["documentID"])?.context("Missing document ID")?;
    document.resolution = number(&manifest, "resolution", 72.0);
    document.layers.clear();
    let records = manifest["layers"].as_array().context("Missing layers")?;
    ensure!(records.len() <= 10_000, "Too many layers");
    let mut image_pixels = 0;
    let mut mask_pixels = 0;
    for record in records {
        let id = identifier(&record["id"])?.context("Missing layer ID")?;
        let mut layer = Layer::blank(
            record["name"].as_str().context("Missing layer name")?,
            width,
            height,
        );
        layer.id = id;
        layer.visible = record["isVisible"].as_bool().unwrap_or(true);
        layer.transform = comp_transform(&record["transform"])?;
        layer.parent = identifier(&record["parentID"])?;
        layer.group = record["isGroup"].as_bool().unwrap_or(false);
        layer.opacity = number(record, "opacity", 1.0);
        let blend_name = record["blendMode"].as_str().unwrap_or("Normal");
        layer.blend = BlendMode::ALL
            .into_iter()
            .find(|b| b.name() == blend_name)
            .context("Unknown blend mode")?;
        layer.clip_to = identifier(&record["maskSourceID"])?;
        if let Some(name) = record["imageFile"].as_str() {
            ensure!(
                name.eq_ignore_ascii_case(&format!("{id}.png")),
                "Unsafe layer asset path"
            );
            let bytes = package_read(path, &Path::new("images").join(name), MAX_ASSET)?;
            layer.pixels = Some(Arc::new(decode_storage(bytes, &mut image_pixels)?));
        }
        if let Some(name) = record["maskFile"].as_str() {
            ensure!(
                name.eq_ignore_ascii_case(&format!("{id}.mask.png")),
                "Unsafe mask asset path"
            );
            let bytes = package_read(path, &Path::new("images").join(name), MAX_ASSET)?;
            let image = decode_image(bytes, &mut mask_pixels)?;
            let high = is_high_depth(image.color()).then(|| Arc::new(image.to_luma16()));
            layer.mask = Some(Mask {
                pixels: Arc::new(image.to_luma8()),
                high,
                enabled: record["maskEnabled"].as_bool().unwrap_or(true),
                linked: record["maskLinked"].as_bool().unwrap_or(true),
                placement: if record["maskPlacement"].is_null() {
                    None
                } else {
                    Some(comp_transform(&record["maskPlacement"])?)
                },
            });
        }
        if let Some(shape) = record["shape"].as_object() {
            let radius = shape
                .get("cornerRadius")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32;
            ensure!(radius.is_finite() && radius >= 0.0, "Invalid shape radius");
            let kind = if record["shape"]["kind"] == "Ellipse" {
                crate::paint::ShapeKind::Ellipse
            } else if radius > 0.0 {
                crate::paint::ShapeKind::RoundedRectangle
            } else {
                crate::paint::ShapeKind::Rectangle
            };
            let color =
                |key| (number(&record["shape"], key, 0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            layer.shape = Some(crate::document::ShapeStyle {
                kind,
                color: [color("red"), color("green"), color("blue"), 255],
                corner_radius: radius,
            });
        }
        if !record["adjustment"].is_null() {
            layer.adjustment = Some(comp_adjustment(&record["adjustment"])?);
        }
        document.layers.push(layer);
    }
    document.active = identifier(&manifest["activeLayerID"])?;
    document.selected = document.active.into_iter().collect();
    document.validate()?;
    Ok(document)
}

/// A document that can be written without touching the 8-bit compositor: one
/// unmodified, full-canvas, identity-transformed 16-bit layer.
fn direct_16bit(document: &Document) -> Option<&Rgba16Image> {
    if document.layers.len() != 1 {
        return None;
    }
    let layer = &document.layers[0];
    if !layer.visible
        || layer.group
        || layer.opacity != 1.0
        || layer.blend != BlendMode::Normal
        || layer.parent.is_some()
        || layer.clip_to.is_some()
        || layer.mask.is_some()
        || layer.standalone_mask
        || layer.adjustment.is_some()
        || layer.filter.is_some()
        || layer.shape.is_some()
        || layer.text.is_some()
        || layer.raw.is_some()
    {
        return None;
    }
    let transform = layer.transform;
    if transform.x != 0.0
        || transform.y != 0.0
        || transform.width != document.width as f32
        || transform.height != document.height as f32
        || transform.rotation != 0.0
        || transform.flip_x
        || transform.flip_y
        || transform.warp.is_some()
    {
        return None;
    }
    let pixels = layer.pixels.as_ref()?.rgba16()?;
    (pixels.dimensions() == (document.width, document.height)).then_some(pixels)
}

fn write_sixteen_bit(
    source: &Rgba16Image,
    document: &Document,
    path: &Path,
    extension: &str,
    parent: &Path,
) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    match extension {
        "png" => {
            let pixels_per_meter = (document.resolution / 0.0254).round() as u32;
            let mut info = icc::png_info(source.width(), source.height());
            info.bit_depth = png::BitDepth::Sixteen;
            info.pixel_dims = Some(png::PixelDimensions {
                xppu: pixels_per_meter,
                yppu: pixels_per_meter,
                unit: png::Unit::Meter,
            });
            let encoder = png::Encoder::with_info(temporary.as_file_mut(), info)?;
            let mut writer = encoder.write_header()?;
            let mut bytes = Vec::with_capacity(source.as_raw().len() * 2);
            for value in source.as_raw() {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            writer.write_image_data(&bytes)?;
        }
        _ => {
            let mut encoder = tiff::encoder::TiffEncoder::new(temporary.as_file_mut())?;
            let mut frame = encoder
                .new_image::<tiff::encoder::colortype::RGBA16>(source.width(), source.height())?;
            frame
                .encoder()
                .write_tag(tiff::tags::Tag::IccProfile, icc::SRGB)?;
            frame.write_data(source.as_raw())?;
        }
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn export(document: &Document, path: &Path, quality: u8) -> Result<()> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let high_depth = document
        .layers
        .iter()
        .filter_map(|layer| layer.pixels.as_deref())
        .any(|pixels| pixels.depth() == 16);
    if matches!(extension.as_str(), "png" | "tif" | "tiff") && high_depth {
        let source = match direct_16bit(document) {
            Some(source) => Cow::Borrowed(source),
            None => Cow::Owned(render::render16(document)),
        };
        write_sixteen_bit(&source, document, path, &extension, parent)?;
        return Ok(());
    }
    let image = render::render(document);
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    match extension.as_str() {
        "jpg" | "jpeg" => {
            let mut encoded = Vec::new();
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut encoded,
                quality.clamp(1, 100),
            );
            encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(
                document.resolution.round() as u16,
            ));
            encoder.encode_image(&render::flatten_white(&image))?;
            icc::embed_jpeg(&mut encoded, icc::SRGB);
            temporary.as_file_mut().write_all(&encoded)?;
        }
        "png" => {
            let pixels_per_meter = (document.resolution / 0.0254).round() as u32;
            let mut info = icc::png_info(image.width(), image.height());
            info.pixel_dims = Some(png::PixelDimensions {
                xppu: pixels_per_meter,
                yppu: pixels_per_meter,
                unit: png::Unit::Meter,
            });
            let encoder = png::Encoder::with_info(temporary.as_file_mut(), info)?;
            encoder.write_header()?.write_image_data(image.as_raw())?;
        }
        "tif" | "tiff" => {
            let mut encoder = tiff::encoder::TiffEncoder::new(temporary.as_file_mut())?;
            let mut frame = encoder
                .new_image::<tiff::encoder::colortype::RGBA8>(image.width(), image.height())?;
            frame
                .encoder()
                .write_tag(tiff::tags::Tag::IccProfile, icc::SRGB)?;
            frame.write_data(image.as_raw())?;
        }
        "webp" => {
            DynamicImage::ImageRgba8(image).write_to(temporary.as_file_mut(), ImageFormat::WebP)?
        }
        _ => bail!("Export as PNG, JPEG, TIFF or WebP"),
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma, Rgba};

    #[test]
    fn exports_all_formats_and_png_print_resolution() {
        let temporary = tempfile::tempdir().unwrap();
        let mut doc = Document::new(12, 8).unwrap();
        doc.resolution = 300.0;
        doc.layers[0].pixels = Some(Arc::new(
            RgbaImage::from_pixel(12, 8, Rgba([180, 90, 30, 128])).into(),
        ));
        for extension in ["png", "jpg", "tiff", "webp"] {
            let path = temporary.path().join(format!("image.{extension}"));
            export(&doc, &path, 95).unwrap();
            let image = import_image(&path).unwrap();
            assert_eq!(image.dimensions(), (12, 8));
            if extension == "jpg" {
                assert_eq!(image.get_pixel(0, 0)[3], 255);
            } else {
                assert_eq!(image.get_pixel(0, 0)[3], 128);
            }
        }
        let decoder = png::Decoder::new(std::io::BufReader::new(
            File::open(temporary.path().join("image.png")).unwrap(),
        ));
        let reader = decoder.read_info().unwrap();
        let density = reader.info().pixel_dims.unwrap();
        assert_eq!(density.xppu, 11811);
        assert_eq!(density.unit, png::Unit::Meter);
    }

    #[test]
    fn exports_embed_srgb_profiles() {
        let temporary = tempfile::tempdir().unwrap();
        let mut doc = Document::new(8, 8).unwrap();
        doc.layers[0].pixels = Some(Arc::new(
            RgbaImage::from_pixel(8, 8, Rgba([40, 80, 120, 255])).into(),
        ));
        for extension in ["png", "jpg", "tiff"] {
            let path = temporary.path().join(format!("tagged.{extension}"));
            export(&doc, &path, 95).unwrap();
            let bytes = fs::read(&path).unwrap();
            assert_eq!(
                icc::extract(&bytes).as_deref(),
                Some(icc::SRGB),
                "{extension} is missing its sRGB profile"
            );
            let imported = import_image(&path).unwrap();
            if extension != "jpg" {
                assert_eq!(
                    imported.get_pixel(0, 0).0,
                    [40, 80, 120, 255],
                    "{extension} changed pixels while re-importing sRGB"
                );
            }
        }
    }

    fn sample_16bit(width: u32, height: u32) -> Rgba16Image {
        Rgba16Image::from_fn(width, height, |x, y| {
            Rgba([
                (x * 4096 + y * 17 + 1000) as u16,
                (x * 1234 + y * 4321 + 7) as u16,
                (x * 7919 + y * 1049 + 30_000) as u16,
                if (x + y) % 5 == 0 { 12_345 } else { 65_535 },
            ])
        })
    }

    #[test]
    fn sixteen_bit_png_and_tiff_round_trip_without_quantizing() {
        let directory = tempfile::tempdir().unwrap();
        let source = sample_16bit(5, 4);
        let input = directory.path().join("source.png");
        fs::write(&input, encode_png16(&source).unwrap()).unwrap();
        let imported = import_pixels(&input).unwrap();
        assert_eq!(imported.depth(), 16);
        assert_eq!(imported.rgba16().unwrap(), &source);

        let mut document = Document::new(5, 4).unwrap();
        document.layers = vec![Layer::image16("16-bit", source.clone())];
        for extension in ["png", "tiff"] {
            let output = directory.path().join(format!("deep.{extension}"));
            export(&document, &output, 95).unwrap();
            let bytes = fs::read(&output).unwrap();
            assert_eq!(icc::extract(&bytes).as_deref(), Some(icc::SRGB));
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert_eq!(
                decoded.color(),
                image::ColorType::Rgba16,
                "{extension} was not written at 16 bits"
            );
            assert_eq!(decoded.to_rgba16(), source, "{extension} quantized");
        }
    }

    #[test]
    fn project_archive_preserves_sixteen_bit_layers() {
        let directory = tempfile::tempdir().unwrap();
        let source = sample_16bit(3, 2);
        let mut document = Document::new(3, 2).unwrap();
        let layer = Layer::image16("16-bit", source.clone());
        document.active = Some(layer.id);
        document.selected = std::iter::once(layer.id).collect();
        document.layers = vec![layer];
        let path = directory.path().join("deep.xuan");
        save(&document, &path).unwrap();
        let loaded = load(&path).unwrap();
        let storage = loaded.layers[0].pixels.as_ref().unwrap();
        assert_eq!(storage.depth(), 16);
        assert_eq!(storage.rgba16().unwrap(), &source);
    }

    #[test]
    fn project_archive_preserves_sixteen_bit_masks() {
        let directory = tempfile::tempdir().unwrap();
        let high = Luma16Image::from_fn(3, 2, |x, y| {
            Luma([(x * 10_000 + y * 20_000 + 1_234) as u16])
        });
        let preview = DynamicImage::ImageLuma16(high.clone()).to_luma8();
        let mut document = Document::new(3, 2).unwrap();
        document.layers[0].mask = Some(Mask {
            pixels: Arc::new(preview.clone()),
            high: Some(Arc::new(high.clone())),
            ..Mask::white()
        });
        let path = directory.path().join("mask16.xuan");
        save(&document, &path).unwrap();
        let loaded = load(&path).unwrap();
        let mask = loaded.layers[0].mask.as_ref().unwrap();
        assert_eq!(mask.pixels.as_ref(), &preview);
        assert_eq!(mask.high.as_deref(), Some(&high));
    }

    #[test]
    fn editing_high_depth_storage_drops_the_sixteen_bit_source() {
        let mut storage = PixelStorage::from_rgba16(sample_16bit(2, 2));
        assert_eq!(storage.depth(), 16);
        storage.rgba8_mut().put_pixel(0, 0, Rgba([1, 2, 3, 4]));
        assert_eq!(storage.depth(), 8);
        assert!(storage.rgba16().is_none());
    }

    #[test]
    fn multilayer_sixteen_bit_export_stays_high_depth() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("stack.png");
        let mut document = Document::new(4, 3).unwrap();
        let mut front = Layer::image16("Front", sample_16bit(4, 3));
        front.opacity = 0.5;
        document.layers = vec![Layer::image16("Back", sample_16bit(4, 3)), front];
        export(&document, &path, 95).unwrap();
        let decoded = image::ImageReader::open(&path)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(decoded.color(), image::ColorType::Rgba16);
        assert_eq!((decoded.width(), decoded.height()), (4, 3));
    }

    #[test]
    #[ignore = "requires a Vulkan or OpenGL compute adapter"]
    fn gpu_multilayer_export_writes_real_sixteen_bit_samples() {
        let instance = wgpu::Instance::new(&Default::default());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let mut descriptor = wgpu::DeviceDescriptor::default();
        if adapter
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
        {
            descriptor.required_features |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor)).unwrap();
        let processor = crate::gpu::Processor::new(device, queue);
        let (width, height) = (160, 110);
        let back = Rgba16Image::from_fn(width, height, |x, y| {
            Rgba([
                ((x * 4001 + y * 37 + 1234) % 65_536) as u16,
                ((x * 733 + y * 5903 + 42) % 65_536) as u16,
                ((x * 9001 + y * 113 + 31_000) % 65_536) as u16,
                65_535,
            ])
        });
        let mut document = Document::new(width, height).unwrap();
        document.layers = vec![
            Layer::image16("Back", back.clone()),
            Layer::image16("Clear top", Rgba16Image::new(width, height)),
        ];
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("stack.png");
        crate::gpu::scope(Some(processor), || export(&document, &path, 95)).unwrap();
        let decoded = image::ImageReader::open(&path)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();
        let image::DynamicImage::ImageRgba16(output) = decoded else {
            panic!("expected a 16-bit RGBA export");
        };
        assert_eq!(output.dimensions(), (width, height));
        let maximum = output
            .as_raw()
            .iter()
            .zip(back.as_raw())
            .map(|(actual, expected)| actual.abs_diff(*expected))
            .max()
            .unwrap();
        assert!(maximum <= 1, "maximum channel delta {maximum}");
    }

    #[test]
    fn imports_swift_enum_dictionaries_and_individual_color_channels() {
        let value = serde_json::json!({"kind":"Hue/Saturation", "hsvSettings": {
            "range":"Reds", "colorize":false, "invertRange":true,
            "adjustments":["Master", {"hue":5,"saturation":0,"lightness":0}, "Reds", {"hue":40,"saturation":-20,"lightness":3}],
            "bands":["Reds", {"falloffStart":310,"rangeStart":340,"rangeEnd":20,"falloffEnd":50}]
        }});
        let Adjustment::HueRanges { settings } = comp_adjustment(&value).unwrap() else {
            panic!("expected selective hue settings");
        };
        assert_eq!(settings.range, 1);
        assert_eq!(settings.adjustments[1], [40.0, -20.0, 3.0]);
        assert_eq!(settings.bands[1], [310.0, 340.0, 20.0, 50.0]);
        assert!(settings.invert_range);
        let default =
            serde_json::json!({"black":0,"gamma":1,"white":255,"outputBlack":0,"outputWhite":255});
        let mut value = serde_json::json!({"kind":"Levels","levels":{"ranges":[default,default,default,default]}});
        value["levels"]["ranges"][1]["gamma"] = serde_json::json!(1.5);
        let Adjustment::LevelsChannels { ranges } = comp_adjustment(&value).unwrap() else {
            panic!("expected channel levels");
        };
        assert_eq!(ranges[1][1], 1.5);
    }

    #[test]
    fn standalone_masks_round_trip_with_scope_and_transforms() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mask.xuan");
        let mut document = Document::new(4, 2).unwrap();
        document.layers[0].pixels = Some(Arc::new(
            RgbaImage::from_pixel(4, 2, Rgba([10, 90, 180, 255])).into(),
        ));
        crate::operations::group(&mut document);
        let mut mask = Layer::mask("Standalone", 4, 2);
        mask.opacity = 0.75;
        mask.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_fn(4, 2, |x, _| Luma([(x * 70) as u8]))),
            placement: Some(Transform {
                x: 1.0,
                ..Transform::new(4, 2)
            }),
            linked: false,
            ..Mask::white()
        });
        document.insert(mask);
        save(&document, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(render::render(&document), render::render(&loaded));
        let expected = document.active().unwrap();
        let actual = loaded.active().unwrap();
        assert!(actual.standalone_mask);
        assert_eq!(actual.parent, expected.parent);
        assert_eq!(actual.opacity, expected.opacity);
        assert_eq!(
            actual.mask.as_ref().unwrap().pixels,
            expected.mask.as_ref().unwrap().pixels
        );
        assert_eq!(
            actual.mask.as_ref().unwrap().placement,
            expected.mask.as_ref().unwrap().placement
        );
        let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        let manifest: Manifest =
            serde_json::from_slice(&zip_read(&mut archive, "manifest.json", MAX_MANIFEST).unwrap())
                .unwrap();
        assert_eq!(manifest.version, 3);
    }

    #[test]
    fn portable_project_round_trip_and_atomic_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.xuan");
        let mut doc = Document::new(3, 2).unwrap();
        doc.layers[0].pixels = Some(Arc::new(
            RgbaImage::from_pixel(3, 2, Rgba([20, 40, 80, 128])).into(),
        ));
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(3, 2, Luma([128]))),
            ..Mask::white()
        });
        save(&doc, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(render::render(&doc), render::render(&loaded));
        doc.layers[0].name = "Renamed".into();
        save(&doc, &path).unwrap();
        assert_eq!(load(&path).unwrap().layers[0].name, "Renamed");
        doc.width = 0;
        assert!(save(&doc, &path).is_err());
        assert_eq!(load(&path).unwrap().width, 3);
    }

    #[test]
    fn imports_swift_transform_and_rejects_path_traversal() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("images")).unwrap();
        let id = Uuid::new_v4();
        let mut value = serde_json::json!({"format":"com.compositor.project", "version":7, "documentID":Uuid::new_v4(), "width":2, "height":2, "activeLayerID":id,
            "layers":[{"id":id,"name":"Test", "isVisible":true,"transform":{"origin":[1,2],"size":[2,2],"rotation":30,"flipX":true,"flipY":false}}]});
        let manifest = directory.path().join("manifest.json");
        fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let document = load_compositor(directory.path()).unwrap();
        assert_eq!(document.layers[0].transform.rotation, 30.0);
        assert!(document.layers[0].transform.flip_x);
        value["layers"][0]["imageFile"] = Value::String("../../outside.png".into());
        fs::write(manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load_compositor(directory.path()).is_err());
    }
}
