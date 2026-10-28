use anyhow::Result;
use image::{Rgba, RgbaImage};

use super::retouch::PortraitSettings;
use super::types::FaceLandmarks;

#[derive(Clone, Debug)]
pub struct WarpGrid {
    size: [u32; 2],
    cell_size: f32,
    cols: usize,
    rows: usize,
    displacements: Vec<[f32; 2]>,
}

impl WarpGrid {
    pub fn identity(size: [u32; 2], cell_size: u32) -> Self {
        let cell_size = cell_size.max(1) as f32;
        let cols = (size[0] as f32 / cell_size).ceil() as usize + 1;
        let rows = (size[1] as f32 / cell_size).ceil() as usize + 1;
        Self {
            size,
            cell_size,
            cols,
            rows,
            displacements: vec![[0.0; 2]; cols * rows],
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn displacement_at(&self, point: [f32; 2]) -> [f32; 2] {
        let gx = (point[0] / self.cell_size).clamp(0.0, (self.cols - 1) as f32);
        let gy = (point[1] / self.cell_size).clamp(0.0, (self.rows - 1) as f32);
        let x0 = gx.floor() as usize;
        let y0 = gy.floor() as usize;
        let x1 = (x0 + 1).min(self.cols - 1);
        let y1 = (y0 + 1).min(self.rows - 1);
        let tx = gx - x0 as f32;
        let ty = gy - y0 as f32;
        let p00 = self.displacements[y0 * self.cols + x0];
        let p10 = self.displacements[y0 * self.cols + x1];
        let p01 = self.displacements[y1 * self.cols + x0];
        let p11 = self.displacements[y1 * self.cols + x1];
        let top = lerp2(p00, p10, tx);
        let bottom = lerp2(p01, p11, tx);
        lerp2(top, bottom, ty)
    }

    fn add_displacement(&mut self, center: [f32; 2], radius: f32, delta: [f32; 2], strength: f32) {
        if radius <= 0.0 || strength == 0.0 {
            return;
        }
        for row in 0..self.rows {
            for col in 0..self.cols {
                let point = [col as f32 * self.cell_size, row as f32 * self.cell_size];
                let dx = point[0] - center[0];
                let dy = point[1] - center[1];
                let distance = (dx * dx + dy * dy).sqrt();
                if distance >= radius {
                    continue;
                }
                let t = 1.0 - distance / radius;
                let weight = t * t * (3.0 - 2.0 * t) * strength;
                let slot = &mut self.displacements[row * self.cols + col];
                slot[0] += delta[0] * weight;
                slot[1] += delta[1] * weight;
            }
        }
    }

    fn add_radial(&mut self, center: [f32; 2], radius: f32, amount: f32, strength: f32) {
        for row in 0..self.rows {
            for col in 0..self.cols {
                let point = [col as f32 * self.cell_size, row as f32 * self.cell_size];
                let dx = point[0] - center[0];
                let dy = point[1] - center[1];
                let distance = (dx * dx + dy * dy).sqrt();
                if distance >= radius || distance < 0.001 {
                    continue;
                }
                let t = 1.0 - distance / radius;
                let weight = t * t * (3.0 - 2.0 * t) * strength;
                let slot = &mut self.displacements[row * self.cols + col];
                slot[0] += dx / distance * amount * weight;
                slot[1] += dy / distance * amount * weight;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiquifyMode {
    Push,
    Pinch,
    Bloat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquifyBrush {
    pub mode: LiquifyMode,
    pub diameter: f32,
    pub strength: f32,
    pub hardness: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquifyStroke {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub brush: LiquifyBrush,
}

impl LiquifyStroke {
    pub fn push(from: [f32; 2], to: [f32; 2], diameter: f32, strength: f32, hardness: f32) -> Self {
        Self {
            from,
            to,
            brush: LiquifyBrush {
                mode: LiquifyMode::Push,
                diameter,
                strength,
                hardness,
            },
        }
    }

    pub fn new(from: [f32; 2], to: [f32; 2], brush: LiquifyBrush) -> Self {
        Self { from, to, brush }
    }
}

pub fn apply_liquify(
    source: &RgbaImage,
    grid: &WarpGrid,
    stroke: LiquifyStroke,
) -> Result<RgbaImage> {
    apply_liquify_many(source, grid, &[stroke])
}

/// Compose several liquify strokes into one grid before sampling the source.
/// This avoids re-warping the entire preview once per mouse/pen segment.
pub fn apply_liquify_many(
    source: &RgbaImage,
    grid: &WarpGrid,
    strokes: &[LiquifyStroke],
) -> Result<RgbaImage> {
    let mut grid = grid.clone();
    for stroke in strokes {
        let radius = (stroke.brush.diameter * 0.5).max(1.0);
        let hardness = (stroke.brush.hardness / 100.0).clamp(0.0, 1.0);
        let strength = ((stroke.brush.strength / 100.0) * (0.5 + hardness * 0.5)).clamp(0.0, 1.0);
        match stroke.brush.mode {
            LiquifyMode::Push => {
                let delta = [stroke.to[0] - stroke.from[0], stroke.to[1] - stroke.from[1]];
                grid.add_displacement(stroke.from, radius, delta, strength);
            }
            LiquifyMode::Pinch | LiquifyMode::Bloat => {
                let amount = radius * 0.25 * strength;
                let signed = if stroke.brush.mode == LiquifyMode::Pinch {
                    -amount
                } else {
                    amount
                };
                grid.add_radial(stroke.from, radius, signed, strength);
            }
        }
    }
    Ok(apply_warp(source, &grid))
}

pub fn apply_face_shape(
    source: &RgbaImage,
    landmarks: &FaceLandmarks,
    settings: &PortraitSettings,
) -> Result<RgbaImage> {
    if [
        settings.face_width,
        settings.jaw,
        settings.chin,
        settings.eye_size,
        settings.nose_width,
        settings.lip_shape,
    ]
    .iter()
    .all(|value| *value == 0.0)
    {
        return Ok(source.clone());
    }
    if landmarks.points.len() != 478 {
        anyhow::bail!(
            "Face landmarks contain {} points, expected 478",
            landmarks.points.len()
        );
    }
    let cell = (source.width().min(source.height()) / 32).clamp(8, 64);
    let mut grid = WarpGrid::identity([source.width(), source.height()], cell);
    let face_width = landmarks.face.rect[2].max(1.0);
    let face_height = landmarks.face.rect[3].max(1.0);
    let radius = face_width * 0.22;
    let displace = |grid: &mut WarpGrid, index: usize, delta: [f32; 2], amount: f32| {
        if amount == 0.0 {
            return;
        }
        if let Some(point) = landmarks.points.get(index) {
            grid.add_displacement(point.position, radius, delta, amount / 100.0);
        }
    };
    displace(
        &mut grid,
        234,
        [-face_width * 0.08, 0.0],
        settings.face_width,
    );
    displace(
        &mut grid,
        454,
        [face_width * 0.08, 0.0],
        settings.face_width,
    );
    displace(&mut grid, 172, [face_width * 0.06, 0.0], settings.jaw);
    displace(&mut grid, 397, [-face_width * 0.06, 0.0], settings.jaw);
    displace(&mut grid, 152, [0.0, face_height * 0.06], settings.chin);
    displace(
        &mut grid,
        129,
        [-face_width * 0.03, 0.0],
        settings.nose_width,
    );
    displace(
        &mut grid,
        358,
        [face_width * 0.03, 0.0],
        settings.nose_width,
    );
    displace(&mut grid, 61, [-face_width * 0.04, 0.0], settings.lip_shape);
    displace(&mut grid, 291, [face_width * 0.04, 0.0], settings.lip_shape);
    let eye_radius = face_width * 0.11;
    let eye_amount = face_width * 0.025 * settings.eye_size / 100.0;
    for index in [33, 263] {
        if let Some(point) = landmarks.points.get(index) {
            grid.add_radial(point.position, eye_radius, eye_amount, 1.0);
        }
    }
    Ok(apply_warp(source, &grid))
}

fn apply_warp(source: &RgbaImage, grid: &WarpGrid) -> RgbaImage {
    RgbaImage::from_fn(source.width(), source.height(), |x, y| {
        let destination = [x as f32, y as f32];
        let displacement = grid.displacement_at(destination);
        let sample = [
            destination[0] - displacement[0],
            destination[1] - displacement[1],
        ];
        sample_rgba(source, sample)
    })
}

fn sample_rgba(image: &RgbaImage, point: [f32; 2]) -> Rgba<u8> {
    let x = point[0].clamp(0.0, image.width().saturating_sub(1) as f32);
    let y = point[1].clamp(0.0, image.height().saturating_sub(1) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(image.width() - 1);
    let y1 = (y0 + 1).min(image.height() - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = image.get_pixel(x0, y0);
    let p10 = image.get_pixel(x1, y0);
    let p01 = image.get_pixel(x0, y1);
    let p11 = image.get_pixel(x1, y1);
    let mut output = [0u8; 4];
    for channel in 0..4 {
        let top = p00[channel] as f32 * (1.0 - tx) + p10[channel] as f32 * tx;
        let bottom = p01[channel] as f32 * (1.0 - tx) + p11[channel] as f32 * tx;
        output[channel] = (top * (1.0 - ty) + bottom * ty).round() as u8;
    }
    Rgba(output)
}

fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portrait::retouch::PortraitSettings;
    use crate::portrait::test_fixtures::{synthetic_landmarks, synthetic_rgba8};

    #[test]
    fn zero_warp_is_identity() {
        let source = synthetic_rgba8(64, 64);
        let warped = apply_face_shape(
            &source,
            &synthetic_landmarks(),
            &PortraitSettings::default(),
        )
        .unwrap();
        assert_eq!(warped.as_raw(), source.as_raw());
    }

    #[test]
    fn push_warp_has_no_holes_or_out_of_bounds_samples() {
        let source = synthetic_rgba8(64, 64);
        let grid = WarpGrid::identity([64, 64], 32);
        let moved = apply_liquify(
            &source,
            &grid,
            LiquifyStroke::push([32.0, 32.0], [8.0, 0.0], 20.0, 100.0, 80.0),
        )
        .unwrap();
        assert_eq!(moved.dimensions(), source.dimensions());
        assert!(moved.pixels().all(|pixel| pixel[3] == 255));
    }
}
