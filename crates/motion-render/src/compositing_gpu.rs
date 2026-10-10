//! GPU resources shared by track-matte extraction and layer transfer.
use crate::{effect_gpu::FxTexture, RenderError};
use std::collections::HashMap;

pub(crate) struct CompositingGpu {
    pub normal: wgpu::RenderPipeline,
    pub additive: wgpu::RenderPipeline,
    pub alpha: wgpu::RenderPipeline,
    pub luma: wgpu::RenderPipeline,
    pub mattes: HashMap<(u64, bool), FxTexture>,
    pub zero: FxTexture,
    pub source: Option<FxTexture>,
    pub extract_buffer: wgpu::Buffer,
    pub extract_group: wgpu::BindGroup,
    pub extract_stride: usize,
    pub blend: wgpu::RenderPipeline,
    pub blend_buffer: wgpu::Buffer,
    pub blend_group: wgpu::BindGroup,
    pub blend_inputs: Vec<wgpu::BindGroup>,
    blend_layout: wgpu::BindGroupLayout,
    blend_stride: usize,
    size: [u32; 2],
}

impl CompositingGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uniform: &wgpu::BindGroupLayout,
        image: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        format: wgpu::TextureFormat,
    ) -> Self {
        let zero =
            crate::effect_gpu::texture(device, image, sampler, 1, 1, wgpu::TextureFormat::R8Unorm);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &zero.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer and matte plane"),
            source: wgpu::ShaderSource::Wgsl(include_str!("plane_matte.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layer matte inputs"),
            bind_group_layouts: &[uniform, image, image, image],
            push_constant_ranges: &[],
        });
        let make = |entry: &str, target, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 20,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2],
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let extract_stride = (std::mem::size_of::<crate::renderer::DrawUniform>() as usize)
            .div_ceil(device.limits().min_uniform_buffer_offset_alignment as usize)
            * device.limits().min_uniform_buffer_offset_alignment as usize;
        let extract_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("matte source uniforms"),
            size: (extract_stride * motion_model::MAX_LAYERS) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let extract_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: uniform,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &extract_buffer,
                    offset: 0,
                    size: std::num::NonZeroU64::new(
                        std::mem::size_of::<crate::renderer::DrawUniform>() as u64,
                    ),
                }),
            }],
        });
        let blend_uniform = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer transfer uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: std::num::NonZeroU64::new(
                        motion_effects::shader::UNIFORM_BYTES as u64,
                    ),
                },
                count: None,
            }],
        });
        let mut entries = Vec::new();
        for i in 0..3 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: i * 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: i * 2 + 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let blend_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer transfer inputs"),
            entries: &entries,
        });
        let compiled = crate::compositing_plan::shader().expect("checked layer transfer shader");
        let blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer transfer"),
            source: wgpu::ShaderSource::Wgsl(compiled.wgsl.clone().into()),
        });
        let blend = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("layer transfer"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[&blend_uniform, &blend_layout],
                    push_constant_ranges: &[],
                }),
            ),
            vertex: wgpu::VertexState {
                module: &blend_shader,
                entry_point: Some("sdk_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &blend_shader,
                entry_point: Some("sdk_fragment"),
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
        let blend_stride = motion_effects::shader::UNIFORM_BYTES
            .div_ceil(device.limits().min_uniform_buffer_offset_alignment as usize)
            * device.limits().min_uniform_buffer_offset_alignment as usize;
        let blend_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer transfer values"),
            size: (blend_stride * motion_model::MAX_LAYERS) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blend_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &blend_uniform,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &blend_buffer,
                    offset: 0,
                    size: std::num::NonZeroU64::new(motion_effects::shader::UNIFORM_BYTES as u64),
                }),
            }],
        });
        Self {
            normal: make(
                "fragment_main",
                format,
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            ),
            additive: make(
                "fragment_main",
                format,
                Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.alpha,
                }),
            ),
            alpha: make("alpha_matte", wgpu::TextureFormat::R8Unorm, None),
            luma: make("luma_matte", wgpu::TextureFormat::R8Unorm, None),
            mattes: HashMap::new(),
            zero,
            source: None,
            size: [0; 2],
            extract_buffer,
            extract_group,
            extract_stride,
            blend,
            blend_buffer,
            blend_group,
            blend_inputs: Vec::new(),
            blend_layout,
            blend_stride,
        }
    }
    pub fn bytes(&self) -> u64 {
        self.mattes
            .values()
            .map(|t| {
                let s = t.texture.size();
                u64::from(s.width) * u64::from(s.height)
            })
            .sum::<u64>()
            + 1
            + self.source.as_ref().map_or(0, |t| {
                let s = t.texture.size();
                u64::from(s.width) * u64::from(s.height) * 4
            })
    }
    pub fn release(&mut self) {
        self.mattes.clear();
        self.source = None;
        self.size = [0; 2];
        self.blend_inputs.clear();
    }
    pub fn prepare_mattes(
        &mut self,
        device: &wgpu::Device,
        image: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: [u32; 2],
        needed: &[(u64, bool)],
        other_bytes: u64,
    ) -> Result<(), RenderError> {
        if self.size != size {
            self.release();
            self.size = size;
        }
        self.mattes.retain(|key, _| needed.contains(key));
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let unique: std::collections::HashSet<_> = needed.iter().copied().collect();
        if other_bytes + 1 + pixels * unique.len() as u64 > crate::renderer::TEXTURE_BUDGET {
            return Err(RenderError::Invalid(
                "track matte textures exceed 128 MiB".into(),
            ));
        }
        for key in needed {
            self.mattes.entry(*key).or_insert_with(|| {
                crate::effect_gpu::texture(
                    device,
                    image,
                    sampler,
                    size[0],
                    size[1],
                    wgpu::TextureFormat::R8Unorm,
                )
            });
        }
        Ok(())
    }
    pub fn prepare_source(
        &mut self,
        device: &wgpu::Device,
        image: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        blend: bool,
    ) -> Result<bool, RenderError> {
        if !blend {
            self.source = None;
            self.blend_inputs.clear();
        }
        let source_bytes = if blend && self.source.is_none() {
            u64::from(self.size[0]) * u64::from(self.size[1]) * 4
        } else {
            0
        };
        if blend
            && u64::from(self.size[0]) * u64::from(self.size[1]) * 12
                > crate::renderer::TEXTURE_BUDGET
        {
            return Err(RenderError::Invalid(
                "layer transfer accumulators exceed 128 MiB".into(),
            ));
        }
        if source_bytes > 0 {
            self.source = Some(crate::effect_gpu::texture(
                device,
                image,
                sampler,
                self.size[0],
                self.size[1],
                wgpu::TextureFormat::Rgba8UnormSrgb,
            ));
        }
        Ok(source_bytes > 0)
    }
    pub fn prepare_blend_inputs(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sampler: &wgpu::Sampler,
        accumulators: &[FxTexture],
        layers: &[crate::DrawLayer],
        changed: bool,
    ) {
        if self.source.is_none() {
            return;
        }
        if changed || self.blend_inputs.is_empty() {
            self.blend_inputs = (0..2)
                .map(|i| {
                    let views = [
                        &accumulators[i].view,
                        &self.source.as_ref().unwrap().view,
                        &self.source.as_ref().unwrap().view,
                    ];
                    let entries: Vec<_> = views
                        .iter()
                        .enumerate()
                        .flat_map(|(j, view)| {
                            [
                                wgpu::BindGroupEntry {
                                    binding: j as u32 * 2,
                                    resource: wgpu::BindingResource::TextureView(view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: j as u32 * 2 + 1,
                                    resource: wgpu::BindingResource::Sampler(sampler),
                                },
                            ]
                        })
                        .collect();
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("layer transfer backdrop"),
                        layout: &self.blend_layout,
                        entries: &entries,
                    })
                })
                .collect();
        }
        for (i, l) in layers.iter().enumerate() {
            let [w, h] = self.size;
            let region = [0., 0., w as f32, h as f32];
            let mut u = crate::effect_plan::EffectUniform {
                size: [w as f32, h as f32, w as f32, h as f32],
                region,
                input_region: region,
                source_region: region,
                clock: [0.; 4],
                mode: [0.; 4],
                output_mode: [0., 0., 1., 1.],
                params: [[0.; 4]; 32],
            };
            u.params[0] = [
                l.blend.mode.code() as f32,
                f32::from(l.blend.space == motion_model::compositing::BlendSpace::Srgb),
                0.,
                0.,
            ];
            queue.write_buffer(
                &self.blend_buffer,
                (i * self.blend_stride) as u64,
                bytemuck::bytes_of(&u),
            );
        }
    }
    pub fn blend_offset(&self, layer: usize) -> u32 {
        (layer * self.blend_stride) as u32
    }
}
