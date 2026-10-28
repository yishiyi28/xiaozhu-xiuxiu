use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Deserializer, Serialize};
use tempfile::NamedTempFile;

use crate::{
    config::Config,
    document::{Adjustment, ColorGrade, Document, Layer, PixelStorage, Rgba16Image},
};

use super::{
    inference::PortraitModels,
    masks::build_portrait_masks,
    retouch::{PortraitSettings, apply_portrait_to_rgba8, apply_portrait_to_rgba16},
    types::FaceLandmarks,
    warp::apply_face_shape,
};

pub const BUILTIN_NAMES: [&str; 8] = [
    "natural",
    "clear-portrait",
    "warm",
    "cool",
    "film",
    "soft-film",
    "mono",
    "high-contrast-mono",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FacePolicy {
    NoPortrait,
    Selected(usize),
    AllFaces,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresetV1 {
    pub schema_version: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorGrade>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portrait: Option<PortraitSettings>,
}

#[derive(Deserialize)]
struct PresetRaw {
    schema_version: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    color: Option<ColorGrade>,
    #[serde(default)]
    portrait: Option<PortraitSettings>,
}

impl<'de> Deserialize<'de> for PresetV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = PresetRaw::deserialize(deserializer)?;
        if raw.schema_version != 1 {
            return Err(serde::de::Error::custom(format!(
                "unsupported schema_version {}",
                raw.schema_version
            )));
        }
        let preset = Self {
            schema_version: raw.schema_version,
            name: raw.name,
            color: raw.color,
            portrait: raw.portrait,
        };
        preset.validate().map_err(serde::de::Error::custom)?;
        Ok(preset)
    }
}

impl Default for PresetV1 {
    fn default() -> Self {
        Self {
            schema_version: 1,
            name: "Unnamed".into(),
            color: None,
            portrait: None,
        }
    }
}

