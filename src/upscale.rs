//! Adapter for an optional external Upscayl-compatible NCNN backend.
//!
//! The application does not bundle the backend or its models. Users configure a
//! separately obtained executable and model directory, keeping licensing and
//! model distribution outside the MIT application package.

use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpscaleOptions {
    pub backend: String,
    pub models: String,
    pub model: String,
    pub scale: u32,
    pub tile_size: u32,
    pub tta: bool,
    pub gpu_id: String,
}

impl Default for UpscaleOptions {
    fn default() -> Self {
        Self {
            backend: String::new(),
            models: String::new(),
            model: "upscayl-standard-4x".into(),
            scale: 4,
            tile_size: 0,
            tta: false,
            gpu_id: String::new(),
        }
    }
}

impl UpscaleOptions {
    pub fn validate(&self) -> Result<()> {
        if self.backend.trim().is_empty() {
            bail!("Choose an Upscayl-compatible backend executable");
        }
        if self.models.trim().is_empty() {
            bail!("Choose a model directory");
        }
        let models = Path::new(self.models.trim());
        if !models.is_dir() {
            bail!("Model directory does not exist: {}", models.display());
        }
        if self.model.trim().is_empty() {
            bail!("Choose an upscale model");
        }
        if !(2..=4).contains(&self.scale) {
            bail!("Upscale scale must be 2, 3 or 4");
        }
        if self.tile_size != 0 && !(64..=4096).contains(&self.tile_size) {
            bail!("Tile size must be 0 or between 64 and 4096");
        }
        let available = discover_models(models)?;
        if !available.iter().any(|model| model == self.model.trim()) {
            bail!("Model files are missing for {}", self.model);
        }
        Ok(())
    }

    pub fn output_arguments(&self, input: &Path, output: &Path) -> Vec<String> {
        let mut args = vec![
            "-i".into(),
            input.display().to_string(),
            "-o".into(),
            output.display().to_string(),
            "-s".into(),
            self.scale.to_string(),
            "-m".into(),
            self.models.trim().to_owned(),
            "-n".into(),
            self.model.trim().to_owned(),
            "-f".into(),
            "png".into(),
        ];
        if self.tile_size > 0 {
            args.push("-t".into());
            args.push(self.tile_size.to_string());
        }
        if self.tta {
            args.push("-x".into());
        }
        if !self.gpu_id.trim().is_empty() {
            args.push("-g".into());
            args.push(self.gpu_id.trim().to_owned());
        }
        args
    }
}

#[derive(Clone, Debug)]
pub struct UpscaleProgress {
    pub percent: Option<f32>,
    pub message: String,
}

#[derive(Debug)]
pub struct UpscaleResult {
    pub output: PathBuf,
    pub elapsed: Duration,
    pub log_tail: String,
}

pub fn discover_models(directory: &Path) -> Result<Vec<String>> {
    if !directory.is_dir() {
        bail!("Model directory does not exist: {}", directory.display());
    }
    let mut parameters = BTreeSet::new();
    let mut weights = BTreeSet::new();
    for entry in
        fs::read_dir(directory).with_context(|| format!("Cannot read {}", directory.display()))?
    {
        let path = entry?.path();
        let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        match path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
        {
            Some("param") => {
                parameters.insert(name.to_owned());
            }
            Some("bin") => {
                weights.insert(name.to_owned());
            }
            _ => {}
        }
    }
    Ok(parameters.intersection(&weights).cloned().collect())
}

pub fn suggest_models_dir(backend: &Path) -> Option<PathBuf> {
    let directory = backend.parent()?;
    [directory.join("models"), directory.parent()?.join("models")]
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

pub fn run_upscale(
    options: &UpscaleOptions,
    input: &Path,
    output: &Path,
    cancel: &AtomicBool,
    progress: Option<Sender<UpscaleProgress>>,
) -> Result<UpscaleResult> {
    options.validate()?;
    if !input.is_file() {
        bail!("Upscale input does not exist: {}", input.display());
    }
    if output.exists() {
        fs::remove_file(output).with_context(|| format!("Cannot replace {}", output.display()))?;
    }
    let parent = output.parent().context("Upscale output has no parent")?;
    fs::create_dir_all(parent)?;

    let mut command = Command::new(options.backend.trim());
    command
        .args(options.output_arguments(input, output))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("Cannot start Upscayl backend {}", options.backend))?;

    let log = Arc::new(Mutex::new(VecDeque::<String>::new()));
    let reader = child.stderr.take().map(|stderr| {
        let log = log.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(sender) = &progress {
                    let _ = sender.send(UpscaleProgress {
                        percent: parse_progress(&line),
                        message: line.clone(),
                    });
                }
                let mut entries = log.lock().unwrap_or_else(|poison| poison.into_inner());
                entries.push_back(line);
                while entries.len() > 80 {
                    entries.pop_front();
                }
            }
        })
    });

    let started = Instant::now();
    let mut cancelled = false;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            let _ = child.kill();
            break child.wait()?;
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(100));
    };
    if let Some(reader) = reader {
        let _ = reader.join();
    }
    if cancelled {
        bail!("AI upscale cancelled");
    }
    let log_tail = log
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if !status.success() {
        bail!(
            "Upscayl backend exited with {}: {}",
            status.code().unwrap_or(-1),
            log_tail
        );
    }
    if !output.is_file() || output.metadata()?.len() == 0 {
        bail!("Upscayl backend did not create {}", output.display());
    }
    Ok(UpscaleResult {
        output: output.to_path_buf(),
        elapsed: started.elapsed(),
        log_tail,
    })
}

