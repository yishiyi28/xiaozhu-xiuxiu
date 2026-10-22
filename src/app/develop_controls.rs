use std::{
    fs::File,
    io::{Read, Write},
    ops::RangeInclusive,
};
use xuan::i18n::tr;

use egui::{Color32, Sense, Stroke, pos2, vec2};
use xuan::raw::{self, DevelopSettings, Overlay, OverlayKind, WhiteBalance};

use super::{develop::Develop, theme, widgets};

fn slider(ui: &mut egui::Ui, label: &str, value: &mut f32, range: RangeInclusive<f32>, unit: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [105.0, 22.0],
            egui::Label::new(label).halign(egui::Align::Min),
        );
        let field_width = widgets::NUMBER_WIDTH + widgets::SLIDER_SPACING;
        ui.spacing_mut().slider_width = (ui.available_width() - field_width).max(40.0);
        ui.add(
            widgets::Slider::new(value, range)
                .clamp_existing_to_range(false)
                .suffix(unit)
                .max_decimals(2),
        );
    });
}

fn percent(ui: &mut egui::Ui, label: &str, value: &mut f32) {
    slider(ui, label, value, -100.0..=100.0, "%");
}

pub(super) fn controls(ui: &mut egui::Ui, d: &mut Develop) {
    if !d.settings.negative.enabled {
        d.film_base_picker = false;
    }
    histogram(ui, d);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!d.undo.is_empty(), widgets::Button::new(tr("Undo")))
            .clicked()
        {
            d.undo(false);
        }
        if ui
            .add_enabled(!d.redo.is_empty(), widgets::Button::new(tr("Redo")))
            .clicked()
        {
            d.undo(true);
        }
        if widgets::button(ui, tr("Reset"))
            .on_hover_text(tr("Reset all Develop adjustments to defaults"))
            .clicked()
        {
            d.settings = DevelopSettings::default();
            d.picker = false;
            d.film_base_picker = false;
            d.selected_overlay = None;
        }
        widgets::PopUp::from_id_salt("raw_presets")
            .selected_text(tr("Presets"))
            .width(90.0)
            .show_ui(ui, |ui| {
                ui.set_min_width(150.0);
                for (name, preset) in [
                    (tr("Natural"), DevelopSettings::default()),
                    (
                        tr("Landscape"),
                        DevelopSettings {
                            contrast: 12.0,
                            highlights: -30.0,
                            shadows: 20.0,
                            vibrance: 20.0,
                            clarity: 12.0,
                            ..Default::default()
                        },
                    ),
                    (
                        tr("Black & white"),
                        DevelopSettings {
                            monochrome: true,
                            contrast: 18.0,
                            ..Default::default()
                        },
                    ),
                ] {
                    if ui.button(name).clicked() {
                        d.settings = preset;
                        d.selected_overlay = None;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(tr("Save settings…")).clicked() {
                    save_preset(d);
                    ui.close();
                }
                if ui.button(tr("Load settings…")).clicked() {
                    load_preset(d);
                    ui.close();
                }
            });
    });
    ui.add_space(8.0);
    widgets::segmented(
        ui,
        &mut d.panel,
        &[
            (0, tr("Basic")),
            (6, tr("Negative")),
            (1, tr("Tone")),
            (2, tr("Detail")),
            (3, tr("Lens")),
            (4, tr("Masks")),
            (5, tr("Info")),
        ],
    );
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("raw_settings")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            match d.panel {
                0 => basic(ui, d),
                1 => tones(ui, d),
                2 => detail(ui, d),
                3 => lens(ui, d),
                4 => masks(ui, d),
                6 => negative(ui, d),
                _ => metadata(ui, d),
            }
            ui.add_space(16.0);
        });
}

fn heading(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.strong(title);
    ui.add_space(6.0);
}

