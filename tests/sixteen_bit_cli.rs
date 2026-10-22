use std::process::Command;

use image::Rgba;

type Rgba16Image = image::ImageBuffer<Rgba<u16>, Vec<u16>>;

fn sample(width: u32, height: u32) -> Rgba16Image {
    Rgba16Image::from_fn(width, height, |x, y| {
        Rgba([
            (x * 5000 + y * 31 + 1234) as u16,
            (x * 777 + y * 6001 + 42) as u16,
            (x * 9001 + y * 123 + 40_000) as u16,
            if (x * 3 + y) % 7 == 0 { 20_000 } else { 65_535 },
        ])
    })
}

#[test]
fn batch_preserves_sixteen_bit_png_samples() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("in");
    let output = directory.path().join("out");
    std::fs::create_dir_all(&input).unwrap();
    let source = sample(9, 7);
    image::DynamicImage::ImageRgba16(source.clone())
        .save(input.join("deep.png"))
        .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_xiaozhu-xiuxiu"))
        .args([
            "--batch",
            input.to_str().unwrap(),
            "--out",
            output.to_str().unwrap(),
            "--format",
            "png",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let bytes = std::fs::read(output.join("deep.png")).unwrap();
    let decoded = image::load_from_memory(&bytes).unwrap();
    assert_eq!(
        decoded.color(),
        image::ColorType::Rgba16,
        "batch output was quantized to 8 bits"
    );
    assert_eq!(decoded.to_rgba16(), source, "16-bit samples changed");
}
