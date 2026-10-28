use anyhow::{Result, ensure};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::document::{PixelStorage, Rgba16Image};

use super::{
    masks::build_portrait_masks,
    retouch::{PortraitSettings, apply_portrait_to_rgba8, apply_portrait_to_rgba16},
    types::FaceLandmarks,
    warp::{LiquifyStroke, WarpGrid, apply_face_shape, apply_liquify_many},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortraitAdjustment {
    pub source_size: [u32; 2],
    pub faces: Vec<FaceLandmarks>,
    pub settings: Vec<PortraitSettings>,
    pub selected: usize,
    #[serde(default)]
    pub apply_to_all: bool,
    #[serde(default)]
    pub liquify: Vec<LiquifyStroke>,
}

impl PortraitAdjustment {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.source_size[0] > 0 && self.source_size[1] > 0,
            "Portrait source size is empty"
        );
        ensure!(
            self.settings.len() == self.faces.len(),
            "Portrait face and settings counts differ"
        );
        ensure!(
            self.faces.is_empty() || self.selected < self.faces.len(),
            "Portrait selected face is out of range"
        );
        for settings in &self.settings {
            settings.validate()?;
        }
        ensure!(
            self.liquify.iter().all(stroke_is_finite),
            "Portrait liquify contains non-finite values"
        );
        Ok(())
    }

    pub fn scaled(&self, target: [u32; 2]) -> Self {
        let sx = target[0] as f32 / self.source_size[0] as f32;
        let sy = target[1] as f32 / self.source_size[1] as f32;
        let faces = self
            .faces
            .iter()
            .map(|face| {
                let mut face = face.clone();
                face.face.rect[0] *= sx;
                face.face.rect[1] *= sy;
                face.face.rect[2] *= sx;
                face.face.rect[3] *= sy;
                face.face.left_eye = [face.face.left_eye[0] * sx, face.face.left_eye[1] * sy];
                face.face.right_eye = [face.face.right_eye[0] * sx, face.face.right_eye[1] * sy];
                for point in &mut face.points {
                    point.position = [point.position[0] * sx, point.position[1] * sy];
                }
                face
            })
            .collect();
        let liquify = self
            .liquify
            .iter()
            .map(|stroke| {
                let mut stroke = *stroke;
                stroke.from = [stroke.from[0] * sx, stroke.from[1] * sy];
                stroke.to = [stroke.to[0] * sx, stroke.to[1] * sy];
                stroke.brush.diameter *= (sx + sy) * 0.5;
                stroke
            })
            .collect();
        Self {
            source_size: target,
            faces,
            settings: self.settings.clone(),
            selected: self.selected,
            apply_to_all: self.apply_to_all,
            liquify,
        }
    }

    pub fn apply_to_rgba8(&self, image: &RgbaImage) -> Result<RgbaImage> {
        let storage = PixelStorage::from_rgba8(image.clone());
        Ok(self.apply_to_storage(&storage)?.into_rgba8())
    }

    pub fn apply_to_rgba16(&self, image: &Rgba16Image) -> Result<Rgba16Image> {
        let storage = PixelStorage::from_rgba16(image.clone());
        match self.apply_to_storage(&storage)? {
            PixelStorage::Rgba16 { pixels, .. } => Ok((*pixels).clone()),
            PixelStorage::Rgba8(_) => unreachable!("portrait 16-bit input produced 8-bit storage"),
        }
    }

    pub fn apply_to_storage(&self, storage: &PixelStorage) -> Result<PixelStorage> {
        self.validate()?;
        let (width, height) = storage.rgba8().dimensions();
        let recipe = if [width, height] == self.source_size {
            self.clone()
        } else {
            self.scaled([width, height])
        };
        let mut current = storage.clone();
        let indices: Vec<usize> = if recipe.apply_to_all {
            (0..recipe.faces.len()).collect()
        } else if recipe.faces.is_empty() {
            Vec::new()
        } else {
            vec![recipe.selected]
        };
        for index in indices {
            let settings = &recipe.settings[index];
            if settings.is_identity() {
                continue;
            }
            current =
                apply_portrait_to_storage_with_faces(&current, &[&recipe.faces[index]], settings)?;
        }
        if !recipe.liquify.is_empty() {
            current = apply_liquify_to_storage(&current, &recipe.liquify)?;
        }
        Ok(current)
    }
}

pub fn apply_portrait_to_storage_with_faces(
    storage: &PixelStorage,
    faces: &[&FaceLandmarks],
    settings: &PortraitSettings,
) -> Result<PixelStorage> {
    settings.validate()?;
    if settings.is_identity() || faces.is_empty() {
        return Ok(storage.clone());
    }
    match storage {
        PixelStorage::Rgba8(source) => {
            let mut current = (**source).clone();
            for face in faces {
                let masks =
                    build_portrait_masks(&current, face, [current.width(), current.height()])?;
                let mut output = current.clone();
                apply_portrait_to_rgba8(&current, &mut output, &masks, settings)?;
                if has_reshape(settings) {
                    output = apply_face_shape(&output, face, settings)?;
                }
                current = output;
            }
            Ok(PixelStorage::from_rgba8(current))
        }
        PixelStorage::Rgba16 { pixels, .. } => {
            let mut current = (**pixels).clone();
            let mut preview = storage.rgba8().clone();
            for face in faces {
                let masks =
                    build_portrait_masks(&preview, face, [preview.width(), preview.height()])?;
                current = apply_portrait_to_rgba16(&current, &masks, settings)?;
                preview = to_rgba8(&current);
            }
            if has_reshape(settings) {
                current = warp_rgba16_preserving_depth(&current, faces, settings)?;
            }
            Ok(PixelStorage::from_rgba16(current))
        }
    }
}

