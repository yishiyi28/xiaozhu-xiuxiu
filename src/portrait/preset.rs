use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize};
use tempfile::NamedTempFile;

use crate::{
    config::Config,
    document::{Adjustment, ColorGrade, Document, Layer},
};

use super::{
    inference::PortraitModels, layer::PortraitAdjustment, retouch::PortraitSettings,
    types::FaceLandmarks,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresetImportPolicy {
    Overwrite,
    #[default]
    Rename,
    Skip,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetPreferences {
    pub favorites: Vec<String>,
    pub recent: Vec<String>,
}

impl PresetPreferences {
    pub fn is_favorite(&self, name: &str) -> bool {
        self.favorites.iter().any(|favorite| favorite == name)
    }

    pub fn toggle_favorite(&mut self, name: &str) {
        if let Some(index) = self.favorites.iter().position(|favorite| favorite == name) {
            self.favorites.remove(index);
        } else {
            self.favorites.push(name.to_owned());
        }
    }

    pub fn touch_recent(&mut self, name: &str) {
        self.recent.retain(|entry| entry != name);
        self.recent.insert(0, name.to_owned());
        self.recent.truncate(8);
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

    pub fn import_with_policy(path: &Path, policy: PresetImportPolicy) -> Result<Option<PresetV1>> {
        import_with_policy_into(path, &Self::user_dir()?, policy)
    }

    pub fn rename(preset: &PresetV1, new_name: &str) -> Result<PresetV1> {
        rename_in_dir(&Self::user_dir()?, preset, new_name)
    }

    pub fn delete(preset: &PresetV1) -> Result<()> {
        delete_in_dir(&Self::user_dir()?, preset)
    }

    pub fn preferences_path() -> Result<PathBuf> {
        Ok(Self::user_dir()?.join("preferences.json"))
    }

    pub fn preferences() -> Result<PresetPreferences> {
        let path = Self::preferences_path()?;
        if !path.exists() {
            return Ok(PresetPreferences::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("Cannot read preset preferences {}", path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("Cannot parse preset preferences {}", path.display()))
    }

    pub fn save_preferences(preferences: &PresetPreferences) -> Result<()> {
        let path = Self::preferences_path()?;
        let parent = path
            .parent()
            .context("Preset preferences path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut file = NamedTempFile::new_in(parent)?;
        file.write_all(serde_json::to_string_pretty(preferences)?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(&path)
            .with_context(|| format!("Cannot save preset preferences {}", path.display()))?;
        Ok(())
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
        Self::import_with_policy(path, PresetImportPolicy::Overwrite)?
            .context("Preset import was skipped")
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

fn preset_file_name(name: &str) -> String {
    let name = name
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
    if name.is_empty() {
        "preset".into()
    } else {
        name
    }
}

fn unique_preset_name(directory: &Path, base: &str) -> String {
    for index in 1u32.. {
        let candidate = format!("{base}-{index}");
        if !directory.join(format!("{candidate}.xzp")).exists() {
            return candidate;
        }
    }
    unreachable!("unbounded preset name search")
}

fn import_with_policy_into(
    source: &Path,
    directory: &Path,
    policy: PresetImportPolicy,
) -> Result<Option<PresetV1>> {
    let mut preset = PresetStore::load(source)?;
    let base = preset_file_name(&preset.name);
    let mut name = base.clone();
    let mut target = directory.join(format!("{name}.xzp"));
    if target.exists() {
        match policy {
            PresetImportPolicy::Skip => return Ok(None),
            PresetImportPolicy::Overwrite => {}
            PresetImportPolicy::Rename => {
                name = unique_preset_name(directory, &base);
                preset.name = name.clone();
                target = directory.join(format!("{name}.xzp"));
            }
        }
    }
    PresetStore::save(&preset, &target)?;
    Ok(Some(preset))
}

fn rename_in_dir(directory: &Path, preset: &PresetV1, new_name: &str) -> Result<PresetV1> {
    let name = new_name.trim();
    if name.is_empty() {
        bail!("Preset name is empty");
    }
    let old = directory.join(format!("{}.xzp", preset_file_name(&preset.name)));
    if !old.exists() {
        bail!("Preset file is missing");
    }
    let mut renamed = preset.clone();
    renamed.name = name.to_owned();
    renamed.validate()?;
    let target = directory.join(format!("{}.xzp", preset_file_name(&renamed.name)));
    if old != target && target.exists() {
        bail!("A preset with that name already exists");
    }
    PresetStore::save(&renamed, &target)?;
    if old != target {
        fs::remove_file(old)?;
    }
    Ok(renamed)
}

fn delete_in_dir(directory: &Path, preset: &PresetV1) -> Result<()> {
    let path = directory.join(format!("{}.xzp", preset_file_name(&preset.name)));
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn apply_preset_to_document(
    document: &mut Document,
    preset: &PresetV1,
    face_policy: FacePolicy,
) -> Result<()> {
    apply_preset_to_document_with_faces(document, preset, face_policy, None)
}

/// Apply a preset while optionally reusing faces that were already analyzed by
/// the caller. Passing an empty slice is meaningful: it means analysis ran and
/// found no face, so the color portion must still be applied without a second
/// model invocation.
pub fn apply_preset_to_document_with_faces(
    document: &mut Document,
    preset: &PresetV1,
    face_policy: FacePolicy,
    known_faces: Option<&[FaceLandmarks]>,
) -> Result<()> {
    preset.validate()?;
    let mut working = document.clone();
    if let Some(settings) = &preset.portrait
        && !matches!(face_policy, FacePolicy::NoPortrait)
        && has_portrait_effect(settings)
    {
        let source_layer = working
            .active()
            .context("Select a pixel layer before applying a portrait preset")?;
        let storage = source_layer
            .pixels
            .as_ref()
            .context("Select a pixel layer before applying a portrait preset")?
            .as_ref()
            .clone();
        let detected;
        let faces = if let Some(faces) = known_faces {
            faces
        } else {
            let models = PortraitModels::shared()?;
            detected = models.detect_with_landmarks(storage.rgba8())?;
            &detected
        };
        let selected = select_faces(faces, face_policy)?;
        if !selected.is_empty() {
            let selected_index = match face_policy {
                FacePolicy::Selected(index) => index,
                _ => 0,
            };
            let source_size = [storage.rgba8().width(), storage.rgba8().height()];
            let mut portrait_layer = Layer::blank(
                format!("{} — Portrait", preset.name),
                working.width,
                working.height,
            );
            portrait_layer.portrait = Some(PortraitAdjustment {
                source_size,
                faces: faces.to_vec(),
                settings: vec![settings.clone(); faces.len()],
                selected: selected_index,
                apply_to_all: matches!(face_policy, FacePolicy::AllFaces),
                liquify: Vec::new(),
            });
            working.insert(portrait_layer);
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

pub fn has_portrait_effect(settings: &PortraitSettings) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Adjustment, Document, Layer, PixelStorage};
    use image::{Rgba, RgbaImage};

    #[test]
    fn preset_import_rename_and_delete_respect_conflicts() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.xzp");
        let mut preset = PresetV1::sample();
        preset.name = "Studio".into();
        PresetStore::save(&preset, &source).unwrap();
        let first =
            import_with_policy_into(&source, directory.path(), PresetImportPolicy::Overwrite)
                .unwrap()
                .unwrap();
        assert_eq!(first.name, "Studio");
        assert!(directory.path().join("Studio.xzp").exists());
        let second = import_with_policy_into(&source, directory.path(), PresetImportPolicy::Rename)
            .unwrap()
            .unwrap();
        assert_eq!(second.name, "Studio-1");
        assert!(directory.path().join("Studio-1.xzp").exists());
        assert!(
            import_with_policy_into(&source, directory.path(), PresetImportPolicy::Skip,)
                .unwrap()
                .is_none()
        );
        let renamed = rename_in_dir(directory.path(), &second, "Final").unwrap();
        assert_eq!(renamed.name, "Final");
        assert!(!directory.path().join("Studio-1.xzp").exists());
        assert!(directory.path().join("Final.xzp").exists());
        delete_in_dir(directory.path(), &renamed).unwrap();
        assert!(!directory.path().join("Final.xzp").exists());
    }

    #[test]
    fn preset_preferences_track_recent_without_duplicates() {
        let mut preferences = PresetPreferences::default();
        preferences.touch_recent("Warm");
        preferences.touch_recent("Cool");
        preferences.touch_recent("Warm");
        assert_eq!(preferences.recent, vec!["Warm", "Cool"]);
        assert!(!preferences.is_favorite("Warm"));
    }

    #[test]
    fn preset_preferences_toggle_favorites() {
        let mut preferences = PresetPreferences::default();
        preferences.toggle_favorite("Natural");
        assert!(preferences.is_favorite("Natural"));
        preferences.toggle_favorite("Natural");
        assert!(!preferences.is_favorite("Natural"));
    }

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
    fn portrait_preset_creates_a_layer_without_touching_source_pixels() {
        let source = RgbaImage::from_fn(64, 64, |x, y| {
            Rgba([(x * 3) as u8, (y * 3) as u8, 120, 255])
        });
        let base = Layer::from_storage("Photo", PixelStorage::from_rgba8(source.clone()));
        let mut document = Document::new(64, 64).unwrap();
        document.active = Some(base.id);
        document.layers = vec![base];
        let face = crate::portrait::test_fixtures::synthetic_landmarks();
        let preset = PresetV1 {
            schema_version: 1,
            name: "Portrait".into(),
            color: None,
            portrait: Some(PortraitSettings {
                skin_smooth: 20.0,
                ..Default::default()
            }),
        };
        apply_preset_to_document_with_faces(
            &mut document,
            &preset,
            FacePolicy::AllFaces,
            Some(&[face]),
        )
        .unwrap();
        assert_eq!(document.layers[0].pixels.as_ref().unwrap().rgba8(), &source);
        assert!(document.layers.iter().any(|layer| {
            layer
                .portrait
                .as_ref()
                .is_some_and(|recipe| recipe.apply_to_all)
        }));
    }

    #[test]
    fn known_empty_face_analysis_still_applies_color_grade() {
        let mut document = Document::new(16, 12).unwrap();
        let layer = Layer::from_storage(
            "Photo",
            PixelStorage::from_rgba8(RgbaImage::from_pixel(16, 12, Rgba([120, 100, 80, 255]))),
        );
        document.active = Some(layer.id);
        document.layers = vec![layer];
        let preset = PresetV1 {
            schema_version: 1,
            name: "Known empty".into(),
            color: Some(crate::document::ColorGrade {
                contrast: 8.0,
                ..Default::default()
            }),
            portrait: Some(PortraitSettings {
                skin_smooth: 20.0,
                ..Default::default()
            }),
        };
        apply_preset_to_document_with_faces(
            &mut document,
            &preset,
            FacePolicy::AllFaces,
            Some(&[]),
        )
        .unwrap();
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
