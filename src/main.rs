#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "小猪修修",
    version,
    about,
    after_help = "小猪修修（原 XUAN）永久免费。项目使用 .xuan 格式，也可打开原有 .comp 目录包。"
)]
struct Args {
    /// Images or projects to open
    #[arg(value_name = "IMAGE|PROJECT")]
    paths: Vec<PathBuf>,

    /// Start with a sample composition
    #[arg(long)]
    demo: bool,

    /// Capture the window to a file and exit
    #[arg(long, value_name = "PATH")]
    screenshot: Option<PathBuf>,

    /// Open a panel for the screenshot
    #[arg(
        long,
        value_name = "PANEL",
        value_parser = ["levels", "hue", "curves", "export", "brush", "selection", "gradient", "shape", "text", "new", "settings"]
    )]
    screenshot_panel: Option<String>,

    /// Export every supported file in a directory without opening the editor
    #[arg(long, value_name = "DIR")]
    batch: Option<PathBuf>,

    /// Output directory for --batch (defaults to the input directory)
    #[arg(long, value_name = "DIR", requires = "batch")]
    out: Option<PathBuf>,

    /// Output format for --batch
    #[arg(
        long,
        value_name = "FORMAT",
        default_value = "png",
        requires = "batch",
        value_parser = ["png", "jpeg", "tiff", "webp"]
    )]
    format: String,

    /// Recurse into subdirectories during --batch
    #[arg(long, requires = "batch")]
    recursive: bool,

    /// JPEG/WebP quality for --batch
    #[arg(
        long,
        default_value_t = 92,
        requires = "batch",
        value_parser = clap::value_parser!(u8).range(1..=100)
    )]
    quality: u8,
}

fn is_batch_supported(extension: &str) -> bool {
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

fn collect_batch_files(
    directory: &std::path::Path,
    recursive: bool,
    files: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if recursive {
                collect_batch_files(&path, recursive, files)?;
            }
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn batch_document(path: &std::path::Path) -> anyhow::Result<xuan::document::Document> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "xuan" => xuan::io::load(path),
        "psd" => xuan::io::import_psd(path),
        _ => {
            let pixels = xuan::io::import_pixels(path)?;
            let (width, height) = pixels.rgba8().dimensions();
            let mut document = xuan::document::Document::new(width, height)?;
            let layer = xuan::document::Layer::from_storage(
                path.file_stem().unwrap_or_default().to_string_lossy(),
                pixels,
            );
            document.layers = vec![layer];
            Ok(document)
        }
    }
}

fn batch_export(args: &Args, input: &std::path::Path) -> anyhow::Result<(usize, usize, usize)> {
    use anyhow::Context;

    if !input.is_dir() {
        anyhow::bail!("--batch expects a directory");
    }
    let output = args.out.clone().unwrap_or_else(|| input.to_path_buf());
    std::fs::create_dir_all(&output)
        .with_context(|| format!("Cannot create {}", output.display()))?;
    let mut files = Vec::new();
    collect_batch_files(input, args.recursive, &mut files)?;
    files.sort();
    let mut exported = 0;
    let mut failed = 0;
    let mut skipped = 0;
    for (index, path) in files.iter().enumerate() {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !is_batch_supported(&extension) {
            skipped += 1;
            continue;
        }
        let document = match batch_document(path) {
            Ok(document) => document,
            Err(error) => {
                eprintln!("[{}] {}: {error:#}", index + 1, path.display());
                failed += 1;
                continue;
            }
        };
        let relative = path.strip_prefix(input).unwrap_or(path.as_path());
        let mut target = output.join(relative);
        target.set_extension(&args.format);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match xuan::io::export(&document, &target, args.quality) {
            Ok(()) => {
                exported += 1;
                println!("[{}] {} -> {}", index + 1, path.display(), target.display());
            }
            Err(error) => {
                eprintln!("[{}] {}: {error:#}", index + 1, path.display());
                failed += 1;
            }
        }
    }
    Ok((exported, failed, skipped))
}

