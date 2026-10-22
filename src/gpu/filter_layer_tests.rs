use super::tests::{compare, readback};
use super::*;
use crate::{document::Mask, effects::Filter};
use image::Rgba;

/// Drives the filter compute pass directly, so a shader regression cannot be
/// hidden by compositing tolerance.
#[test]
#[ignore = "requires a Vulkan or OpenGL compute adapter"]
fn filter_layer_shader_matches_cpu_reference() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let size = [24u32, 16u32];
    let image = RgbaImage::from_fn(size[0], size[1], |x, y| {
        Rgba([
            (x * 11) as u8,
            (y * 7) as u8,
            (x * 3 + y * 5) as u8,
            if x < 12 { 255 } else { 72 },
        ])
    });
    let source = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("filter shader source"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        image.as_raw(),
    );
    let mut layers = super::filter_layers::FilterLayers::new(&device, size);
    for filter in [
        Filter::GaussianBlur { radius: 1.5 },
        Filter::UnsharpMask {
            amount: 80.0,
            radius: 1.5,
            threshold: 2.0,
        },
        Filter::HighPass { radius: 3.0 },
        Filter::Posterize { levels: 6 },
        Filter::Threshold { level: 128.0 },
        Filter::Pixelate { size: 5.0 },
        Filter::Vignette {
            amount: 45.0,
            midpoint: 50.0,
            softness: 30.0,
        },
    ] {
        let mut encoder = device.create_command_encoder(&Default::default());
        let output = layers
            .render(&device, &mut encoder, &source, &filter, false)
            .clone();
        let stride = (size[0] * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("filter shader readback"),
            size: u64::from(stride * size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap()
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receive.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range();
        let actual: Vec<u8> = mapped
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..size[0] as usize * 4].iter().copied())
            .collect();
        drop(mapped);
        buffer.unmap();

        let expected = crate::effects::filtered(&image, &filter);
        let mut worst = 0;
        let mut worst_pixel = 0;
        for (index, (a, e)) in actual
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.pixels())
            .enumerate()
        {
            // The compute pass writes straight alpha.
            let e = [e[0], e[1], e[2], e[3]];
            for (a, e) in a.iter().zip(e) {
                if a.abs_diff(e) > worst {
                    worst = a.abs_diff(e);
                    worst_pixel = index;
                }
            }
        }
        eprintln!(
            "{filter:?}: max error {worst} at ({}, {})",
            worst_pixel % size[0] as usize,
            worst_pixel / size[0] as usize
        );
        assert!(worst <= 3, "{filter:?}: shader error {worst}");
    }
}

