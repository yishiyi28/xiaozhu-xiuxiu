use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

use anyhow::{Context, Result};
use egui::{ColorImage, Sense, TextureHandle, TextureOptions, vec2};
use image::RgbaImage;
use uuid::Uuid;
use xuan::{
    i18n::tr,
    portrait::{
        inference::PortraitModels,
        layer::PortraitAdjustment,
        masks::build_portrait_masks,
        retouch::{PortraitSettings, apply_portrait_to_rgba8},
        types::FaceLandmarks,
        warp::{
            LiquifyBrush, LiquifyMode, LiquifyStroke, WarpGrid, apply_face_shape,
            apply_liquify_many,
        },
    },
};

use super::{EditorApp, widgets};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortraitState {
    Idle,
    LoadingModels,
    Analyzing,
    Ready,
}

#[derive(Clone, Debug)]
struct NormalizedLiquifyStroke {
    from: [f32; 2],
    to: [f32; 2],
    mode: LiquifyMode,
    diameter: f32,
    strength: f32,
    hardness: f32,
}

#[derive(Clone, Debug)]
pub struct LiquifySession {
    strokes: Vec<NormalizedLiquifyStroke>,
    brush: LiquifyBrush,
    dragging_from: Option<[f32; 2]>,
}

impl Default for LiquifySession {
    fn default() -> Self {
        Self {
            strokes: Vec::new(),
            brush: LiquifyBrush {
                mode: LiquifyMode::Push,
                diameter: 120.0,
                strength: 50.0,
                hardness: 60.0,
            },
            dragging_from: None,
        }
    }
}

pub struct PortraitWorkspace {
    pub state: PortraitState,
    pub faces: Vec<FaceLandmarks>,
    pub selected: usize,
    pub apply_to_all: bool,
    pub settings: Vec<PortraitSettings>,
    pub preview: Option<TextureHandle>,
    original_preview: Option<TextureHandle>,
    pub compare_original: bool,
    pub error: Option<String>,
    pub liquify: Option<LiquifySession>,
    editing_layer: Option<Uuid>,
    source: RgbaImage,
    preview_revision: u64,
    last_preview_revision: u64,
}

impl PortraitWorkspace {
    pub fn analyzing() -> Self {
        Self {
            state: PortraitState::Analyzing,
            faces: Vec::new(),
            selected: 0,
            apply_to_all: false,
            settings: Vec::new(),
            preview: None,
            original_preview: None,
            compare_original: false,
            error: None,
            liquify: Some(LiquifySession::default()),
            editing_layer: None,
            source: RgbaImage::new(1, 1),
            preview_revision: 1,
            last_preview_revision: 0,
        }
    }

    #[cfg(test)]
    pub fn from_analysis(result: Result<Vec<FaceLandmarks>, String>) -> Result<Self> {
        Self::from_analysis_with_source(result, RgbaImage::new(1, 1))
    }

    pub fn from_analysis_with_source(
        result: Result<Vec<FaceLandmarks>, String>,
        source: RgbaImage,
    ) -> Result<Self> {
        let mut workspace = Self::analyzing();
        workspace.source = source;
        match result {
            Ok(faces) if faces.is_empty() => {
                workspace.state = PortraitState::Ready;
                workspace.error = Some(tr("No face detected").into());
            }
            Ok(faces) => {
                workspace.state = PortraitState::Ready;
                workspace.settings = vec![PortraitSettings::default(); faces.len()];
                workspace.faces = faces;
            }
            Err(error) => {
                workspace.state = PortraitState::Ready;
                workspace.error = Some(error);
            }
        }
        Ok(workspace)
    }

