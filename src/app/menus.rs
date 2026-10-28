use egui::{Button, RichText};
use xuan::i18n::tr;
use xuan::{
    document::{Adjustment, Point},
    effects::Filter,
};

use super::{EditorApp, theme, widgets};

fn menu_bar_button(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(Button::new(label).fill(egui::Color32::TRANSPARENT));
    let rect = response.rect.expand2(egui::vec2(4.0, 0.0));
    let popup_id = egui::Popup::default_response_id(&response);
    let open_menu_id = ui.id().with("menu_bar_open_popup");
    let open_menu = ui
        .data(|data| data.get_temp::<egui::Id>(open_menu_id))
        .filter(|id| egui::Popup::is_id_open(ui.ctx(), *id));
    if response.hovered() && !response.clicked() && open_menu.is_some_and(|id| id != popup_id) {
        egui::Popup::open_id(ui.ctx(), popup_id);
    }

    let config = egui::containers::menu::MenuConfig::find(ui);
    egui::Popup::menu(&response)
        .anchor(rect)
        .close_behavior(config.close_behavior)
        .style(config.style)
        .show(content);
    let visuals = if egui::Popup::is_id_open(ui.ctx(), popup_id) {
        ui.data_mut(|data| data.insert_temp(open_menu_id, popup_id));
        &ui.visuals().widgets.open
    } else {
        ui.style().interact(&response)
    };

    // The dropdown and the expanded highlight share the same left edge.
    ui.painter().set(
        background,
        egui::epaint::RectShape::filled(rect, visuals.corner_radius, visuals.weak_bg_fill),
    );
}

fn item(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    command: &'static str,
    action: &mut Option<&'static str>,
) {
    if ui.add(Button::new(label).shortcut_text(shortcut)).clicked() {
        *action = Some(command);
        ui.close();
    }
}

pub(super) fn filter_menu(ui: &mut egui::Ui) -> Option<Filter> {
    let mut result = None;
    for filter in [
        Filter::GaussianBlur { radius: 4.0 },
        Filter::MotionBlur {
            distance: 15.0,
            angle: 0.0,
        },
        Filter::Noise {
            amount: 10.0,
            monochrome: true,
        },
        Filter::LensCorrection {
            distortion: 0.0,
            vignette: 0.0,
        },
        Filter::UnsharpMask {
            amount: 80.0,
            radius: 1.5,
            threshold: 2.0,
        },
        Filter::HighPass { radius: 4.0 },
        Filter::Posterize { levels: 6 },
        Filter::Threshold { level: 128.0 },
        Filter::Pixelate { size: 8.0 },
        Filter::Vignette {
            amount: 45.0,
            midpoint: 60.0,
            softness: 35.0,
        },
    ] {
        if ui.button(tr(filter.name())).clicked() {
            result = Some(filter);
            ui.close();
        }
    }
    result
}

pub(super) fn adjustment_menu(ui: &mut egui::Ui) -> Option<Adjustment> {
    let mut result = None;
    for adjustment in [
        Adjustment::HueRanges {
            settings: Box::default(),
        },
        Adjustment::LevelsChannels {
            ranges: [xuan::color::DEFAULT_LEVELS; 4],
        },
        Adjustment::CurvesChannels {
            channels: std::array::from_fn(|_| vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]),
        },
        Adjustment::Exposure {
            exposure: 0.0,
            offset: 0.0,
            gamma: 1.0,
        },
        Adjustment::ColorGrade {
            grade: Default::default(),
        },
        Adjustment::GradientMap {
            shadows: [0, 0, 0, 255],
            highlights: [255; 4],
        },
        Adjustment::FilmGrain {
            amount: 10.0,
            size: 1.0,
            roughness: 50.0,
            seed: 1,
        },
    ] {
        if ui.button(tr(adjustment.name())).clicked() {
            result = Some(adjustment);
            ui.close();
        }
    }
    result
}

