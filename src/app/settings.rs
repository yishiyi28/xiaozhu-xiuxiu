use super::{EditorApp, theme, widgets};
use xuan::{
    config::{Config, Language},
    i18n::{self, tr},
};

impl EditorApp {
    pub(super) fn load_config(&mut self) {
        match Config::path().and_then(|path| Config::load(&path)) {
            Ok(config) => self.config = config,
            Err(error) => {
                self.error = Some(format!("{}\n\n{error:#}", tr("Could not load settings")))
            }
        }
        i18n::set_language(self.config.language);
    }

    pub(super) fn settings_dialog(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut done = false;
        let mut language = self.config.language;
        let mut autosave_seconds = self.config.autosave_seconds;
        widgets::Window::new(tr("Settings"))
            .id("app_settings")
            .default_width(580.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.set_height(240.0);
                    ui.vertical(|ui| {
                        ui.set_width(125.0);
                        ui.set_min_height(240.0);
                        ui.selectable_label(true, tr("General"))
                            .on_hover_text(tr("General preferences"));
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_min_width(365.0);
                        ui.heading(tr("General"));
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            ui.label(tr("Language"));
                            widgets::PopUp::from_id_salt("settings_language")
                                .selected_text(language.name())
                                .width(180.0)
                                .show_ui(ui, |ui| {
                                    for option in [Language::English, Language::SimplifiedChinese] {
                                        widgets::menu_choice(
                                            ui,
                                            &mut language,
                                            option,
                                            option.name(),
                                        );
                                    }
                                });
                        });
                        ui.add_space(8.0);
                        ui.label(tr("Language changes apply immediately."));
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            ui.label(tr("Autosave interval"));
                            ui.add(egui::Slider::new(&mut autosave_seconds, 0..=600).suffix(" s"));
                        });
                        ui.label(tr(
                            "Crash recovery snapshots unsaved projects. Set to 0 to disable.",
                        ));
                        ui.add_space(28.0);
                        ui.label(egui::RichText::new(tr("Configuration file")).color(theme::MUTED));
                        if let Ok(path) = Config::path() {
                            ui.add(egui::Label::new(path.display().to_string()).wrap());
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary_button(ui, tr("Done")).clicked() {
                            done = true;
                        }
                    });
                });
            });
        if language != self.config.language || autosave_seconds != self.config.autosave_seconds {
            let config = Config {
                language,
                autosave_seconds,
            };
            match Config::path().and_then(|path| config.save(&path)) {
                Ok(()) => {
                    self.config = config;
                    i18n::set_language(language);
                    ctx.request_repaint();
                }
                Err(error) => {
                    self.error = Some(format!("{}\n\n{error:#}", tr("Could not save settings")))
                }
            }
        }
        if !open || done || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dialog = None;
        }
    }
}