    pub fn from_adjustment(
        source: RgbaImage,
        recipe: PortraitAdjustment,
        editing_layer: Uuid,
    ) -> Self {
        let mut workspace = Self::analyzing();
        workspace.source = source;
        workspace.state = PortraitState::Ready;
        workspace.faces = recipe.faces;
        workspace.apply_to_all = recipe.apply_to_all;
        workspace.settings = recipe.settings;
        workspace.selected = recipe.selected.min(workspace.faces.len().saturating_sub(1));
        workspace.editing_layer = Some(editing_layer);
        let width = workspace.source.width() as f32;
        let height = workspace.source.height() as f32;
        let longest = width.max(height).max(1.0);
        workspace.liquify = Some(LiquifySession {
            strokes: recipe
                .liquify
                .into_iter()
                .map(|stroke| NormalizedLiquifyStroke {
                    from: [stroke.from[0] / width, stroke.from[1] / height],
                    to: [stroke.to[0] / width, stroke.to[1] / height],
                    mode: stroke.brush.mode,
                    diameter: stroke.brush.diameter / longest * 900.0,
                    strength: stroke.brush.strength,
                    hardness: stroke.brush.hardness,
                })
                .collect(),
            ..LiquifySession::default()
        });
        workspace
    }

    pub fn cancel(&mut self) {
        self.state = PortraitState::Idle;
        self.faces.clear();
        self.settings.clear();
        self.preview = None;
        self.original_preview = None;
        self.error = None;
        self.liquify = None;
        self.editing_layer = None;
    }

    fn selected_settings(&self) -> Option<&PortraitSettings> {
        self.settings.get(self.selected)
    }

    fn selected_settings_mut(&mut self) -> Option<&mut PortraitSettings> {
        self.settings.get_mut(self.selected)
    }

    fn invalidate_preview(&mut self) {
        self.preview_revision = self.preview_revision.wrapping_add(1);
    }
}

pub struct PortraitAnalysisJob {
    receive: Receiver<Result<Vec<FaceLandmarks>, String>>,
    cancel: Arc<AtomicBool>,
}

impl PortraitAnalysisJob {
    pub fn spawn(source: RgbaImage, context: egui::Context) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (send, receive) = mpsc::channel();
        xuan::gpu::spawn(move || {
            let result = PortraitModels::shared()
                .and_then(|models| models.detect_with_landmarks(&source))
                .map_err(|error| format!("{error:#}"));
            if !worker_cancel.load(Ordering::Relaxed) {
                let _ = send.send(result);
            }
            context.request_repaint();
        });
        Self { receive, cancel }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn poll(&self) -> Option<Result<Vec<FaceLandmarks>, String>> {
        match self.receive.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(tr("Portrait analysis stopped unexpectedly").into()))
            }
        }
    }
}

fn scaled_face(face: &FaceLandmarks, scale: f32) -> FaceLandmarks {
    let mut face = face.clone();
    face.face.rect = face.face.rect.map(|value| value * scale);
    face.face.left_eye = face.face.left_eye.map(|value| value * scale);
    face.face.right_eye = face.face.right_eye.map(|value| value * scale);
    for point in &mut face.points {
        point.position = point.position.map(|value| value * scale);
    }
    face
}

fn downscaled_source(source: &RgbaImage) -> (RgbaImage, f32) {
    let longest = source.width().max(source.height());
    if longest <= 1024 {
        return (source.clone(), 1.0);
    }
    let scale = 1024.0 / longest as f32;
    let width = (source.width() as f32 * scale).round().max(1.0) as u32;
    let height = (source.height() as f32 * scale).round().max(1.0) as u32;
    (
        image::imageops::resize(source, width, height, image::imageops::FilterType::Triangle),
        scale,
    )
}

