use image::imageops::FilterType;
use wgpu::util::DeviceExt;

use crate::portrait::{layer::PortraitAdjustment, masks::PortraitMasks};

use super::target;

pub(super) struct PortraitLayers {
    pipeline: wgpu::ComputePipeline,
    high_pipeline: wgpu::ComputePipeline,
    output: wgpu::Texture,
    high_output: wgpu::Texture,
    scratch: wgpu::Texture,
    high_scratch: wgpu::Texture,
    layout: wgpu::BindGroupLayout,
    high_layout: wgpu::BindGroupLayout,
}

impl PortraitLayers {
    pub(super) fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let layout = make_layout(device, wgpu::TextureFormat::Rgba8Unorm);
        let high_layout = make_layout(device, wgpu::TextureFormat::Rgba32Float);
        Self {
            pipeline: pipeline(device, "rgba8unorm", &layout),
            high_pipeline: pipeline(device, "rgba32float", &high_layout),
            output: target(device, size, wgpu::TextureFormat::Rgba8Unorm, 1),
            high_output: target(device, size, wgpu::TextureFormat::Rgba32Float, 1),
            scratch: target(device, size, wgpu::TextureFormat::Rgba8Unorm, 1),
            high_scratch: target(device, size, wgpu::TextureFormat::Rgba32Float, 1),
            layout,
            high_layout,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        blurred: &wgpu::Texture,
        recipe: &PortraitAdjustment,
        high: bool,
    ) -> Option<wgpu::Texture> {
        let size = [source.width(), source.height()];
        if size[0] == 0 || size[1] == 0 {
            return None;
        }
        if [self.output.width(), self.output.height()] != size {
            self.output = target(device, size, wgpu::TextureFormat::Rgba8Unorm, 1);
            self.high_output = target(device, size, wgpu::TextureFormat::Rgba32Float, 1);
            self.scratch = target(device, size, wgpu::TextureFormat::Rgba8Unorm, 1);
            self.high_scratch = target(device, size, wgpu::TextureFormat::Rgba32Float, 1);
        }
        let masks = recipe.masks.as_ref()?;
        if masks.len() != recipe.faces.len() || recipe.settings.len() != recipe.faces.len() {
            return None;
        }
        let indices: Vec<usize> = if recipe.apply_to_all {
            (0..recipe.faces.len()).collect()
        } else if recipe.faces.is_empty() {
            Vec::new()
        } else {
            vec![recipe.selected]
        };
        if indices.is_empty() {
            return Some(source.clone());
        }
        let (first, second, pipeline, layout) = if high {
            (
                &self.high_output,
                &self.high_scratch,
                &self.high_pipeline,
                &self.high_layout,
            )
        } else {
            (&self.output, &self.scratch, &self.pipeline, &self.layout)
        };
        let mut current = source;
        let mut use_first = true;
        for index in indices {
            let settings = &recipe.settings[index];
            if settings.is_identity() {
                continue;
            }
            let mask_texture = mask_texture(device, queue, &masks[index], size);
            let config = config(size, settings, masks[index].skin_average);
            let config_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("xuan portrait texture config"),
                contents: bytemuck::cast_slice(&config),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let target_texture = if use_first { first } else { second };
            dispatch(
                device,
                encoder,
                pipeline,
                layout,
                current,
                blurred,
                &mask_texture,
                target_texture,
                &config_buffer,
            );
            current = target_texture;
            use_first = !use_first;
        }
        Some(current.clone())
    }
}

fn make_layout(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::BindGroupLayout {
    let texture = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("xuan portrait texture layout"),
        entries: &[
            texture(0),
            texture(1),
            texture(2),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn pipeline(
    device: &wgpu::Device,
    format: &str,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("xuan portrait texture"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("portrait_texture.wgsl")
                .replace("OUTPUT_FORMAT", format)
                .into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("xuan portrait texture pipeline layout"),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("xuan portrait texture pipeline"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("portrait_texture"),
        compilation_options: Default::default(),
        cache: None,
    })
}

fn mask_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    masks: &PortraitMasks,
    size: [u32; 2],
) -> wgpu::Texture {
    let resize = |mask: &image::GrayImage| {
        if [mask.width(), mask.height()] == size {
            mask.clone()
        } else {
            image::imageops::resize(mask, size[0], size[1], FilterType::Triangle)
        }
    };
    let skin = resize(&masks.skin);
    let eyes = resize(&masks.eyes);
    let lips = resize(&masks.lips);
    let teeth = resize(&masks.teeth);
    let mut bytes = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for index in 0..skin.as_raw().len() {
        let x = index as u32 % size[0];
        let y = index as u32 / size[0];
        bytes.push(skin.get_pixel(x, y)[0]);
        bytes.push(eyes.get_pixel(x, y)[0]);
        bytes.push(lips.get_pixel(x, y)[0]);
        bytes.push(teeth.get_pixel(x, y)[0]);
    }
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("xuan portrait masks"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &bytes,
    )
}

fn config(
    size: [u32; 2],
    settings: &crate::portrait::retouch::PortraitSettings,
    average: [f32; 3],
) -> [[f32; 4]; 5] {
    [
        [size[0] as f32, size[1] as f32, 0.0, 0.0],
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
    ]
}

#[allow(clippy::too_many_arguments)]
fn dispatch(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::Texture,
    blurred: &wgpu::Texture,
    mask: &wgpu::Texture,
    output: &wgpu::Texture,
    config: &wgpu::Buffer,
) {
    let source_view = source.create_view(&Default::default());
    let blurred_view = blurred.create_view(&Default::default());
    let mask_view = mask.create_view(&Default::default());
    let output_view = output.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("xuan portrait texture bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&blurred_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&output_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: config.as_entire_binding(),
            },
        ],
    });
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("xuan portrait texture pass"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.dispatch_workgroups(source.width().div_ceil(8), source.height().div_ceil(8), 1);
}
