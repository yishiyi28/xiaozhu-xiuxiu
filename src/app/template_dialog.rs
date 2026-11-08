use anyhow::{Context, Result};
use egui::{ColorImage, TextureHandle, TextureOptions};
use image::RgbaImage;
use xuan::{
    document::{Document, Layer},
    i18n::tr,
    portrait::{
        inference::PortraitModels,
        preset::{
            FacePolicy, PresetStrength, PresetV1, apply_preset_to_document_with_faces, scale_preset,
        },
        types::FaceLandmarks,
    },
    render,
};

use super::{EditorApp, preset_dialog::preset_chip, widgets};

const PREVIEW_MAX_EDGE: u32 = 720;
const FEATURED_TEMPLATES: [(&str, &str); 4] = [
    ("refined", "Refined"),
    ("natural", "Natural"),
    ("clear-portrait", "Clear Portrait"),
    ("warm", "Warm"),
];

pub struct TemplatePreviewState {
    base: Document,
    faces: Vec<FaceLandmarks>,
    selected: String,
    strength: PresetStrength,
    recommended: &'static str,
    recommendation_reason: &'static str,
    original: TextureHandle,
    preview: Option<TextureHandle>,
    preview_key: Option<(String, PresetStrength)>,
    error: Option<String>,
}

impl TemplatePreviewState {
    pub(super) fn from_document(
        context: &egui::Context,
        document: &Document,
        selected: &str,
        known_faces: Option<Vec<FaceLandmarks>>,
    ) -> Result<Self> {
        let scale = (PREVIEW_MAX_EDGE as f32 / document.width.max(document.height) as f32).min(1.0);
        let width = (document.width as f32 * scale).round().max(1.0) as u32;
        let height = (document.height as f32 * scale).round().max(1.0) as u32;
        let image = render::render_scaled(document, width, height);
        let faces = match known_faces {
            Some(faces) => faces,
            None => PortraitModels::shared()?.detect_with_landmarks(&image)?,
        };
        Self::from_image(context, image, selected, faces)
    }

    fn from_image(
        context: &egui::Context,
        image: RgbaImage,
        selected: &str,
        faces: Vec<FaceLandmarks>,
    ) -> Result<Self> {
        let width = image.width();
        let height = image.height();
        let mut base = Document::new(width, height)?;
        let layer = Layer::image(tr("Template preview"), image.clone());
        let id = layer.id;
        base.layers = vec![layer];
        base.select(id, false);
        let (recommended, recommendation_reason) = recommend_template(&image, !faces.is_empty());
        let selected = if FEATURED_TEMPLATES.iter().any(|(slug, _)| *slug == selected) {
            selected.to_owned()
        } else {
            recommended.to_owned()
        };
        Ok(Self {
            original: texture(context, format!("template-original-{}", base.id), &image),
            base,
            faces,
            selected,
            strength: PresetStrength::Standard,
            recommended,
            recommendation_reason,
            preview: None,
            preview_key: None,
            error: None,
        })
    }

    fn select(&mut self, slug: &str) {
        if self.selected != slug {
            self.selected = slug.to_owned();
            self.preview = None;
            self.preview_key = None;
            self.error = None;
        }
    }

    fn select_strength(&mut self, strength: PresetStrength) {
        if self.strength != strength {
            self.strength = strength;
            self.preview = None;
            self.preview_key = None;
            self.error = None;
        }
    }

    fn ensure_preview(&mut self, context: &egui::Context) {
        let key = (self.selected.clone(), self.strength);
        if self.preview.is_some() && self.preview_key.as_ref() == Some(&key) {
            return;
        }
        let result = (|| -> Result<RgbaImage> {
            let preset = scale_preset(&PresetV1::builtin(&self.selected)?, self.strength);
            let mut document = self.base.clone();
            apply_preset_to_document_with_faces(
                &mut document,
                &preset,
                FacePolicy::AllFaces,
                Some(&self.faces),
            )?;
            Ok(render::render(&document))
        })();
        match result {
            Ok(image) => {
                self.preview = Some(texture(
                    context,
                    format!("template-preview-{}", self.base.id),
                    &image,
                ));
                self.preview_key = Some(key);
                self.error = None;
            }
            Err(error) => {
                self.preview_key = Some(key);
                self.error = Some(format!("{error:#}"));
            }
        }
    }
}

