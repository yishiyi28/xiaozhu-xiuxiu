use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
};

use anyhow::{Context, Result};
use xuan::{
    config::Config,
    document::Document,
    i18n::tr,
    render,
    upscale::{
        UpscaleOptions, UpscaleProgress, UpscaleResult, discover_models, run_upscale,
        suggest_models_dir,
    },
};

use super::{EditorApp, theme, widgets};

pub(super) struct UpscaleDialogState {
    pub options: UpscaleOptions,
    pub models: Vec<String>,
    pub models_key: String,
    pub models_error: Option<String>,
    pub status: Option<String>,
    pub error: Option<String>,
    pub progress: Option<f32>,
    pub progress_line: String,
}

impl UpscaleDialogState {
    pub(super) fn from_config(options: &UpscaleOptions) -> Self {
        let mut state = Self {
            options: options.clone(),
            models: Vec::new(),
            models_key: String::new(),
            models_error: None,
            status: None,
            error: None,
            progress: None,
            progress_line: String::new(),
        };
        state.refresh_models();
        state
    }

    fn refresh_models(&mut self) {
        if self.models_key == self.options.models {
            return;
        }
        self.models_key = self.options.models.clone();
        self.models.clear();
        self.models_error = None;
        if self.options.models.trim().is_empty() {
            return;
        }
        match discover_models(Path::new(self.options.models.trim())) {
            Ok(models) => {
                self.models = models;
                if !self.models.iter().any(|model| model == &self.options.model)
                    && let Some(first) = self.models.first()
                {
                    self.options.model = first.clone();
                }
            }
            Err(error) => self.models_error = Some(format!("{error:#}")),
        }
    }
}

pub(super) struct UpscaleJob {
    receive: Receiver<std::result::Result<UpscaleResult, String>>,
    progress: Receiver<UpscaleProgress>,
    cancel: Arc<AtomicBool>,
    title: String,
    source_path: Option<PathBuf>,
    _temporary: tempfile::TempDir,
}

impl UpscaleJob {
    pub(super) fn spawn(
        document: Document,
        title: String,
        source_path: Option<PathBuf>,
        options: UpscaleOptions,
        context: egui::Context,
    ) -> Result<Self> {
        options.validate()?;
        let temporary = tempfile::Builder::new()
            .prefix("xiaozhu-upscale-")
            .tempdir()
            .context("Cannot create AI upscale temporary directory")?;
        let input = temporary.path().join("input.png");
        let output = temporary.path().join("upscaled.png");
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (send, receive) = mpsc::channel();
        let (progress_send, progress) = mpsc::channel();
        let worker_input = input.clone();
        let worker_output = output.clone();
        std::thread::spawn(move || {
            let _ = progress_send.send(UpscaleProgress {
                percent: Some(0.0),
                message: tr("Preparing image…").into(),
            });
            let result = (|| -> Result<UpscaleResult> {
                render::render(&document)
                    .save(&worker_input)
                    .with_context(|| format!("Cannot write {}", worker_input.display()))?;
                run_upscale(
                    &options,
                    &worker_input,
                    &worker_output,
                    &worker_cancel,
                    Some(progress_send),
                )
            })()
            .map_err(|error| format!("{error:#}"));
            let _ = send.send(result);
            context.request_repaint();
        });
        Ok(Self {
            receive,
            progress,
            cancel,
            title,
            source_path,
            _temporary: temporary,
        })
    }

    pub(super) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub(super) fn take_progress(&self) -> Option<UpscaleProgress> {
        let mut latest = None;
        while let Ok(progress) = self.progress.try_recv() {
            latest = Some(progress);
        }
        latest
    }

    pub(super) fn poll(&self) -> Option<std::result::Result<UpscaleResult, String>> {
        match self.receive.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(tr("AI upscale worker stopped unexpectedly").into()))
            }
        }
    }

    pub(super) fn title(&self) -> &str {
        &self.title
    }
}

impl EditorApp {
    fn save_upscale_options(&mut self, options: UpscaleOptions) -> Result<()> {
        let config = Config {
            language: self.config.language,
            autosave_seconds: self.config.autosave_seconds,
            upscale: options.clone(),
        };
        Config::path().and_then(|path| config.save(&path))?;
        self.config.upscale = options;
        Ok(())
    }

