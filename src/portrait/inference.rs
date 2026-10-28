use anyhow::{Context, Result, bail};
use image::RgbaImage;
use tract::prelude::*;

use super::types::{DetectedFace, FaceLandmarks, LandmarkPoint};

const DETECTOR_MODEL: &[u8] = include_bytes!("../../assets/models/blazeface.onnx");
const LANDMARK_MODEL: &[u8] = include_bytes!("../../assets/models/face_landmarks.onnx");
const DETECTOR_SIZE: u32 = 128;
const LANDMARK_SIZE: u32 = 256;
const MAX_FACES: usize = 8;
const MIN_FACE_EDGE: f32 = 64.0;
const DETECTION_THRESHOLD: f32 = 0.5;
const PRESENCE_THRESHOLD: f32 = 0.5;
const NMS_IOU_THRESHOLD: f32 = 0.3;

pub struct PortraitModels {
    detector: Runnable,
    landmarks: Runnable,
}

impl PortraitModels {
    pub fn load() -> Result<Self> {
        let detector = tract::onnx()?
            .load_buffer(DETECTOR_MODEL)?
            .into_model()?
            .into_runnable()?;
        let landmarks = tract::onnx()?
            .load_buffer(LANDMARK_MODEL)?
            .into_model()?
            .into_runnable()?;
        Ok(Self {
            detector,
            landmarks,
        })
    }

    pub fn detector_input_shape(&self) -> [usize; 4] {
        [1, DETECTOR_SIZE as usize, DETECTOR_SIZE as usize, 3]
    }

    pub fn detector_output_shapes(&self) -> ([usize; 3], [usize; 3]) {
        ([1, 896, 16], [1, 896, 1])
    }

    pub fn landmark_input_shape(&self) -> [usize; 4] {
        [1, LANDMARK_SIZE as usize, LANDMARK_SIZE as usize, 3]
    }

    pub fn landmark_output_shape(&self) -> [usize; 4] {
        [1, 1, 1, 478 * 3]
    }

    pub fn detect(&self, image: &RgbaImage) -> Result<Vec<DetectedFace>> {
        let (pixels, letterbox) = letterbox_rgb(image, DETECTOR_SIZE);
        let (regressors, scores) = self.run_detector(&pixels)?;
        let mut candidates = decode_detections(&regressors, &scores, DETECTION_THRESHOLD)?;
        for face in &mut candidates {
            map_face_from_model(face, &letterbox);
        }
        candidates.retain(|face| face.rect[2].min(face.rect[3]) >= MIN_FACE_EDGE);
        let mut faces = weighted_nms(candidates, NMS_IOU_THRESHOLD);
        sort_faces(&mut faces);
        faces.truncate(MAX_FACES);
        Ok(faces
            .into_iter()
            .enumerate()
            .map(|(index, face)| DetectedFace {
                id: index as u32 + 1,
                rect: face.rect,
                score: face.score,
                left_eye: face.left_eye,
                right_eye: face.right_eye,
            })
            .collect())
    }

    pub fn landmarks(&self, image: &RgbaImage, face: &DetectedFace) -> Result<FaceLandmarks> {
        let first = self.landmarks_in_crop(image, face, None)?;
        let transform = refined_crop(&first.0, face)?;
        let (points, presence) = self.landmarks_in_crop(image, face, Some(&transform))?;
        if !presence.is_finite() {
            bail!("Face landmark model returned a non-finite presence score");
        }
        Ok(FaceLandmarks {
            face: face.clone(),
            points: points
                .into_iter()
                .map(|point| LandmarkPoint {
                    position: [point[0], point[1]],
                    depth: point[2],
                })
                .collect(),
            presence,
        })
    }

    pub fn detect_with_landmarks(&self, image: &RgbaImage) -> Result<Vec<FaceLandmarks>> {
        let mut result = Vec::new();
        for face in self.detect(image)? {
            let landmarks = self.landmarks(image, &face)?;
            if landmarks.presence >= PRESENCE_THRESHOLD {
                result.push(landmarks);
            }
        }
        Ok(result)
    }

