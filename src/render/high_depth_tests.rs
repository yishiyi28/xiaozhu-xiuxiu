use super::*;
use image::Rgba;

#[test]
fn cpu_fallback_preserves_sixteen_bit_samples() {
    let source = Rgba16Image::from_fn(8, 8, |x, y| {
        Rgba([
            ((1_000 + x * 3_000 + y * 37) % 65_536) as u16,
            ((2_000 + x * 1_001 + y * 59) % 65_536) as u16,
            ((3_000 + x * 2_003 + y * 71) % 65_536) as u16,
            65_535,
        ])
    });
    let mut document = Document::new(8, 8).unwrap();
    document.layers = vec![Layer::image16("Deep", source.clone())];
    document.active = Some(document.layers[0].id);
    let rendered = render16(&document);
    let maximum = rendered
        .as_raw()
        .iter()
        .zip(source.as_raw())
        .map(|(actual, expected)| actual.abs_diff(*expected))
        .max()
        .unwrap();
    eprintln!("CPU 16-bit fallback maximum delta: {maximum}");
    assert!(maximum <= 1, "CPU fallback delta {maximum}");

    let pixel = pixel_at(&document, Point::new(0.5, 0.5));
    let expected = source
        .get_pixel(0, 0)
        .0
        .map(|value| value as f32 / 65_535.0);
    for (actual, expected) in pixel.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-6, "{pixel:?} vs {expected}");
    }
}

#[test]
fn cpu_fallback_composites_high_depth_layers_in_float() {
    let bottom = Rgba16Image::from_pixel(4, 4, Rgba([10_000, 20_000, 30_000, 65_535]));
    let top_pixel = Rgba([50_000, 1_000, 4_000, 32_768]);
    let mut top = Layer::image16("Top", Rgba16Image::from_pixel(4, 4, top_pixel));
    top.opacity = 0.5;
    let mut document = Document::new(4, 4).unwrap();
    document.layers = vec![Layer::image16("Bottom", bottom.clone()), top];
    document.active = Some(document.layers[1].id);
    let rendered = render16(&document);
    let mut source = top_pixel.0.map(|value| value as f32 / 65_535.0);
    source[3] *= 0.5;
    let expected = composite(
        bottom
            .get_pixel(0, 0)
            .0
            .map(|value| value as f32 / 65_535.0),
        source,
        crate::blend::BlendMode::Normal,
    )
    .map(|value| (value.clamp(0.0, 1.0) * 65_535.0).round() as u16);
    let actual = rendered.get_pixel(0, 0).0;
    eprintln!("CPU float composite: {actual:?} vs {expected:?}");
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.abs_diff(expected) <= 1)
    );
}

#[test]
fn cpu_filter_layers_use_sixteen_bit_backdrop() {
    let mut document = Document::new(8, 8).unwrap();
    let background = Layer::image16(
        "Deep",
        Rgba16Image::from_pixel(8, 8, Rgba([32_768, 32_768, 32_768, 65_535])),
    );
    let mut filter = Layer::blank("Threshold", 8, 8);
    filter.filter = Some(crate::effects::Filter::Threshold { level: 128.0 });
    document.active = Some(filter.id);
    document.layers = vec![background, filter];
    let rendered = render16(&document);
    let pixel = rendered.get_pixel(0, 0).0;
    eprintln!("CPU high-depth filter layer: {pixel:?}");
    assert_eq!(pixel, [0, 0, 0, 65_535]);
}
