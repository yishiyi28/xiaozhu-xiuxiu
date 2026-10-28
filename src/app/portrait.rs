use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

use anyhow::{Context, Result};
use egui::{ColorImage, Sense, TextureHandle, TextureOptions, vec2};
use image::{Rgba, RgbaImage};
use xuan::{
    document::{PixelStorage, Rgba16Image},
    portrait::{
        inference::PortraitModels,
        masks::build_portrait_masks,
        preset::{FacePolicy, PresetV1, apply_preset_to_document},
        retouch::{PortraitSettings, apply_portrait_to_rgba8},
        types::FaceLandmarks,
        warp::{
            LiquifyBrush, LiquifyMode, LiquifyStroke, WarpGrid, apply_face_shape, apply_liquify,
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
    pub settings: Vec<PortraitSettings>,
    pub preview: Option<TextureHandle>,
    original_preview: Option<TextureHandle>,
    pub compare_original: bool,
    pub error: Option<String>,
    pub liquify: Option<LiquifySession>,
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
            settings: Vec::new(),
            preview: None,
            original_preview: None,
            compare_original: false,
            error: None,
            liquify: Some(LiquifySession::default()),
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
                workspace.error = Some("未检测到人脸".into());
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

    pub fn cancel(&mut self) {
        self.state = PortraitState::Idle;
        self.faces.clear();
        self.settings.clear();
        self.preview = None;
        self.original_preview = None;
        self.error = None;
        self.liquify = None;
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
            let result = PortraitModels::load()
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
            Err(TryRecvError::Disconnected) => Some(Err("人像分析线程意外停止".into())),
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
    let mut current = source.clone();
    for stroke in strokes {
        let grid = WarpGrid::identity([current.width(), current.height()], cell);
        let from = [
            stroke.from[0] * source.width() as f32,
            stroke.from[1] * source.height() as f32,
        ];
        let to = [
            stroke.to[0] * source.width() as f32,
            stroke.to[1] * source.height() as f32,
        ];
        let liquify = LiquifyStroke::new(
            from,
            to,
            LiquifyBrush {
                mode: stroke.mode,
                diameter: stroke.diameter / 900.0 * longest,
                strength: stroke.strength,
                hardness: stroke.hardness,
            },
        );
        current = apply_liquify(&current, &grid, liquify)?;
    }
    Ok(current)
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
            self.error = Some("请先打开一张图片".into());
            return;
        };
        let Some(layer) = session.document.active() else {
            self.error = Some("请先选择一个像素图层".into());
            return;
        };
        let Some(storage) = &layer.pixels else {
            self.error = Some("请先选择一个像素图层".into());
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
        self.status = "已取消人像精修".into();
    }

    fn apply_portrait_workspace(&mut self, workspace: &PortraitWorkspace) -> Result<()> {
        let session = self.session_mut().context("请先打开一张图片")?;
        session.history.begin("人像精修", &session.document);
        let mut document = session.document.clone();
        let result = (|| -> Result<()> {
            if !workspace.faces.is_empty() {
                let settings = workspace
                    .selected_settings()
                    .context("请选择一张脸")?
                    .clone();
                let preset = PresetV1 {
                    schema_version: 1,
                    name: "人像精修".into(),
                    color: None,
                    portrait: Some(settings),
                };
                apply_preset_to_document(
                    &mut document,
                    &preset,
                    FacePolicy::Selected(workspace.selected),
                )?;
            }
            if let Some(liquify) = &workspace.liquify
                && !liquify.strokes.is_empty()
            {
                apply_liquify_to_active_layer(&mut document, &liquify.strokes)?;
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
                self.status = "已应用人像精修".into();
                Ok(())
            }
            Err(error) => {
                session.history.cancel(&mut session.document);
                Err(error)
            }
        }
    }
}

fn apply_liquify_to_active_layer(
    document: &mut xuan::document::Document,
    strokes: &[NormalizedLiquifyStroke],
) -> Result<()> {
    let layer = document.active_mut().context("请选择一个像素图层")?;
    let storage = layer
        .pixels
        .as_ref()
        .context("图层没有像素")?
        .as_ref()
        .clone();
    match storage {
        PixelStorage::Rgba8(source) => {
            let output = apply_strokes_normalized(&source, strokes)?;
            layer.pixels = Some(Arc::new(PixelStorage::from_rgba8(output)));
        }
        PixelStorage::Rgba16 { ref pixels, .. } => {
            let before = storage.rgba8().clone();
            let after = apply_strokes_normalized(&before, strokes)?;
            let mapped: Rgba16Image =
                image::ImageBuffer::from_fn(pixels.width(), pixels.height(), |x, y| {
                    let old = pixels.get_pixel(x, y);
                    let old8 = before.get_pixel(x, y);
                    let new8 = after.get_pixel(x, y);
                    let scale = |before: u8, after: u8, value: u16| -> u16 {
                        if before == 0 {
                            u16::from(after) * 257
                        } else {
                            (u32::from(value) * u32::from(after) / u32::from(before))
                                .min(u32::from(u16::MAX)) as u16
                        }
                    };
                    Rgba([
                        scale(old8[0], new8[0], old[0]),
                        scale(old8[1], new8[1], old[1]),
                        scale(old8[2], new8[2], old[2]),
                        old[3],
                    ])
                });
            layer.pixels = Some(Arc::new(PixelStorage::from_rgba16(mapped)));
        }
    }
    Ok(())
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
        egui::Window::new("人像精修")
            .default_width(430.0)
            .max_height(760.0)
            .open(&mut open)
            .show(context, |ui| match workspace.state {
                PortraitState::Idle => {}
                PortraitState::LoadingModels | PortraitState::Analyzing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("正在本地分析人脸…");
                    });
                    ui.label("首次使用需要加载本地模型。");
                    if widgets::button(ui, "取消").clicked() {
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
                                    .selectable_label(selected, format!("人脸 {}", index + 1))
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
                            ui.collapsing("皮肤", |ui| {
                                changed |= portrait_slider(
                                    ui,
                                    "平滑",
                                    &mut settings.skin_smooth,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "纹理",
                                    &mut settings.skin_texture,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "匀肤",
                                    &mut settings.skin_tone,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "祛痘",
                                    &mut settings.blemish,
                                    0.0..=100.0,
                                    "%",
                                );
                            });
                            ui.collapsing("眼睛与嘴唇", |ui| {
                                changed |= portrait_slider(
                                    ui,
                                    "眼睛亮度",
                                    &mut settings.eye_brightness,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "眼睛清晰度",
                                    &mut settings.eye_clarity,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "黑眼圈",
                                    &mut settings.dark_circles,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "牙齿美白",
                                    &mut settings.teeth_whiten,
                                    0.0..=100.0,
                                    "%",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "唇色强度",
                                    &mut settings.lip_tint,
                                    0.0..=100.0,
                                    "%",
                                );
                            });
                            ui.collapsing("脸型与五官", |ui| {
                                changed |= portrait_slider(
                                    ui,
                                    "脸宽",
                                    &mut settings.face_width,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "下颌",
                                    &mut settings.jaw,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "下巴",
                                    &mut settings.chin,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "眼睛大小",
                                    &mut settings.eye_size,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "鼻翼宽度",
                                    &mut settings.nose_width,
                                    -100.0..=100.0,
                                    "",
                                );
                                changed |= portrait_slider(
                                    ui,
                                    "嘴唇形状",
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
                            ui.collapsing("手动液化", |ui| {
                                ui.horizontal(|ui| {
                                    for (mode, label) in [
                                        (LiquifyMode::Push, "推移"),
                                        (LiquifyMode::Pinch, "收缩"),
                                        (LiquifyMode::Bloat, "膨胀"),
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
                                        .text("大小")
                                        .suffix(" px"),
                                    )
                                    .changed();
                                changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.strength,
                                            1.0..=100.0,
                                        )
                                        .text("强度")
                                        .suffix("%"),
                                    )
                                    .changed();
                                changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.hardness,
                                            0.0..=100.0,
                                        )
                                        .text("硬度")
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
                        let texture = if workspace.compare_original {
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
                        widgets::checkbox(ui, &mut workspace.compare_original, "查看原图");
                        ui.separator();
                        ui.horizontal(|ui| {
                            if widgets::primary_button(ui, "应用").clicked() {
                                apply = true;
                            }
                            if widgets::button(ui, "取消").clicked() {
                                cancel = true;
                            }
                            let mut reset_liquify = false;
                            if let Some(liquify) = &mut workspace.liquify
                                && widgets::button(ui, "重置液化").clicked()
                            {
                                liquify.strokes.clear();
                                reset_liquify = true;
                            }
                            if reset_liquify {
                                workspace.invalidate_preview();
                            }
                        });
                    } else {
                        ui.label("未检测到人脸，仍可使用手动液化。");
                        let mut liquify_changed = false;
                        if let Some(liquify) = &mut workspace.liquify {
                            ui.collapsing("手动液化", |ui| {
                                ui.horizontal(|ui| {
                                    for (mode, label) in [
                                        (LiquifyMode::Push, "推移"),
                                        (LiquifyMode::Pinch, "收缩"),
                                        (LiquifyMode::Bloat, "膨胀"),
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
                                        .text("大小")
                                        .suffix(" px"),
                                    )
                                    .changed();
                                liquify_changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.strength,
                                            1.0..=100.0,
                                        )
                                        .text("强度")
                                        .suffix("%"),
                                    )
                                    .changed();
                                liquify_changed |= ui
                                    .add(
                                        widgets::Slider::new(
                                            &mut liquify.brush.hardness,
                                            0.0..=100.0,
                                        )
                                        .text("硬度")
                                        .suffix("%"),
                                    )
                                    .changed();
                            });
                        }
                        if liquify_changed {
                            workspace.invalidate_preview();
                        }
                        ui.separator();
                        let texture = if workspace.compare_original {
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
                        widgets::checkbox(ui, &mut workspace.compare_original, "查看原图");
                        ui.separator();
                        ui.horizontal(|ui| {
                            if widgets::primary_button(ui, "应用").clicked() {
                                apply = true;
                            }
                            if widgets::button(ui, "取消").clicked() {
                                cancel = true;
                            }
                            let mut reset_liquify = false;
                            if let Some(liquify) = &mut workspace.liquify
                                && widgets::button(ui, "重置液化").clicked()
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
}
