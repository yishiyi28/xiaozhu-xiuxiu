use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
};

use xuan::{
    batch::{BatchOptions, BatchProgress, BatchReport, ConflictPolicy, run_batch_with_progress},
    i18n::tr,
    portrait::preset::PresetV1,
};

use super::{EditorApp, widgets};

pub struct BatchDialogState {
    pub input: String,
    pub output: String,
    pub recursive: bool,
    pub preset: Option<PresetV1>,
    pub user_presets: Vec<PresetV1>,
    pub format: String,
    pub quality: u8,
    pub max_edge: u32,
    pub suffix: String,
    pub conflict: ConflictPolicy,
    pub error: Option<String>,
    pub report: Option<BatchReport>,
    pub progress: Option<BatchProgress>,
}

impl Default for BatchDialogState {
    fn default() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            recursive: false,
            preset: None,
            user_presets: xuan::portrait::preset::PresetStore::user_presets().unwrap_or_default(),
            format: "png".into(),
            quality: 92,
            max_edge: 0,
            suffix: "_xiaozhu".into(),
            conflict: ConflictPolicy::Skip,
            error: None,
            report: None,
            progress: None,
        }
    }
}

impl BatchDialogState {
    pub fn ready(&self) -> bool {
        let valid_edge = self.max_edge == 0 || (64..=30_000).contains(&self.max_edge);
        !self.input.trim().is_empty()
            && !self.output.trim().is_empty()
            && self.preset.is_some()
            && valid_edge
    }
}

pub struct BatchJob {
    receive: Receiver<Result<BatchReport, String>>,
    progress: Receiver<BatchProgress>,
    cancel: Arc<AtomicBool>,
}

impl BatchJob {
    pub fn spawn(options: BatchOptions, context: egui::Context) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut options = options;
        options.cancel = Some(cancel.clone());
        let (send, receive) = mpsc::channel();
        let (progress_send, progress_receive) = mpsc::channel();
        xuan::gpu::spawn(move || {
            let result = run_batch_with_progress(options, Some(progress_send))
                .map_err(|error| format!("{error:#}"));
            let _ = send.send(result);
            context.request_repaint();
        });
        Self {
            receive,
            progress: progress_receive,
            cancel,
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn take_progress(&self) -> Option<BatchProgress> {
        let mut latest = None;
        while let Ok(progress) = self.progress.try_recv() {
            latest = Some(progress);
        }
        latest
    }

    fn poll(&self) -> Option<Result<BatchReport, String>> {
        match self.receive.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(tr("Batch worker stopped unexpectedly").into()))
            }
        }
    }
}