    pub(super) fn poll_upscale(&mut self) {
        let Some(job) = &self.upscale_job else {
            return;
        };
        if let Some(progress) = job.take_progress()
            && let Some(dialog) = &mut self.upscale_dialog
        {
            dialog.progress = progress.percent;
            dialog.progress_line = progress.message;
        }
        let Some(result) = job.poll() else {
            self.context
                .request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        let job = self.upscale_job.take().expect("upscale job");
        match result {
            Ok(result) if !job.cancelled() => {
                let title = job.title().to_owned();
                self.open_path(&result.output, false);
                if self.error.is_none()
                    && let Some(session) = self.session_mut()
                {
                    session.title = title;
                    session.source_path = job.source_path.clone();
                    self.status = format!(
                        "{} · {:.1}s",
                        tr("AI upscale complete"),
                        result.elapsed.as_secs_f64()
                    );
                    self.dialog = None;
                    self.upscale_dialog = None;
                }
            }
            Ok(_) => {
                if let Some(dialog) = &mut self.upscale_dialog {
                    dialog.status = Some(tr("AI upscale cancelled").into());
                    dialog.progress = None;
                    dialog.progress_line.clear();
                }
            }
            Err(_error) if job.cancelled() => {
                if let Some(dialog) = &mut self.upscale_dialog {
                    dialog.status = Some(tr("AI upscale cancelled").into());
                    dialog.progress = None;
                    dialog.progress_line.clear();
                }
            }
            Err(error) => {
                if let Some(dialog) = &mut self.upscale_dialog {
                    dialog.error = Some(error);
                    dialog.progress = None;
                }
            }
        }
    }

    pub(super) fn upscale_dialog(&mut self, context: &egui::Context) {
        let Some(mut dialog) = self.upscale_dialog.take() else {
            self.dialog = None;
            return;
        };
        let running = self.upscale_job.is_some();
        let has_document = self.session().is_some();
        let mut open = true;
        let mut start = false;
        let mut save_settings = false;
        let mut cancel = false;
        let mut refresh = false;
        widgets::Window::new(tr("AI Upscale"))
            .id("ai_upscale")
            .default_width(680.0)
            .open(&mut open)
            .show(context, |ui| {
                ui.label(
                    egui::RichText::new(tr(
                        "Uses a separately installed Upscayl-compatible NCNN backend. Backend and model licenses are not bundled with 小猪修修.",
                    ))
                    .color(theme::MUTED),
                );
                ui.add_space(8.0);
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tr("Backend executable"));
                        ui.text_edit_singleline(&mut dialog.options.backend);
                        if widgets::button(ui, tr("Choose…")).clicked()
                            && let Some(path) = rfd::FileDialog::new().pick_file()
                        {
                            dialog.options.backend = path.display().to_string();
                            if let Some(models) = suggest_models_dir(&path) {
                                dialog.options.models = models.display().to_string();
                                dialog.models_key.clear();
                                refresh = true;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Model folder"));
                        ui.text_edit_singleline(&mut dialog.options.models);
                        if widgets::button(ui, tr("Choose…")).clicked()
                            && let Some(path) = rfd::FileDialog::new().pick_folder()
                        {
                            dialog.options.models = path.display().to_string();
                            dialog.models_key.clear();
                            refresh = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Model"));
                        widgets::PopUp::from_id_salt("upscale_model")
                            .selected_text(if dialog.options.model.is_empty() {
                                tr("No models found").to_owned()
                            } else {
                                dialog.options.model.clone()
                            })
                            .width(230.0)
                            .show_ui(ui, |ui| {
                                for model in &dialog.models {
                                    widgets::menu_choice(
                                        ui,
                                        &mut dialog.options.model,
                                        model.clone(),
                                        model,
                                    );
                                }
                            });
                        if widgets::button(ui, tr("Refresh models")).clicked() {
                            dialog.models_key.clear();
                            refresh = true;
                        }
                    });
                    if let Some(error) = &dialog.models_error {
                        ui.colored_label(egui::Color32::from_rgb(230, 110, 100), error);
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr("Scale"));
                        for scale in [2, 3, 4] {
                            if ui
                                .selectable_label(dialog.options.scale == scale, format!("{scale}×"))
                                .clicked()
                            {
                                dialog.options.scale = scale;
                            }
                        }
                    });
                    ui.add(
                        widgets::Slider::new(&mut dialog.options.tile_size, 0..=1024)
                            .text(tr("Tile size (0 = automatic)"))
                            .suffix(" px"),
                    );
                    ui.checkbox(&mut dialog.options.tta, tr("TTA mode (slower, higher quality)"));
                    ui.horizontal(|ui| {
                        ui.label(tr("GPU ID"));
                        ui.text_edit_singleline(&mut dialog.options.gpu_id)
                            .on_hover_text(tr("Optional backend GPU index"));
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(tr(
                            "The result opens as a new flattened document. Layers, 16-bit precision and project masks are not preserved.",
                        ))
                        .color(theme::MUTED),
                    );
                });
                if let Some(error) = &dialog.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 100), error);
                }
                if let Some(status) = &dialog.status {
                    ui.label(status);
                }
                if running {
                    ui.separator();
                    if let Some(progress) = dialog.progress {
                        ui.add(egui::ProgressBar::new(progress / 100.0).show_percentage());
                    } else {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr("Processing…"));
                        });
                    }
                    if !dialog.progress_line.is_empty() {
                        ui.label(
                            egui::RichText::new(&dialog.progress_line)
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if running {
                        if widgets::button(ui, tr("Cancel")).clicked() {
                            cancel = true;
                        }
                    } else {
                        if has_document {
                            if widgets::primary_button(ui, tr("Start AI Upscale")).clicked() {
                                start = true;
                            }
                        } else {
                            ui.label(tr("Open an image to start AI upscale."));
                        }
                        if widgets::button(ui, tr("Save settings")).clicked() {
                            save_settings = true;
                        }
                    }
                });
            });
        if refresh {
            dialog.refresh_models();
        }
        if cancel && let Some(job) = &self.upscale_job {
            job.cancel();
            dialog.status = Some(tr("Cancelling…").into());
        }
        if save_settings {
            match self.save_upscale_options(dialog.options.clone()) {
                Ok(()) => dialog.status = Some(tr("AI upscale settings saved").into()),
                Err(error) => dialog.error = Some(format!("{error:#}")),
            }
        }
        if start {
            let Some(session) = self.session() else {
                dialog.error = Some(tr("Open an image first").into());
                self.upscale_dialog = Some(dialog);
                return;
            };
            let document = session.document.clone();
            let source_title = session.title.clone();
            let source_path = session.source_path.clone().or_else(|| session.path.clone());
            let title = format!(
                "{} — {} {}×",
                source_title,
                tr("AI Upscale"),
                dialog.options.scale
            );
            match UpscaleJob::spawn(
                document,
                title,
                source_path,
                dialog.options.clone(),
                context.clone(),
            ) {
                Ok(job) => {
                    self.upscale_job = Some(job);
                    dialog.error = None;
                    dialog.status = Some(tr("Starting AI upscale…").into());
                    if let Err(error) = self.save_upscale_options(dialog.options.clone()) {
                        dialog.error = Some(format!("{error:#}"));
                    }
                }
                Err(error) => dialog.error = Some(format!("{error:#}")),
            }
        }
        if self.upscale_job.is_some() {
            context.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !open {
            if !save_settings && let Err(error) = self.save_upscale_options(dialog.options.clone())
            {
                self.error = Some(format!("{error:#}"));
            }
            if let Some(job) = &self.upscale_job {
                job.cancel();
            }
            self.dialog = None;
            self.upscale_dialog = None;
        } else {
            self.upscale_dialog = Some(dialog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Dialog;

    #[test]
    #[ignore = "requires XIAOZHU_UPSCAYL_BIN, XIAOZHU_UPSCAYL_MODELS and XIAOZHU_UPSCAYL_MODEL"]
    fn external_app_upscale_opens_a_new_document() {
        let options = UpscaleOptions {
            backend: std::env::var("XIAOZHU_UPSCAYL_BIN").unwrap(),
            models: std::env::var("XIAOZHU_UPSCAYL_MODELS").unwrap(),
            model: std::env::var("XIAOZHU_UPSCAYL_MODEL").unwrap(),
            scale: 2,
            ..Default::default()
        };
        let context = egui::Context::default();
        let mut app = EditorApp::with_context(&context, Vec::new(), false, None);
        app.dimensions = [32, 24];
        app.new_document();
        app.command("fill_fg");
        let document = app.session().unwrap().document.clone();
        let source_path = PathBuf::from("C:/photos/source.jpg");
        let job = UpscaleJob::spawn(
            document,
            "Upscale smoke".into(),
            Some(source_path.clone()),
            options.clone(),
            context.clone(),
        )
        .unwrap();
        app.upscale_dialog = Some(UpscaleDialogState::from_config(&options));
        app.dialog = Some(Dialog::Upscale);
        app.upscale_job = Some(job);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.upscale_job.is_some() && std::time::Instant::now() < deadline {
            app.poll_upscale();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(app.upscale_job.is_none(), "upscale job timed out");
        assert!(app.error.is_none(), "{:?}", app.error);
        let session = app.session().unwrap();
        assert_eq!(session.source_path.as_ref(), Some(&source_path));
        let document = &session.document;
        assert_eq!((document.width, document.height), (64, 48));
    }
}
