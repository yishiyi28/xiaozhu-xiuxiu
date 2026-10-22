use super::*;
use crate::document::Mask;
use image::Luma;

/// Reads an R16Unorm coverage texture back as u16 samples.
fn read_coverage16(
    texture: &wgpu::Texture,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Vec<u16> {
    let size = [texture.width(), texture.height()];
    let stride = (size[0] * 2).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("coverage readback"),
        size: u64::from(stride * size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
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
    let mut samples = Vec::with_capacity((size[0] * size[1]) as usize);
    for row in mapped.chunks_exact(stride as usize) {
        for x in 0..size[0] as usize {
            samples.push(u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]));
        }
    }
    drop(mapped);
    buffer.unmap();
    samples
}

#[test]
#[ignore = "requires a Vulkan or OpenGL compute adapter"]
fn coverage_keeps_sixteen_bit_mask_products() {
    let (device, queue) = super::tests::high_depth_device();
    if !device
        .features()
        .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM)
    {
        eprintln!("adapter lacks TEXTURE_FORMAT_16BIT_NORM; byte fallback stays active");
        return;
    }
    let processor = Processor::new(device.clone(), queue.clone());
    let size = [16u32, 16u32];
    let mut document = Document::new(size[0], size[1]).unwrap();
    let mut group = Layer::blank("Group", size[0], size[1]);
    group.group = true;
    group.opacity = 0.3;
    let mut child = Layer::blank("Child", size[0], size[1]);
    child.parent = Some(group.id);
    child.mask = Some(Mask {
        pixels: Arc::new(image::GrayImage::from_pixel(1, 1, Luma([128]))),
        high: Some(Arc::new(crate::document::Luma16Image::from_pixel(
            1,
            1,
            Luma([32_768]),
        ))),
        ..Mask::white()
    });
    document.layers = vec![group, child];
    document.active = Some(document.layers[1].id);
    document.validate().unwrap();
    let texture = processor
        .coverage(&document, &document.layers[1], size)
        .unwrap();
    assert_eq!(texture.format(), wgpu::TextureFormat::R16Unorm);
    let samples = read_coverage16(&texture, &device, &queue);
    let expected = (0.3f32 * 32_768.0).round() as u16;
    let maximum = samples
        .iter()
        .map(|actual| actual.abs_diff(expected))
        .max()
        .unwrap();
    eprintln!(
        "coverage sample {expected} (8-bit preview would be {}), maximum delta {maximum}",
        (0.3f32 * 128.0 / 255.0 * 255.0).round() as u8
    );
    assert!(maximum <= 1, "coverage maximum delta {maximum}");
}

#[test]
#[ignore = "requires a Vulkan or OpenGL compute adapter"]
fn compose16_uses_fractional_group_and_mask_coverage() {
    use crate::document::Rgba16Image;
    use image::Rgba;

    let (device, queue) = super::tests::high_depth_device();
    let processor = Processor::new(device, queue);
    let (width, height) = (160u32, 110u32);
    let source = Rgba16Image::from_pixel(width, height, Rgba([65_535, 40_000, 20_000, 65_535]));
    let mut document = Document::new(width, height).unwrap();
    let mut group = Layer::blank("Group", width, height);
    group.group = true;
    group.opacity = 0.3;
    let mut layer = Layer::image16("Deep", source.clone());
    layer.parent = Some(group.id);
    layer.mask = Some(Mask {
        pixels: Arc::new(image::GrayImage::from_pixel(1, 1, Luma([128]))),
        high: Some(Arc::new(crate::document::Luma16Image::from_pixel(
            1,
            1,
            Luma([32_768]),
        ))),
        ..Mask::white()
    });
    document.layers = vec![group, layer];
    document.active = Some(document.layers[1].id);
    document.validate().unwrap();
    let result = scope(Some(processor), || crate::render::render16(&document));
    let expected_alpha = (0.3f32 * 32_768.0).round() as u16;
    let mut maximum = 0u16;
    for pixel in result.pixels() {
        for channel in 0..3 {
            maximum = maximum.max(pixel[channel].abs_diff(source.get_pixel(0, 0)[channel]));
        }
        maximum = maximum.max(pixel[3].abs_diff(expected_alpha));
    }
    eprintln!("compose16 masked alpha {expected_alpha}, maximum delta {maximum}");
    assert!(maximum <= 1, "compose16 masked delta {maximum}");
}