impl PresetV1 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("unsupported schema_version {}", self.schema_version);
        }
        if self.name.trim().is_empty() {
            bail!("preset name is empty");
        }
        if let Some(color) = &self.color {
            color.validate()?;
        }
        if let Some(portrait) = &self.portrait {
            portrait.validate()?;
        }
        Ok(())
    }

    pub fn builtin(name: &str) -> Result<Self> {
        let data = match name {
            "natural" => include_str!("../../assets/presets/natural.xzp"),
            "clear-portrait" => include_str!("../../assets/presets/clear-portrait.xzp"),
            "warm" => include_str!("../../assets/presets/warm.xzp"),
            "cool" => include_str!("../../assets/presets/cool.xzp"),
            "film" => include_str!("../../assets/presets/film.xzp"),
            "soft-film" => include_str!("../../assets/presets/soft-film.xzp"),
            "mono" => include_str!("../../assets/presets/mono.xzp"),
            "high-contrast-mono" => {
                include_str!("../../assets/presets/high-contrast-mono.xzp")
            }
            _ => bail!("unknown built-in preset {name}"),
        };
        serde_json::from_str(data).with_context(|| format!("Invalid built-in preset {name}"))
    }

    pub fn builtins() -> Result<Vec<Self>> {
        BUILTIN_NAMES
            .iter()
            .map(|name| Self::builtin(name))
            .collect()
    }

    #[cfg(test)]
    pub fn sample() -> Self {
        Self {
            schema_version: 1,
            name: "Sample".into(),
            color: Some(ColorGrade {
                exposure: 0.25,
                temperature: 12.0,
                grain: 5.0,
                ..Default::default()
            }),
            portrait: Some(PortraitSettings {
                skin_smooth: 25.0,
                eye_size: -5.0,
                ..Default::default()
            }),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PresetStore;

impl PresetStore {
    pub fn user_dir() -> Result<PathBuf> {
        let config = Config::path()?;
        let parent = config
            .parent()
            .context("Configuration path has no parent")?;
        Ok(parent.join("presets"))
    }

    pub fn load(path: &Path) -> Result<PresetV1> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("Cannot read preset {}", path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("Cannot parse preset {}", path.display()))
    }

    pub fn save(preset: &PresetV1, path: &Path) -> Result<()> {
        preset.validate()?;
        let parent = path.parent().context("Preset path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut file = NamedTempFile::new_in(parent)?;
        file.write_all(serde_json::to_string_pretty(preset)?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(path)
            .with_context(|| format!("Cannot save preset {}", path.display()))?;
        Ok(())
    }

    pub fn import(path: &Path) -> Result<PresetV1> {
        let preset = Self::load(path)?;
        let name = preset
            .name
            .chars()
            .map(|ch| {
                if ch.is_alphanumeric() || matches!(ch, ' ' | '-' | '_') {
                    ch
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim()
            .to_owned();
        let name = if name.is_empty() { "preset" } else { &name };
        let target = Self::user_dir()?.join(format!("{name}.xzp"));
        Self::save(&preset, &target)?;
        Ok(preset)
    }

    pub fn user_presets() -> Result<Vec<PresetV1>> {
        let directory = Self::user_dir()?;
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut presets = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) == Some("xzp")
                && let Ok(preset) = Self::load(&path)
            {
                presets.push(preset);
            }
        }
        presets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(presets)
    }
}

pub fn apply_preset_to_document(
    document: &mut Document,
    preset: &PresetV1,
    face_policy: FacePolicy,
) -> Result<()> {
    preset.validate()?;
    let mut working = document.clone();
    if let Some(settings) = &preset.portrait
        && !matches!(face_policy, FacePolicy::NoPortrait)
        && has_portrait_effect(settings)
    {
        let layer = working
            .active_mut()
            .context("Select a pixel layer before applying a portrait preset")?;
        let storage = layer
            .pixels
            .as_ref()
            .context("Select a pixel layer before applying a portrait preset")?
            .as_ref()
            .clone();
        let models = PortraitModels::load()?;
        let faces = models.detect_with_landmarks(storage.rgba8())?;
        let selected = select_faces(&faces, face_policy)?;
        if !selected.is_empty() {
            layer.pixels = Some(std::sync::Arc::new(apply_portrait_to_storage(
                &storage, &selected, settings,
            )?));
        }
    }
    if let Some(color) = &preset.color {
        let mut layer = Layer::blank(
            format!("{} — Color", preset.name),
            working.width,
            working.height,
        );
        layer.adjustment = Some(Adjustment::ColorGrade {
            grade: color.clone(),
        });
        working.layers.push(layer);
    }
    *document = working;
    Ok(())
}

fn select_faces(faces: &[FaceLandmarks], policy: FacePolicy) -> Result<Vec<&FaceLandmarks>> {
    Ok(match policy {
        FacePolicy::NoPortrait => Vec::new(),
        FacePolicy::AllFaces => faces.iter().collect(),
        FacePolicy::Selected(index) => vec![
            faces
                .get(index)
                .with_context(|| format!("Selected face {index} is no longer available"))?,
        ],
    })
}

fn has_portrait_effect(settings: &PortraitSettings) -> bool {
    [
        settings.skin_smooth,
        settings.skin_texture,
        settings.skin_tone,
        settings.blemish,
        settings.eye_brightness,
        settings.eye_clarity,
        settings.dark_circles,
        settings.teeth_whiten,
        settings.lip_tint,
        settings.face_width,
        settings.jaw,
        settings.chin,
        settings.eye_size,
        settings.nose_width,
        settings.lip_shape,
    ]
    .iter()
    .any(|value| *value != 0.0)
}

fn apply_portrait_to_storage(
    storage: &PixelStorage,
    faces: &[&FaceLandmarks],
    settings: &PortraitSettings,
) -> Result<PixelStorage> {
    match storage {
        PixelStorage::Rgba8(source) => {
            let mut current = (**source).clone();
            for face in faces {
                let masks =
                    build_portrait_masks(&current, face, [current.width(), current.height()])?;
                let mut output = current.clone();
                apply_portrait_to_rgba8(&current, &mut output, &masks, settings)?;
                if has_reshape(settings) {
                    output = apply_face_shape(&output, face, settings)?;
                }
                current = output;
            }
            Ok(PixelStorage::from_rgba8(current))
        }
        PixelStorage::Rgba16 { pixels, .. } => {
            let mut current = (**pixels).clone();
            let mut preview = storage.rgba8().clone();
            for face in faces {
                let masks =
                    build_portrait_masks(&preview, face, [preview.width(), preview.height()])?;
                current = apply_portrait_to_rgba16(&current, &masks, settings)?;
                preview = to_rgba8(&current);
            }
            if has_reshape(settings) {
                current = warp_rgba16_preserving_depth(&current, faces, settings)?;
            }
            Ok(PixelStorage::from_rgba16(current))
        }
    }
}

fn has_reshape(settings: &PortraitSettings) -> bool {
    [
        settings.face_width,
        settings.jaw,
        settings.chin,
        settings.eye_size,
        settings.nose_width,
        settings.lip_shape,
    ]
    .iter()
    .any(|value| *value != 0.0)
}

fn warp_rgba16_preserving_depth(
    source: &Rgba16Image,
    faces: &[&FaceLandmarks],
    settings: &PortraitSettings,
) -> Result<Rgba16Image> {
    let before = to_rgba8(source);
    let mut after = before.clone();
    for face in faces {
        after = apply_face_shape(&after, face, settings)?;
    }
    Ok(image::ImageBuffer::from_fn(
        source.width(),
        source.height(),
        |x, y| {
            let old = source.get_pixel(x, y);
            let old8 = before.get_pixel(x, y);
            let new8 = after.get_pixel(x, y);
            let scale = |before: u8, after: u8, value: u16| -> u16 {
                if before == 0 {
                    u16::from(after) * 257
                } else {
                    (u32::from(value) * u32::from(after) / u32::from(before))
                        .min(u32::from(u16::MAX)) as u16
                }
            };
            Rgba([
                scale(old8[0], new8[0], old[0]),
                scale(old8[1], new8[1], old[1]),
                scale(old8[2], new8[2], old[2]),
                old[3],
            ])
        },
    ))
}

fn to_rgba8(source: &Rgba16Image) -> RgbaImage {
    RgbaImage::from_fn(source.width(), source.height(), |x, y| {
        let pixel = source.get_pixel(x, y);
        Rgba([
            (pixel[0] >> 8) as u8,
            (pixel[1] >> 8) as u8,
            (pixel[2] >> 8) as u8,
            (pixel[3] >> 8) as u8,
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Adjustment, Document, Layer, PixelStorage};
    use image::{Rgba, RgbaImage};

    #[test]
    fn preset_roundtrip_preserves_all_values() {
        let preset = PresetV1::sample();
        let text = serde_json::to_string_pretty(&preset).unwrap();
        assert_eq!(serde_json::from_str::<PresetV1>(&text).unwrap(), preset);
    }

    #[test]
    fn preset_rejects_unknown_future_version() {
        let error = serde_json::from_str::<PresetV1>(r#"{"schema_version":2,"name":"future"}"#)
            .unwrap_err();
        assert!(error.to_string().contains("schema_version"));
    }

    #[test]
    fn all_builtins_parse_and_validate() {
        for name in BUILTIN_NAMES {
            let preset = PresetV1::builtin(name).unwrap();
            assert_eq!(preset.schema_version, 1);
            assert!(!preset.name.is_empty());
        }
    }

    #[test]
    fn all_faces_preset_keeps_color_grade_without_a_detected_face() {
        let mut document = Document::new(16, 12).unwrap();
        let layer = Layer::from_storage(
            "Photo",
            PixelStorage::from_rgba8(RgbaImage::from_pixel(16, 12, Rgba([120, 100, 80, 255]))),
        );
        document.active = Some(layer.id);
        document.layers = vec![layer];
        let preset = PresetV1 {
            schema_version: 1,
            name: "Mixed batch".into(),
            color: Some(crate::document::ColorGrade {
                contrast: 8.0,
                ..Default::default()
            }),
            portrait: Some(PortraitSettings {
                skin_smooth: 20.0,
                ..Default::default()
            }),
        };
        apply_preset_to_document(&mut document, &preset, FacePolicy::AllFaces).unwrap();
        assert!(
            document
                .layers
                .iter()
                .any(|layer| matches!(layer.adjustment, Some(Adjustment::ColorGrade { .. })))
        );
    }

    #[test]
    fn applying_color_preset_adds_one_color_grade_layer() {
        let mut document = Document::new(16, 12).unwrap();
        document.layers = vec![Layer::from_storage(
            "Photo",
            PixelStorage::from_rgba8(RgbaImage::from_pixel(16, 12, Rgba([120, 100, 80, 255]))),
        )];
        apply_preset_to_document(
            &mut document,
            &PresetV1::builtin("natural").unwrap(),
            FacePolicy::NoPortrait,
        )
        .unwrap();
        assert_eq!(
            document
                .layers
                .iter()
                .filter(|layer| matches!(layer.adjustment, Some(Adjustment::ColorGrade { .. })))
                .count(),
            1
        );
    }
}