fn parse_progress(line: &str) -> Option<f32> {
    let value = line
        .split_whitespace()
        .next()?
        .trim_end_matches('%')
        .parse::<f32>()
        .ok()?;
    (0.0..=100.0).contains(&value).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_complete_model_pairs_only() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["alpha", "beta", "orphan"] {
            fs::write(directory.path().join(format!("{name}.param")), b"param").unwrap();
        }
        for name in ["alpha", "beta"] {
            fs::write(directory.path().join(format!("{name}.bin")), b"weights").unwrap();
        }
        fs::write(directory.path().join("GAMMA.PARAM"), b"param").unwrap();
        fs::write(directory.path().join("GAMMA.BIN"), b"weights").unwrap();
        assert_eq!(
            discover_models(directory.path()).unwrap(),
            vec!["GAMMA", "alpha", "beta"]
        );
    }

    #[test]
    fn command_arguments_omit_disabled_options() {
        let options = UpscaleOptions {
            backend: "upscayl-bin".into(),
            models: "models".into(),
            model: "upscayl-standard-4x".into(),
            scale: 2,
            tile_size: 0,
            tta: false,
            gpu_id: String::new(),
        };
        let args = options.output_arguments(Path::new("in.png"), Path::new("out.png"));
        assert!(args.windows(2).any(|pair| pair == ["-s", "2"]));
        assert!(
            !args
                .iter()
                .any(|arg| arg == "-t" || arg == "-x" || arg == "-g")
        );

        let options = UpscaleOptions {
            tile_size: 256,
            tta: true,
            gpu_id: "1".into(),
            ..options
        };
        let args = options.output_arguments(Path::new("in.png"), Path::new("out.png"));
        assert!(args.windows(2).any(|pair| pair == ["-t", "256"]));
        assert!(args.iter().any(|arg| arg == "-x"));
        assert!(args.windows(2).any(|pair| pair == ["-g", "1"]));
    }

    #[test]
    fn suggests_models_next_to_upscayl_resources() {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("resources/bin");
        let models = directory.path().join("resources/models");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&models).unwrap();
        let backend = bin.join("upscayl-bin.exe");
        fs::write(&backend, b"").unwrap();
        assert_eq!(suggest_models_dir(&backend), Some(models));
    }

    #[test]
    fn parses_backend_progress_lines() {
        assert_eq!(parse_progress("42.50%"), Some(42.5));
        assert_eq!(parse_progress("100% complete"), Some(100.0));
        assert_eq!(parse_progress("Upscaling"), None);
        assert_eq!(parse_progress("120%"), None);
    }

    #[test]
    #[ignore = "requires XIAOZHU_UPSCAYL_BIN, XIAOZHU_UPSCAYL_MODELS and XIAOZHU_UPSCAYL_MODEL"]
    fn external_upscayl_backend_smoke() {
        let backend = std::env::var("XIAOZHU_UPSCAYL_BIN").unwrap();
        let models = std::env::var("XIAOZHU_UPSCAYL_MODELS").unwrap();
        let model = std::env::var("XIAOZHU_UPSCAYL_MODEL").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let image = image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 5) as u8, ((x + y) * 3) as u8, 255])
        });
        image.save(&input).unwrap();
        let options = UpscaleOptions {
            backend,
            models,
            model,
            scale: 2,
            ..Default::default()
        };
        let result = run_upscale(&options, &input, &output, &AtomicBool::new(false), None).unwrap();
        let output = image::open(result.output).unwrap().to_rgba8();
        assert_eq!(output.dimensions(), (128, 96));
    }
}