fn basic(ui: &mut egui::Ui, d: &mut Develop) {
    ui.add_enabled_ui(!d.settings.negative.enabled, |ui| white_balance(ui, d));
    if d.settings.negative.enabled {
        ui.small(tr(
            "Use film color balance in the Negative tab instead of camera white balance.",
        ));
    }
    heading(ui, tr("Light"));

    if ui
        .add_enabled(
            !d.settings.negative.enabled,
            widgets::Button::new(tr("Auto exposure")),
        )
        .clicked()
        && let Some(raw) = &d.proxy
    {
        d.settings.exposure = raw::auto_exposure(raw);
    }
    slider(
        ui,
        tr("Exposure"),
        &mut d.settings.exposure,
        -10.0..=10.0,
        " EV",
    );
    percent(ui, tr("Brightness"), &mut d.settings.brightness);
    percent(ui, tr("Contrast"), &mut d.settings.contrast);
    percent(ui, tr("Highlights"), &mut d.settings.highlights);
    percent(ui, tr("Shadows"), &mut d.settings.shadows);
    percent(ui, tr("Whites"), &mut d.settings.whites);
    percent(ui, tr("Blacks"), &mut d.settings.blacks);
    heading(ui, tr("Presence"));
    percent(ui, tr("Clarity"), &mut d.settings.clarity);
    percent(ui, tr("Texture"), &mut d.settings.texture);
    percent(ui, tr("Dehaze"), &mut d.settings.dehaze);
    percent(ui, tr("Vibrance"), &mut d.settings.vibrance);
    percent(ui, tr("Saturation"), &mut d.settings.saturation);
}

fn white_balance(ui: &mut egui::Ui, d: &mut Develop) {
    heading(ui, tr("White balance"));
    ui.horizontal(|ui| {
        widgets::PopUp::from_id_salt("raw_wb")
            .selected_text(match d.settings.white_balance {
                WhiteBalance::AsShot => tr("As shot"),
                WhiteBalance::Temperature => tr("Temperature"),
                WhiteBalance::Custom => tr("Sampled neutral"),
            })
            .show_ui(ui, |ui| {
                widgets::menu_choice(
                    ui,
                    &mut d.settings.white_balance,
                    WhiteBalance::AsShot,
                    tr("As shot"),
                );
                widgets::menu_choice(
                    ui,
                    &mut d.settings.white_balance,
                    WhiteBalance::Temperature,
                    tr("Temperature"),
                );
                for (name, kelvin) in [
                    (tr("Daylight"), 5500.0),
                    (tr("Cloudy"), 6500.0),
                    (tr("Shade"), 7500.0),
                    (tr("Tungsten"), 2850.0),
                    (tr("Flash"), 6000.0),
                ] {
                    if ui.button(name).clicked() {
                        d.settings.white_balance = WhiteBalance::Temperature;
                        d.settings.temperature = kelvin;
                        d.settings.tint = 0.0;
                        ui.close();
                    }
                }
            });
        if widgets::checkbox(ui, &mut d.picker, tr("Pick neutral")).changed() {
            d.draw_overlay = false;
            d.film_base_picker = false;
        }
    });
    ui.add_enabled_ui(
        d.settings.white_balance == WhiteBalance::Temperature,
        |ui| {
            slider(
                ui,
                tr("Temperature"),
                &mut d.settings.temperature,
                2000.0..=25_000.0,
                " K",
            );
        },
    );
    slider(ui, tr("Tint"), &mut d.settings.tint, -150.0..=150.0, "");
}

fn negative(ui: &mut egui::Ui, d: &mut Develop) {
    heading(ui, tr("Film negative"));
    let unconfigured = d.settings.negative == raw::NegativeSettings::default();
    if widgets::checkbox(
        ui,
        &mut d.settings.negative.enabled,
        tr("Convert negative to positive"),
    )
    .changed()
    {
        d.picker = false;
        d.film_base_picker = false;
        if d.settings.negative.enabled
            && unconfigured
            && let Some(raw) = &d.proxy
        {
            d.settings.negative = raw::analyze_negative(raw, &d.settings);
        }
    }
    if !d.settings.negative.enabled {
        return;
    }
    ui.small(tr("Crop out the holder and borders, then analyze. Sample an unexposed film edge for a more accurate orange mask."));
    ui.horizontal(|ui| {
        if widgets::button(ui, tr("Analyze crop")).clicked()
            && let Some(raw) = &d.proxy
        {
            let calibration = raw::analyze_negative(raw, &d.settings);
            d.settings.negative.film_base = calibration.film_base;
            d.settings.negative.density_range = calibration.density_range;
        }
        if widgets::checkbox(ui, &mut d.film_base_picker, tr("Pick film base")).changed() {
            d.picker = false;
            d.draw_overlay = false;
            if d.film_base_picker {
                d.compare = super::develop::Compare::Original;
            }
        }
    });
    slider(
        ui,
        tr("Black point"),
        &mut d.settings.negative.black_point,
        -0.5..=0.5,
        " D",
    );
    slider(
        ui,
        tr("Film gamma"),
        &mut d.settings.negative.gamma,
        0.5..=4.0,
        "",
    );
    for (c, label) in [tr("Red balance"), tr("Green balance"), tr("Blue balance")]
        .iter()
        .enumerate()
    {
        slider(
            ui,
            label,
            &mut d.settings.negative.balance[c],
            -3.0..=3.0,
            " EV",
        );
    }
    egui::CollapsingHeader::new(tr("Film calibration")).id_salt("film_calibration").show(ui, |ui| {
        ui.small(tr("Film base is measured in linear camera RGB. Density range controls each channel's white point."));
        for (c, label) in [tr("Red base"), tr("Green base"), tr("Blue base")].iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(*label);
                ui.add(egui::DragValue::new(&mut d.settings.negative.film_base[c]).range(0.00001..=16.0).speed(0.001).max_decimals(5));
            });
        }
        for (c, label) in [tr("Red density"), tr("Green density"), tr("Blue density")].iter().enumerate() {
            slider(ui, label, &mut d.settings.negative.density_range[c], 0.1..=6.0, " D");
        }
    });
}