fn apply_liquify_to_storage(
    storage: &PixelStorage,
    strokes: &[LiquifyStroke],
) -> Result<PixelStorage> {
    if strokes.is_empty() {
        return Ok(storage.clone());
    }
    let preview = storage.rgba8();
    let cell = (preview.width().min(preview.height()) / 32).clamp(8, 64);
    let grid = WarpGrid::identity([preview.width(), preview.height()], cell);
    let after = apply_liquify_many(preview, &grid, strokes)?;
    match storage {
        PixelStorage::Rgba8(_) => Ok(PixelStorage::from_rgba8(after)),
        PixelStorage::Rgba16 { pixels, .. } => {
            let before = preview;
            let mapped = image::ImageBuffer::from_fn(pixels.width(), pixels.height(), |x, y| {
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
            Ok(PixelStorage::from_rgba16(mapped))
        }
    }
}

fn has_reshape(settings: &PortraitSettings) -> bool {
    [
        settings.face_width,
        settings.jaw,
        settings.chin,
        settings.eye_size,
        settings.nose_width,
        settings.lip_shape,
    ]
    .iter()
    .any(|value| *value != 0.0)
}

fn warp_rgba16_preserving_depth(
    source: &Rgba16Image,
    faces: &[&FaceLandmarks],
    settings: &PortraitSettings,
) -> Result<Rgba16Image> {
    let before = to_rgba8(source);
    let mut after = before.clone();
    for face in faces {
        after = apply_face_shape(&after, face, settings)?;
    }
    Ok(image::ImageBuffer::from_fn(
        source.width(),
        source.height(),
        |x, y| {
            let old = source.get_pixel(x, y);
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
        },
    ))
}

fn to_rgba8(source: &Rgba16Image) -> RgbaImage {
    RgbaImage::from_fn(source.width(), source.height(), |x, y| {
        let pixel = source.get_pixel(x, y);
        Rgba([
            (pixel[0] >> 8) as u8,
            (pixel[1] >> 8) as u8,
            (pixel[2] >> 8) as u8,
            (pixel[3] >> 8) as u8,
        ])
    })
}

fn stroke_is_finite(stroke: &LiquifyStroke) -> bool {
    stroke
        .from
        .iter()
        .chain(&stroke.to)
        .all(|value| value.is_finite())
        && stroke.brush.diameter.is_finite()
        && stroke.brush.strength.is_finite()
        && stroke.brush.hardness.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Layer, PixelStorage};
    use crate::portrait::test_fixtures;
    use image::{Rgba, RgbaImage};

    fn liquify_recipe() -> PortraitAdjustment {
        PortraitAdjustment {
            source_size: [64, 64],
            faces: Vec::new(),
            settings: Vec::new(),
            selected: 0,
            apply_to_all: false,
            liquify: vec![LiquifyStroke::push(
                [20.0, 20.0],
                [30.0, 30.0],
                20.0,
                80.0,
                60.0,
            )],
        }
    }

    #[test]
    fn applies_liquify_recipe_without_losing_dimensions() {
        let source = test_fixtures::synthetic_rgba8(64, 64);
        let output = liquify_recipe().apply_to_rgba8(&source).unwrap();
        assert_eq!(source.dimensions(), output.dimensions());
        assert_ne!(source, output);
    }

    #[test]
    fn scales_face_coordinates_for_preview_size() {
        let mut recipe = liquify_recipe();
        recipe.faces = vec![test_fixtures::synthetic_landmarks()];
        recipe.settings = vec![PortraitSettings::default()];
        let scaled = recipe.scaled([128, 96]);
        assert_eq!(scaled.source_size, [128, 96]);
        assert_eq!(scaled.faces[0].face.rect, [16.0, 12.0, 112.0, 84.0]);
    }

    #[test]
    fn portrait_layer_round_trips_through_document_json() {
        let mut document = Document::new(64, 64).unwrap();
        let layer = Layer::from_storage(
            "Photo",
            PixelStorage::from_rgba8(RgbaImage::from_pixel(64, 64, Rgba([180, 130, 110, 255]))),
        );
        let mut portrait_layer = Layer::blank("Portrait", 64, 64);
        portrait_layer.portrait = Some(liquify_recipe());
        document.layers = vec![layer, portrait_layer];
        let json = serde_json::to_string(&document).unwrap();
        let decoded: Document = serde_json::from_str(&json).unwrap();
        assert!(decoded.layers[1].portrait.is_some());
        assert!(decoded.layers[0].portrait.is_none());
    }

    #[test]
    fn rendering_portrait_layer_does_not_mutate_source_layer() {
        let source = RgbaImage::from_fn(64, 64, |x, y| {
            Rgba([(x * 4) as u8, (y * 4) as u8, 120, 255])
        });
        let base = Layer::from_storage("Photo", PixelStorage::from_rgba8(source.clone()));
        let mut portrait_layer = Layer::blank("Portrait", 64, 64);
        portrait_layer.portrait = Some(liquify_recipe());
        let mut document = Document::new(64, 64).unwrap();
        document.layers = vec![base, portrait_layer];
        let rendered = crate::render::render(&document);
        assert_ne!(rendered, source);
        assert_eq!(document.layers[0].pixels.as_ref().unwrap().rgba8(), &source);
    }
}
