use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
};

use xuan::{
    batch::{BatchOptions, BatchReport, run_batch},
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
    pub overwrite: bool,
    pub error: Option<String>,
    pub report: Option<BatchReport>,
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
            overwrite: false,
            error: None,
            report: None,
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
    cancel: Arc<AtomicBool>,
}

impl BatchJob {
    pub fn spawn(options: BatchOptions, context: egui::Context) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut options = options;
        options.cancel = Some(cancel.clone());
        let (send, receive) = mpsc::channel();
        xuan::gpu::spawn(move || {
            let result = run_batch(options).map_err(|error| format!("{error:#}"));
            let _ = send.send(result);
            context.request_repaint();
        });
        Self { receive, cancel }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn poll(&self) -> Option<Result<BatchReport, String>> {
        match self.receive.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("批量线程意外停止".into())),
        }
    }
}

impl EditorApp {
    pub(super) fn batch_dialog(&mut self, context: &egui::Context) {
        if let Some(job) = &self.batch_job
            && let Some(result) = job.poll()
        {
            self.batch_job = None;
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
        widgets::Window::new("批量出片")
            .default_width(460.0)
            .open(&mut open)
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    ui.label("输入目录");
                    ui.text_edit_singleline(&mut dialog.input);
                    if widgets::button(ui, "选择…").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        dialog.input = path.display().to_string();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("输出目录");
                    ui.text_edit_singleline(&mut dialog.output);
                    if widgets::button(ui, "选择…").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        dialog.output = path.display().to_string();
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut dialog.recursive, "包含子目录");
                    ui.checkbox(&mut dialog.overwrite, "覆盖同名文件");
                });
                ui.separator();
                ui.label("预设");
                ui.horizontal_wrapped(|ui| {
                    for (slug, name) in [
                        ("natural", "自然通透"),
                        ("clear-portrait", "清透人像"),
                        ("warm", "暖阳"),
                        ("cool", "冷调"),
                        ("film", "胶片"),
                        ("soft-film", "柔雾胶片"),
                        ("mono", "黑白"),
                        ("high-contrast-mono", "高反差黑白"),
                    ] {
                        let selected = dialog
                            .preset
                            .as_ref()
                            .is_some_and(|preset| preset.name == name);
                        if ui.selectable_label(selected, name).clicked() {
                            dialog.preset = PresetV1::builtin(slug).ok();
                        }
                    }
                });
                if !dialog.user_presets.is_empty() {
                    ui.label("用户预设");
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
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("格式");
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
                        .text("质量")
                        .suffix("%"),
                );
                ui.add(
                    widgets::Slider::new(&mut dialog.max_edge, 0..=30_000)
                        .text("最长边")
                        .suffix(" px (0 = 原尺寸)"),
                );
                ui.horizontal(|ui| {
                    ui.label("文件名后缀");
                    ui.text_edit_singleline(&mut dialog.suffix);
                });
                if !dialog.ready() {
                    ui.label("请选择输入目录、输出目录和预设。");
                }
                if let Some(error) = &dialog.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 100), error);
                }
                if let Some(report) = &dialog.report {
                    ui.separator();
                    ui.label(format!(
                        "成功 {}，失败 {}，跳过 {}",
                        report.exported, report.failed, report.skipped
                    ));
                    for failure in &report.failures {
                        ui.label(format!("{}: {}", failure.path.display(), failure.message));
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if let Some(job) = &self.batch_job {
                        ui.spinner();
                        ui.label("正在处理…");
                        if widgets::button(ui, "取消").clicked() {
                            job.cancel();
                            cancel = true;
                        }
                    } else if widgets::primary_button(ui, "开始").clicked() {
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
                overwrite: dialog.overwrite,
                cancel: None,
            };
            self.batch_job = Some(BatchJob::spawn(options, context.clone()));
            dialog.report = None;
            dialog.error = None;
        }
        if cancel {
            self.status = "已请求取消批量任务".into();
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
