//! User preferences, independent of projects and egui's window persistence.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub language: Language,
    /// Interval between crash-recovery snapshots. Zero disables autosave.
    pub autosave_seconds: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::default(),
            autosave_seconds: 120,
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let variable = if cfg!(windows) {
            "APPDATA"
        } else {
            "XDG_CONFIG_HOME"
        };
        let config_home = std::env::var_os(variable).map(PathBuf::from);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let path = config_path(config_home.clone(), home.clone())?;
        // Migrate settings from the original XUAN namespace on first launch.
        if !path.exists()
            && let Ok(legacy) = config_path_named(config_home, home, "xuan")
            && legacy.exists()
        {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(legacy, &path);
        }
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("Cannot parse {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("Cannot read {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        // Preserve unknown preferences. An invalid existing file is never overwritten.
        let mut table = match fs::read_to_string(path) {
            Ok(text) => toml::from_str::<toml::Table>(&text)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
            Err(error) => return Err(error.into()),
        };
        table.insert("language".into(), toml::Value::try_from(self.language)?);
        table.insert(
            "autosave_seconds".into(),
            toml::Value::Integer(i64::from(self.autosave_seconds)),
        );
        let parent = path.parent().context("Configuration path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(toml::to_string_pretty(&table)?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(path)
            .with_context(|| format!("Cannot save {}", path.display()))?;
        Ok(())
    }
}

fn config_path(config_home: Option<PathBuf>, home: Option<PathBuf>) -> Result<PathBuf> {
    config_path_named(config_home, home, "xiaozhu-xiuxiu")
}

fn config_path_named(
    config_home: Option<PathBuf>,
    home: Option<PathBuf>,
    namespace: &str,
) -> Result<PathBuf> {
    // The XDG specification requires absolute paths; ignore empty/relative values.
    if let Some(path) = config_home.filter(|p| p.is_absolute()) {
        return Ok(path.join(namespace).join("config.toml"));
    }
    if !cfg!(windows)
        && let Some(path) = home.filter(|p| p.is_absolute())
    {
        return Ok(path.join(".config").join(namespace).join("config.toml"));
    }
    bail!("Cannot locate the user configuration directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_roundtrip_preserves_unknown_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("xiaozhu-xiuxiu/config.toml");
        assert_eq!(Config::load(&path).unwrap(), Config::default());
        let chinese = Config {
            language: Language::SimplifiedChinese,
            autosave_seconds: 60,
        };
        chinese.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), chinese);
        fs::write(&path, "language = 'zh-CN'\n[future]\noption = 42\n").unwrap();
        Config::default().save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("option = 42"));
        assert_eq!(Config::load(&path).unwrap(), Config::default());
    }

    #[test]
    fn invalid_preferences_are_reported_and_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "language = [broken").unwrap();
        assert!(Config::load(&path).is_err());
        assert!(Config::default().save(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "language = [broken");
        assert!(toml::from_str::<Config>("language = 'unknown'").is_err());
    }

    #[test]
    fn uses_platform_config_home() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        assert_eq!(
            config_path(Some(base.clone()), None).unwrap(),
            base.join("xiaozhu-xiuxiu/config.toml")
        );
        if !cfg!(windows) {
            assert_eq!(
                config_path(Some("relative".into()), Some(base.clone())).unwrap(),
                base.join(".config/xiaozhu-xiuxiu/config.toml")
            );
        }
        assert!(config_path(None, None).is_err());
    }
}