fn tones(ui: &mut egui::Ui, d: &mut Develop) {
    heading(ui, tr("Tone curve"));
    widgets::segmented(
        ui,
        &mut d.curve_channel,
        &[
            (0, "RGB"),
            (1, tr("Red")),
            (2, tr("Green")),
            (3, tr("Blue")),
        ],
    );
    curve(ui, &mut d.settings.curves[d.curve_channel], d.curve_channel);
    ui.horizontal(|ui| {
        if widgets::button(ui, tr("Linear")).clicked() {
            d.settings.curves[d.curve_channel] = [0.0, 0.25, 0.5, 0.75, 1.0];
        }
        if widgets::button(ui, tr("S curve")).clicked() {
            d.settings.curves[d.curve_channel] = [0.0, 0.18, 0.5, 0.82, 1.0];
        }
        if widgets::button(ui, tr("Lift blacks")).clicked() {
            d.settings.curves[d.curve_channel] = [0.08, 0.28, 0.5, 0.75, 1.0];
        }
    });
    heading(ui, tr("Color mixer"));
    widgets::PopUp::from_id_salt("raw_hsl")
        .selected_text(
            [
                tr("Red"),
                tr("Orange"),
                tr("Yellow"),
                tr("Green"),
                tr("Aqua"),
                tr("Blue"),
                tr("Purple"),
                tr("Magenta"),
            ][d.hsl_band],
        )
        .show_ui(ui, |ui| {
            for (i, name) in [
                tr("Red"),
                tr("Orange"),
                tr("Yellow"),
                tr("Green"),
                tr("Aqua"),
                tr("Blue"),
                tr("Purple"),
                tr("Magenta"),
            ]
            .iter()
            .enumerate()
            {
                widgets::menu_choice(ui, &mut d.hsl_band, i, *name);
            }
        });
    percent(ui, tr("Hue"), &mut d.settings.hsl[d.hsl_band][0]);
    percent(ui, tr("Saturation"), &mut d.settings.hsl[d.hsl_band][1]);
    percent(ui, tr("Lightness"), &mut d.settings.hsl[d.hsl_band][2]);
    heading(ui, tr("Black & white"));
    widgets::checkbox(ui, &mut d.settings.monochrome, tr("Monochrome"));
    ui.add_enabled_ui(d.settings.monochrome, |ui| {
        for (i, label) in [tr("Red mix"), tr("Green mix"), tr("Blue mix")]
            .iter()
            .enumerate()
        {
            slider(ui, label, &mut d.settings.bw_mix[i], -1.0..=2.0, "");
        }
    });
    heading(ui, tr("Split toning"));
    slider(
        ui,
        tr("Shadow hue"),
        &mut d.settings.shadow_tone[0],
        0.0..=360.0,
        "°",
    );
    slider(
        ui,
        tr("Shadow amount"),
        &mut d.settings.shadow_tone[1],
        0.0..=100.0,
        "%",
    );
    slider(
        ui,
        tr("Highlight hue"),
        &mut d.settings.highlight_tone[0],
        0.0..=360.0,
        "°",
    );
    slider(
        ui,
        tr("Highlight amount"),
        &mut d.settings.highlight_tone[1],
        0.0..=100.0,
        "%",
    );
    percent(ui, tr("Balance"), &mut d.settings.tone_balance);
}

