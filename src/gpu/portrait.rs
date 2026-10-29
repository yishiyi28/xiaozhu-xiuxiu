use image::{GrayImage, RgbaImage};

use crate::portrait::{masks::PortraitMasks, retouch::PortraitSettings};

use super::Processor;

pub(crate) struct PortraitPipeline {
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) pipeline: wgpu::ComputePipeline,
}

impl PortraitPipeline {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xuan portrait retouch"),
            source: wgpu::ShaderSource::Wgsl(include_str!("portrait.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("xuan portrait layout"),
            entries: &[
                storage_entry(0, false),
                storage_entry(1, false),
                storage_entry(2, false),
                storage_entry(3, true),
                storage_entry(4, false),
            ],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("xuan portrait pipeline"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("xuan portrait pipeline layout"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                }),
            ),
            module: &shader,
            entry_point: Some("portrait"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { layout, pipeline }
    }
}

fn storage_entry(binding: u32, read_write: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage {
                read_only: !read_write,
            },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(crate) fn apply(
    processor: &Processor,
    source: &RgbaImage,
    masks: &PortraitMasks,
    settings: &PortraitSettings,
) -> Option<RgbaImage> {
    if source.width() == 0 || source.height() == 0 {
        return None;
    }
    for mask in [
        &masks.face,
        &masks.skin,
        &masks.eyes,
        &masks.lips,
        &masks.teeth,
    ] {
        if mask.dimensions() != source.dimensions() {
            return None;
        }
    }
    settings.validate().ok()?;
    if settings.is_retouch_identity() {
        return Some(source.clone());
    }
    let radius = (1.0 + settings.skin_smooth / 18.0).round().max(1.0);
    let blurred =
        if settings.skin_smooth > 0.0 || settings.eye_clarity > 0.0 || settings.blemish > 0.0 {
            crate::gpu::filter(source, &crate::effects::Filter::GaussianBlur { radius })?
        } else {
            source.clone()
        };
    let packed_masks = pack_masks(masks);
    let average = skin_average(source, &masks.skin);
    let config = [
        [source.width() as f32, source.height() as f32, 0.0, 0.0],
        [
            settings.skin_smooth,
            settings.skin_texture,
            settings.skin_tone,
            settings.blemish,
        ],
        [
            settings.eye_brightness,
            settings.eye_clarity,
            settings.dark_circles,
            settings.teeth_whiten,
        ],
        [
            settings.lip_tint,
            settings.lip_color[0] as f32 / 255.0,
            settings.lip_color[1] as f32 / 255.0,
            settings.lip_color[2] as f32 / 255.0,
        ],
        [average[0], average[1], average[2], 0.0],
    ];
    let source_buffer = processor.buffer(source.as_raw()).ok()?;
    let blurred_buffer = processor.buffer(blurred.as_raw()).ok()?;
    let mask_bytes = bytemuck::cast_slice(&packed_masks);
    let mask_buffer = processor.buffer(mask_bytes).ok()?;
    let output_buffer = processor.empty(source.as_raw().len() as u64).ok()?;
    let config_buffer = processor.buffer(bytemuck::cast_slice(&config)).ok()?;
    let mut cache = processor.portrait.lock().ok()?;
    if cache.is_none() {
        *cache = Some(PortraitPipeline::new(&processor.device));
    }
    let pipeline = cache.as_ref()?;
    let bind_group = processor
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xuan portrait bind group"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: blurred_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: mask_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: output_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: config_buffer.as_entire_binding(),
                },
            ],
        });
    let mut encoder = processor.encoder();
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("xuan portrait pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(source.width().div_ceil(8), source.height().div_ceil(8), 1);
    }
    let bytes = processor
        .read(encoder, &output_buffer, source.as_raw().len() as u64)
        .ok()?;
    RgbaImage::from_raw(source.width(), source.height(), bytes)
}

fn pack_masks(masks: &PortraitMasks) -> Vec<u32> {
    let (width, height) = masks.skin.dimensions();
    let mut packed = Vec::with_capacity((width * height) as usize);
    for (x, y, skin) in masks.skin.enumerate_pixels() {
        let eyes = masks.eyes.get_pixel(x, y)[0];
        let lips = masks.lips.get_pixel(x, y)[0];
        let teeth = masks.teeth.get_pixel(x, y)[0];
        packed.push(
            u32::from(skin[0])
                | (u32::from(eyes) << 8)
                | (u32::from(lips) << 16)
                | (u32::from(teeth) << 24),
        );
    }
    packed
}

fn skin_average(image: &RgbaImage, mask: &GrayImage) -> [f32; 3] {
    let mut sum = [0.0; 3];
    let mut count = 0.0;
    for (x, y, pixel) in image.enumerate_pixels() {
        let weight = mask.get_pixel(x, y)[0] as f32 / 255.0;
        if weight > 0.0 {
            for channel in 0..3 {
                sum[channel] += pixel[channel] as f32 * weight;
            }
            count += weight;
        }
    }
    if count == 0.0 {
        [128.0 / 255.0; 3]
    } else {
        sum.map(|value| value / count / 255.0)
    }
}
