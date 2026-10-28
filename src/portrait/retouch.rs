use anyhow::{Result, ensure};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::document::Rgba16Image;

use super::masks::PortraitMasks;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortraitSettings {
    pub skin_smooth: f32,
    pub skin_texture: f32,
    pub skin_tone: f32,
    pub blemish: f32,
    pub eye_brightness: f32,
    pub eye_clarity: f32,
    pub dark_circles: f32,
    pub teeth_whiten: f32,
    pub lip_tint: f32,
    pub lip_color: [u8; 4],
    pub face_width: f32,
    pub jaw: f32,
    pub chin: f32,
    pub eye_size: f32,
    pub nose_width: f32,
    pub lip_shape: f32,
}

impl Default for PortraitSettings {
    fn default() -> Self {
        Self {
            skin_smooth: 0.0,
            skin_texture: 0.0,
            skin_tone: 0.0,
            blemish: 0.0,
            eye_brightness: 0.0,
            eye_clarity: 0.0,
            dark_circles: 0.0,
            teeth_whiten: 0.0,
            lip_tint: 0.0,
            lip_color: [255, 96, 112, 255],
            face_width: 0.0,
            jaw: 0.0,
            chin: 0.0,
            eye_size: 0.0,
            nose_width: 0.0,
            lip_shape: 0.0,
        }
    }
}

impl PortraitSettings {
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("skin_smooth", self.skin_smooth),
            ("skin_texture", self.skin_texture),
            ("skin_tone", self.skin_tone),
            ("blemish", self.blemish),
            ("eye_brightness", self.eye_brightness),
            ("eye_clarity", self.eye_clarity),
            ("dark_circles", self.dark_circles),
            ("teeth_whiten", self.teeth_whiten),
            ("lip_tint", self.lip_tint),
        ] {
            ensure!((0.0..=100.0).contains(&value), "{name} is out of range");
        }
        for (name, value) in [
            ("face_width", self.face_width),
            ("jaw", self.jaw),
            ("chin", self.chin),
            ("eye_size", self.eye_size),
            ("nose_width", self.nose_width),
            ("lip_shape", self.lip_shape),
        ] {
            ensure!((-100.0..=100.0).contains(&value), "{name} is out of range");
        }
        Ok(())
    }

    pub fn is_retouch_identity(&self) -> bool {
        [
            self.skin_smooth,
            self.skin_texture,
            self.skin_tone,
            self.blemish,
            self.eye_brightness,
            self.eye_clarity,
            self.dark_circles,
            self.teeth_whiten,
            self.lip_tint,
        ]
        .iter()
        .all(|value| *value == 0.0)
    }

    /// True when neither retouching nor geometric reshaping changes pixels.
    pub fn is_identity(&self) -> bool {
        self.is_retouch_identity()
            && [
                self.face_width,
                self.jaw,
                self.chin,
                self.eye_size,
                self.nose_width,
                self.lip_shape,
            ]
            .iter()
            .all(|value| *value == 0.0)
    }
}

pub fn apply_portrait_to_rgba8(
    source: &RgbaImage,
    output: &mut RgbaImage,
    masks: &PortraitMasks,
    settings: &PortraitSettings,
) -> Result<()> {
    settings.validate()?;
    ensure!(
        source.dimensions() == output.dimensions(),
        "Portrait output size mismatch"
    );
    validate_masks(masks, source.dimensions())?;
    if settings.is_retouch_identity() {
        return Ok(());
    }
    let radius = (1.0 + settings.skin_smooth / 18.0).round() as u32;
    let blurred = blur_rgb(source, radius);
    let average = skin_average(source, &masks.skin);
    let mut current = source.clone();
    for y in 0..source.height() {
        for x in 0..source.width() {
            let skin = masks.skin.get_pixel(x, y)[0] as f32 / 255.0;
            let original = source.get_pixel(x, y);
            let soft = blurred.get_pixel(x, y);
            let mut color = [original[0] as f32, original[1] as f32, original[2] as f32];
            let smooth = settings.skin_smooth / 100.0 * skin;
            if smooth > 0.0 {
                for channel in 0..3 {
                    let high = color[channel] - soft[channel] as f32;
                    let target = soft[channel] as f32 + high * settings.skin_texture / 100.0;
                    color[channel] += (target - color[channel]) * smooth;
                }
            }
            let tone = settings.skin_tone / 100.0 * skin;
            if tone > 0.0 {
                for channel in 0..3 {
                    color[channel] += (average[channel] - color[channel]) * tone * 0.4;
                }
            }
            let blemish = settings.blemish / 100.0 * skin;
            if blemish > 0.0 {
                let source_luma = luma(color);
                let soft_luma = luma([soft[0] as f32, soft[1] as f32, soft[2] as f32]);
                if source_luma < soft_luma - 18.0 {
                    for channel in 0..3 {
                        color[channel] += (soft[channel] as f32 - color[channel]) * blemish;
                    }
                }
            }
            current.put_pixel(
                x,
                y,
                Rgba([
                    clamp_u8(color[0]),
                    clamp_u8(color[1]),
                    clamp_u8(color[2]),
                    original[3],
                ]),
            );
        }
    }
    enhance_eyes(&mut current, &blurred, masks, settings);
    whiten_teeth(&mut current, masks, settings.teeth_whiten);
    tint_lips(&mut current, masks, settings);
    output.copy_from_slice(&current);
    Ok(())
}

