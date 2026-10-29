#![allow(dead_code)]

use image::{GrayImage, Rgba, RgbaImage};

pub fn synthetic_rgba8(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x * 13 + y * 7) as u8,
            (x * 3 + y * 17) as u8,
            (x * 19 + y * 5) as u8,
            255,
        ])
    })
}

pub fn synthetic_rgba16(width: u32, height: u32) -> crate::document::Rgba16Image {
    image::ImageBuffer::from_fn(width, height, |x, y| {
        image::Rgba([
            (x * 1021 + y * 37) as u16,
            (x * 211 + y * 1031) as u16,
            (x * 509 + y * 43) as u16,
            u16::MAX,
        ])
    })
}

pub fn synthetic_masks() -> crate::portrait::masks::PortraitMasks {
    let width = 32;
    let height = 24;
    let full = GrayImage::from_pixel(width, height, image::Luma([255]));
    crate::portrait::masks::PortraitMasks {
        face: full.clone(),
        skin: full.clone(),
        eyes: GrayImage::new(width, height),
        brows: GrayImage::new(width, height),
        lips: GrayImage::new(width, height),
        teeth: GrayImage::new(width, height),
        skin_average: [128.0 / 255.0; 3],
    }
}

pub fn synthetic_face_masks() -> crate::portrait::masks::PortraitMasks {
    let mut masks = synthetic_masks();
    masks.face = GrayImage::from_pixel(200, 160, image::Luma([255]));
    masks.skin = masks.face.clone();
    for x in 65..=95 {
        for y in 85..=115 {
            masks.skin.put_pixel(x, y, image::Luma([0]));
        }
    }
    for x in 105..=135 {
        for y in 85..=115 {
            masks.skin.put_pixel(x, y, image::Luma([0]));
        }
    }
    masks
}

pub fn synthetic_landmarks() -> crate::portrait::types::FaceLandmarks {
    let mut points = vec![
        crate::portrait::types::LandmarkPoint {
            position: [0.0, 0.0],
            depth: 0.0,
        };
        478
    ];
    for (index, point) in points.iter_mut().enumerate() {
        let angle = index as f32 / 478.0 * std::f32::consts::TAU;
        point.position = [32.0 + angle.cos() * 20.0, 32.0 + angle.sin() * 20.0];
    }
    crate::portrait::types::FaceLandmarks {
        face: crate::portrait::types::DetectedFace {
            id: 1,
            rect: [8.0, 8.0, 56.0, 56.0],
            score: 0.99,
            left_eye: [24.0, 28.0],
            right_eye: [40.0, 28.0],
        },
        points,
        presence: 0.99,
    }
}