fn detail(ui: &mut egui::Ui, d: &mut Develop) {
    heading(ui, tr("Noise reduction"));
    slider(
        ui,
        tr("Luminance"),
        &mut d.settings.luminance_noise,
        0.0..=100.0,
        "%",
    );
    slider(
        ui,
        tr("Color"),
        &mut d.settings.color_noise,
        0.0..=100.0,
        "%",
    );
    heading(ui, tr("Sharpening"));
    slider(ui, tr("Amount"), &mut d.settings.sharpen, 0.0..=200.0, "%");
    slider(
        ui,
        tr("Radius"),
        &mut d.settings.sharpen_radius,
        0.3..=5.0,
        " px",
    );
    slider(
        ui,
        tr("Threshold"),
        &mut d.settings.sharpen_threshold,
        0.0..=0.2,
        "",
    );
    ui.add_space(12.0);
    widgets::checkbox(
        ui,
        &mut d.full_preview,
        tr("Always use full-resolution preview"),
    );
    ui.label(egui::RichText::new(tr("Zooming in loads full detail automatically. Use 100% to judge sharpening and noise reduction. Always using full resolution also applies it to Fit view and takes longer to update.")).color(theme::MUTED));
}

fn lens(ui: &mut egui::Ui, d: &mut Develop) {
    heading(ui, tr("Manual lens correction"));
    if let Some(asset) = &d.asset
        && !asset.metadata.lens.is_empty()
    {
        ui.label(&asset.metadata.lens);
    }
    percent(ui, tr("Distortion"), &mut d.settings.distortion);
    percent(ui, tr("Red / cyan"), &mut d.settings.chromatic_red);
    percent(ui, tr("Blue / yellow"), &mut d.settings.chromatic_blue);
    slider(
        ui,
        tr("Defringe"),
        &mut d.settings.defringe,
        0.0..=100.0,
        "%",
    );
    percent(ui, tr("Vignetting"), &mut d.settings.vignette);
    heading(ui, tr("Geometry"));
    ui.horizontal(|ui| {
        if widgets::button(ui, tr("Rotate left 90°")).clicked() {
            d.rotate(false);
        }
        if widgets::button(ui, tr("Rotate right 90°")).clicked() {
            d.rotate(true);
        }
    });
    slider(
        ui,
        tr("Straighten"),
        &mut d.settings.rotation,
        -45.0..=45.0,
        "°",
    );
    percent(ui, tr("Horizontal"), &mut d.settings.perspective[0]);
    percent(ui, tr("Vertical"), &mut d.settings.perspective[1]);
    heading(ui, tr("Crop"));
    ui.label(
        egui::RichText::new(tr("Bounds as a fraction of the rotated image"))
            .small()
            .color(theme::MUTED),
    );
    let mut crop = d.settings.display_crop();
    let original_crop = crop;
    let [left, top, right, bottom] = crop;
    slider(ui, tr("Left"), &mut crop[0], 0.0..=(right - 0.01), "");
    slider(ui, tr("Top"), &mut crop[1], 0.0..=(bottom - 0.01), "");
    slider(ui, tr("Right"), &mut crop[2], (left + 0.01)..=1.0, "");
    slider(ui, tr("Bottom"), &mut crop[3], (top + 0.01)..=1.0, "");
    if crop != original_crop {
        d.settings.set_display_crop(crop);
    }
    ui.horizontal(|ui| {
        if widgets::button(ui, tr("Uncrop")).clicked() {
            d.settings.crop = [0.0, 0.0, 1.0, 1.0];
        }
        if widgets::button(ui, tr("Square")).clicked()
            && let Some(raw) = &d.proxy
        {
            let aspect = raw.camera.width() as f32 / raw.camera.height() as f32;
            d.settings.crop = if aspect >= 1.0 {
                let margin = (1.0 - 1.0 / aspect) * 0.5;
                [margin, 0.0, 1.0 - margin, 1.0]
            } else {
                let margin = (1.0 - aspect) * 0.5;
                [0.0, margin, 1.0, 1.0 - margin]
            };
        }
    });
}

