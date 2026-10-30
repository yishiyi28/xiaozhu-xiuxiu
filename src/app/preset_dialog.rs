use xuan::{
    i18n::tr,
    portrait::preset::{PresetImportPolicy, PresetPreferences, PresetStore, PresetV1},
};

use super::{EditorApp, widgets};

#[derive(Clone, Debug)]
pub struct PresetDialogState {
    pub user_presets: Vec<PresetV1>,
    pub selected: Option<PresetV1>,
    pub preferences: PresetPreferences,
    pub import_policy: PresetImportPolicy,
    pub rename: Option<String>,
    pub confirm_delete: bool,
    pub error: Option<String>,
}

impl Default for PresetDialogState {
    fn default() -> Self {
        Self {
            user_presets: PresetStore::user_presets().unwrap_or_default(),
            selected: None,
            preferences: PresetStore::preferences().unwrap_or_default(),
            import_policy: PresetImportPolicy::Rename,
            rename: None,
            confirm_delete: false,
            error: None,
        }
    }
}

impl PresetDialogState {
    #[cfg(test)]
    pub fn select_builtin(&mut self, slug: &str) {
        match PresetV1::builtin(slug) {
            Ok(preset) => self.select(preset),
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    pub fn select(&mut self, preset: PresetV1) {
        self.preferences.touch_recent(&preset.name);
        self.selected = Some(preset);
        self.rename = None;
        self.confirm_delete = false;
        self.error = None;
    }

    fn preset_named(&self, name: &str) -> Option<PresetV1> {
        self.user_presets
            .iter()
            .find(|preset| preset.name == name)
            .cloned()
            .or_else(|| {
                PresetV1::builtins()
                    .ok()
                    .and_then(|presets| presets.into_iter().find(|preset| preset.name == name))
            })
    }

    fn selected_is_user(&self) -> bool {
        self.selected.as_ref().is_some_and(|selected| {
            self.user_presets
                .iter()
                .any(|preset| preset.name == selected.name)
        })
    }

    #[cfg(test)]
    pub fn import_text(&mut self, text: &str) {
        match serde_json::from_str::<PresetV1>(text) {
            Ok(preset) => {
                self.selected = Some(preset);
                self.error = None;
            }
            Err(error) => self.error = Some(format!("{}: {error}", tr("Invalid preset"))),
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
        let mut preferences_changed = false;
        let mut rename_apply = false;
        let mut delete = false;
        widgets::Window::new(tr("Presets"))
            .default_width(360.0)
            .open(&mut open)
            .show(context, |ui| {
                let recent = dialog.preferences.recent.clone();
                if !recent.is_empty() {
                    ui.label(tr("Recent presets"));
                    ui.horizontal_wrapped(|ui| {
                        for name in &recent {
                            if let Some(preset) = dialog.preset_named(name) {
                                let selected = dialog.selected_name() == preset.name;
                                if preset_chip(ui, selected, &preset, &preset.name) {
                                    dialog.select(preset);
                                    preferences_changed = true;
                                }
                            }
                        }
                    });
                    ui.separator();
                }
                let favorites = dialog.preferences.favorites.clone();
                if !favorites.is_empty() {
                    ui.label(tr("Favorite presets"));
                    ui.horizontal_wrapped(|ui| {
                        for name in &favorites {
                            if let Some(preset) = dialog.preset_named(name) {
                                let selected = dialog.selected_name() == preset.name;
                                if preset_chip(ui, selected, &preset, &preset.name) {
                                    dialog.select(preset);
                                    preferences_changed = true;
                                }
                            }
                        }
                    });
                    ui.separator();
                }
                ui.label(tr("Built-in presets"));
                ui.horizontal_wrapped(|ui| {
                    for (slug, label) in [
                        ("natural", tr("Natural")),
                        ("clear-portrait", tr("Clear Portrait")),
                        ("warm", tr("Warm")),
                        ("cool", tr("Cool")),
                        ("film", tr("Film")),
                        ("soft-film", tr("Soft Film")),
                        ("mono", tr("Mono")),
                        ("high-contrast-mono", tr("High-contrast Mono")),
                    ] {
                        if let Ok(preset) = PresetV1::builtin(slug) {
                            let selected = dialog.selected_name() == preset.name;
                            if preset_chip(ui, selected, &preset, label) {
                                dialog.select(preset);
                                preferences_changed = true;
                            }
                        }
                    }
                });
                if !dialog.user_presets.is_empty() {
                    ui.separator();
                    ui.label(tr("User presets"));
                    let user_presets = dialog.user_presets.clone();
                    ui.horizontal_wrapped(|ui| {
                        for preset in &user_presets {
                            let selected = dialog.selected_name() == preset.name;
                            if preset_chip(ui, selected, preset, &preset.name) {
                                dialog.select(preset.clone());
                                preferences_changed = true;
                            }
                        }
                    });
                }
                if let Some(selected) = dialog.selected.clone() {
                    ui.separator();
                    let favorite = dialog.preferences.is_favorite(&selected.name);
                    if widgets::button(
                        ui,
                        if favorite {
                            tr("Remove favorite")
                        } else {
                            tr("Add favorite")
                        },
                    )
                    .clicked()
                    {
                        dialog.preferences.toggle_favorite(&selected.name);
                        preferences_changed = true;
                    }
                    if dialog.selected_is_user() {
                        if widgets::button(ui, tr("Rename preset…")).clicked() {
                            dialog.rename = Some(selected.name.clone());
                        }
                        if dialog.confirm_delete {
                            ui.label(tr("Delete this preset?"));
                            if widgets::button(ui, tr("Delete")).clicked() {
                                delete = true;
                            }
                            if widgets::button(ui, tr("Cancel")).clicked() {
                                dialog.confirm_delete = false;
                            }
                        } else if widgets::button(ui, tr("Delete preset")).clicked() {
                            dialog.confirm_delete = true;
                        }
                    }
                    let mut cancel_rename = false;
                    if let Some(name) = &mut dialog.rename {
                        ui.horizontal(|ui| {
                            ui.label(tr("Name"));
                            ui.text_edit_singleline(name);
                            if widgets::button(ui, tr("Rename")).clicked() {
                                rename_apply = true;
                            }
                            if widgets::button(ui, tr("Cancel")).clicked() {
                                cancel_rename = true;
                            }
                        });
                    }
                    if cancel_rename {
                        dialog.rename = None;
                    }
                }
                if let Some(error) = &dialog.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 100), error);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(tr("If preset exists"));
                    for (policy, label) in [
                        (PresetImportPolicy::Skip, tr("Skip")),
                        (PresetImportPolicy::Overwrite, tr("Overwrite")),
                        (PresetImportPolicy::Rename, tr("Auto-number")),
                    ] {
                        if ui
                            .selectable_label(dialog.import_policy == policy, label)
                            .clicked()
                        {
                            dialog.import_policy = policy;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if widgets::button(ui, tr("Import…")).clicked() {
                        import = true;
                    }
                    if widgets::button(ui, tr("Export…")).clicked() {
                        export = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary_button(ui, tr("Apply")).clicked() {
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
            match PresetStore::import_with_policy(&path, dialog.import_policy) {
                Ok(Some(preset)) => {
                    dialog.user_presets = PresetStore::user_presets().unwrap_or_default();
                    dialog.select(preset);
                    preferences_changed = true;
                }
                Ok(None) => dialog.error = Some(tr("Preset import skipped").into()),
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
        if rename_apply
            && let Some(preset) = dialog.selected.clone()
            && let Some(name) = dialog.rename.clone()
        {
            match PresetStore::rename(&preset, &name) {
                Ok(renamed) => {
                    dialog.user_presets = PresetStore::user_presets().unwrap_or_default();
                    dialog.select(renamed);
                    preferences_changed = true;
                }
                Err(error) => dialog.error = Some(format!("{error:#}")),
            }
        }
        if delete && let Some(preset) = dialog.selected.clone() {
            match PresetStore::delete(&preset) {
                Ok(()) => {
                    dialog
                        .preferences
                        .recent
                        .retain(|name| name != &preset.name);
                    dialog
                        .preferences
                        .favorites
                        .retain(|name| name != &preset.name);
                    dialog.user_presets = PresetStore::user_presets().unwrap_or_default();
                    dialog.selected = None;
                    dialog.confirm_delete = false;
                    preferences_changed = true;
                }
                Err(error) => dialog.error = Some(format!("{error:#}")),
            }
        }
        if preferences_changed
            && let Err(error) = PresetStore::save_preferences(&dialog.preferences)
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

pub(super) fn preset_chip(
    ui: &mut egui::Ui,
    selected: bool,
    preset: &PresetV1,
    label: &str,
) -> bool {
    let response = ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
        let sample = |rgb: [u8; 3]| {
            preset
                .color
                .as_ref()
                .map_or(rgb, |grade| grade.apply_rgb(rgb))
        };
        let base = sample([196, 154, 130]);
        let light = sample([238, 222, 204]);
        let dark = sample([68, 88, 108]);
        ui.painter().rect_filled(
            rect,
            3.0,
            egui::Color32::from_rgb(base[0], base[1], base[2]),
        );
        ui.painter().circle_filled(
            rect.center() + egui::vec2(0.0, -3.0),
            5.0,
            egui::Color32::from_rgb(light[0], light[1], light[2]),
        );
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - 7.0),
                egui::pos2(rect.right(), rect.bottom()),
            ),
            0.0,
            egui::Color32::from_rgb(dark[0], dark[1], dark[2]),
        );
        ui.selectable_label(selected, label).clicked()
    });
    response.inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_presets_updates_recent_preferences() {
        let mut dialog = PresetDialogState::default();
        dialog.select_builtin("natural");
        dialog.select_builtin("warm");
        assert_eq!(dialog.preferences.recent[0], "暖阳");
        assert!(dialog.preferences.recent.contains(&"自然通透".to_owned()));
    }

    #[test]
    fn invalid_preset_import_keeps_previous_selection() {
        let mut dialog = PresetDialogState::default();
        dialog.select_builtin("natural");
        dialog.import_text("{broken");
        assert_eq!(dialog.selected_name(), "自然通透");
        assert!(dialog.error.is_some());
    }
}