fn apply_strokes_normalized(
    source: &RgbaImage,
    strokes: &[NormalizedLiquifyStroke],
) -> Result<RgbaImage> {
    if strokes.is_empty() {
        return Ok(source.clone());
    }
    let longest = source.width().max(source.height()) as f32;
    let cell = (source.width().min(source.height()) / 32).clamp(8, 64);
    let grid = WarpGrid::identity([source.width(), source.height()], cell);
    let liquify = strokes
        .iter()
        .map(|stroke| {
            let from = [
                stroke.from[0] * source.width() as f32,
                stroke.from[1] * source.height() as f32,
            ];
            let to = [
                stroke.to[0] * source.width() as f32,
                stroke.to[1] * source.height() as f32,
            ];
            LiquifyStroke::new(
                from,
                to,
                LiquifyBrush {
                    mode: stroke.mode,
                    diameter: stroke.diameter / 900.0 * longest,
                    strength: stroke.strength,
                    hardness: stroke.hardness,
                },
            )
        })
        .collect::<Vec<_>>();
    apply_liquify_many(source, &grid, &liquify)
}

fn preview_texture(context: &egui::Context, image: &RgbaImage) -> TextureHandle {
    let color = ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    );
    context.load_texture("portrait-preview", color, TextureOptions::LINEAR)
}

fn refresh_workspace_preview(context: &egui::Context, workspace: &mut PortraitWorkspace) {
    if workspace.state != PortraitState::Ready
        || workspace.last_preview_revision == workspace.preview_revision
    {
        return;
    }
    let (source, scale) = downscaled_source(&workspace.source);
    let result = if let Some(face) = workspace.faces.get(workspace.selected).cloned() {
        let Some(settings) = workspace.selected_settings().cloned() else {
            return;
        };
        let face = scaled_face(&face, scale);
        (|| -> Result<RgbaImage> {
            let masks = build_portrait_masks(&source, &face, [source.width(), source.height()])?;
            let mut output = source.clone();
            apply_portrait_to_rgba8(&source, &mut output, &masks, &settings)?;
            let output = apply_face_shape(&output, &face, &settings)?;
            if let Some(liquify) = &workspace.liquify {
                apply_strokes_normalized(&output, &liquify.strokes)
            } else {
                Ok(output)
            }
        })()
    } else if let Some(liquify) = &workspace.liquify {
        apply_strokes_normalized(&source, &liquify.strokes)
    } else {
        Ok(source)
    };
    match result {
        Ok(image) => {
            workspace.preview = Some(preview_texture(context, &image));
            workspace.error = None;
        }
        Err(error) => workspace.error = Some(format!("{error:#}")),
    }
    workspace.last_preview_revision = workspace.preview_revision;
}

impl PortraitWorkspace {
    pub fn analyzing_with_source(source: RgbaImage) -> Self {
        let mut workspace = Self::analyzing();
        workspace.source = source;
        workspace.state = PortraitState::LoadingModels;
        workspace
    }
}

impl EditorApp {
    pub(super) fn open_portrait_workspace(&mut self, context: &egui::Context) {
        if let Some(job) = &self.portrait_job {
            job.cancel();
        }
        self.portrait_job = None;
        let Some(session) = self.session() else {
            self.error = Some(tr("Open an image first").into());
            return;
        };
        let Some(layer) = session.document.active() else {
            self.error = Some(tr("Select a pixel layer first").into());
            return;
        };
        if let Some(recipe) = layer.portrait.clone() {
            let id = layer.id;
            let index = session
                .document
                .layers
                .iter()
                .position(|candidate| candidate.id == id)
                .unwrap_or(0);
            let mut below = session.document.clone();
            below.layers.truncate(index);
            let source = xuan::render::render(&below);
            self.portrait = Some(PortraitWorkspace::from_adjustment(source, recipe, id));
            self.portrait_job = None;
            context.request_repaint();
            return;
        }
        let Some(storage) = &layer.pixels else {
            self.error = Some(tr("Select a pixel layer first").into());
            return;
        };
        let source = storage.rgba8().clone();
        self.portrait = Some(PortraitWorkspace::analyzing_with_source(source.clone()));
        self.portrait_job = Some(PortraitAnalysisJob::spawn(source, context.clone()));
        context.request_repaint();
    }

