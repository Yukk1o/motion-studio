use crate::Yuv420Frame;
use std::collections::HashMap;

struct Input {
    size: (u32, u32, u32, u32),
    y: wgpu::Texture,
    uv: wgpu::Texture,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
}
pub(crate) struct VideoGpu {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    inputs: HashMap<u64, Input>,
}
impl VideoGpu {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video YUV planes"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video SDR conversion"),
            source: wgpu::ShaderSource::Wgsl(include_str!("video_yuv.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video conversion layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video GPU YUV conversion"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if format == wgpu::TextureFormat::Rgba8Unorm { "fragment_main" } else { "fragment_srgb" }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        Self {
            layout,
            pipeline,
            inputs: HashMap::new(),
        }
    }
    pub fn bytes(&self) -> u64 {
        self.inputs
            .values()
            .map(|i| {
                u64::from(i.size.0) * u64::from(i.size.1)
                    + 2 * u64::from(i.size.2) * u64::from(i.size.3)
            })
            .sum()
    }
    pub fn replacement_bytes(&self, object: u64, frame: &Yuv420Frame) -> u64 {
        let previous = self.inputs.get(&object).map_or(0, |i| {
            u64::from(i.size.0) * u64::from(i.size.1)
                + 2 * u64::from(i.size.2) * u64::from(i.size.3)
        });
        self.bytes() - previous + frame.bytes() as u64
    }
    pub fn clear(&mut self) {
        self.inputs.clear();
    }
    pub fn remove(&mut self, object: u64) {
        self.inputs.remove(&object);
    }
    pub fn convert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        object: u64,
        frame: &Yuv420Frame,
        target: &wgpu::TextureView,
    ) {
        let (cw, ch) = frame.chroma_size();
        let size = (frame.width, frame.height, cw, ch);
        if self.inputs.get(&object).is_none_or(|i| i.size != size) {
            let create = |width, height, format| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("reusable video plane"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            let y = create(frame.width, frame.height, wgpu::TextureFormat::R8Uint);
            let uv = create(cw, ch, wgpu::TextureFormat::Rg8Uint);
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("video conversion parameters"),
                size: 32,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video plane binding"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &y.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            &uv.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
            self.inputs.insert(
                object,
                Input {
                    size,
                    y,
                    uv,
                    uniform,
                    group,
                },
            );
        }
        let input = &self.inputs[&object];
        let write = |texture, pixels: &[u8], width, height, bpp| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * bpp),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            )
        };
        write(&input.y, &frame.y, frame.width, frame.height, 1);
        write(&input.uv, &frame.uv, cw, ch, 2);
        let params = [
            frame.width,
            frame.height,
            frame.rotation,
            frame.standard,
            frame.range,
            frame.phase[0],
            frame.phase[1],
            0,
        ];
        queue.write_buffer(&input.uniform, 0, bytemuck::cast_slice(&params));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("video GPU conversion"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video YUV to RGB"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &input.group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}
