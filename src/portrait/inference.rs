use image::RgbaImage;
use tract::prelude::*;

const DETECTOR_MODEL: &[u8] = include_bytes!("../../assets/models/blazeface.onnx");
const LANDMARK_MODEL: &[u8] = include_bytes!("../../assets/models/face_landmarks.onnx");

pub struct PortraitModels {
    detector: Runnable,
    landmarks: Runnable,
}

impl PortraitModels {
    pub fn load() -> anyhow::Result<Self> {
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
        [1, 128, 128, 3]
    }

    pub fn detector_output_shapes(&self) -> ([usize; 3], [usize; 3]) {
        ([1, 896, 16], [1, 896, 1])
    }

    pub fn landmark_input_shape(&self) -> [usize; 4] {
        [1, 256, 256, 3]
    }

    pub fn landmark_output_shape(&self) -> [usize; 4] {
        [1, 1, 1, 1434]
    }

    pub fn detect(&self, _image: &RgbaImage) -> anyhow::Result<Vec<DetectedFace>> {
        let _ = &self.detector;
        anyhow::bail!("portrait detection implementation is added in the next task")
    }

    pub fn landmarks(
        &self,
        _image: &RgbaImage,
        _face: &DetectedFace,
    ) -> anyhow::Result<FaceLandmarks> {
        let _ = &self.landmarks;
        anyhow::bail!("portrait landmark implementation is added in the next task")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetectedFace;

#[derive(Clone, Debug, PartialEq)]
pub struct FaceLandmarks;

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