pub fn apply_portrait_to_rgba16(
    source: &Rgba16Image,
    masks: &PortraitMasks,
    settings: &PortraitSettings,
) -> Result<Rgba16Image> {
    settings.validate()?;
    validate_masks(masks, source.dimensions())?;
    if settings.is_retouch_identity() {
        return Ok(source.clone());
    }
    let source8 = RgbaImage::from_fn(source.width(), source.height(), |x, y| {
        let pixel = source.get_pixel(x, y);
        Rgba([
            (pixel[0] >> 8) as u8,
            (pixel[1] >> 8) as u8,
            (pixel[2] >> 8) as u8,
            (pixel[3] >> 8) as u8,
        ])
    });
    let mut output8 = source8.clone();
    apply_portrait_to_rgba8(&source8, &mut output8, masks, settings)?;
    Ok(image::ImageBuffer::from_fn(
        source.width(),
        source.height(),
        |x, y| {
            let old = source.get_pixel(x, y);
            let before = source8.get_pixel(x, y);
            let after = output8.get_pixel(x, y);
            let scale = |before: u8, after: u8, value: u16| -> u16 {
                if before == 0 {
                    u16::from(after) * 257
                } else {
                    (u32::from(value) * u32::from(after) / u32::from(before))
                        .min(u32::from(u16::MAX)) as u16
                }
            };
            Rgba([
                scale(before[0], after[0], old[0]),
                scale(before[1], after[1], old[1]),
                scale(before[2], after[2], old[2]),
                old[3],
            ])
        },
    ))
}

fn enhance_eyes(
    image: &mut RgbaImage,
    blurred: &RgbaImage,
    masks: &PortraitMasks,
    settings: &PortraitSettings,
) {
    let dark = translated_mask(&masks.eyes, 0, (image.height() / 12) as i32);
    for y in 0..image.height() {
        for x in 0..image.width() {
            let eye = masks.eyes.get_pixel(x, y)[0] as f32 / 255.0;
            let circles = (dark.get_pixel(x, y)[0] as f32 / 255.0)
                * (masks.skin.get_pixel(x, y)[0] as f32 / 255.0);
            if eye > 0.0 {
                let pixel = image.get_pixel_mut(x, y);
                let soft = blurred.get_pixel(x, y);
                let brighten = 1.0 + settings.eye_brightness / 100.0 * eye * 0.35;
                let detail = settings.eye_clarity / 100.0 * eye;
                for channel in 0..3 {
                    let sharpened = pixel[channel] as f32
                        + (pixel[channel] as f32 - soft[channel] as f32) * detail * 1.5;
                    pixel[channel] = clamp_u8(sharpened * brighten);
                }
            }
            if circles > 0.0 && settings.dark_circles > 0.0 {
                let pixel = image.get_pixel_mut(x, y);
                let amount = settings.dark_circles / 100.0 * circles * 0.3;
                for channel in 0..3 {
                    pixel[channel] =
                        clamp_u8(pixel[channel] as f32 + (255.0 - pixel[channel] as f32) * amount);
                }
            }
        }
    }
}

fn whiten_teeth(image: &mut RgbaImage, masks: &PortraitMasks, amount: f32) {
    if amount == 0.0 {
        return;
    }
    for y in 0..image.height() {
        for x in 0..image.width() {
            let mask = masks.teeth.get_pixel(x, y)[0] as f32 / 255.0;
            if mask == 0.0 {
                continue;
            }
            let pixel = image.get_pixel_mut(x, y);
            let gray = luma([pixel[0] as f32, pixel[1] as f32, pixel[2] as f32]);
            let whiten = amount / 100.0 * mask;
            for channel in 0..3 {
                let target = (pixel[channel] as f32 * 0.35 + gray * 0.65 + 28.0).min(255.0);
                pixel[channel] =
                    clamp_u8(pixel[channel] as f32 + (target - pixel[channel] as f32) * whiten);
            }
        }
    }
}

fn tint_lips(image: &mut RgbaImage, masks: &PortraitMasks, settings: &PortraitSettings) {
    if settings.lip_tint == 0.0 {
        return;
    }
    for y in 0..image.height() {
        for x in 0..image.width() {
            let mask = masks.lips.get_pixel(x, y)[0] as f32 / 255.0;
            if mask == 0.0 {
                continue;
            }
            let pixel = image.get_pixel_mut(x, y);
            let amount = settings.lip_tint / 100.0 * mask;
            for channel in 0..3 {
                let target = settings.lip_color[channel] as f32;
                pixel[channel] =
                    clamp_u8(pixel[channel] as f32 + (target - pixel[channel] as f32) * amount);
            }
        }
    }
}