fn masks(ui: &mut egui::Ui, d: &mut Develop) {
    heading(ui, tr("Local adjustments"));
    ui.add_enabled_ui(d.settings.overlays.len() < 32, |ui| {
        ui.horizontal(|ui| {
            for (name, kind) in [
                (tr("Linear"), OverlayKind::Linear),
                (tr("Radial"), OverlayKind::Radial),
                (tr("Brush"), OverlayKind::Brush),
            ] {
                if widgets::button(ui, format!("+ {name}")).clicked() {
                    d.settings.overlays.push(Overlay {
                        name: format!("{name} {}", d.settings.overlays.len() + 1),
                        kind,
                        ..Default::default()
                    });
                    d.selected_overlay = Some(d.settings.overlays.len() - 1);
                    d.draw_overlay = true;
                    d.show_mask = true;
                    d.picker = false;
                    d.film_base_picker = false;
                }
            }
        });
    });
    for (i, overlay) in d.settings.overlays.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            widgets::checkbox(ui, &mut overlay.enabled, "");
            widgets::selectable_value(ui, &mut d.selected_overlay, Some(i), &overlay.name);
        });
    }
    let Some(index) = d
        .selected_overlay
        .filter(|i| *i < d.settings.overlays.len())
    else {
        ui.label(tr("Add a mask, then drag over the photo to place it."));
        return;
    };
    ui.separator();
    ui.horizontal(|ui| {
        if widgets::checkbox(ui, &mut d.draw_overlay, tr("Draw mask")).changed() {
            d.picker = false;
            d.film_base_picker = false;
        }
        widgets::checkbox(ui, &mut d.show_mask, tr("Show guides"));
    });
    let overlay = &mut d.settings.overlays[index];
    ui.text_edit_singleline(&mut overlay.name);
    widgets::checkbox(ui, &mut overlay.invert, tr("Invert mask"));
    if overlay.kind == OverlayKind::Brush {
        slider(ui, tr("Brush radius"), &mut overlay.radius, 0.005..=0.3, "");
        if widgets::button(ui, tr("Clear brush")).clicked() {
            overlay.points.clear();
        }
    }
    if overlay.kind != OverlayKind::Linear {
        slider(ui, tr("Feather"), &mut overlay.feather, 0.01..=1.0, "");
    }
    slider(
        ui,
        tr("Exposure"),
        &mut overlay.exposure,
        -10.0..=10.0,
        " EV",
    );
    percent(ui, tr("Warmth"), &mut overlay.warmth);
    percent(ui, tr("Saturation"), &mut overlay.saturation);
    if widgets::button(ui, tr("Delete mask")).clicked() {
        d.settings.overlays.remove(index);
        d.selected_overlay = None;
        d.draw_overlay = false;
    }
}

fn metadata(ui: &mut egui::Ui, d: &Develop) {
    let Some(asset) = &d.asset else {
        return;
    };
    let m = &asset.metadata;
    heading(ui, tr("Camera information"));
    ui.strong(&m.camera);
    if !m.lens.is_empty() {
        ui.label(&m.lens);
    }
    ui.add_space(12.0);
    egui::Grid::new("raw_metadata")
        .spacing(vec2(14.0, 9.0))
        .show(ui, |ui| {
            for (label, value) in [
                (tr("Source"), asset.filename.clone()),
                (tr("Dimensions"), format!("{} × {}", m.width, m.height)),
                (tr("Decoded depth"), format!("{} bit", m.bits)),
                ("ISO", m.iso.map_or("—".into(), |v| v.to_string())),
                (
                    tr("Aperture"),
                    m.aperture.map_or("—".into(), |v| format!("f/{v:.1}")),
                ),
                (
                    tr("Shutter"),
                    m.shutter.map_or("—".into(), |v| {
                        if v > 0.0 && v < 1.0 {
                            format!("1/{:.0} s", 1.0 / v)
                        } else {
                            format!("{v:.2} s")
                        }
                    }),
                ),
                (
                    tr("Focal length"),
                    m.focal_length.map_or("—".into(), |v| format!("{v:.0} mm")),
                ),
                (
                    tr("RAW storage"),
                    format!(
                        "{} · {:.1} MiB",
                        tr("Embedded"),
                        asset.bytes.len() as f64 / 1_048_576.0
                    ),
                ),
            ] {
                ui.label(egui::RichText::new(label).color(theme::MUTED));
                ui.label(value);
                ui.end_row();
            }
        });
    heading(ui, tr("Output"));
    ui.label(tr("Embedded RAW layer with an sRGB photo render. Double-click the layer to return to Develop."));
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Lens corrections are manual. The photo editor currently uses 8-bit sRGB; RAW data and Develop settings retain their original precision.")).color(theme::MUTED));
}