impl EditorApp {
    pub(super) fn open_welcome_template(
        &mut self,
        context: &egui::Context,
        path: &std::path::Path,
        slug: &str,
    ) {
        let before = self.sessions.len();
        self.open_path(path, false);
        if self.sessions.len() == before {
            return;
        }
        let result = self
            .session()
            .map(|session| {
                TemplatePreviewState::from_document(context, &session.document, slug, None)
            })
            .context("No image is open");
        match result.and_then(|result| result) {
            Ok(state) => self.template_dialog = Some(state),
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    pub(super) fn template_dialog(&mut self, context: &egui::Context) {
        let Some(mut state) = self.template_dialog.take() else {
            return;
        };
        state.ensure_preview(context);
        let mut open = true;
        let mut apply = false;
        let mut apply_and_export = false;
        let mut cancel = false;
        let window_width = (context.content_rect().width() - 40.0).clamp(420.0, 720.0);
        let preview_height = (context.content_rect().height() - 280.0).clamp(160.0, 460.0);
        widgets::Window::new(tr("One-click finish"))
            .default_width(window_width)
            .open(&mut open)
            .show(context, |ui| {
                ui.label(
                    egui::RichText::new(tr(
                        "Preview locally. Nothing is applied until you choose.",
                    ))
                    .color(egui::Color32::from_gray(170)),
                );
                if state.faces.is_empty() {
                    ui.label(tr("No face detected. Color preview is still available."));
                } else {
                    ui.label(format!(
                        "{} {}",
                        state.faces.len(),
                        tr("faces detected locally")
                    ));
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for (slug, label) in FEATURED_TEMPLATES {
                        if let Ok(preset) = PresetV1::builtin(slug) {
                            let label = if state.recommended == slug {
                                format!("{} · {}", tr(label), tr("Recommended"))
                            } else {
                                tr(label).to_owned()
                            };
                            if preset_chip(ui, state.selected == slug, &preset, &label) {
                                state.select(slug);
                            }
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(tr("Effect strength"));
                    let mut strength = state.strength;
                    let labels = [
                        tr("Light strength"),
                        tr("Standard strength"),
                        tr("Strong strength"),
                    ];
                    let response = widgets::segmented(
                        ui,
                        &mut strength,
                        &[
                            (PresetStrength::Light, labels[0]),
                            (PresetStrength::Standard, labels[1]),
                            (PresetStrength::Strong, labels[2]),
                        ],
                    );
                    if response.changed() {
                        state.select_strength(strength);
                    }
                });
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {}",
                        tr("Recommended for this photo"),
                        tr(state.recommendation_reason)
                    ))
                    .color(super::theme::MUTED),
                );
                if state.selected == "natural" {
                    ui.label(
                        egui::RichText::new(tr(
                            "Natural is intentionally subtle. Choose Refined for a more visible result.",
                        ))
                        .size(11.0)
                        .color(super::theme::MUTED),
                    );
                }
                if let Some(error) = &state.error {
                    ui.colored_label(egui::Color32::from_rgb(230, 110, 100), error);
                }
                let column_width = ((ui.available_width() - 8.0) * 0.5).max(120.0);
                ui.columns(2, |columns| {
                    columns[0].label(
                        egui::RichText::new(tr("Original"))
                            .size(11.0)
                            .color(super::theme::MUTED),
                    );
                    let original_size = state.original.size_vec2();
                    let original_scale = (column_width / original_size.x)
                        .min(preview_height / original_size.y)
                        .min(1.0);
                    columns[0].image((state.original.id(), original_size * original_scale));

                    columns[1].label(
                        egui::RichText::new(tr("After"))
                            .size(11.0)
                            .color(super::theme::MUTED),
                    );
                    let after = state.preview.as_ref().unwrap_or(&state.original);
                    let after_size = after.size_vec2();
                    let after_scale = (column_width / after_size.x)
                        .min(preview_height / after_size.y)
                        .min(1.0);
                    columns[1].image((after.id(), after_size * after_scale));
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary_button(ui, tr("Apply & Export…")).clicked() {
                            apply = true;
                            apply_and_export = true;
                        }
                        if widgets::button(ui, tr("Apply template")).clicked() {
                            apply = true;
                        }
                        if widgets::button(ui, tr("Cancel")).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if apply {
            match PresetV1::builtin(&state.selected) {
                Ok(preset) => {
                    let preset = scale_preset(&preset, state.strength);
                    let preset_name = preset.name.clone();
                    self.apply_preset(preset);
                    if self.error.is_none() {
                        self.status = format!(
                            "{}: {} · {}",
                            tr("Preset"),
                            preset_name,
                            tr(state.strength.label())
                        );
                    }
                    if apply_and_export {
                        self.dialog = Some(super::Dialog::Export);
                        self.export_changed = true;
                    }
                    return;
                }
                Err(error) => state.error = Some(format!("{error:#}")),
            }
        }
        if cancel {
            open = false;
        }
        if open {
            self.template_dialog = Some(state);
        }
    }
}

fn texture(context: &egui::Context, name: String, image: &RgbaImage) -> TextureHandle {
    let color = ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    context.load_texture(name, color, TextureOptions::LINEAR)
}

fn recommend_template(image: &RgbaImage, has_face: bool) -> (&'static str, &'static str) {
    let mut weight = 0.0;
    let mut luma = 0.0;
    let mut warmth = 0.0;
    let mut saturation = 0.0;
    for pixel in image.pixels() {
        let alpha = pixel[3] as f32 / 255.0;
        if alpha <= 0.0 {
            continue;
        }
        let r = pixel[0] as f32 / 255.0;
        let g = pixel[1] as f32 / 255.0;
        let b = pixel[2] as f32 / 255.0;
        weight += alpha;
        luma += (0.2126 * r + 0.7152 * g + 0.0722 * b) * alpha;
        warmth += (r - b) * alpha;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        saturation += if max > 0.0 { (max - min) / max } else { 0.0 } * alpha;
    }
    if weight == 0.0 {
        return ("natural", "Balanced light: Natural keeps the look subtle.");
    }
    let luma = luma / weight;
    let warmth = warmth / weight;
    let saturation = saturation / weight;
    if luma < 0.38 {
        ("warm", "Dark photo: Warm adds light and energy.")
    } else if warmth < -0.035 {
        ("warm", "Cool cast: Warm balances the image.")
    } else if luma > 0.68 || (warmth > 0.09 && saturation > 0.32) {
        (
            "clear-portrait",
            "Bright photo: Clear Portrait restores dimension.",
        )
    } else if has_face {
        (
            "refined",
            "Portrait detected: Refined adds a visible but natural lift.",
        )
    } else {
        ("natural", "Balanced light: Natural keeps the look subtle.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn image(color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(32, 32, Rgba(color))
    }

    #[test]
    fn portrait_recommendation_defaults_to_refined_at_standard_strength() {
        let context = egui::Context::default();
        let state = TemplatePreviewState::from_image(
            &context,
            image([130, 132, 128, 255]),
            "natural",
            Vec::new(),
        )
        .unwrap();

        assert_eq!(state.strength, PresetStrength::Standard);
        assert_eq!(
            recommend_template(&image([130, 132, 128, 255]), true).0,
            "refined"
        );
    }

    #[test]
    fn recommendation_adapts_to_exposure_and_color_cast() {
        assert_eq!(
            recommend_template(&image([20, 24, 28, 255]), false).0,
            "warm"
        );
        assert_eq!(
            recommend_template(&image([235, 238, 242, 255]), false).0,
            "clear-portrait"
        );
        assert_eq!(
            recommend_template(&image([130, 132, 128, 255]), false).0,
            "natural"
        );
    }

    #[test]
    fn preview_uses_known_faces_without_mutating_the_source() {
        let context = egui::Context::default();
        let mut source = Document::new(32, 32).unwrap();
        let layer = Layer::image("Source", image([90, 110, 140, 255]));
        let id = layer.id;
        source.layers = vec![layer];
        source.select(id, false);
        let before = render::render(&source);
        let state =
            TemplatePreviewState::from_document(&context, &source, "warm", Some(Vec::new()))
                .unwrap();
        assert_eq!(state.selected, "warm");
        assert_eq!(state.faces.len(), 0);
        assert_eq!(render::render(&source), before);
    }
}
