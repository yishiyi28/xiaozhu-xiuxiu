use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};

use crate::{
    document::{Document, Layer},
    io, operations,
    portrait::{
        inference::PortraitModels,
        preset::{FacePolicy, PresetV1, apply_preset_to_document_with_faces, has_portrait_effect},
    },
};

#[derive(Clone, Debug)]
pub struct BatchProgress {
    pub completed: usize,
    pub total: usize,
    pub exported: usize,
    pub failed: usize,
    pub skipped: usize,
    pub current: Option<PathBuf>,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub struct BatchOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    pub recursive: bool,
    pub preset: Option<PresetV1>,
    pub format: String,
    pub quality: u8,
    pub max_edge: Option<u32>,
    pub suffix: String,
    pub overwrite: bool,
    pub cancel: Option<Arc<AtomicBool>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchFailure {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchReport {
    pub exported: usize,
    pub failed: usize,
    pub skipped: usize,
    pub failures: Vec<BatchFailure>,
}

impl BatchOptions {
    #[cfg(test)]
    pub fn test_fixture(input: &Path, overwrite: bool) -> Self {
        Self {
            input: input.to_path_buf(),
            output: input.to_path_buf(),
            recursive: false,
            preset: None,
            format: "png".into(),
            quality: 92,
            max_edge: None,
            suffix: "_xiaozhu".into(),
            overwrite,
            cancel: None,
        }
    }
}

pub fn run_batch(options: BatchOptions) -> Result<BatchReport> {
    run_batch_with_progress(options, None)
}

pub fn run_batch_with_progress(
    options: BatchOptions,
    progress: Option<Sender<BatchProgress>>,
) -> Result<BatchReport> {
    if !options.input.is_dir() {
        bail!("--batch expects a directory");
    }
    fs::create_dir_all(&options.output)
        .with_context(|| format!("Cannot create {}", options.output.display()))?;
    let mut files = Vec::new();
    collect_files(&options.input, options.recursive, &mut files)?;
    files.sort();
    let models = if options
        .preset
        .as_ref()
        .and_then(|preset| preset.portrait.as_ref())
        .is_some_and(has_portrait_effect)
    {
        Some(PortraitModels::shared()?)
    } else {
        None
    };
    let total = files.len();
    let started = Instant::now();
    let mut completed = 0;
    let mut report = BatchReport::default();
    for path in files {
        send_progress(
            progress.as_ref(),
            completed,
            total,
            &report,
            Some(path.clone()),
            started.elapsed(),
        );
        if options
            .cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
        {
            break;
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !is_batch_supported(&extension) {
            report.skipped += 1;
            completed += 1;
            continue;
        }
        let relative = path.strip_prefix(&options.input).unwrap_or(path.as_path());
        let mut target_relative = relative.to_path_buf();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        target_relative.set_file_name(format!("{stem}{}.{}", options.suffix, options.format));
        let target = options.output.join(target_relative);
        if !options.overwrite && target.exists() {
            report.skipped += 1;
            completed += 1;
            continue;
        }
        let result = process_file(&path, &target, &options, models.as_deref());
        match result {
            Ok(()) => report.exported += 1,
            Err(error) => {
                report.failed += 1;
                report.failures.push(BatchFailure {
                    path: path.clone(),
                    message: format!("{error:#}"),
                });
            }
        }
        completed += 1;
    }
    send_progress(
        progress.as_ref(),
        completed,
        total,
        &report,
        None,
        started.elapsed(),
    );
    Ok(report)
}

fn send_progress(
    sender: Option<&Sender<BatchProgress>>,
    completed: usize,
    total: usize,
    report: &BatchReport,
    current: Option<PathBuf>,
    elapsed: Duration,
) {
    if let Some(sender) = sender {
        let _ = sender.send(BatchProgress {
            completed,
            total,
            exported: report.exported,
            failed: report.failed,
            skipped: report.skipped,
            current,
            elapsed,
        });
    }
}

pub fn is_batch_supported(extension: &str) -> bool {
    matches!(
        extension,
        "xuan"
            | "psd"
            | "png"
            | "jpg"
            | "jpeg"
            | "tif"
            | "tiff"
            | "webp"
            | "bmp"
            | "gif"
            | "heic"
            | "heif"
            | "hif"
    )
}

pub fn load_batch_document(path: &Path) -> Result<Document> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "xuan" => io::load(path),
        "psd" => io::import_psd(path),
        _ => {
            let pixels = io::import_pixels(path)?;
            let (width, height) = pixels.rgba8().dimensions();
            let mut document = Document::new(width, height)?;
            document.layers = vec![Layer::from_storage(
                path.file_stem().unwrap_or_default().to_string_lossy(),
                pixels,
            )];
            Ok(document)
        }
    }
}

fn collect_files(directory: &Path, recursive: bool, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if recursive {
                collect_files(&path, recursive, files)?;
            }
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn process_file(
    source: &Path,
    target: &Path,
    options: &BatchOptions,
    models: Option<&PortraitModels>,
) -> Result<()> {
    let mut document = load_batch_document(source)?;
    if let Some(preset) = &options.preset {
        let known_faces = if let Some(models) = models
            && preset.portrait.as_ref().is_some_and(has_portrait_effect)
        {
            let layer = document.active().context("Batch layer is missing")?;
            let storage = layer.pixels.as_ref().context("Batch layer has no pixels")?;
            Some(models.detect_with_landmarks(storage.rgba8())?)
        } else {
            None
        };
        apply_preset_to_document_with_faces(
            &mut document,
            preset,
            FacePolicy::AllFaces,
            known_faces.as_deref(),
        )?;
    }
    if let Some(max_edge) = options.max_edge
        && max_edge > 0
    {
        let longest = document.width.max(document.height);
        if longest > max_edge {
            let scale = max_edge as f32 / longest as f32;
            let width = (document.width as f32 * scale).round().max(1.0) as u32;
            let height = (document.height as f32 * scale).round().max(1.0) as u32;
            operations::image_size(&mut document, width, height)?;
        }
    }
    let parent = target.parent().context("Output path has no parent")?;
    fs::create_dir_all(parent)?;
    // io::export writes through its own sibling temporary file and atomically
    // persists the completed image. The pre-flight exists check above enforces
    // the no-overwrite policy before any decoding or rendering work begins.
    io::export(&document, target, options.quality)
        .with_context(|| format!("Cannot export {}", target.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portrait::preset::apply_preset_to_document;
    use image::{Rgba, RgbaImage};
    use std::fs;

    #[test]
    fn batch_skips_existing_target_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        write_png(&dir.path().join("photo.png"));
        fs::write(dir.path().join("photo_xiaozhu.png"), b"keep").unwrap();
        let report = run_batch(BatchOptions::test_fixture(dir.path(), false)).unwrap();
        assert_eq!(report.skipped, 1);
        assert_eq!(
            fs::read(dir.path().join("photo_xiaozhu.png")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn batch_reports_bad_file_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        write_png(&dir.path().join("good.png"));
        fs::write(dir.path().join("bad.png"), b"not an image").unwrap();
        let mut options = BatchOptions::test_fixture(dir.path(), true);
        options.output = dir.path().join("out");
        fs::create_dir_all(&options.output).unwrap();
        let report = run_batch(options).unwrap();
        assert_eq!(report.failed, 1);
        assert_eq!(report.exported, 1);
    }

    #[test]
    fn batch_resizes_longest_edge_and_applies_color_preset() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("wide.png");
        RgbaImage::from_pixel(128, 64, Rgba([120, 100, 80, 255]))
            .save(&source)
            .unwrap();
        let mut options = BatchOptions::test_fixture(dir.path(), true);
        options.output = dir.path().join("out");
        options.max_edge = Some(64);
        options.suffix = "_edit".into();
        options.preset = Some(crate::portrait::preset::PresetV1 {
            schema_version: 1,
            name: "Warm".into(),
            color: Some(crate::document::ColorGrade {
                temperature: 20.0,
                ..Default::default()
            }),
            portrait: None,
        });
        let report = run_batch(options).unwrap();
        assert_eq!(report.exported, 1);
        let output = image::open(dir.path().join("out/wide_edit.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(output.dimensions(), (64, 32));
    }

    #[test]
    fn batch_honours_cancellation_between_files() {
        let dir = tempfile::tempdir().unwrap();
        write_png(&dir.path().join("one.png"));
        write_png(&dir.path().join("two.png"));
        let mut options = BatchOptions::test_fixture(dir.path(), true);
        options.output = dir.path().join("out");
        options.cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            true,
        )));
        let report = run_batch(options).unwrap();
        assert_eq!(report.exported, 0);
        assert!(!dir.path().join("out/one_xiaozhu.png").exists());
    }

    #[test]
    fn batch_output_matches_single_image_preset_render() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("input.png");
        let pixels = RgbaImage::from_fn(32, 24, |x, y| {
            Rgba([(x * 5) as u8, (y * 7) as u8, ((x + y) * 3) as u8, 255])
        });
        pixels.save(&source).unwrap();
        let preset = crate::portrait::preset::PresetV1 {
            schema_version: 1,
            name: "Test".into(),
            color: Some(crate::document::ColorGrade {
                contrast: 12.0,
                temperature: 9.0,
                ..Default::default()
            }),
            portrait: None,
        };
        let mut single = Document::new(32, 24).unwrap();
        single.layers = vec![Layer::from_storage(
            "input",
            crate::document::PixelStorage::from_rgba8(pixels),
        )];
        apply_preset_to_document(&mut single, &preset, FacePolicy::NoPortrait).unwrap();
        let expected = crate::render::render(&single);
        let mut options = BatchOptions::test_fixture(dir.path(), true);
        options.output = dir.path().join("out");
        options.suffix = "_batch".into();
        options.preset = Some(preset);
        std::fs::create_dir_all(&options.output).unwrap();
        let report = run_batch(options).unwrap();
        assert_eq!(report.exported, 1);
        let actual = image::open(dir.path().join("out/input_batch.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(expected, actual);
    }

    #[test]
    fn batch_reports_progress_and_finishes_with_completed_total() {
        let dir = tempfile::tempdir().unwrap();
        write_png(&dir.path().join("one.png"));
        write_png(&dir.path().join("two.png"));
        let mut options = BatchOptions::test_fixture(dir.path(), true);
        options.output = dir.path().join("out");
        let (send, receive) = std::sync::mpsc::channel();
        let report = run_batch_with_progress(options, Some(send)).unwrap();
        assert_eq!(report.exported, 2);
        let events: Vec<_> = receive.try_iter().collect();
        assert!(
            events
                .iter()
                .any(|event| event.total == 2 && event.current.is_some())
        );
        let last = events.last().unwrap();
        assert_eq!(last.current, None);
        assert_eq!(last.completed, 2);
        assert_eq!(last.total, 2);
    }

    fn write_png(path: &Path) {
        let image = RgbaImage::from_pixel(8, 8, Rgba([20, 40, 60, 255]));
        image.save(path).unwrap();
    }
}