fn histogram(ui: &mut egui::Ui, d: &Develop) {
    ui.horizontal(|ui| {
        ui.strong(tr("Histogram"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new("RGB").small().color(theme::MUTED));
        });
    });
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, theme::FIELD);
    for n in 1..4 {
        let x = rect.left() + rect.width() * n as f32 / 4.0;
        ui.painter().line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(0.5_f32, theme::DIVIDER),
        );
    }
    let max = d
        .histogram
        .iter()
        .flatten()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    for (c, color) in [
        Color32::from_rgb(235, 90, 98),
        Color32::from_rgb(103, 208, 135),
        Color32::from_rgb(90, 157, 255),
    ]
    .iter()
    .enumerate()
    {
        let points: Vec<_> = d.histogram[c]
            .iter()
            .enumerate()
            .map(|(i, count)| {
                pos2(
                    rect.left() + rect.width() * i as f32 / 255.0,
                    rect.bottom() - (*count as f32 / max).sqrt() * rect.height(),
                )
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, Stroke::new(1.0_f32, *color)));
    }
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} {:.2}%", tr("Shadows"), d.clipping[0]))
                .small()
                .color(theme::MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} {:.2}%", tr("Highlights"), d.clipping[1]))
                    .small()
                    .color(theme::MUTED),
            );
        });
    });
}

fn curve(ui: &mut egui::Ui, knots: &mut [f32; 5], channel: usize) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 175.0), Sense::click_and_drag());
    let rect = rect.shrink(5.0);
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::FIELD);
    for i in 1..4 {
        let t = i as f32 / 4.0;
        painter.line_segment(
            [
                pos2(rect.left() + t * rect.width(), rect.top()),
                pos2(rect.left() + t * rect.width(), rect.bottom()),
            ],
            Stroke::new(0.5_f32, theme::DIVIDER),
        );
        painter.line_segment(
            [
                pos2(rect.left(), rect.top() + t * rect.height()),
                pos2(rect.right(), rect.top() + t * rect.height()),
            ],
            Stroke::new(0.5_f32, theme::DIVIDER),
        );
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(0.5_f32, theme::MUTED),
    );
    if (response.dragged() || response.clicked())
        && let Some(point) = response.interact_pointer_pos()
    {
        let index = (((point.x - rect.left()) / rect.width()) * 4.0)
            .round()
            .clamp(0.0, 4.0) as usize;
        knots[index] = ((rect.bottom() - point.y) / rect.height()).clamp(0.0, 1.0);
    }
    let points: Vec<_> = knots
        .iter()
        .enumerate()
        .map(|(i, value)| {
            pos2(
                rect.left() + rect.width() * i as f32 / 4.0,
                rect.bottom() - rect.height() * value,
            )
        })
        .collect();
    let color = [
        Color32::WHITE,
        Color32::LIGHT_RED,
        Color32::LIGHT_GREEN,
        Color32::LIGHT_BLUE,
    ][channel];
    painter.add(egui::Shape::line(
        points.clone(),
        Stroke::new(1.5_f32, color),
    ));
    for point in points {
        painter.circle_filled(point, 4.0, color);
    }
}

fn save_preset(d: &mut Develop) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(tr("RAW settings"), &["json"])
        .set_file_name("raw-settings.json")
        .save_file()
    else {
        return;
    };
    let result = (|| -> anyhow::Result<()> {
        d.settings.validate()?;
        let parent = path.parent().unwrap_or(std::path::Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&serde_json::to_vec_pretty(&d.settings)?)?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    })();
    if let Err(error) = result {
        d.error = Some(error.to_string());
    }
}

fn load_preset(d: &mut Develop) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(tr("RAW settings"), &["json"])
        .pick_file()
    else {
        return;
    };
    let result = (|| -> anyhow::Result<DevelopSettings> {
        let mut bytes = Vec::new();
        File::open(path)?.take(1_048_577).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 1_048_576, "RAW settings file exceeds 1 MiB");
        let settings: DevelopSettings = serde_json::from_slice(&bytes)?;
        settings.validate()?;
        Ok(settings)
    })();
    match result {
        Ok(settings) => {
            d.settings = settings;
            d.selected_overlay = None;
        }
        Err(error) => d.error = Some(format!("{}: {error}", tr("Could not load RAW settings"))),
    }
}
