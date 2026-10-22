use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::document::Point;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhiteBalance {
    #[default]
    AsShot,
    Temperature,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayKind {
    #[default]
    Linear,
    Radial,
    Brush,
}

/// Overlay coordinates refer to the uncropped, oriented image, in 0..1 units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overlay {
    pub name: String,
    pub enabled: bool,
    pub kind: OverlayKind,
    pub start: Point,
    pub end: Point,
    pub radius: f32,
    pub feather: f32,
    pub invert: bool,
    pub points: Vec<Point>,
    pub exposure: f32,
    pub warmth: f32,
    pub saturation: f32,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            name: "Local adjustment".into(),
            enabled: true,
            kind: OverlayKind::Linear,
            start: Point::new(0.5, 0.2),
            end: Point::new(0.5, 0.7),
            radius: 0.08,
            feather: 0.7,
            invert: false,
            points: Vec::new(),
            exposure: 0.0,
            warmth: 0.0,
            saturation: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DevelopSettings {
    pub negative: super::NegativeSettings,
    pub white_balance: WhiteBalance,
    pub temperature: f32,
    pub tint: f32,
    pub custom_wb: [f32; 3],
    pub exposure: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub saturation: f32,
    pub vibrance: f32,
    pub clarity: f32,
    pub texture: f32,
    pub dehaze: f32,
    /// Master, red, green, blue; five evenly spaced curve knots each.
    pub curves: [[f32; 5]; 4],
    /// Eight hue bands, each containing hue shift, saturation and lightness.
    pub hsl: [[f32; 3]; 8],
    pub monochrome: bool,
    pub bw_mix: [f32; 3],
    /// Hue (degrees), saturation (percent).
    pub shadow_tone: [f32; 2],
    pub highlight_tone: [f32; 2],
    pub tone_balance: f32,
    pub luminance_noise: f32,
    pub color_noise: f32,
    pub sharpen: f32,
    pub sharpen_radius: f32,
    pub sharpen_threshold: f32,
    pub distortion: f32,
    pub chromatic_red: f32,
    pub chromatic_blue: f32,
    pub defringe: f32,
    pub vignette: f32,
    /// Clockwise quarter turns applied after development and cropping.
    pub quarter_turns: u8,
    /// Fine straightening in degrees, before cropping.
    pub rotation: f32,
    pub perspective: [f32; 2],
    /// Left, top, right, bottom in normalized oriented image coordinates.
    pub crop: [f32; 4],
    pub overlays: Vec<Overlay>,
}

impl Default for DevelopSettings {
    fn default() -> Self {
        Self {
            negative: Default::default(),
            white_balance: WhiteBalance::AsShot,
            temperature: 6500.0,
            tint: 0.0,
            custom_wb: [1.0; 3],
            exposure: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            saturation: 0.0,
            vibrance: 0.0,
            clarity: 0.0,
            texture: 0.0,
            dehaze: 0.0,
            curves: [[0.0, 0.25, 0.5, 0.75, 1.0]; 4],
            hsl: [[0.0; 3]; 8],
            monochrome: false,
            bw_mix: [0.2126, 0.7152, 0.0722],
            shadow_tone: [220.0, 0.0],
            highlight_tone: [45.0, 0.0],
            tone_balance: 0.0,
            luminance_noise: 0.0,
            color_noise: 20.0,
            sharpen: 25.0,
            sharpen_radius: 1.0,
            sharpen_threshold: 0.01,
            distortion: 0.0,
            chromatic_red: 0.0,
            chromatic_blue: 0.0,
            defringe: 0.0,
            vignette: 0.0,
            quarter_turns: 0,
            rotation: 0.0,
            perspective: [0.0; 2],
            crop: [0.0, 0.0, 1.0, 1.0],
            overlays: Vec::new(),
        }
    }
}

fn range(value: f32, min: f32, max: f32) -> Result<()> {
    ensure!(
        value.is_finite() && (min..=max).contains(&value),
        "Invalid RAW development setting"
    );
    Ok(())
}

impl DevelopSettings {
    pub fn crop_pixels(&self, size: [u32; 2]) -> [u32; 4] {
        [
            (self.crop[0] * size[0] as f32).floor() as u32,
            (self.crop[1] * size[1] as f32).floor() as u32,
            (self.crop[2] * size[0] as f32).ceil().min(size[0] as f32) as u32,
            (self.crop[3] * size[1] as f32).ceil().min(size[1] as f32) as u32,
        ]
    }

    pub fn output_size(&self, source: [u32; 2]) -> [u32; 2] {
        let [left, top, right, bottom] = self.crop_pixels(source);
        if self.quarter_turns.is_multiple_of(2) {
            [right - left, bottom - top]
        } else {
            [bottom - top, right - left]
        }
    }

    /// Map an uncropped development coordinate to the rotated, cropped display.
    pub fn display_point(&self, point: Point) -> Point {
        rotate_point(
            Point::new(
                (point.x - self.crop[0]) / (self.crop[2] - self.crop[0]),
                (point.y - self.crop[1]) / (self.crop[3] - self.crop[1]),
            ),
            self.quarter_turns,
        )
    }

    pub fn image_point(&self, point: Point) -> Point {
        let point = rotate_point(point, (4 - self.quarter_turns) % 4);
        Point::new(
            self.crop[0] + point.x * (self.crop[2] - self.crop[0]),
            self.crop[1] + point.y * (self.crop[3] - self.crop[1]),
        )
    }

    pub fn display_crop(&self) -> [f32; 4] {
        rotate_crop(self.crop, self.quarter_turns)
    }

    pub fn set_display_crop(&mut self, crop: [f32; 4]) {
        self.crop = rotate_crop(crop, (4 - self.quarter_turns) % 4);
    }

    pub(crate) fn crop_source_pixel(&self, x: u32, y: u32, size: [u32; 2]) -> [u32; 2] {
        match self.quarter_turns {
            1 => [y, size[1] - 1 - x],
            2 => [size[0] - 1 - x, size[1] - 1 - y],
            3 => [size[0] - 1 - y, x],
            _ => [x, y],
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.negative.validate()?;
        range(self.temperature, 2000.0, 25_000.0)?;
        range(self.tint, -150.0, 150.0)?;
        range(self.exposure, -10.0, 10.0)?;
        for value in self.custom_wb {
            range(value, 0.01, 100.0)?;
        }
        for value in [
            self.brightness,
            self.contrast,
            self.highlights,
            self.shadows,
            self.whites,
            self.blacks,
            self.saturation,
            self.vibrance,
            self.clarity,
            self.texture,
            self.dehaze,
            self.tone_balance,
            self.distortion,
            self.chromatic_red,
            self.chromatic_blue,
            self.vignette,
        ] {
            range(value, -100.0, 100.0)?;
        }
        for value in [self.luminance_noise, self.color_noise, self.defringe] {
            range(value, 0.0, 100.0)?;
        }
        range(self.sharpen, 0.0, 200.0)?;
        range(self.sharpen_radius, 0.3, 5.0)?;
        range(self.sharpen_threshold, 0.0, 1.0)?;
        range(self.rotation, -45.0, 45.0)?;
        ensure!(self.quarter_turns < 4, "Invalid RAW rotation");
        for value in self.perspective {
            range(value, -100.0, 100.0)?;
        }
        for value in self.curves.iter().flatten() {
            range(*value, 0.0, 1.0)?;
        }
        for value in self.hsl.iter().flatten() {
            range(*value, -100.0, 100.0)?;
        }
        for value in self.bw_mix {
            range(value, -1.0, 2.0)?;
        }
        for tone in [self.shadow_tone, self.highlight_tone] {
            range(tone[0], 0.0, 360.0)?;
            range(tone[1], 0.0, 100.0)?;
        }
        for value in self.crop {
            range(value, 0.0, 1.0)?;
        }
        ensure!(
            self.crop[2] - self.crop[0] >= 0.01 && self.crop[3] - self.crop[1] >= 0.01,
            "RAW crop must retain at least 1% of each dimension"
        );
        ensure!(
            self.overlays.len() <= 32,
            "Too many RAW overlays (maximum 32)"
        );
        let mut points = 0;
        for overlay in &self.overlays {
            ensure!(overlay.name.len() <= 256, "RAW overlay name is too long");
            for point in [overlay.start, overlay.end].iter().chain(&overlay.points) {
                range(point.x, 0.0, 1.0)?;
                range(point.y, 0.0, 1.0)?;
            }
            points += overlay.points.len();
            range(overlay.radius, 0.001, 1.0)?;
            range(overlay.feather, 0.01, 1.0)?;
            range(overlay.exposure, -10.0, 10.0)?;
            range(overlay.warmth, -100.0, 100.0)?;
            range(overlay.saturation, -100.0, 100.0)?;
        }
        ensure!(points <= 8192, "Too many RAW brush points (maximum 8192)");
        Ok(())
    }
}

/// Rotate normalized coordinates clockwise, including coordinates outside the image.
pub fn rotate_point(point: Point, quarter_turns: u8) -> Point {
    match quarter_turns % 4 {
        1 => Point::new(1.0 - point.y, point.x),
        2 => Point::new(1.0 - point.x, 1.0 - point.y),
        3 => Point::new(point.y, 1.0 - point.x),
        _ => point,
    }
}

pub(super) fn rotate_crop(crop: [f32; 4], quarter_turns: u8) -> [f32; 4] {
    let a = rotate_point(Point::new(crop[0], crop[1]), quarter_turns);
    let b = rotate_point(Point::new(crop[2], crop[3]), quarter_turns);
    [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)]
}