#[test]
#[ignore = "requires a Vulkan or OpenGL compute adapter"]
fn filter_layers_match_cpu_and_reuse_sources() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let processor = Processor::new(device.clone(), queue.clone());
    let mut compositor = GpuCompositor::new(device, queue);
    let mut document = Document::new(160, 144).unwrap();
    let source = Layer::image(
        "Source",
        RgbaImage::from_fn(160, 144, |x, y| {
            Rgba([
                x as u8,
                y as u8,
                (x + y) as u8,
                if x < 80 { 255 } else { 64 },
            ])
        }),
    );
    let mut group = Layer::blank("Group", 160, 144);
    group.group = true;
    group.opacity = 0.7;
    let mut effect = Layer::blank("Filter", 160, 144);
    effect.parent = Some(group.id);
    effect.opacity = 0.8;
    effect.mask = Some(Mask {
        pixels: Arc::new(image::GrayImage::from_fn(160, 144, |x, _| {
            image::Luma([if x < 48 { 0 } else { 192 }])
        })),
        ..Mask::white()
    });
    let mut mask = Layer::mask("Group mask", 160, 144);
    mask.parent = Some(group.id);
    mask.opacity = 0.4;
    mask.mask.as_mut().unwrap().pixels =
        Arc::new(image::GrayImage::from_pixel(1, 1, image::Luma([192])));
    let mut adjustment = Layer::blank("Invert", 160, 144);
    adjustment.adjustment = Some(Adjustment::Invert);
    document.select(source.id, false);
    document.layers = vec![source, group, effect, mask, adjustment];

    for filter in [
        Filter::GaussianBlur { radius: 0.5 },
        Filter::GaussianBlur { radius: 1.5 },
        Filter::GaussianBlur { radius: 2.5 },
        Filter::GaussianBlur { radius: 4.0 },
        Filter::GaussianBlur { radius: 35.0 },
        Filter::MotionBlur {
            distance: 15.0,
            angle: 32.0,
        },
        Filter::MotionBlur {
            distance: 200.0,
            angle: -120.0,
        },
        Filter::Noise {
            amount: 24.0,
            monochrome: false,
        },
        Filter::Noise {
            amount: 24.0,
            monochrome: true,
        },
        Filter::LensCorrection {
            distortion: 22.0,
            vignette: 30.0,
        },
        Filter::LensCorrection {
            distortion: -22.0,
            vignette: -30.0,
        },
        Filter::UnsharpMask {
            amount: 80.0,
            radius: 1.5,
            threshold: 2.0,
        },
        Filter::UnsharpMask {
            amount: 200.0,
            radius: 4.0,
            threshold: 0.0,
        },
        Filter::HighPass { radius: 3.0 },
        // Posterize and Threshold are exact on a shared input; the compositor
        // backdrops differ by sub-byte amounts, so they are covered by
        // `filter_layer_shader_matches_cpu_reference` instead.
        Filter::Pixelate { size: 7.0 },
        Filter::Vignette {
            amount: 45.0,
            midpoint: 50.0,
            softness: 30.0,
        },
        Filter::Vignette {
            amount: -40.0,
            midpoint: 20.0,
            softness: 60.0,
        },
    ] {
        document.layers[2].filter = Some(filter.clone());
        document.validate().unwrap();
        for size in [[160, 144], [80, 72]] {
            // Repeated changes must reuse the source upload, even though filtering changes.
            scope(Some(processor.clone()), || {
                compositor.render(&document, size)
            });
            let textures: Vec<_> = compositor
                .sources
                .iter()
                .map(|(key, source)| (*key, source.texture.clone()))
                .collect();
            let actual = readback(&compositor);
            let expected = render::render_scaled(&document, size[0], size[1]);
            for (index, (a, b)) in actual
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.pixels())
                .enumerate()
            {
                let expected = [
                    (b[0] as f32 * b[3] as f32 / 255.0).round() as u8,
                    (b[1] as f32 * b[3] as f32 / 255.0).round() as u8,
                    (b[2] as f32 * b[3] as f32 / 255.0).round() as u8,
                    b[3],
                ];
                assert!(
                    a.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3),
                    "{filter:?} {size:?} pixel {index}: GPU {a:?}, CPU {expected:?}"
                );
            }
            document.layers[2].opacity = 0.5;
            compositor.render(&document, size);
            for (key, texture) in textures {
                assert_eq!(compositor.sources[&key].texture, texture);
            }
            document.layers[2].opacity = 0.8;
        }
        let actual = processor.compose(&document, 160, 144).unwrap();
        let expected = render::render(&document);
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            assert!(
                a.0.iter().zip(b.0).all(|(a, b)| a.abs_diff(b) <= 4),
                "{filter:?}: GPU {a:?}, CPU {b:?}"
            );
        }
    }

    let mut second = Layer::blank("Second filter", 160, 144);
    second.filter = Some(Filter::MotionBlur {
        distance: 9.0,
        angle: 45.0,
    });
    document.layers.push(second);
    for hidden in [None, Some(1), Some(2), Some(5)] {
        if let Some(index) = hidden {
            document.layers[index].visible = false;
        }
        for accelerated in [false, true] {
            scope(accelerated.then(|| processor.clone()), || {
                compositor.render(&document, [160, 144]);
            });
            compare(
                &document,
                &readback(&compositor),
                "Stacked filters and visibility",
            );
        }
        if let Some(index) = hidden {
            document.layers[index].visible = true;
        }
    }
}

