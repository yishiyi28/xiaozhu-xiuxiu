use anyhow::{Result, bail};

use super::types::{DetectedFace, FaceLandmarks};

const OVAL: [usize; 36] = [
    10, 338, 297, 332, 284, 251, 389, 356, 454, 323, 361, 288, 397, 365, 379, 378, 400, 377, 152,
    148, 176, 149, 150, 136, 172, 58, 132, 93, 234, 127, 162, 21, 54, 103, 67, 109,
];
const RIGHT_EYE: [usize; 9] = [33, 7, 163, 144, 145, 153, 154, 155, 133];
const LEFT_EYE: [usize; 9] = [263, 249, 390, 373, 374, 380, 381, 382, 362];
const RIGHT_BROW: [usize; 10] = [70, 63, 105, 66, 107, 55, 65, 52, 53, 46];
const LEFT_BROW: [usize; 10] = [300, 293, 334, 296, 336, 285, 295, 282, 283, 276];
const OUTER_LIPS: [usize; 18] = [
    61, 185, 40, 39, 37, 0, 267, 269, 270, 409, 291, 375, 321, 405, 314, 17, 84, 181,
];
const INNER_LIPS: [usize; 18] = [
    78, 191, 80, 81, 82, 13, 312, 311, 310, 415, 308, 324, 318, 402, 317, 14, 87, 178,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceCropTransform {
    pub center: [f32; 2],
    pub size: f32,
    pub angle: f32,
}

impl FaceCropTransform {
    pub fn new(center: [f32; 2], size: f32, angle: f32) -> Self {
        Self {
            center,
            size,
            angle,
        }
    }

    pub fn to_crop(self, point: [f32; 2]) -> [f32; 2] {
        let (sin, cos) = self.angle.sin_cos();
        let dx = (point[0] - self.center[0]) / self.size;
        let dy = (point[1] - self.center[1]) / self.size;
        [dx * cos + dy * sin, -dx * sin + dy * cos]
    }

    pub fn to_image(self, point: [f32; 2]) -> [f32; 2] {
        let (sin, cos) = self.angle.sin_cos();
        let dx = point[0] * self.size;
        let dy = point[1] * self.size;
        [
            self.center[0] + dx * cos - dy * sin,
            self.center[1] + dx * sin + dy * cos,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceRegions {
    pub oval: Vec<[f32; 2]>,
    pub left_eye: Vec<[f32; 2]>,
    pub right_eye: Vec<[f32; 2]>,
    pub brows: Vec<[f32; 2]>,
    pub outer_lips: Vec<[f32; 2]>,
    pub inner_lips: Vec<[f32; 2]>,
    pub teeth: Vec<[f32; 2]>,
}

pub fn stable_face_order(faces: &mut [DetectedFace]) {
    faces.sort_by(|a, b| {
        let center = |face: &DetectedFace| {
            [
                face.rect[0] + face.rect[2] * 0.5,
                face.rect[1] + face.rect[3] * 0.5,
            ]
        };
        let (a_center, b_center) = (center(a), center(b));
        a_center[0]
            .total_cmp(&b_center[0])
            .then_with(|| a_center[1].total_cmp(&b_center[1]))
            .then_with(|| (b.rect[2] * b.rect[3]).total_cmp(&(a.rect[2] * a.rect[3])))
    });
}

pub fn build_face_regions(landmarks: &FaceLandmarks) -> Result<FaceRegions> {
    if landmarks.points.len() != 478 {
        bail!(
            "Face landmarks contain {} points, expected 478",
            landmarks.points.len()
        );
    }
    let polygon = |indices: &[usize]| {
        indices
            .iter()
            .map(|index| landmarks.points[*index].position)
            .collect::<Vec<_>>()
    };
    Ok(FaceRegions {
        oval: polygon(&OVAL),
        left_eye: polygon(&LEFT_EYE),
        right_eye: polygon(&RIGHT_EYE),
        brows: polygon(&RIGHT_BROW)
            .into_iter()
            .chain(polygon(&LEFT_BROW))
            .collect(),
        outer_lips: polygon(&OUTER_LIPS),
        inner_lips: polygon(&INNER_LIPS),
        teeth: polygon(&INNER_LIPS),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::portrait::types::DetectedFace;

    #[test]
    fn crop_transform_round_trip_stays_subpixel() {
        let transform = FaceCropTransform::new([200.0, 150.0], 300.0, 0.35);
        for point in [[100.0, 50.0], [220.0, 180.0], [300.0, 250.0]] {
            let local = transform.to_crop(point);
            let round_trip = transform.to_image(local);
            assert!((round_trip[0] - point[0]).abs() < 0.001);
            assert!((round_trip[1] - point[1]).abs() < 0.001);
        }
    }

    #[test]
    fn faces_are_ordered_left_to_right_then_top() {
        let mut faces = vec![
            face(3, 300.0, 100.0),
            face(1, 100.0, 200.0),
            face(2, 100.0, 120.0),
        ];
        stable_face_order(&mut faces);
        assert_eq!(
            faces.iter().map(|face| face.id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
    }

    fn face(id: u32, x: f32, y: f32) -> DetectedFace {
        DetectedFace {
            id,
            rect: [x, y, 80.0, 80.0],
            score: 0.9,
            left_eye: [x + 20.0, y + 30.0],
            right_eye: [x + 60.0, y + 30.0],
        }
    }
}
