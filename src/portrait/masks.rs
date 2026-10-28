use anyhow::Result;
use image::{GrayImage, Luma, RgbaImage};

use super::geometry::{FaceRegions, build_face_regions};
use super::types::FaceLandmarks;

#[derive(Clone, Debug)]
pub struct PortraitMasks {
    pub face: GrayImage,
    pub skin: GrayImage,
    pub eyes: GrayImage,
    pub brows: GrayImage,
    pub lips: GrayImage,
    pub teeth: GrayImage,
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
    for (x, y, pixel) in image.enumerate_pixels() {
        if x >= size[0] || y >= size[1] {
            continue;
        }
        let current = skin.get_pixel(x, y)[0];
        if current == 0 || pixel[3] == 0 || !is_skin_like(pixel.0) {
            skin.put_pixel(x, y, Luma([0]));
        }
    }
    Ok(PortraitMasks {
        face,
        skin,
        eyes,
        brows,
        lips,
        teeth,
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
