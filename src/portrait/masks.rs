use anyhow::Result;
use image::{GrayImage, Luma, RgbaImage};

use super::geometry::{FaceRegions, build_face_regions};
use super::types::FaceLandmarks;

#[derive(Clone, Debug, PartialEq)]
pub struct PortraitMasks {
    pub face: GrayImage,
    pub skin: GrayImage,
    pub eyes: GrayImage,
    pub brows: GrayImage,
    pub lips: GrayImage,
    pub teeth: GrayImage,
    pub skin_average: [f32; 3],
}

pub fn rasterize_polygon(size: [u32; 2], polygon: &[[f32; 2]], feather: f32) -> GrayImage {
    let mut mask = GrayImage::new(size[0], size[1]);
    if polygon.len() < 3 {
        return mask;
    }
    let min_y = polygon
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as u32;
    let max_y = polygon
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(size[1] as f32) as u32;
    let mut intersections = Vec::new();
    for y in min_y..max_y {
        intersections.clear();
        let scan_y = y as f32 + 0.5;
        for index in 0..polygon.len() {
            let a = polygon[index];
            let b = polygon[(index + 1) % polygon.len()];
            if (a[1] <= scan_y && b[1] > scan_y) || (b[1] <= scan_y && a[1] > scan_y) {
                let t = (scan_y - a[1]) / (b[1] - a[1]);
                intersections.push(a[0] + (b[0] - a[0]) * t);
            }
        }
        intersections.sort_by(f32::total_cmp);
        for pair in intersections.as_chunks::<2>().0 {
            let start = pair[0].ceil().max(0.0) as u32;
            let end = pair[1].ceil().min(size[0] as f32).max(0.0) as u32;
            for x in start..end {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
    }
    if feather > 0.0 {
        mask = blur_mask(&mask, (feather / 2.0).round().max(1.0) as u32);
    }
    mask
}

pub fn build_portrait_masks(
    image: &RgbaImage,
    landmarks: &FaceLandmarks,
    size: [u32; 2],
) -> Result<PortraitMasks> {
    let regions = build_face_regions(landmarks)?;
    let face_width = bounds_width(&regions.oval);
    let feather = (face_width * 0.06).max(1.0);
    let face = rasterize_polygon(size, &regions.oval, feather);
    let eyes = rasterize_polygons(size, &[&regions.left_eye, &regions.right_eye], 1.0);
    let brows = rasterize_polygon(size, &regions.brows, 1.0);
    let lips = rasterize_polygon(size, &regions.outer_lips, 1.0);
    let teeth = rasterize_polygon(size, &regions.inner_lips, 1.0);
    let mut excluded = union_masks(&eyes, &brows);
    excluded = union_masks(&excluded, &lips);
    let nose = nose_polygon(&regions);
    excluded = union_masks(&excluded, &rasterize_polygon(size, &nose, 1.0));
    let mut skin = subtract_mask(&face, &excluded);
    let skin_model = estimate_skin_model(image, &face, &excluded);
    for (x, y, pixel) in image.enumerate_pixels() {
        if x >= size[0] || y >= size[1] {
            continue;
        }
        let current = skin.get_pixel(x, y)[0];
        if current == 0 || pixel[3] == 0 {
            continue;
        }
        let confidence = skin_model.map_or_else(
            || u8::from(is_skin_like(pixel.0)) * 255,
            |model| adaptive_skin_value(pixel.0, model),
        );
        let filtered = (u16::from(current) * u16::from(confidence) / 255) as u8;
        skin.put_pixel(x, y, Luma([filtered]));
    }
    let skin_average = average_rgb(image, &skin);
    Ok(PortraitMasks {
        face,
        skin,
        eyes,
        brows,
        lips,
        teeth,
        skin_average,
    })
}

fn rasterize_polygons(size: [u32; 2], polygons: &[&[[f32; 2]]], feather: f32) -> GrayImage {
    let mut result = GrayImage::new(size[0], size[1]);
    for polygon in polygons {
        let mask = rasterize_polygon(size, polygon, feather);
        result = union_masks(&result, &mask);
    }
    result
}

fn bounds_width(points: &[[f32; 2]]) -> f32 {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for point in points {
        min = min.min(point[0]);
        max = max.max(point[0]);
    }
    (max - min).max(1.0)
}

fn nose_polygon(regions: &FaceRegions) -> Vec<[f32; 2]> {
    let center = bounds_center(&regions.oval);
    let width = bounds_width(&regions.oval);
    let height = bounds_height(&regions.oval);
    vec![
        [center[0] - width * 0.06, center[1] + height * 0.08],
        [center[0] + width * 0.06, center[1] + height * 0.08],
        [center[0] + width * 0.05, center[1] + height * 0.23],
        [center[0] - width * 0.05, center[1] + height * 0.23],
    ]
}

fn bounds_center(points: &[[f32; 2]]) -> [f32; 2] {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for point in points {
        min[0] = min[0].min(point[0]);
        min[1] = min[1].min(point[1]);
        max[0] = max[0].max(point[0]);
        max[1] = max[1].max(point[1]);
    }
    [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5]
}

fn bounds_height(points: &[[f32; 2]]) -> f32 {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for point in points {
        min = min.min(point[1]);
        max = max.max(point[1]);
    }
    (max - min).max(1.0)
}

#[derive(Clone, Copy, Debug)]
struct SkinModel {
    luma: f32,
    cb: f32,
    cr: f32,
    luma_radius: f32,
    cb_radius: f32,
    cr_radius: f32,
}

fn ycbcr(rgb: [u8; 4]) -> [f32; 3] {
    let r = rgb[0] as f32;
    let g = rgb[1] as f32;
    let b = rgb[2] as f32;
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
    let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
    [y, cb, cr]
}

fn adaptive_skin_value(rgb: [u8; 4], model: SkinModel) -> u8 {
    let [luma, cb, cr] = ycbcr(rgb);
    let dl = (luma - model.luma) / model.luma_radius.max(1.0);
    let dcb = (cb - model.cb) / model.cb_radius.max(1.0);
    let dcr = (cr - model.cr) / model.cr_radius.max(1.0);
    let distance = (dcb * dcb + dcr * dcr + dl * dl * 0.35).sqrt();
    let score = (1.0 - distance).clamp(0.0, 1.0);
    let smooth = score * score * (3.0 - 2.0 * score);
    (smooth * 255.0).round().clamp(0.0, 255.0) as u8
}

fn estimate_skin_model(
    image: &RgbaImage,
    face: &GrayImage,
    excluded: &GrayImage,
) -> Option<SkinModel> {
    let mut count = 0u32;
    let mut sums = [0.0f64; 3];
    let mut squares = [0.0f64; 3];
    for (x, y, pixel) in image.enumerate_pixels() {
        if x >= face.width() || y >= face.height() || face.get_pixel(x, y)[0] <= 128 {
            continue;
        }
        if excluded.get_pixel(x, y)[0] > 32 || pixel[3] == 0 {
            continue;
        }
        let sample = ycbcr(pixel.0);
        if !(12.0..=245.0).contains(&sample[0])
            || !(60.0..=155.0).contains(&sample[1])
            || !(112.0..=210.0).contains(&sample[2])
        {
            continue;
        }
        count += 1;
        for channel in 0..3 {
            let value = sample[channel] as f64;
            sums[channel] += value;
            squares[channel] += value * value;
        }
    }
    if count < 16 {
        return None;
    }
    let mean = sums.map(|sum| sum / count as f64);
    let variance =
        |channel: usize| (squares[channel] / count as f64 - mean[channel].powi(2)).max(0.0);
    Some(SkinModel {
        luma: mean[0] as f32,
        cb: mean[1] as f32,
        cr: mean[2] as f32,
        luma_radius: (variance(0).sqrt() as f32 * 2.0).clamp(45.0, 110.0),
        cb_radius: (variance(1).sqrt() as f32 * 2.5).clamp(14.0, 35.0),
        cr_radius: (variance(2).sqrt() as f32 * 2.5).clamp(14.0, 35.0),
    })
}

fn average_rgb(image: &RgbaImage, mask: &GrayImage) -> [f32; 3] {
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for (x, y, pixel) in image.enumerate_pixels() {
        let weight = mask.get_pixel(x, y)[0] as f32 / 255.0;
        if weight > 0.0 {
            for channel in 0..3 {
                sum[channel] += pixel[channel] as f32 * weight;
            }
            count += weight;
        }
    }
    if count == 0.0 {
        [128.0 / 255.0; 3]
    } else {
        sum.map(|value| value / count / 255.0)
    }
}

fn is_skin_like(rgb: [u8; 4]) -> bool {
    let r = rgb[0] as f32;
    let g = rgb[1] as f32;
    let b = rgb[2] as f32;
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
    let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
    (40.0..=250.0).contains(&y) && (77.0..=127.0).contains(&cb) && (133.0..=173.0).contains(&cr)
}

fn union_masks(a: &GrayImage, b: &GrayImage) -> GrayImage {
    let mut result = a.clone();
    for (x, y, pixel) in b.enumerate_pixels() {
        let value = result.get_pixel(x, y)[0].max(pixel[0]);
        result.put_pixel(x, y, Luma([value]));
    }
    result
}

fn subtract_mask(base: &GrayImage, cutout: &GrayImage) -> GrayImage {
    let mut result = base.clone();
    for (x, y, pixel) in cutout.enumerate_pixels() {
        if pixel[0] > 0 {
            result.put_pixel(x, y, Luma([0]));
        }
    }
    result
}

fn blur_mask(mask: &GrayImage, radius: u32) -> GrayImage {
    if radius == 0 {
        return mask.clone();
    }
    let mut horizontal = GrayImage::new(mask.width(), mask.height());
    let mut output = GrayImage::new(mask.width(), mask.height());
    let diameter = radius * 2 + 1;
    for y in 0..mask.height() {
        for x in 0..mask.width() {
            let mut sum = 0u32;
            for offset in 0..diameter {
                let sample = x as i64 + offset as i64 - radius as i64;
                if sample >= 0 && sample < mask.width() as i64 {
                    sum += mask.get_pixel(sample as u32, y)[0] as u32;
                }
            }
            horizontal.put_pixel(x, y, Luma([(sum / diameter) as u8]));
        }
    }
    for y in 0..mask.height() {
        for x in 0..mask.width() {
            let mut sum = 0u32;
            for offset in 0..diameter {
                let sample = y as i64 + offset as i64 - radius as i64;
                if sample >= 0 && sample < mask.height() as i64 {
                    sum += horizontal.get_pixel(x, sample as u32)[0] as u32;
                }
            }
            output.put_pixel(x, y, Luma([(sum / diameter) as u8]));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skin_mask_excludes_eye_and_mouth_polygons() {
        let masks = crate::portrait::test_fixtures::synthetic_face_masks();
        assert!(masks.skin.get_pixel(100, 100)[0] > 0);
        assert_eq!(masks.skin.get_pixel(80, 100)[0], 0);
        assert_eq!(masks.skin.get_pixel(120, 100)[0], 0);
    }

    #[test]
    fn adaptive_skin_score_accepts_dark_skin_and_rejects_green_background() {
        let model = SkinModel {
            luma: 68.0,
            cb: 108.0,
            cr: 145.0,
            luma_radius: 70.0,
            cb_radius: 18.0,
            cr_radius: 18.0,
        };
        assert!(adaptive_skin_value([92, 58, 44, 255], model) > 128);
        assert_eq!(adaptive_skin_value([30, 120, 20, 255], model), 0);
    }

    #[test]
    fn polygon_rasterization_fills_the_interior() {
        let mask = rasterize_polygon(
            [20, 20],
            &[[4.0, 4.0], [16.0, 4.0], [16.0, 16.0], [4.0, 16.0]],
            0.0,
        );
        assert_eq!(mask.get_pixel(10, 10)[0], 255);
        assert_eq!(mask.get_pixel(1, 1)[0], 0);
    }
}