impl EditorApp {
    pub(super) fn batch_dialog(&mut self, context: &egui::Context) {
        let progress = self.batch_job.as_ref().and_then(BatchJob::take_progress);
        if let Some(progress) = progress
            && let Some(dialog) = &mut self.batch_dialog
        {
            dialog.progress = Some(progress);
        }
        let result = self.batch_job.as_ref().and_then(BatchJob::poll);
        if let Some(result) = result {
            self.batch_job = None;
            if let Some(dialog) = &mut self.batch_dialog {
                dialog.progress = None;
            }
            match result {
                Ok(report) => {
                    if let Some(dialog) = &mut self.batch_dialog {
                        dialog.report = Some(report);
                        dialog.error = None;
                    }
                }
                Err(error) => {
                    if let Some(dialog) = &mut self.batch_dialog {
                        dialog.error = Some(error);
                    }
                }
            }
        }
        let Some(mut dialog) = self.batch_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut run = false;
        let mut cancel = false;
        widgets::Window::new(tr("Batch Export"))
            .default_width(460.0)
            .open(&mut open)
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("Input folder"));
                    ui.text_edit_singleline(&mut dialog.input);
                    if widgets::button(ui, tr("Choose…")).clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        dialog.input = path.display().to_string();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(tr("Output folder"));
                    ui.text_edit_singleline(&mut dialog.output);
                    if widgets::button(ui, tr("Choose…")).clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        dialog.output = path.display().to_string();
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut dialog.recursive, tr("Include subfolders"));
                });
                ui.horizontal(|ui| {
                    ui.label(tr("Existing files"));
                    for (policy, label) in [
                        (ConflictPolicy::Skip, tr("Skip")),
                        (ConflictPolicy::Overwrite, tr("Overwrite")),
                        (ConflictPolicy::Rename, tr("Auto-number")),
                    ] {
                        if ui
                            .selectable_label(dialog.conflict == policy, label)
                            .clicked()
                        {
                            dialog.conflict = policy;
                        }
                    }
                });
                ui.separator();
                ui.label(tr("Preset"));
                ui.horizontal_wrapped(|ui| {
                    for (slug, name) in [
                        ("natural", tr("Natural")),
                        ("clear-portrait", tr("Clear Portrait")),
                        ("warm", tr("Warm")),
                        ("cool", tr("Cool")),
                        ("film", tr("Film")),
                        ("soft-film", tr("Soft Film")),
                        ("mono", tr("Mono")),
                        ("high-contrast-mono", tr("High-contrast Mono")),
                    ] {
                        let builtin = PresetV1::builtin(slug).ok();
                        let selected = dialog
                            .preset
                            .as_ref()
                            .zip(builtin.as_ref())
                            .is_some_and(|(current, builtin)| current.name == builtin.name);
                        if ui.selectable_label(selected, name).clicked() {
                            dialog.preset = builtin;
                        }
                    }
                });
                if !dialog.user_presets.is_empty() {
                    ui.label(tr("User presets"));
                    ui.horizontal_wrapped(|ui| {
                        for preset in &dialog.user_presets {
                            let selected = dialog
                                .preset
                                .as_ref()
                                .is_some_and(|current| current.name == preset.name);
                            if ui.selectable_label(selected, &preset.name).clicked() {
                                dialog.preset = Some(preset.clone());
                            }
                        }
                    });
                }
                if let Some(preset) = &dialog.preset {
                    ui.label(format!("{}: {}", tr("Selected preset"), preset.name));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(tr("Format"));
                    for format in ["png", "jpeg", "tiff", "webp"] {
                        if ui
                            .selectable_label(dialog.format == format, format.to_uppercase())
                            .clicked()
                        {
                            dialog.format = format.into();
                        }
                    }
                });
                ui.add(
                    widgets::Slider::new(&mut dialog.quality, 1..=100)
                        .text(tr("Quality"))
                        .suffix("%"),
                );
                ui.add(
                    widgets::Slider::new(&mut dialog.max_edge, 0..=30_000)
                        .text(tr("Max edge"))
                        .suffix(tr(" px (0 = original)")),
                );
                ui.horizontal(|ui| {
                    ui.label(tr("Filename suffix"));
                    ui.text_edit_singleline(&mut dialog.suffix);
                });
                if !dialog.ready() {
                    ui.label(tr("Choose input, output, and a preset."));
                }
                if let Some(error) = &dialog.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 100), error);
                }
                if let Some(progress) = &dialog.progress {
                    ui.separator();
                    ui.label(format!(
                        "{} {}/{}",
                        tr("Progress"),
                        progress.completed,
                        progress.total
                    ));
                    if let Some(path) = &progress.current {
                        ui.label(format!("{}: {}", tr("Current"), path.display()));
                    }
                    let elapsed = progress.elapsed.as_secs_f64();
                    let eta = if progress.completed > 0 {
                        elapsed / progress.completed as f64
                            * progress.total.saturating_sub(progress.completed) as f64
                    } else {
                        0.0
                    };
                    ui.label(format!(
                        "{} {:.1}s  {} {:.1}s",
                        tr("Elapsed"),
                        elapsed,
                        tr("ETA"),
                        eta
                    ));
                    ui.label(format!(
                        "{} {}  {} {}  {} {}",
                        tr("Completed"),
                        progress.exported,
                        tr("Failed"),
                        progress.failed,
                        tr("Skipped"),
                        progress.skipped
                    ));
                }
                if let Some(report) = &dialog.report {
                    ui.separator();
                    ui.label(format!(
                        "{} {}  {} {}  {} {}",
                        tr("Completed"),
                        report.exported,
                        tr("Failed"),
                        report.failed,
                        tr("Skipped"),
                        report.skipped
                    ));
                    for failure in &report.failures {
                        ui.label(format!("{}: {}", failure.path.display(), failure.message));
                    }
                    if !report.failures.is_empty() {
                        let failure_text = report
                            .failures
                            .iter()
                            .map(|failure| {
                                format!("{}: {}", failure.path.display(), failure.message)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        ui.horizontal(|ui| {
                            if widgets::button(ui, tr("Copy failures")).clicked() {
                                ui.ctx().copy_text(failure_text.clone());
                            }
                            if widgets::button(ui, tr("Export failures…")).clicked()
                                && let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Text", &["txt"])
                                    .set_file_name("xiaozhu-batch-failures.txt")
                                    .save_file()
                            {
                                dialog.error = fs::write(&path, failure_text)
                                    .err()
                                    .map(|error| format!("{error:#}"));
                            }
                        });
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if let Some(job) = &self.batch_job {
                        ui.spinner();
                        ui.label(tr("Processing…"));
                        if widgets::button(ui, tr("Cancel")).clicked() {
                            job.cancel();
                            cancel = true;
                        }
                    } else if widgets::primary_button(ui, tr("Start")).clicked() {
                        run = true;
                    }
                });
            });
        if run && dialog.ready() {
            let options = BatchOptions {
                input: PathBuf::from(&dialog.input),
                output: PathBuf::from(&dialog.output),
                recursive: dialog.recursive,
                preset: dialog.preset.clone(),
                format: dialog.format.clone(),
                quality: dialog.quality,
                max_edge: (dialog.max_edge > 0).then_some(dialog.max_edge),
                suffix: dialog.suffix.clone(),
                conflict: dialog.conflict,
                cancel: None,
            };
            self.batch_job = Some(BatchJob::spawn(options, context.clone()));
            dialog.report = None;
            dialog.progress = None;
            dialog.error = None;
        }
        if cancel {
            self.status = tr("Batch cancellation requested").into();
        }
        if open {
            self.batch_dialog = Some(dialog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_dialog_requires_input_output_and_preset() {
        let mut dialog = BatchDialogState::default();
        assert!(!dialog.ready());
        dialog.input = "in".into();
        dialog.output = "out".into();
        assert!(!dialog.ready());
        dialog.preset = Some(xuan::portrait::preset::PresetV1::builtin("natural").unwrap());
        assert!(dialog.ready());
    }
}