    pub(super) fn poll_portrait_job(&mut self, context: &egui::Context) {
        let Some(job) = &self.portrait_job else {
            return;
        };
        let Some(result) = job.poll() else {
            context.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        };
        self.portrait_job = None;
        let source = self
            .portrait
            .as_ref()
            .map(|workspace| workspace.source.clone())
            .unwrap_or_else(|| RgbaImage::new(1, 1));
        match PortraitWorkspace::from_analysis_with_source(result, source) {
            Ok(mut workspace) => {
                workspace.invalidate_preview();
                self.portrait = Some(workspace);
            }
            Err(error) => {
                self.portrait = None;
                self.error = Some(format!("{error:#}"));
            }
        }
        context.request_repaint();
    }

    fn cancel_portrait_workspace(&mut self) {
        if let Some(job) = &self.portrait_job {
            job.cancel();
        }
        self.portrait_job = None;
        if let Some(workspace) = &mut self.portrait {
            workspace.cancel();
        }
        self.portrait = None;
        self.status = tr("Portrait retouch cancelled").into();
    }

    fn apply_portrait_workspace(&mut self, workspace: &PortraitWorkspace) -> Result<()> {
        let selected_face = workspace
            .faces
            .get(workspace.selected)
            .map(|face| (face, workspace.selected_settings().cloned()));
        let has_portrait = if workspace.apply_to_all {
            workspace
                .settings
                .iter()
                .any(|settings| !settings.is_identity())
        } else {
            selected_face
                .as_ref()
                .and_then(|(_, settings)| settings.as_ref())
                .is_some_and(|settings| !settings.is_identity())
        };
        let has_liquify = workspace
            .liquify
            .as_ref()
            .is_some_and(|liquify| !liquify.strokes.is_empty());
        if !has_portrait && !has_liquify {
            self.portrait = None;
            self.portrait_job = None;
            self.status = tr("No portrait changes to apply").into();
            return Ok(());
        }
        let session = self.session_mut().context(tr("Open an image first"))?;
        session
            .history
            .begin(tr("Portrait Retouch"), &session.document);
        let mut document = session.document.clone();
        let result = (|| -> Result<()> {
            let width = document.width;
            let height = document.height;
            let longest = width.max(height) as f32;
            let liquify = workspace
                .liquify
                .as_ref()
                .map(|session| {
                    session
                        .strokes
                        .iter()
                        .map(|stroke| {
                            LiquifyStroke::new(
                                [
                                    stroke.from[0] * width as f32,
                                    stroke.from[1] * height as f32,
                                ],
                                [stroke.to[0] * width as f32, stroke.to[1] * height as f32],
                                LiquifyBrush {
                                    mode: stroke.mode,
                                    diameter: stroke.diameter / 900.0 * longest,
                                    strength: stroke.strength,
                                    hardness: stroke.hardness,
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let recipe = PortraitAdjustment {
                source_size: [width, height],
                faces: workspace.faces.clone(),
                settings: workspace.settings.clone(),
                selected: workspace.selected,
                apply_to_all: workspace.apply_to_all,
                liquify,
            };
            if let Some(id) = workspace.editing_layer {
                let layer = document
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == id)
                    .context(tr("Portrait layer is no longer available"))?;
                if layer.portrait.is_none() {
                    anyhow::bail!(tr("Portrait layer is no longer available"));
                }
                layer.portrait = Some(recipe);
            } else {
                if !document
                    .active()
                    .is_some_and(|layer| layer.pixels.is_some())
                {
                    anyhow::bail!(tr("Select a pixel layer"));
                }
                let mut layer = xuan::document::Layer::blank(tr("Portrait Retouch"), width, height);
                layer.portrait = Some(recipe);
                document.insert(layer);
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                session.document = document;
                session.document.promote_image_masks();
                session.history.commit();
                session.invalidate();
                self.portrait = None;
                self.portrait_job = None;
                self.status = tr("Portrait retouch applied").into();
                Ok(())
            }
            Err(error) => {
                session.history.cancel(&mut session.document);
                Err(error)
            }
        }
    }
}

impl EditorApp {
    pub(super) fn portrait_workspace(&mut self, context: &egui::Context) {
        let Some(mut workspace) = self.portrait.take() else {
            return;
        };
        if workspace.state == PortraitState::Ready {
            refresh_workspace_preview(context, &mut workspace);
            if workspace.original_preview.is_none() {
                let (source, _) = downscaled_source(&workspace.source);
                workspace.original_preview = Some(preview_texture(context, &source));
            }
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        let shift_compare =
            context.input(|input| input.modifiers.shift && input.key_down(egui::Key::P));
        egui::Window::new(tr("Portrait Retouch"))
            .default_width(430.0)
            .max_height(760.0)
            .open(&mut open)
            .show(context, |ui| match workspace.state {
                PortraitState::Idle => {}
                PortraitState::LoadingModels | PortraitState::Analyzing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr("Analyzing faces locally…"));
                    });
                    ui.label(tr("The local models are loaded on first use."));
                    if widgets::button(ui, tr("Cancel")).clicked() {
                        cancel = true;
                    }
                }
                PortraitState::Ready => {
                    if let Some(error) = &workspace.error {
                        ui.colored_label(egui::Color32::from_rgb(230, 110, 100), error);
                    }
                    if !workspace.faces.is_empty() {
                        let face_count = workspace.faces.len();
                        ui.horizontal_wrapped(|ui| {
                            for index in 0..face_count {
                                let selected = index == workspace.selected;
                                if ui
                                    .selectable_label(
                                        selected,
                                        format!("{} {}", tr("Face"), index + 1),
                                    )
                                    .clicked()
                                {
                                    workspace.selected = index;
                                    workspace.invalidate_preview();
                                }
                            }
                        });
                        ui.separator();
                        if let Some(settings) = workspace.selected_settings_mut() {
                            let mut changed = false;
                            ui.collapsing(tr("Skin"), |ui| {
                                changed |= portrait_slider(
                                    ui,
                                    tr("Smooth"),
                                    &mut settings.skin_smooth,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Texture"),
                                    &mut settings.skin_texture,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Even tone"),
                                    &mut settings.skin_tone,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Blemish"),
                                    &mut settings.blemish,
                                    0.0..=100.0,
                                    "%",
                                );
                            });
                            ui.collapsing(tr("Eyes & Lips"), |ui| {
                                changed |= portrait_slider(
                                    ui,
                                    tr("Eye brightness"),
                                    &mut settings.eye_brightness,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Eye clarity"),
                                    &mut settings.eye_clarity,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Dark circles"),
                                    &mut settings.dark_circles,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Teeth whitening"),
                                    &mut settings.teeth_whiten,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Lip tint"),
                                    &mut settings.lip_tint,
                                    0.0..=100.0,
                                    "%",
                                );
                            });
                            ui.collapsing(tr("Shape & features"), |ui| {
                                changed |= portrait_slider(
                                    ui,
                                    tr("Face width"),
                                    &mut settings.face_width,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Jaw"),
                                    &mut settings.jaw,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Chin"),
                                    &mut settings.chin,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Eye size"),
                                    &mut settings.eye_size,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Nose width"),
                                    &mut settings.nose_width,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    tr("Lip shape"),
                                    &mut settings.lip_shape,
                                    -100.0..=100.0,
                                    "",
                                );
                            });
                            if changed {
                                workspace.invalidate_preview();
                            }
                        }
                        ui.separator();
                        let mut liquify_changed = false;
                        if let Some(liquify) = &mut workspace.liquify {
                            ui.collapsing(tr("Manual liquify"), |ui| {
                                ui.horizontal(|ui| {
                                    for (mode, label) in [
                                        (LiquifyMode::Push, tr("Push")),
                                        (LiquifyMode::Pinch, tr("Pinch")),
                                        (LiquifyMode::Bloat, tr("Bloat")),
                                    ] {
                                        if ui
                                            .selectable_label(liquify.brush.mode == mode, label)
                                            .clicked()
                                        {
                                            liquify.brush.mode = mode;
                                        }
                                    }
                                });
                                let mut changed = false;
                                changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.diameter,
                                            8.0..=800.0,
                                        )
                                        .text(tr("Size"))
                                        .suffix(" px"),
                                    )
                                    .changed();
                                changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.strength,
                                            1.0..=100.0,
                                        )
                                        .text(tr("Strength"))
                                        .suffix("%"),
                                    )
                                    .changed();
                                changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.hardness,
                                            0.0..=100.0,
                                        )
                                        .text(tr("Hardness"))
                                        .suffix("%"),
                                    )
                                    .changed();
                                if changed {
                                    liquify_changed = true;
                                }
                            });
                        }
                        if liquify_changed {
                            workspace.invalidate_preview();
                        }
                        ui.separator();
                        let compare_button = ui.add(egui::Button::new(tr("Hold to compare")));
                        let hold_compare = compare_button.hovered()
                            && context.input(|input| input.pointer.primary_down());
                        let texture = if workspace.compare_original || shift_compare || hold_compare
                        {
                            workspace.original_preview.as_ref()
                        } else {
                            workspace.preview.as_ref()
                        };
                        if let Some(texture) = texture {
                            let size = texture.size_vec2();
                            let width = 380.0_f32.min(size.x);
                            let scale = width / size.x.max(1.0);
                            let display = vec2(width, size.y * scale);
                            let (rect, response) = ui.allocate_exact_size(display, Sense::drag());
                            ui.painter().image(
                                texture.id(),
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                            let mut stroke_added = false;
                            if let Some(liquify) = &mut workspace.liquify {
                                let pointer = context.input(|input| input.pointer.interact_pos());
                                if let Some(pointer) = pointer.filter(|point| rect.contains(*point))
                                {
                                    let normalized = [
                                        (pointer.x - rect.left()) / rect.width(),
                                        (pointer.y - rect.top()) / rect.height(),
                                    ];
                                    if response.drag_started() {
                                        liquify.dragging_from = Some(normalized);
                                    } else if response.dragged()
                                        && let Some(from) = liquify.dragging_from
                                        && (from[0] - normalized[0]).abs()
                                            + (from[1] - normalized[1]).abs()
                                            > 0.0005
                                    {
                                        liquify.strokes.push(NormalizedLiquifyStroke {
                                            from,
                                            to: normalized,
                                            mode: liquify.brush.mode,
                                            diameter: liquify.brush.diameter,
                                            strength: liquify.brush.strength,
                                            hardness: liquify.brush.hardness,
                                        });
                                        liquify.dragging_from = Some(normalized);
                                        stroke_added = true;
                                    }
                                    if response.drag_stopped() {
                                        liquify.dragging_from = None;
                                    }
                                }
                            }
                            if stroke_added {
                                workspace.invalidate_preview();
                            }
                        }
                        widgets::checkbox(
                            ui,
                            &mut workspace.compare_original,
                            tr("Show original (hold Shift+P)"),
                        );
                        ui.separator();
                        ui.horizontal(|ui| {
                            if widgets::primary_button(ui, tr("Apply")).clicked() {
                                apply = true;
                            }
                            if widgets::button(ui, tr("Cancel")).clicked() {
                                cancel = true;
                            }
                            let mut reset_liquify = false;
                            if let Some(liquify) = &mut workspace.liquify
                                && widgets::button(ui, tr("Reset liquify")).clicked()
                            {
                                liquify.strokes.clear();
                                reset_liquify = true;
                            }
                            if reset_liquify {
                                workspace.invalidate_preview();
                            }
                        });
                    } else {
                        ui.label(tr("No face detected. Manual liquify is still available."));
                        let mut liquify_changed = false;
                        if let Some(liquify) = &mut workspace.liquify {
                            ui.collapsing(tr("Manual liquify"), |ui| {
                                ui.horizontal(|ui| {
                                    for (mode, label) in [
                                        (LiquifyMode::Push, tr("Push")),
                                        (LiquifyMode::Pinch, tr("Pinch")),
                                        (LiquifyMode::Bloat, tr("Bloat")),
                                    ] {
                                        if ui
                                            .selectable_label(liquify.brush.mode == mode, label)
                                            .clicked()
                                        {
                                            liquify.brush.mode = mode;
                                        }
                                    }
                                });
                                liquify_changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.diameter,
                                            8.0..=800.0,
                                        )
                                        .text(tr("Size"))
                                        .suffix(" px"),
                                    )
                                    .changed();
                                liquify_changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.strength,
                                            1.0..=100.0,
                                        )
                                        .text(tr("Strength"))
                                        .suffix("%"),
                                    )
                                    .changed();
                                liquify_changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.hardness,
                                            0.0..=100.0,
                                        )
                                        .text(tr("Hardness"))
                                        .suffix("%"),
                                    )
                                    .changed();
                            });
                        }
                        if liquify_changed {
                            workspace.invalidate_preview();
                        }
                        ui.separator();
                        let compare_button = ui.add(egui::Button::new(tr("Hold to compare")));
                        let hold_compare = compare_button.hovered()
                            && context.input(|input| input.pointer.primary_down());
                        let texture = if workspace.compare_original || shift_compare || hold_compare
                        {
                            workspace.original_preview.as_ref()
                        } else {
                            workspace.preview.as_ref()
                        };
                        if let Some(texture) = texture {
                            let size = texture.size_vec2();
                            let width = 380.0_f32.min(size.x);
                            let scale = width / size.x.max(1.0);
                            let display = vec2(width, size.y * scale);
                            let (rect, response) = ui.allocate_exact_size(display, Sense::drag());
                            ui.painter().image(
                                texture.id(),
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                            let mut stroke_added = false;
                            if let Some(liquify) = &mut workspace.liquify {
                                let pointer = context.input(|input| input.pointer.interact_pos());
                                if let Some(pointer) = pointer.filter(|point| rect.contains(*point))
                                {
                                    let normalized = [
                                        (pointer.x - rect.left()) / rect.width(),
                                        (pointer.y - rect.top()) / rect.height(),
                                    ];
                                    if response.drag_started() {
                                        liquify.dragging_from = Some(normalized);
                                    } else if response.dragged()
                                        && let Some(from) = liquify.dragging_from
                                        && (from[0] - normalized[0]).abs()
                                            + (from[1] - normalized[1]).abs()
                                            > 0.0005
                                    {
                                        liquify.strokes.push(NormalizedLiquifyStroke {
                                            from,
                                            to: normalized,
                                            mode: liquify.brush.mode,
                                            diameter: liquify.brush.diameter,
                                            strength: liquify.brush.strength,
                                            hardness: liquify.brush.hardness,
                                        });
                                        liquify.dragging_from = Some(normalized);
                                        stroke_added = true;
                                    }
                                    if response.drag_stopped() {
                                        liquify.dragging_from = None;
                                    }
                                }
                            }
                            if stroke_added {
                                workspace.invalidate_preview();
                            }
                        }
                        widgets::checkbox(
                            ui,
                            &mut workspace.compare_original,
                            tr("Show original (hold Shift+P)"),
                        );
                        ui.separator();
                        ui.horizontal(|ui| {
                            if widgets::primary_button(ui, tr("Apply")).clicked() {
                                apply = true;
                            }
                            if widgets::button(ui, tr("Cancel")).clicked() {
                                cancel = true;
                            }
                            let mut reset_liquify = false;
                            if let Some(liquify) = &mut workspace.liquify
                                && widgets::button(ui, tr("Reset liquify")).clicked()
                            {
                                liquify.strokes.clear();
                                reset_liquify = true;
                            }
                            if reset_liquify {
                                workspace.invalidate_preview();
                            }
                        });
                    }
                }
            });
        if apply {
            match self.apply_portrait_workspace(&workspace) {
                Ok(()) => return,
                Err(error) => {
                    self.error = Some(format!("{error:#}"));
                    self.portrait = Some(workspace);
                    return;
                }
            }
        }
        if cancel || !open {
            self.cancel_portrait_workspace();
            return;
        }
        self.portrait = Some(workspace);
    }
}