    #[cfg(test)]
    pub(crate) fn run_detector_zeros(&self) -> Result<(Vec<f32>, Vec<f32>)> {
        self.run_detector(&vec![0.0; (DETECTOR_SIZE * DETECTOR_SIZE * 3) as usize])
    }

    #[cfg(test)]
    pub(crate) fn run_landmarks_zeros(&self) -> Result<(Vec<f32>, f32)> {
        let outputs =
            self.run_landmark_model(&vec![0.0; (LANDMARK_SIZE * LANDMARK_SIZE * 3) as usize])?;
        let landmarks = outputs
            .first()
            .context("Face landmark model returned no landmark tensor")?
            .as_slice::<f32>()?
            .to_vec();
        let logit = outputs
            .get(1)
            .context("Face landmark model returned no presence tensor")?
            .as_slice::<f32>()?
            .first()
            .copied()
            .context("Face landmark presence tensor is empty")?;
        Ok((landmarks, sigmoid(logit)))
    }

    fn run_detector(&self, input: &[f32]) -> Result<(Vec<f32>, Vec<f32>)> {
        let tensor = Tensor::from_slice(
            &[1, DETECTOR_SIZE as usize, DETECTOR_SIZE as usize, 3],
            input,
        )?;
        let outputs = self.detector.run([tensor])?;
        let regressors = outputs
            .first()
            .context("BlazeFace returned no regressors")?
            .as_slice::<f32>()?
            .to_vec();
        let scores = outputs
            .get(1)
            .context("BlazeFace returned no classificators")?
            .as_slice::<f32>()?
            .to_vec();
        Ok((regressors, scores))
    }

    fn run_landmark_model(&self, input: &[f32]) -> Result<Vec<Tensor>> {
        let tensor = Tensor::from_slice(
            &[1, LANDMARK_SIZE as usize, LANDMARK_SIZE as usize, 3],
            input,
        )?;
        self.landmarks.run([tensor])
    }