/// Reads an f32 filter target back as 16-bit samples.
fn read_high(output: &wgpu::Texture, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u16> {
    let size = [output.width(), output.height()];
    let stride = (size[0] * 16).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("high filter readback"),
        size: u64::from(stride * size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(size[1]),
            },
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap()
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    let mut samples = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for row in mapped.chunks_exact(stride as usize) {
        for x in 0..size[0] as usize {
            for channel in 0..4 {
                let offset = x * 16 + channel * 4;
                let value = f32::from_le_bytes([
                    row[offset],
                    row[offset + 1],
                    row[offset + 2],
                    row[offset + 3],
                ]);
                samples.push((value.clamp(0.0, 1.0) * 65_535.0).round() as u16);
            }
        }
    }
    drop(mapped);
    buffer.unmap();
    samples
}

#[test]
#[ignore = "requires a Vulkan or OpenGL compute adapter"]
fn high_precision_filter_layers_match_sixteen_bit_cpu() {
    use crate::document::Rgba16Image;
    use image::Rgba;

    let (device, queue) = super::tests::high_depth_device();
    let size = [24u32, 16u32];
    let source16 = Rgba16Image::from_fn(size[0], size[1], |x, y| {
        Rgba([
            ((x * 4099 + y * 37 + 1237) % 60_000 + 2_817) as u16,
            ((x * 733 + y * 7919 + 42) % 60_000 + 2_817) as u16,
            ((x * 9001 + y * 113 + 31_000) % 60_000 + 2_817) as u16,
            65_535,
        ])
    });
    let floats: Vec<f32> = source16
        .as_raw()
        .iter()
        .map(|value| *value as f32 / 65_535.0)
        .collect();
    let source = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: Some("high filter source"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::cast_slice(&floats),
    );
    let mut layers = super::filter_layers::FilterLayers::new(&device, size);

    // A zero-radius blur is the identity; the old byte path rounded every
    // sample to 8 bits, so this asserts the high pipeline keeps 16-bit data.
    let mut encoder = device.create_command_encoder(&Default::default());
    let output = layers
        .render(
            &device,
            &mut encoder,
            &source,
            &Filter::GaussianBlur { radius: 0.01 },
            true,
        )
        .clone();
    assert_eq!(output.format(), wgpu::TextureFormat::Rgba32Float);
    queue.submit([encoder.finish()]);
    let actual = read_high(&output, &device, &queue);
    let maximum = actual
        .iter()
        .zip(source16.as_raw())
        .map(|(actual, expected)| actual.abs_diff(*expected))
        .max()
        .unwrap();
    eprintln!("identity high filter maximum delta: {maximum}");
    assert!(maximum <= 1, "identity high filter delta {maximum}");

    // Posterize still rounds values, but at 16 bits rather than 8.
    let levels = 4.0f32;
    let mut encoder = device.create_command_encoder(&Default::default());
    let output = layers
        .render(
            &device,
            &mut encoder,
            &source,
            &Filter::Posterize { levels: 4 },
            true,
        )
        .clone();
    queue.submit([encoder.finish()]);
    let actual = read_high(&output, &device, &queue);
    let mut worst = 0u16;
    for (index, (pixel, expected)) in actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(source16.pixels())
        .enumerate()
    {
        for channel in 0..3 {
            let value = expected[channel] as f32 / 65_535.0;
            let step = (value * levels).floor().min(levels - 1.0);
            let expected = (step / (levels - 1.0) * 65_535.0)
                .round()
                .clamp(0.0, 65_535.0) as u16;
            worst = worst.max(pixel[channel].abs_diff(expected));
            if pixel[channel].abs_diff(expected) > 1 {
                panic!(
                    "posterize pixel {index} channel {channel}: {} vs {expected}",
                    pixel[channel]
                );
            }
        }
        worst = worst.max(pixel[3].abs_diff(expected[3]));
    }
    eprintln!("sixteen-bit posterize maximum delta: {worst}");
    assert!(worst <= 1, "posterize high filter delta {worst}");
}

#[test]
#[ignore = "requires a Vulkan or OpenGL compute adapter"]
fn filter_layer_preserves_sixteen_bit_document_precision() {
    use crate::document::Rgba16Image;
    use image::Rgba;

    let (device, queue) = super::tests::high_depth_device();
    let processor = Processor::new(device, queue);
    // Above the 16_384-pixel GPU threshold so `compose16` takes the f32 path.
    let (width, height) = (160u32, 110u32);
    let source = Rgba16Image::from_fn(width, height, |x, y| {
        Rgba([
            ((x * 4001 + y * 37 + 1234) % 65_536) as u16,
            ((x * 733 + y * 5903 + 42) % 65_536) as u16,
            ((x * 9001 + y * 113 + 31_000) % 65_536) as u16,
            if x < 9 { 40_000 } else { 65_535 },
        ])
    });
    let mut document = Document::new(width, height).unwrap();
    let mut filter = Layer::blank("Filter", width, height);
    filter.filter = Some(Filter::GaussianBlur { radius: 0.01 });
    document.layers = vec![Layer::image16("Deep", source.clone()), filter];
    document.active = Some(document.layers[0].id);
    document.validate().unwrap();
    let result = scope(Some(processor), || crate::render::render16(&document));
    let maximum = result
        .as_raw()
        .iter()
        .zip(source.as_raw())
        .map(|(actual, expected)| actual.abs_diff(*expected))
        .max()
        .unwrap();
    eprintln!("16-bit filter layer maximum delta: {maximum}");
    assert!(maximum <= 1, "16-bit filter layer delta {maximum}");
}