fn validate_masks(masks: &PortraitMasks, size: (u32, u32)) -> Result<()> {
    for mask in [
        &masks.face,
        &masks.skin,
        &masks.eyes,
        &masks.brows,
        &masks.lips,
        &masks.teeth,
    ] {
        ensure!(mask.dimensions() == size, "Portrait mask size mismatch");
    }
    Ok(())
}

fn skin_average(image: &RgbaImage, mask: &GrayImage) -> [f32; 3] {
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for (x, y, pixel) in image.enumerate_pixels() {
        let weight = mask.get_pixel(x, y)[0] as f32 / 255.0;
        if weight > 0.0 {
            sum[0] += pixel[0] as f32 * weight;
            sum[1] += pixel[1] as f32 * weight;
            sum[2] += pixel[2] as f32 * weight;
            count += weight;
        }
    }
    if count == 0.0 {
        [128.0; 3]
    } else {
        [sum[0] / count, sum[1] / count, sum[2] / count]
    }
}

fn blur_rgb(image: &RgbaImage, radius: u32) -> RgbaImage {
    if radius == 0 {
        return image.clone();
    }
    let diameter = radius * 2 + 1;
    let mut horizontal = RgbaImage::new(image.width(), image.height());
    let mut output = RgbaImage::new(image.width(), image.height());
    for y in 0..image.height() {
        for x in 0..image.width() {
            let mut sum = [0u32; 3];
            let mut count = 0;
            for offset in 0..diameter {
                let sample = x as i64 + offset as i64 - radius as i64;
                if sample >= 0 && sample < image.width() as i64 {
                    let pixel = image.get_pixel(sample as u32, y);
                    for channel in 0..3 {
                        sum[channel] += pixel[channel] as u32;
                    }
                    count += 1;
                }
            }
            horizontal.put_pixel(
                x,
                y,
                Rgba([
                    (sum[0] / count.max(1)) as u8,
                    (sum[1] / count.max(1)) as u8,
                    (sum[2] / count.max(1)) as u8,
                    image.get_pixel(x, y)[3],
                ]),
            );
        }
    }
    for y in 0..image.height() {
        for x in 0..image.width() {
            let mut sum = [0u32; 3];
            let mut count = 0;
            for offset in 0..diameter {
                let sample = y as i64 + offset as i64 - radius as i64;
                if sample >= 0 && sample < image.height() as i64 {
                    let pixel = horizontal.get_pixel(x, sample as u32);
                    for channel in 0..3 {
                        sum[channel] += pixel[channel] as u32;
                    }
                    count += 1;
                }
            }
            output.put_pixel(
                x,
                y,
                Rgba([
                    (sum[0] / count.max(1)) as u8,
                    (sum[1] / count.max(1)) as u8,
                    (sum[2] / count.max(1)) as u8,
                    image.get_pixel(x, y)[3],
                ]),
            );
        }
    }
    output
}

fn translated_mask(mask: &GrayImage, dx: i32, dy: i32) -> GrayImage {
    GrayImage::from_fn(mask.width(), mask.height(), |x, y| {
        let sx = x as i32 - dx;
        let sy = y as i32 - dy;
        if sx < 0 || sy < 0 || sx >= mask.width() as i32 || sy >= mask.height() as i32 {
            Luma([0])
        } else {
            *mask.get_pixel(sx as u32, sy as u32)
        }
    })
}

fn luma(rgb: [f32; 3]) -> f32 {
    0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]
}

fn clamp_u8(value: f32) -> u8 {
    value.clamp(0.0, 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portrait::test_fixtures::{synthetic_masks, synthetic_rgba8, synthetic_rgba16};

    #[test]
    fn zero_settings_preserve_rgba8_bytes() {
        let source = synthetic_rgba8(32, 24);
        let mut output = source.clone();
        apply_portrait_to_rgba8(
            &source,
            &mut output,
            &synthetic_masks(),
            &PortraitSettings::default(),
        )
        .unwrap();
        assert_eq!(output.as_raw(), source.as_raw());
    }

    #[test]
    fn skin_smoothing_preserves_alpha_and_high_depth_samples() {
        let source = synthetic_rgba16(32, 24);
        let settings = PortraitSettings {
            skin_smooth: 50.0,
            ..Default::default()
        };
        let output = apply_portrait_to_rgba16(&source, &synthetic_masks(), &settings).unwrap();
        for (before, after) in source.pixels().zip(output.pixels()) {
            assert_eq!(before[3], after[3]);
        }
        assert_ne!(source.as_raw(), output.as_raw());
    }

    #[test]
    fn masked_out_pixels_are_unchanged() {
        let source = synthetic_rgba8(32, 24);
        let mut output = source.clone();
        let mut masks = synthetic_masks();
        masks.skin = image::GrayImage::new(32, 24);
        apply_portrait_to_rgba8(
            &source,
            &mut output,
            &masks,
            &PortraitSettings {
                skin_smooth: 100.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(output.as_raw(), source.as_raw());
    }

    #[test]
    fn settings_reject_out_of_range_values() {
        assert!(
            PortraitSettings {
                skin_smooth: 101.0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            PortraitSettings {
                face_width: -101.0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