    fn landmarks_in_crop(
        &self,
        image: &RgbaImage,
        face: &DetectedFace,
        transform: Option<&CropTransform>,
    ) -> Result<(Vec<[f32; 3]>, f32)> {
        let owned;
        let transform = if let Some(transform) = transform {
            transform
        } else {
            owned = detector_crop(face)?;
            &owned
        };
        let input = transform.image_to_tensor(image);
        let outputs = self.run_landmark_model(&input)?;
        let raw = outputs
            .first()
            .context("Face landmark model returned no landmarks")?
            .as_slice::<f32>()?;
        let presence_logit = outputs
            .get(1)
            .context("Face landmark model returned no presence score")?
            .as_slice::<f32>()?
            .first()
            .copied()
            .context("Face landmark presence tensor is empty")?;
        if raw.len() != 478 * 3 {
            bail!(
                "Face landmark model returned {} values, expected 1434",
                raw.len()
            );
        }
        let mut points = Vec::with_capacity(478);
        for point in raw.as_chunks::<3>().0 {
            let image_point = transform.crop_point_to_image([point[0], point[1]]);
            points.push([image_point[0], image_point[1], point[2]]);
        }
        Ok((points, sigmoid(presence_logit)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letterbox {
    pub scale: f32,
    pub pad: [f32; 2],
    pub source_size: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceBox {
    pub rect: [f32; 4],
    pub score: f32,
    pub left_eye: [f32; 2],
    pub right_eye: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
struct Anchor {
    x: f32,
    y: f32,
}

pub fn letterbox_rgb(image: &RgbaImage, size: u32) -> (Vec<f32>, Letterbox) {
    let target = size as f32;
    let source_size = [image.width() as f32, image.height() as f32];
    let scale = (target / source_size[0]).min(target / source_size[1]);
    let scaled = [source_size[0] * scale, source_size[1] * scale];
    let pad = [(target - scaled[0]) * 0.5, (target - scaled[1]) * 0.5];
    let mut pixels = vec![0.0; (size * size * 3) as usize];
    for y in 0..size {
        for x in 0..size {
            let source = [
                ((x as f32 + 0.5) - pad[0]) / scale - 0.5,
                ((y as f32 + 0.5) - pad[1]) / scale - 0.5,
            ];
            if source[0] < -0.5
                || source[1] < -0.5
                || source[0] > source_size[0] - 0.5
                || source[1] > source_size[1] - 0.5
            {
                continue;
            }
            let color = sample_rgb(image, source[0], source[1]);
            let index = ((y * size + x) * 3) as usize;
            pixels[index] = color[0] / 127.5 - 1.0;
            pixels[index + 1] = color[1] / 127.5 - 1.0;
            pixels[index + 2] = color[2] / 127.5 - 1.0;
        }
    }
    (
        pixels,
        Letterbox {
            scale,
            pad,
            source_size,
        },
    )
}

pub fn decode_detections(
    regressors: &[f32],
    classificators: &[f32],
    threshold: f32,
) -> Result<Vec<FaceBox>> {
    let anchors = blaze_face_anchors();
    if regressors.len() != anchors.len() * 16 {
        bail!(
            "BlazeFace returned {} regressor values, expected {}",
            regressors.len(),
            anchors.len() * 16
        );
    }
    if classificators.len() != anchors.len() {
        bail!(
            "BlazeFace returned {} scores, expected {}",
            classificators.len(),
            anchors.len()
        );
    }
    let mut faces = Vec::new();
    for (anchor, (raw, logit)) in anchors
        .iter()
        .zip(regressors.as_chunks::<16>().0.iter().zip(classificators))
    {
        let score = sigmoid(*logit);
        if score < threshold {
            continue;
        }
        let center = [
            anchor.x + raw[0] / DETECTOR_SIZE as f32,
            anchor.y + raw[1] / DETECTOR_SIZE as f32,
        ];
        let size = [raw[2] / DETECTOR_SIZE as f32, raw[3] / DETECTOR_SIZE as f32];
        let keypoint = |index: usize| {
            [
                anchor.x + raw[4 + index * 2] / DETECTOR_SIZE as f32,
                anchor.y + raw[5 + index * 2] / DETECTOR_SIZE as f32,
            ]
        };
        // BlazeFace returns right-eye, left-eye, nose, mouth and both ears.
        let first_eye = keypoint(0);
        let second_eye = keypoint(1);
        let (left_eye, right_eye) = if first_eye[0] <= second_eye[0] {
            (first_eye, second_eye)
        } else {
            (second_eye, first_eye)
        };
        faces.push(FaceBox {
            rect: [
                center[0] - size[0] * 0.5,
                center[1] - size[1] * 0.5,
                size[0],
                size[1],
            ],
            score,
            left_eye,
            right_eye,
        });
    }
    Ok(faces)
}

pub fn weighted_nms(mut boxes: Vec<FaceBox>, iou_threshold: f32) -> Vec<FaceBox> {
    boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept = Vec::new();
    while let Some(best) = boxes.first().copied() {
        let mut cluster = vec![best];
        boxes.remove(0);
        let mut index = 0;
        while index < boxes.len() {
            if iou(best.rect, boxes[index].rect) > iou_threshold {
                cluster.push(boxes.remove(index));
            } else {
                index += 1;
            }
        }
        let total: f32 = cluster.iter().map(|candidate| candidate.score).sum();
        let weighted = |value: fn(&FaceBox) -> f32| {
            cluster
                .iter()
                .map(|candidate| value(candidate) * candidate.score)
                .sum::<f32>()
                / total
        };
        let weighted_pair = |value: fn(&FaceBox) -> [f32; 2]| {
            let x = cluster
                .iter()
                .map(|candidate| value(candidate)[0] * candidate.score)
                .sum::<f32>()
                / total;
            let y = cluster
                .iter()
                .map(|candidate| value(candidate)[1] * candidate.score)
                .sum::<f32>()
                / total;
            [x, y]
        };
        let rect0 = weighted(|candidate| candidate.rect[0]);
        let rect1 = weighted(|candidate| candidate.rect[1]);
        let rect2 = weighted(|candidate| candidate.rect[2]);
        let rect3 = weighted(|candidate| candidate.rect[3]);
        kept.push(FaceBox {
            rect: [rect0, rect1, rect2, rect3],
            score: best.score,
            left_eye: weighted_pair(|candidate| candidate.left_eye),
            right_eye: weighted_pair(|candidate| candidate.right_eye),
        });
    }
    kept
}

fn blaze_face_anchors() -> Vec<Anchor> {
    let mut anchors = Vec::with_capacity(896);
    for grid in [16usize, 8] {
        let repeats = if grid == 16 { 2 } else { 6 };
        for y in 0..grid {
            for x in 0..grid {
                for _ in 0..repeats {
                    anchors.push(Anchor {
                        x: (x as f32 + 0.5) / grid as f32,
                        y: (y as f32 + 0.5) / grid as f32,
                    });
                }
            }
        }
    }
    anchors
}

fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let left = a[0].max(b[0]);
    let top = a[1].max(b[1]);
    let right = (a[0] + a[2]).min(b[0] + b[2]);
    let bottom = (a[1] + a[3]).min(b[1] + b[3]);
    let intersection = (right - left).max(0.0) * (bottom - top).max(0.0);
    let union = a[2] * a[3] + b[2] * b[3] - intersection;
    if union <= f32::EPSILON {
        0.0
    } else {
        intersection / union
    }
}

fn map_face_from_model(face: &mut FaceBox, transform: &Letterbox) {
    let x = (face.rect[0] * DETECTOR_SIZE as f32 - transform.pad[0]) / transform.scale;
    let y = (face.rect[1] * DETECTOR_SIZE as f32 - transform.pad[1]) / transform.scale;
    let width = face.rect[2] * DETECTOR_SIZE as f32 / transform.scale;
    let height = face.rect[3] * DETECTOR_SIZE as f32 / transform.scale;
    face.rect = [
        x.clamp(0.0, transform.source_size[0]),
        y.clamp(0.0, transform.source_size[1]),
        width.max(1.0),
        height.max(1.0),
    ];
    for eye in [&mut face.left_eye, &mut face.right_eye] {
        eye[0] = (eye[0] * DETECTOR_SIZE as f32 - transform.pad[0]) / transform.scale;
        eye[1] = (eye[1] * DETECTOR_SIZE as f32 - transform.pad[1]) / transform.scale;
    }
}

fn sort_faces(faces: &mut [FaceBox]) {
    faces.sort_by(|a, b| {
        let center = |face: &FaceBox| {
            [
                face.rect[0] + face.rect[2] * 0.5,
                face.rect[1] + face.rect[3] * 0.5,
            ]
        };
        let (center_a, center_b) = (center(a), center(b));
        center_a[0]
            .total_cmp(&center_b[0])
            .then_with(|| center_a[1].total_cmp(&center_b[1]))
            .then_with(|| (b.rect[2] * b.rect[3]).total_cmp(&(a.rect[2] * a.rect[3])))
    });
}

fn sample_rgb(image: &RgbaImage, x: f32, y: f32) -> [f32; 3] {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let mut output = [0.0; 3];
    for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
        for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
            let weight = wx * wy;
            if weight == 0.0 {
                continue;
            }
            let px = (x0 + dx).clamp(0, image.width() as i32 - 1) as u32;
            let py = (y0 + dy).clamp(0, image.height() as i32 - 1) as u32;
            let pixel = image.get_pixel(px, py);
            output[0] += pixel[0] as f32 * weight;
            output[1] += pixel[1] as f32 * weight;
            output[2] += pixel[2] as f32 * weight;
        }
    }
    output
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

#[derive(Clone, Copy, Debug)]
struct CropTransform {
    center: [f32; 2],
    size: f32,
    angle: f32,
    source_size: [f32; 2],
}

impl CropTransform {
    fn image_to_tensor(self, image: &RgbaImage) -> Vec<f32> {
        let size = LANDMARK_SIZE as usize;
        let mut output = vec![0.0; size * size * 3];
        let (sin, cos) = self.angle.sin_cos();
        for y in 0..size {
            for x in 0..size {
                let dx = (x as f32 + 0.5) / size as f32 - 0.5;
                let dy = (y as f32 + 0.5) / size as f32 - 0.5;
                let source = [
                    self.center[0] + (dx * cos - dy * sin) * self.size,
                    self.center[1] + (dx * sin + dy * cos) * self.size,
                ];
                if source[0] < 0.0
                    || source[1] < 0.0
                    || source[0] >= self.source_size[0]
                    || source[1] >= self.source_size[1]
                {
                    continue;
                }
                let color = sample_rgb(image, source[0] - 0.5, source[1] - 0.5);
                let index = (y * size + x) * 3;
                output[index] = color[0] / 255.0;
                output[index + 1] = color[1] / 255.0;
                output[index + 2] = color[2] / 255.0;
            }
        }
        output
    }

    fn crop_point_to_image(self, point: [f32; 2]) -> [f32; 2] {
        let dx = point[0] / LANDMARK_SIZE as f32 - 0.5;
        let dy = point[1] / LANDMARK_SIZE as f32 - 0.5;
        let (sin, cos) = self.angle.sin_cos();
        [
            self.center[0] + (dx * cos - dy * sin) * self.size,
            self.center[1] + (dx * sin + dy * cos) * self.size,
        ]
    }
}

fn detector_crop(face: &DetectedFace) -> Result<CropTransform> {
    let delta = [
        face.right_eye[0] - face.left_eye[0],
        face.right_eye[1] - face.left_eye[1],
    ];
    if delta[0].abs() + delta[1].abs() < f32::EPSILON {
        bail!("Detected face has no usable eye axis");
    }
    Ok(CropTransform {
        center: [
            face.rect[0] + face.rect[2] * 0.5,
            face.rect[1] + face.rect[3] * 0.5,
        ],
        size: face.rect[2].max(face.rect[3]) * 1.5,
        angle: delta[1].atan2(delta[0]),
        source_size: [f32::INFINITY, f32::INFINITY],
    })
}

fn refined_crop(points: &[[f32; 3]], face: &DetectedFace) -> Result<CropTransform> {
    const OVAL: [usize; 36] = [
        10, 338, 297, 332, 284, 251, 389, 356, 454, 323, 361, 288, 397, 365, 379, 378, 400, 377,
        152, 148, 176, 149, 150, 136, 172, 58, 132, 93, 234, 127, 162, 21, 54, 103, 67, 109,
    ];
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for index in OVAL {
        let point = points
            .get(index)
            .context("Face Mesh oval index is missing")?;
        min[0] = min[0].min(point[0]);
        min[1] = min[1].min(point[1]);
        max[0] = max[0].max(point[0]);
        max[1] = max[1].max(point[1]);
    }
    let left_eye = points.get(33).context("Face Mesh left eye is missing")?;
    let right_eye = points.get(263).context("Face Mesh right eye is missing")?;
    let delta = [right_eye[0] - left_eye[0], right_eye[1] - left_eye[1]];
    let angle = if delta[0].abs() + delta[1].abs() < f32::EPSILON {
        (face.right_eye[1] - face.left_eye[1]).atan2(face.right_eye[0] - face.left_eye[0])
    } else {
        delta[1].atan2(delta[0])
    };
    Ok(CropTransform {
        center: [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5],
        size: (max[0] - min[0]).max(max[1] - min[1]) * 1.5,
        angle,
        source_size: [f32::INFINITY, f32::INFINITY],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_models_load_with_expected_io() -> anyhow::Result<()> {
        let models = PortraitModels::load()?;
        assert_eq!(models.detector_input_shape(), [1, 128, 128, 3]);
        assert_eq!(models.detector_output_shapes(), ([1, 896, 16], [1, 896, 1]));
        assert_eq!(models.landmark_input_shape(), [1, 256, 256, 3]);
        assert_eq!(models.landmark_output_shape(), [1, 1, 1, 1434]);
        Ok(())
    }

    #[test]
    fn decode_detections_uses_scores_and_eye_keypoints() {
        let mut regressors = vec![0.0f32; 896 * 16];
        let mut scores = vec![-20.0f32; 896];
        regressors[0..16].copy_from_slice(&[
            0.5, 0.5, 0.1, 0.2, 0.30, 0.5, 0.70, 0.5, 0.2, 0.2, 0.8, 0.2, 0.0, 0.0, 0.0, 0.0,
        ]);
        scores[0] = 2.0;
        let faces = decode_detections(&regressors, &scores, 0.5).unwrap();
        assert_eq!(faces.len(), 1);
        assert!((faces[0].score - 0.880_797).abs() < 0.000_1);
        assert!(faces[0].right_eye[0] > faces[0].left_eye[0]);
    }

    #[test]
    fn weighted_nms_suppresses_overlapping_boxes() {
        let boxes = vec![
            FaceBox {
                rect: [0.0, 0.0, 100.0, 100.0],
                score: 0.9,
                left_eye: [25.0, 35.0],
                right_eye: [75.0, 35.0],
            },
            FaceBox {
                rect: [4.0, 4.0, 100.0, 100.0],
                score: 0.8,
                left_eye: [25.0, 35.0],
                right_eye: [75.0, 35.0],
            },
            FaceBox {
                rect: [200.0, 0.0, 100.0, 100.0],
                score: 0.7,
                left_eye: [225.0, 35.0],
                right_eye: [275.0, 35.0],
            },
        ];
        let kept = weighted_nms(boxes, 0.3);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn letterbox_rgb_preserves_aspect_ratio() {
        let image = RgbaImage::from_pixel(64, 32, image::Rgba([255, 0, 0, 255]));
        let (pixels, transform) = letterbox_rgb(&image, 128);
        assert_eq!(transform.scale, 2.0);
        assert_eq!(transform.pad, [0.0, 32.0]);
        assert_eq!(&pixels[0..3], &[0.0, 0.0, 0.0]);
        let center = (64 * 128 + 64) * 3;
        assert_eq!(&pixels[center..center + 3], &[1.0, -1.0, -1.0]);
    }

    #[test]
    fn zero_input_runs_are_finite() -> anyhow::Result<()> {
        let models = PortraitModels::load()?;
        let (regressors, scores) = models.run_detector_zeros()?;
        assert_eq!(regressors.len(), 896 * 16);
        assert_eq!(scores.len(), 896);
        assert!(regressors.iter().chain(&scores).all(|v| v.is_finite()));
        let (landmarks, presence) = models.run_landmarks_zeros()?;
        assert_eq!(landmarks.len(), 478 * 3);
        assert!(landmarks.iter().all(|v| v.is_finite()));
        assert!(presence.is_finite());
        Ok(())
    }

    #[test]
    fn embedded_models_match_pinned_hashes() {
        use sha2::{Digest, Sha256};
        for (bytes, expected) in [
            (
                DETECTOR_MODEL,
                "02a04d5d37c3558dc4d5274f7f8f0f0f01ac94e46c5ffb2cee82395d47e23181",
            ),
            (
                LANDMARK_MODEL,
                "7d6e82dee82a1dca5fbddb282b3cc74571833a530de317fc22ae325c3358beeb",
            ),
        ] {
            let actual = format!("{:x}", Sha256::digest(bytes));
            assert_eq!(actual, expected);
        }
    }
}