impl EditorApp {
    pub(super) fn menus(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let mut adjustment = None;
        let mut filter = None;
        let mut adjustment_layer = false;
        let mut filter_layer = false;
        let developing = self.develop.is_some();
        let has_doc = self.session().is_some() && !developing;
        let can_view = self.develop.as_ref().is_none_or(|d| d.ready());
        let blocked = self.job.is_some()
            || self.dialog.is_some()
            || self.close_app
            || self.develop_close_requested.is_some()
            || self.close_tab.is_some();
        super::chrome::title_bar(ctx, "menubar").show(ctx, |ui| {
            egui::MenuBar::new()
                .config(egui::containers::menu::MenuConfig::new().style(theme::menu_style))
                .ui(ui, |ui| {
                    self.window_controls(ui);
                    ui.add_enabled_ui(!blocked, |ui| {
                        // Leave a small gap between the expanded highlights.
                        ui.spacing_mut().item_spacing.x += 2.0;
                        menu_bar_button(ui, tr("File"), |ui| {
                            item(ui, tr("New Canvas…"), "Ctrl+N", "new", &mut action);
                            item(ui, tr("Open…"), "Ctrl+O", "open", &mut action);
                            item(
                                ui,
                                tr("Open Image from Clipboard"),
                                "",
                                "open_clipboard",
                                &mut action,
                            );
                            item(
                                ui,
                                tr("Open Compositor Package…"),
                                "",
                                "open_comp",
                                &mut action,
                            );
                            ui.add_enabled_ui(!developing, |ui| {
                                item(
                                    ui,
                                    tr("Import Image as Layer…"),
                                    "Ctrl+Shift+O",
                                    "import",
                                    &mut action,
                                );
                            });
                            ui.menu_button(tr("Presets"), |ui| {
                                for (label, command) in [
                                    ("Natural", "preset_natural"),
                                    ("Clear Portrait", "preset_clear-portrait"),
                                    ("Warm", "preset_warm"),
                                    ("Cool", "preset_cool"),
                                    ("Film", "preset_film"),
                                    ("Soft Film", "preset_soft-film"),
                                    ("Mono", "preset_mono"),
                                    ("High Contrast Mono", "preset_high-contrast-mono"),
                                ] {
                                    item(ui, label, "", command, &mut action);
                                }
                                ui.separator();
                                item(ui, tr("Import Preset…"), "", "preset_import", &mut action);
                            });
                            ui.separator();
                            ui.add_enabled_ui(has_doc, |ui| {
                                item(ui, tr("Save"), "Ctrl+S", "save", &mut action);
                                item(ui, tr("Save As…"), "Ctrl+Shift+S", "save_as", &mut action);
                                item(
                                    ui,
                                    tr("Export Image…"),
                                    "Ctrl+Alt+Shift+S",
                                    "export",
                                    &mut action,
                                );
                                ui.separator();
                                item(ui, tr("Close Project"), "Ctrl+W", "close", &mut action);
                            });
                            if developing {
                                item(ui, tr("Close RAW Develop"), "Ctrl+W", "close", &mut action);
                            }
                        });
                        menu_bar_button(ui, tr("Edit"), |ui| {
                            item(ui, tr("Settings…"), "Ctrl+,", "settings", &mut action);
                            item(ui, tr("Portrait Retouch…"), "P", "portrait", &mut action);
                            ui.separator();
                            let (undo, redo) = if let Some(d) = &self.develop {
                                (
                                    (d.ready() && !d.undo.is_empty())
                                        .then_some(tr("RAW adjustment")),
                                    (d.ready() && !d.redo.is_empty())
                                        .then_some(tr("RAW adjustment")),
                                )
                            } else {
                                (
                                    self.session().and_then(|s| s.history.undo_name()),
                                    self.session().and_then(|s| s.history.redo_name()),
                                )
                            };
                            ui.add_enabled_ui(undo.is_some(), |ui| {
                                item(
                                    ui,
                                    &format!("{} {}", tr("Undo"), tr(undo.unwrap_or(""))),
                                    "Ctrl+Z",
                                    "undo",
                                    &mut action,
                                )
                            });
                            ui.add_enabled_ui(redo.is_some(), |ui| {
                                item(
                                    ui,
                                    &format!("{} {}", tr("Redo"), tr(redo.unwrap_or(""))),
                                    "Ctrl+Shift+Z",
                                    "redo",
                                    &mut action,
                                )
                            });
                            ui.separator();
                            ui.add_enabled_ui(has_doc, |ui| {
                                item(ui, tr("Cut"), "Ctrl+X", "cut", &mut action);
                                item(ui, tr("Copy"), "Ctrl+C", "copy", &mut action);
                                item(
                                    ui,
                                    tr("Copy Merged"),
                                    "Ctrl+Shift+C",
                                    "copy_merged",
                                    &mut action,
                                );
                            });
                            ui.add_enabled_ui(!developing, |ui| {
                                item(ui, tr("Paste"), "Ctrl+V", "paste", &mut action);
                            });
                            ui.add_enabled_ui(has_doc, |ui| {
                                ui.separator();
                                item(
                                    ui,
                                    tr("Fill Foreground"),
                                    "Alt+Backspace",
                                    "fill_fg",
                                    &mut action,
                                );
                                item(
                                    ui,
                                    tr("Fill Background"),
                                    "Ctrl+Backspace",
                                    "fill_bg",
                                    &mut action,
                                );
                                item(ui, tr("Clear Pixels"), "Delete", "clear", &mut action);
                                item(
                                    ui,
                                    tr("Content-Aware Fill"),
                                    "Shift+F5",
                                    "content_fill",
                                    &mut action,
                                );
                            });
                        });
                        menu_bar_button(ui, tr("Image"), |ui| {
                            ui.add_enabled_ui(has_doc, |ui| {
                                ui.menu_button(tr("Adjustments"), |ui| {
                                    adjustment = adjustment_menu(ui);
                                    ui.separator();
                                    item(ui, tr("Invert"), "Ctrl+I", "invert", &mut action);
                                });
                                ui.separator();
                                item(ui, tr("Image Size…"), "", "image_size", &mut action);
                                item(ui, tr("Canvas Size…"), "", "canvas_size", &mut action);
                                ui.separator();
                                item(
                                    ui,
                                    tr("Flip Canvas Horizontal"),
                                    "",
                                    "flip_canvas_h",
                                    &mut action,
                                );
                                item(
                                    ui,
                                    tr("Flip Canvas Vertical"),
                                    "",
                                    "flip_canvas_v",
                                    &mut action,
                                );
                            });
                        });
                        menu_bar_button(ui, tr("Layer"), |ui| {
                            ui.add_enabled_ui(has_doc, |ui| {
                                item(
                                    ui,
                                    tr("New Layer"),
                                    "Ctrl+Shift+N",
                                    "new_layer",
                                    &mut action,
                                );
                                item(
                                    ui,
                                    tr("Duplicate Layers"),
                                    "Ctrl+J",
                                    "duplicate",
                                    &mut action,
                                );
                                item(ui, tr("Delete Layers"), "", "delete_layer", &mut action);
                                ui.add_enabled_ui(
                                    self.session()
                                        .and_then(|s| s.document.active())
                                        .is_some_and(|l| l.raw.is_some() && !l.locked),
                                    |ui| {
                                        item(ui, tr("Develop RAW…"), "", "develop", &mut action);
                                        item(
                                            ui,
                                            tr("Rasterize RAW Layer"),
                                            "",
                                            "rasterize_raw",
                                            &mut action,
                                        );
                                    },
                                );
                                ui.separator();
                                item(ui, tr("Group Layers"), "Ctrl+G", "group", &mut action);
                                item(
                                    ui,
                                    tr("Ungroup Layers"),
                                    "Ctrl+Shift+G",
                                    "ungroup",
                                    &mut action,
                                );
                                item(
                                    ui,
                                    tr("Merge Down / Selected"),
                                    "Ctrl+E",
                                    "merge",
                                    &mut action,
                                );
                                item(ui, tr("Flatten Image"), "", "flatten", &mut action);
                                ui.separator();
                                ui.menu_button(tr("New Adjustment Layer"), |ui| {
                                    adjustment = adjustment_menu(ui);
                                    adjustment_layer = true;
                                });
                                ui.menu_button(tr("New Filter Layer"), |ui| {
                                    filter = filter_menu(ui);
                                    filter_layer = true;
                                });
                                ui.menu_button(tr("Layer Mask"), |ui| {
                                    item(
                                        ui,
                                        tr("New Mask Layer"),
                                        "",
                                        "new_mask_layer",
                                        &mut action,
                                    );
                                    item(
                                        ui,
                                        tr("Add Mask from Selection"),
                                        "",
                                        "mask",
                                        &mut action,
                                    );
                                    item(
                                        ui,
                                        tr("Enable / Disable"),
                                        "",
                                        "disable_mask",
                                        &mut action,
                                    );
                                    item(ui, tr("Link / Unlink"), "", "link_mask", &mut action);
                                    item(ui, tr("Delete Mask"), "", "delete_mask", &mut action);
                                });
                                item(
                                    ui,
                                    tr("Create / Release Clipping Mask"),
                                    "Ctrl+Alt+G",
                                    "clip",
                                    &mut action,
                                );
                                ui.separator();
                                item(ui, tr("Flip Horizontal"), "", "flip_h", &mut action);
                                item(ui, tr("Flip Vertical"), "", "flip_v", &mut action);
                            });
                        });
                        menu_bar_button(ui, tr("Select"), |ui| {
                            ui.add_enabled_ui(has_doc, |ui| {
                                item(ui, tr("All"), "Ctrl+A", "select_all", &mut action);
                                item(ui, tr("Deselect"), "Ctrl+D", "deselect", &mut action);
                                item(
                                    ui,
                                    tr("Inverse"),
                                    "Ctrl+Shift+I",
                                    "invert_selection",
                                    &mut action,
                                );
                                item(
                                    ui,
                                    tr("Load Layer / Mask"),
                                    "",
                                    "load_selection",
                                    &mut action,
                                );
                                item(ui, tr("Feather 3 px"), "", "feather", &mut action);
                            });
                        });
                        menu_bar_button(ui, tr("Filter"), |ui| {
                            ui.add_enabled_ui(has_doc, |ui| {
                                item(
                                    ui,
                                    tr("Remove Background (edge colors)"),
                                    "",
                                    "remove_background",
                                    &mut action,
                                );
                                ui.separator();
                                for f in [
                                    Filter::GaussianBlur { radius: 4.0 },
                                    Filter::MotionBlur {
                                        distance: 15.0,
                                        angle: 0.0,
                                    },
                                    Filter::Noise {
                                        amount: 10.0,
                                        monochrome: true,
                                    },
                                    Filter::LensCorrection {
                                        distortion: 0.0,
                                        vignette: 0.0,
                                    },
                                    Filter::UnsharpMask {
                                        amount: 80.0,
                                        radius: 1.5,
                                        threshold: 2.0,
                                    },
                                    Filter::HighPass { radius: 4.0 },
                                    Filter::Posterize { levels: 6 },
                                    Filter::Threshold { level: 128.0 },
                                    Filter::Pixelate { size: 8.0 },
                                    Filter::Vignette {
                                        amount: 45.0,
                                        midpoint: 60.0,
                                        softness: 35.0,
                                    },
                                ] {
                                    if ui.button(format!("{}…", tr(f.name()))).clicked() {
                                        filter = Some(f);
                                        ui.close();
                                    }
                                }
                            });
                        });
                        menu_bar_button(ui, tr("View"), |ui| {
                            ui.add_enabled_ui(can_view, |ui| {
                                item(ui, tr("Fit Canvas"), "Ctrl+0", "fit", &mut action);
                                item(ui, tr("Actual Pixels"), "Ctrl+1", "actual", &mut action);
                                item(ui, tr("Zoom In"), "Ctrl++", "zoom_in", &mut action);
                                item(ui, tr("Zoom Out"), "Ctrl+−", "zoom_out", &mut action);
                            });
                            ui.separator();
                            ui.add_enabled_ui(!developing, |ui| {
                                widgets::checkbox(
                                    ui,
                                    &mut self.show_controls,
                                    tr("Show Transform Controls"),
                                );
                                widgets::checkbox(
                                    ui,
                                    &mut self.snap,
                                    tr("Snap to Canvas and Layers"),
                                );
                            });
                        });
                        menu_bar_button(ui, tr("Help"), |ui| {
                            item(ui, tr("Keyboard Shortcuts"), "F1", "shortcuts", &mut action);
                            item(ui, tr("About 小猪修修"), "", "about", &mut action);
                        });
                    });
                    self.titlebar_drag(ui);
                });
        });
        if let Some(action) = action {
            self.command(action);
        }
        if let Some(adjustment) = adjustment {
            self.start_adjustment(adjustment, adjustment_layer);
        }
        if let Some(filter) = filter {
            if filter_layer {
                self.start_filter_layer(filter);
            } else {
                self.start_filter(filter);
            }
        }
    }