fn main() -> eframe::Result {
    let args = Args::parse();
    if let Some(input) = args.batch.clone() {
        match batch_export(&args, &input) {
            Ok((exported, failed, skipped)) => {
                println!("batch: {exported} exported, {failed} failed, {skipped} skipped");
                std::process::exit(if failed == 0 { 0 } else { 1 });
            }
            Err(error) => {
                eprintln!("batch export failed: {error:#}");
                std::process::exit(1);
            }
        }
    }
    let Args {
        paths,
        demo,
        screenshot,
        screenshot_panel,
        ..
    } = args;
    let icon = image::load_from_memory(include_bytes!(
        "../assets/icons/hicolor/256x256/apps/com.xiaozhu.xiuxiu.png"
    ))
    .expect("bundled application icon")
    .to_rgba8();
    let icon = egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    };
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::default();
    let default_descriptor = setup.device_descriptor.clone();
    setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut descriptor = default_descriptor(adapter);
        // Full-resolution float processing needs more than wgpu's portable 128 MiB
        // storage binding default. Request only the limits the adapter supports.
        let limits = adapter.limits();
        if limits.max_storage_buffers_per_shader_stage >= 4 {
            descriptor.required_limits.max_storage_buffer_binding_size =
                limits.max_storage_buffer_binding_size;
            descriptor.required_limits.max_buffer_size = limits.max_buffer_size;
            descriptor.required_limits.max_texture_dimension_2d = limits.max_texture_dimension_2d;
        }
        // Keep imported 16-bit layers in an unorm texture when the GPU supports it.
        if adapter
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
        {
            descriptor.required_features |= wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
        }
        descriptor
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("小猪修修")
            .with_decorations(false)
            .with_transparent(true)
            .with_icon(icon)
            .with_app_id("com.xiaozhu.xiuxiu")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([850.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(setup),
            ..Default::default()
        },
        persist_window: screenshot.is_none(),
        ..Default::default()
    };
    eframe::run_native(
        "小猪修修",
        options,
        Box::new(move |cc| {
            let mut app = app::EditorApp::new(cc, paths, demo, screenshot);
            if let Some(panel) = screenshot_panel {
                app.preview_panel(&panel);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn parses_batch_options() {
        let args = Args::try_parse_from([
            "xuan",
            "--batch",
            "in",
            "--out",
            "out",
            "--format",
            "jpeg",
            "--recursive",
            "--quality",
            "80",
        ])
        .unwrap();
        assert_eq!(args.batch, Some(PathBuf::from("in")));
        assert_eq!(args.out, Some(PathBuf::from("out")));
        assert_eq!(args.format, "jpeg");
        assert!(args.recursive);
        assert_eq!(args.quality, 80);
    }

    #[test]
    fn launches_without_arguments() {
        let args = Args::try_parse_from(["xuan"]).unwrap();
        assert!(args.paths.is_empty());
        assert!(!args.demo);
        assert!(args.screenshot.is_none());
        assert!(args.screenshot_panel.is_none());
    }

    #[test]
    fn accepts_paths_interleaved_with_options() {
        let args = Args::try_parse_from([
            "xuan",
            "photo.png",
            "--demo",
            "composition.xuan",
            "--screenshot",
            "preview.png",
            "--screenshot-panel",
            "levels",
            "original.comp",
        ])
        .unwrap();

        assert_eq!(
            args.paths,
            ["photo.png", "composition.xuan", "original.comp"].map(PathBuf::from)
        );
        assert!(args.demo);
        assert_eq!(args.screenshot, Some(PathBuf::from("preview.png")));
        assert_eq!(args.screenshot_panel.as_deref(), Some("levels"));
    }

    #[test]
    fn accepts_option_like_paths_after_separator() {
        let args = Args::try_parse_from(["xuan", "--", "--demo", "-photo.png"]).unwrap();
        assert_eq!(args.paths, ["--demo", "-photo.png"].map(PathBuf::from));
        assert!(!args.demo);
    }

    #[cfg(unix)]
    #[test]
    fn accepts_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path = OsString::from_vec(b"photo-\xff.png".to_vec());
        let args = Args::try_parse_from([
            OsString::from("xuan"),
            path.clone(),
            OsString::from("--screenshot"),
            path.clone(),
        ])
        .unwrap();

        assert_eq!(args.paths, [PathBuf::from(&path)]);
        assert_eq!(args.screenshot, Some(PathBuf::from(path)));
    }

    #[test]
    fn rejects_invalid_arguments() {
        for (args, kind) in [
            (vec!["xuan", "--unknown"], ErrorKind::UnknownArgument),
            (vec!["xuan", "--screenshot"], ErrorKind::InvalidValue),
            (vec!["xuan", "--screenshot-panel"], ErrorKind::InvalidValue),
            (
                vec!["xuan", "--screenshot-panel", "unknown"],
                ErrorKind::InvalidValue,
            ),
            (
                vec!["xuan", "--screenshot", "--demo"],
                ErrorKind::InvalidValue,
            ),
        ] {
            let error = Args::try_parse_from(&args).unwrap_err();
            assert_eq!(error.kind(), kind, "arguments: {args:?}");
        }
    }

    #[test]
    fn displays_help_and_version() {
        for (flag, kind) in [
            ("--help", ErrorKind::DisplayHelp),
            ("-h", ErrorKind::DisplayHelp),
            ("--version", ErrorKind::DisplayVersion),
            ("-V", ErrorKind::DisplayVersion),
        ] {
            let error = Args::try_parse_from(["xuan", flag]).unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(error.exit_code(), 0);
        }
    }
}