fn portrait_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) -> bool {
    ui.add(
        widgets::Slider::new(value, range)
            .text(label)
            .suffix(suffix),
    )
    .changed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use xuan::document::PixelStorage;

    #[test]
    fn no_face_workspace_can_apply_manual_liquify() {
        let context = egui::Context::default();
        let mut app = EditorApp::with_context(&context, Vec::new(), false, None);
        app.dimensions = [64, 64];
        app.new_document();
        let source =
            RgbaImage::from_fn(64, 64, |x, y| Rgba([(x * 4) as u8, (y * 4) as u8, 96, 255]));
        let session = app.session_mut().unwrap();
        session.document.active_mut().unwrap().pixels =
            Some(Arc::new(PixelStorage::from_rgba8(source.clone())));
        let mut workspace =
            PortraitWorkspace::from_analysis_with_source(Ok(Vec::new()), source).unwrap();
        workspace
            .liquify
            .as_mut()
            .unwrap()
            .strokes
            .push(NormalizedLiquifyStroke {
                from: [0.4, 0.4],
                to: [0.6, 0.6],
                mode: LiquifyMode::Push,
                diameter: 120.0,
                strength: 80.0,
                hardness: 70.0,
            });
        app.apply_portrait_workspace(&workspace).unwrap();
        assert_eq!(app.session().unwrap().history.names().count(), 1);
    }

    #[test]
    fn active_portrait_layer_reopens_without_reanalysis() {
        let context = egui::Context::default();
        let mut app = EditorApp::with_context(&context, Vec::new(), false, None);
        app.dimensions = [32, 32];
        app.new_document();
        let session = app.session_mut().unwrap();
        let source = RgbaImage::from_pixel(32, 32, Rgba([20, 40, 60, 255]));
        session.document.active_mut().unwrap().pixels =
            Some(Arc::new(PixelStorage::from_rgba8(source)));
        let mut portrait_layer = xuan::document::Layer::blank("Portrait", 32, 32);
        portrait_layer.portrait = Some(PortraitAdjustment {
            source_size: [32, 32],
            faces: Vec::new(),
            settings: Vec::new(),
            selected: 0,
            apply_to_all: false,
            liquify: Vec::new(),
        });
        let id = portrait_layer.id;
        session.document.layers.push(portrait_layer);
        session.document.select(id, false);
        app.open_portrait_workspace(&context);
        assert!(app.portrait_job.is_none());
        let workspace = app.portrait.as_ref().expect("portrait workspace");
        assert_eq!(workspace.editing_layer, Some(id));
    }

    #[test]
    fn identity_workspace_does_not_create_history_step() {
        let context = egui::Context::default();
        let mut app = EditorApp::with_context(&context, Vec::new(), false, None);
        app.dimensions = [32, 32];
        app.new_document();
        let source = RgbaImage::from_pixel(32, 32, Rgba([20, 40, 60, 255]));
        let workspace =
            PortraitWorkspace::from_analysis_with_source(Ok(Vec::new()), source).unwrap();
        app.apply_portrait_workspace(&workspace).unwrap();
        assert_eq!(app.session().unwrap().history.names().count(), 0);
    }
}