    pub(super) fn tabs(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let mut switch = None;
        let mut close_project = None;
        let mut raw_switch = None;
        let mut raw_close = None;
        let mut copy = None;
        let developing = self.develop.is_some();
        let blocked = self.job.is_some()
            || self.dialog.is_some()
            || self.error.is_some()
            || self.close_app
            || self.close_tab.is_some()
            || self.develop_close_requested.is_some();
        egui::TopBottomPanel::top("project_tabs")
            .exact_height(46.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::TITLEBAR)
                    .inner_margin(egui::Margin::symmetric(14, 9)),
            )
            .show(ctx, |ui| {
                ui.add_enabled_ui(!blocked, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add(widgets::Button::new("+").min_size(egui::vec2(28.0, 28.0)))
                            .on_hover_text(tr("New canvas (Ctrl+N)"))
                            .clicked()
                        {
                            action = Some("new");
                        }
                        ui.separator();
                        let available = (ui.available_width() - 228.0).max(150.0);
                        ui.allocate_ui(egui::vec2(available, 28.0), |ui| {
                            egui::ScrollArea::horizontal()
                                .id_salt("tabs_scroll")
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .show(ui, |ui| {
                                    ui.horizontal_centered(|ui| {
                                        if self.sessions.is_empty()
                                            && !developing
                                            && self.inactive_develop.is_empty()
                                        {
                                            ui.label(
                                                RichText::new(tr("Untitled")).color(theme::MUTED),
                                            );
                                        }
                                        for (index, session) in self.sessions.iter().enumerate() {
                                            let (response, close) = widgets::project_tab(
                                                ui,
                                                &session.title,
                                                session.history.dirty(),
                                                !developing && self.current == index,
                                            );
                                            if !developing {
                                                if let Some(layer) = response
                                                    .dnd_release_payload::<super::LayerDrag>()
                                                {
                                                    copy = Some((*layer, index));
                                                }
                                                if response
                                                    .dnd_hover_payload::<super::LayerDrag>()
                                                    .is_some()
                                                {
                                                    ui.painter().rect_stroke(
                                                        response.rect,
                                                        theme::BUTTON_RADIUS,
                                                        egui::Stroke::new(2.0_f32, theme::ACCENT),
                                                        egui::StrokeKind::Inside,
                                                    );
                                                }
                                            }
                                            if response.clicked() {
                                                switch = Some(index);
                                            }
                                            if close
                                                || response.clicked_by(egui::PointerButton::Middle)
                                            {
                                                close_project = Some(index);
                                            }
                                        }
                                        let mut raw_tabs: Vec<_> = self
                                            .develop
                                            .iter()
                                            .chain(&self.inactive_develop)
                                            .collect();
                                        raw_tabs.sort_by_key(|d| d.opened);
                                        for develop in raw_tabs {
                                            let selected = self
                                                .develop
                                                .as_ref()
                                                .is_some_and(|d| d.id == develop.id);
                                            let dirty =
                                                develop.asset.as_ref().is_some_and(|asset| {
                                                    asset.settings != develop.settings
                                                });
                                            let (response, close) = ui
                                                .push_id(develop.id, |ui| {
                                                    widgets::project_tab(
                                                        ui,
                                                        &format!("{} · RAW", develop.title),
                                                        dirty,
                                                        selected,
                                                    )
                                                })
                                                .inner;
                                            if response.clicked() {
                                                raw_switch = Some(develop.id);
                                            }
                                            if close
                                                || response.clicked_by(egui::PointerButton::Middle)
                                            {
                                                raw_close = Some(develop.id);
                                            }
                                        }
                                    });
                                });
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_enabled_ui(
                                self.develop.as_ref().is_none_or(|d| d.ready()),
                                |ui| {
                                    if widgets::button(ui, "−")
                                        .on_hover_text(tr("Zoom out"))
                                        .clicked()
                                    {
                                        action = Some("zoom_out");
                                    }
                                    if widgets::button(ui, "+")
                                        .on_hover_text(tr("Zoom in"))
                                        .clicked()
                                    {
                                        action = Some("zoom_in");
                                    }
                                    if widgets::button(ui, "100%").clicked() {
                                        action = Some("actual");
                                    }
                                    if widgets::button(ui, tr("Fit")).clicked() {
                                        action = Some("fit");
                                    }
                                },
                            );
                        });
                    });
                });
            });
        if let Some((layer, destination)) = copy {
            self.copy_layer_to_project(layer, destination);
        }
        if let Some(index) = switch {
            self.cancel_gesture();
            self.suspend_develop();
            self.current = index;
            self.mask_target = false;
        }
        if let Some(id) = raw_switch.or(raw_close) {
            self.activate_develop(id);
        }
        if raw_close.is_some() {
            self.request_develop_close(super::develop::DevelopClose::Tab);
        }
        if let Some(index) = close_project {
            self.request_project_close(index);
        }
        if let Some(action) = action {
            self.command(action);
        }
    }
}
