#[derive(Clone, Debug, PartialEq)]
pub struct DetectedFace {
    pub id: u32,
    /// Face rectangle in source-image pixels: x, y, width, height.
    pub rect: [f32; 4],
    pub score: f32,
    pub left_eye: [f32; 2],
    pub right_eye: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct LandmarkPoint {
    /// Landmark centre in source-image pixels.
    pub position: [f32; 2],
    /// Relative depth returned by Face Mesh; negative values are closer.
    pub depth: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceLandmarks {
    pub face: DetectedFace,
    pub points: Vec<LandmarkPoint>,
    pub presence: f32,
}
