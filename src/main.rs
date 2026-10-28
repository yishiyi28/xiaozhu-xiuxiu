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

    /// Apply an .xzp preset during --batch
    #[arg(long, value_name = "PRESET", requires = "batch")]
    preset: Option<PathBuf>,

    /// Resize the longest edge during --batch (0 keeps original size)
    #[arg(long, value_name = "PIXELS", requires = "batch", value_parser = parse_max_edge)]
    max_edge: Option<u32>,

    /// Output filename suffix during --batch
    #[arg(long, value_name = "SUFFIX", requires = "batch")]
    suffix: Option<String>,

    /// Replace existing output files during --batch
    #[arg(long, requires = "batch")]
    overwrite: bool,
}

fn parse_max_edge(value: &str) -> Result<u32, String> {
    let value = value
        .parse::<u32>()
        .map_err(|_| "max edge must be an integer".to_owned())?;
    if value == 0 || (64..=30_000).contains(&value) {
        Ok(value)
    } else {
        Err("max edge must be 0 or between 64 and 30000".into())
    }
}

fn main() -> eframe::Result {
    let args = Args::parse();
    if let Some(input) = args.batch.clone() {
        let preset = match args.preset.as_ref() {
            Some(path) => match xuan::portrait::preset::PresetStore::load(path) {
                Ok(preset) => Some(preset),
                Err(error) => {
                    eprintln!("batch preset failed: {error:#}");
                    std::process::exit(1);
                }
            },
            None => None,
        };
        let suffix = args.suffix.clone().unwrap_or_else(|| {
            if preset.is_some() {
                "_xiaozhu".into()
            } else {
                String::new()
            }
        });
        let options = xuan::batch::BatchOptions {
            output: args.out.clone().unwrap_or_else(|| input.clone()),
            input,
            recursive: args.recursive,
            preset,
            format: args.format.clone(),
            quality: args.quality,
            max_edge: args.max_edge.filter(|value| *value > 0),
            suffix,
            overwrite: args.overwrite || args.preset.is_none(),
            cancel: None,
        };
        match xuan::batch::run_batch(options) {
            Ok(report) => {
                for failure in &report.failures {
                    eprintln!("{}: {}", failure.path.display(), failure.message);
                }
                println!(
                    "batch: {} exported, {} failed, {} skipped",
                    report.exported, report.failed, report.skipped
                );
                std::process::exit(if report.failed == 0 { 0 } else { 1 });
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
            .with_min_inner_size([850.0, 560.0])
            .with_clamp_size_to_monitor_size(true),
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
    fn parses_preset_batch_options() {
        let args = Args::try_parse_from([
            "xuan",
            "--batch",
            "in",
            "--out",
            "out",
            "--preset",
            "look.xzp",
            "--max-edge",
            "2048",
            "--suffix",
            "_edit",
            "--overwrite",
        ])
        .unwrap();
        assert_eq!(args.preset, Some(PathBuf::from("look.xzp")));
        assert_eq!(args.max_edge, Some(2048));
        assert_eq!(args.suffix.as_deref(), Some("_edit"));
        assert!(args.overwrite);
    }

    #[test]
    fn rejects_unsafe_max_edge_range() {
        let error =
            Args::try_parse_from(["xuan", "--batch", "in", "--max-edge", "63"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ValueValidation);
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
