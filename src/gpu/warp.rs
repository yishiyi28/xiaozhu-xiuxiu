use image::RgbaImage;

use crate::portrait::warp::WarpGrid;

use super::Processor;

pub(crate) struct WarpPipeline {
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) pipeline: wgpu::ComputePipeline,
}

impl WarpPipeline {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xuan portrait warp"),
            source: wgpu::ShaderSource::Wgsl(include_str!("warp.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("xuan portrait warp layout"),
            entries: &[
                storage_entry(0, false),
                storage_entry(1, false),
                storage_entry(2, true),
                storage_entry(3, false),
            ],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("xuan portrait warp pipeline"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("xuan portrait warp pipeline layout"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                }),
            ),
            module: &shader,
            entry_point: Some("warp"),
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
    grid: &WarpGrid,
) -> Option<RgbaImage> {
    let size = [source.width(), source.height()];
    if size != grid.size() || size.contains(&0) {
        return None;
    }
    let cols = grid.cols() as u32;
    let rows = grid.rows() as u32;
    let config = [
        [
            size[0] as f32,
            size[1] as f32,
            grid.cell_size(),
            cols as f32,
        ],
        [rows as f32, 0.0, 0.0, 0.0],
    ];
    let source_buffer = processor.buffer(source.as_raw()).ok()?;
    let grid_buffer = processor
        .buffer(bytemuck::cast_slice(grid.displacements()))
        .ok()?;
    let output_buffer = processor.empty(source.as_raw().len() as u64).ok()?;
    let config_buffer = processor.buffer(bytemuck::cast_slice(&config)).ok()?;
    let mut cache = processor.warp.lock().ok()?;
    if cache.is_none() {
        *cache = Some(WarpPipeline::new(&processor.device));
    }
    let pipeline = cache.as_ref()?;
    let bind_group = processor
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xuan portrait warp bind group"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: grid_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: config_buffer.as_entire_binding(),
                },
            ],
        });
    let mut encoder = processor.encoder();
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("xuan portrait warp pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
    }
    let bytes = processor
        .read(encoder, &output_buffer, source.as_raw().len() as u64)
        .ok()?;
    RgbaImage::from_raw(size[0], size[1], bytes)
}
