use xuan::portrait::preset::{PresetStore, PresetV1};

use super::{EditorApp, widgets};

#[derive(Clone, Debug)]
pub struct PresetDialogState {
    pub user_presets: Vec<PresetV1>,
    pub selected: Option<PresetV1>,
    pub error: Option<String>,
}

impl Default for PresetDialogState {
    fn default() -> Self {
        Self {
            user_presets: PresetStore::user_presets().unwrap_or_default(),
            selected: None,
            error: None,
        }
    }
}

impl PresetDialogState {
    pub fn select_builtin(&mut self, slug: &str) {
        match PresetV1::builtin(slug) {
            Ok(preset) => {
                self.selected = Some(preset);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    #[cfg(test)]
    pub fn import_text(&mut self, text: &str) {
        match serde_json::from_str::<PresetV1>(text) {
            Ok(preset) => {
                self.selected = Some(preset);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("预设格式错误: {error}")),
        }
    }

    pub fn selected_name(&self) -> &str {
        self.selected
            .as_ref()
            .map_or("", |preset| preset.name.as_str())
    }
}

impl EditorApp {
    pub(super) fn preset_dialog(&mut self, context: &egui::Context) {
        let Some(mut dialog) = self.preset_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut import = false;
        let mut export = false;
        widgets::Window::new("预设")
            .default_width(360.0)
            .open(&mut open)
            .show(context, |ui| {
                ui.label("内置预设");
                ui.horizontal_wrapped(|ui| {
                    for (slug, preset) in [
                        ("natural", "自然通透"),
                        ("clear-portrait", "清透人像"),
                        ("warm", "暖阳"),
                        ("cool", "冷调"),
                        ("film", "胶片"),
                        ("soft-film", "柔雾胶片"),
                        ("mono", "黑白"),
                        ("high-contrast-mono", "高反差黑白"),
                    ] {
                        if ui
                            .selectable_label(dialog.selected_name() == preset, preset)
                            .clicked()
                        {
                            dialog.select_builtin(slug);
                        }
                    }
                });
                if !dialog.user_presets.is_empty() {
                    ui.separator();
                    ui.label("用户预设");
                    ui.horizontal_wrapped(|ui| {
                        for preset in &dialog.user_presets {
                            if ui
                                .selectable_label(
                                    dialog.selected_name() == preset.name,
                                    &preset.name,
                                )
                                .clicked()
                            {
                                dialog.selected = Some(preset.clone());
                                dialog.error = None;
                            }
                        }
                    });
                }
                if let Some(error) = &dialog.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 100), error);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if widgets::button(ui, "导入…").clicked() {
                        import = true;
                    }
                    if widgets::button(ui, "导出…").clicked() {
                        export = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary_button(ui, "应用").clicked() {
                            apply = true;
                        }
                    });
                });
            });
        if import
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("小猪修修 preset", &["xzp"])
                .pick_file()
        {
            match PresetStore::import(&path) {
                Ok(preset) => {
                    dialog.selected = Some(preset);
                    dialog.user_presets = PresetStore::user_presets().unwrap_or_default();
                    dialog.error = None;
                }
                Err(error) => dialog.error = Some(format!("{error:#}")),
            }
        }
        if export
            && let Some(preset) = &dialog.selected
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("小猪修修 preset", &["xzp"])
                .set_file_name(format!("{}.xzp", preset.name))
                .save_file()
            && let Err(error) = PresetStore::save(preset, &path)
        {
            dialog.error = Some(format!("{error:#}"));
        }
        if apply && let Some(preset) = dialog.selected.clone() {
            self.apply_preset(preset);
            self.preset_dialog = None;
            return;
        }
        if open {
            self.preset_dialog = Some(dialog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_preset_import_keeps_previous_selection() {
        let mut dialog = PresetDialogState::default();
        dialog.select_builtin("natural");
        dialog.import_text("{broken");
        assert_eq!(dialog.selected_name(), "自然通透");
        assert!(dialog.error.is_some());
    }
}
