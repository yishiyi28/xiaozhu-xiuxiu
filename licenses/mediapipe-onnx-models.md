# MediaPipe ONNX models

The application embeds ONNX conversions of Google MediaPipe face models.

## BlazeFace short-range face detector

- Upstream model: Google MediaPipe BlazeFace short-range
- ONNX conversion: https://huggingface.co/fernandotonon/QtMeshEditor-blazeface-onnx
- Source file: `face_detector.onnx`
- SHA-256: `02a04d5d37c3558dc4d5274f7f8f0f0f01ac94e46c5ffb2cee82395d47e23181`
- License: Apache License 2.0

## Face Mesh V2 and iris landmarks

- Upstream model: Google MediaPipe Face Mesh V2 with iris landmarks
- ONNX conversion: https://huggingface.co/senty-au/face_landmarks_detector-ONNX
- Source file: `onnx/model.onnx`
- SHA-256: `7d6e82dee82a1dca5fbddb282b3cc74571833a530de317fc22ae325c3358beeb`
- License: Apache License 2.0

The model weights are Copyright Google LLC and are redistributed under the
Apache License 2.0. See `mediapipe-apache-2.0.txt`. The ONNX files are format
conversions only; model weights are unchanged.
