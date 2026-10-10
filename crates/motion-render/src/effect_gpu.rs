use crate::{
    effect_plan::{EffectFramePlan, PlanBuilder},
    renderer::GpuImage,
    RenderError,
};
use crate::Scene;
use motion_effects::Registry;
use std::collections::HashMap;

pub(crate) struct FxTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub composite: wgpu::BindGroup,
}
struct Buffer {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}
struct Binding {
    key: (
        i32,
        i32,
        i32,
        u32,
        u64,
        Option<crate::renderer::TextureKey>,
        usize,
    ),
    group: wgpu::BindGroup,
}
struct Lut {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    last: [u8; 1024],
}
pub(crate) struct EffectGpu {
    pub builder: PlanBuilder,
    pub state: GpuState,
}
pub(crate) struct GpuState {
    uniform_layout: wgpu::BindGroupLayout,
    input_layout: wgpu::BindGroupLayout,
    resource_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: FxTexture,
    identity: Lut,
    luts: Vec<Lut>,
    pipelines: HashMap<(u32, u32), wgpu::RenderPipeline>,
    resources: HashMap<u32, wgpu::BindGroup>,
    resource_textures: HashMap<(String, String), FxTexture>,
    pub resource_bytes: u64,
    pub parameter_resource_upload_bytes: u64,
    pub failed_program: Option<(u32, String)>,
    pub pool: Vec<Option<FxTexture>>,
    pool_width: u32,
    pool_height: u32,
    pool_slots: u32,
    pool_sizes: [[u32; 2]; 8],
    epoch: u64,
    buffers: Vec<Buffer>,
    sprite_buffer: wgpu::Buffer,
    bindings: Vec<Option<Binding>>,
    input_resources: Vec<Option<(u32, usize, wgpu::BindGroup)>>,
}
pub(crate) fn texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
) -> FxTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("motion-studio effect texture"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("effect composite input"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    FxTexture {
        texture,
        view,
        composite,
    }
}
fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture, w: u32, h: u32, bytes: &[u8]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}
fn bindings(device: &wgpu::Device, name: &str, texture_count: u32) -> wgpu::BindGroupLayout {
    let mut entries = Vec::new();
    for i in 0..texture_count {
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
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(name),
        entries: &entries,
    })
}
impl EffectGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        composite: &wgpu::BindGroupLayout,
    ) -> Result<Self, RenderError> {
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect SDK uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(
                        motion_effects::shader::UNIFORM_BYTES as u64,
                    ),
                },
                count: None,
            }],
        });
        let input_layout = bindings(device, "effect input/source/LUT", 3);
        let resource_layout = bindings(device, "effect resource textures", 4);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effect sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = texture(
            device,
            composite,
            &sampler,
            1,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        upload(queue, &white.texture, 1, 1, &[255; 4]);
        let identity_texture = texture(
            device,
            composite,
            &sampler,
            256,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let identity_bytes: [u8; 1024] = std::array::from_fn(|i| (i / 4) as u8);
        upload(queue, &identity_texture.texture, 256, 1, &identity_bytes);
        let identity = Lut {
            texture: identity_texture.texture,
            view: identity_texture.view,
            last: identity_bytes,
        };
        Ok(Self {
            builder: PlanBuilder::new(
                Registry::new_with_builtins().map_err(|e| RenderError::Invalid(e.to_string()))?,
            )
            .map_err(RenderError::Invalid)?,
            state: GpuState {
                uniform_layout,
                input_layout,
                resource_layout,
                sampler,
                white,
                identity,
                luts: Vec::new(),
                pipelines: HashMap::new(),
                resources: HashMap::new(),
                resource_textures: HashMap::new(),
                resource_bytes: 0,
                parameter_resource_upload_bytes: 0,
                failed_program: None,
                pool: (0..8).map(|_| None).collect(),
                pool_width: 0,
                pool_height: 0,
                pool_slots: 0,
                pool_sizes: [[0; 2]; 8],
                epoch: 0,
                buffers: Vec::new(),
                sprite_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("bounded scene instances"),
                    size: (motion_effects::MAX_SPRITES * 48) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                bindings: Vec::new(),
                input_resources: Vec::new(),
            },
        })
    }
    pub fn set_registry(&mut self, r: Registry) {
        self.builder.set_registry(r);
        self.state.pipelines.clear();
        self.state.resources.clear();
        self.state.resource_textures.clear();
        self.state.resource_bytes = 0;
        self.state.invalidate();
    }
    pub fn invalidate(&mut self) {
        self.state.invalidate();
    }
}
impl GpuState {
    pub fn invalidate(&mut self) {
        self.epoch += 1;
        self.bindings.clear();
        self.input_resources.clear();
    }
    pub fn release_scratch(&mut self) {
        if self.pool_slots != 0 {
            self.pool.iter_mut().for_each(|v| *v = None);
            self.pool_width = 0;
            self.pool_height = 0;
            self.pool_slots = 0;
            self.pool_sizes = [[0; 2]; 8];
            self.luts.clear();
            self.buffers.clear();
            self.invalidate();
        }
    }
    pub fn bytes(&self) -> u64 {
        crate::effect_plan::scratch_capacity_bytes(&self.pool_sizes)
            + self.resource_bytes
            + self.luts.len() as u64 * 1024
    }
    /// Reserve validated package PNGs before uploading, so idle preview images
    /// cannot turn an otherwise valid active effect into a budget failure.
    pub fn pending_resource_bytes(&self, builder: &PlanBuilder) -> u64 {
        let mut seen = std::collections::HashSet::new();
        let mut bytes = 0;
        for pass in &builder.frame.passes {
            let program = &builder.programs[pass.program as usize];
            if self.resources.contains_key(&pass.program) { continue; }
            if let Some(package) = &program.package {
                for path in &program.resources {
                    let key = (package.hash.clone(), path.clone());
                    if self.resource_textures.contains_key(&key) || !seen.insert(key) { continue; }
                    if let Some(data) = package.files.get(path) {
                        if let Ok(reader) = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format() {
                            if let Ok((w, h)) = reader.into_dimensions() { bytes += u64::from(w) * u64::from(h) * 4; }
                        }
                    }
                }
            }
        }
        bytes
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        composite: &wgpu::BindGroupLayout,
        builder: &PlanBuilder,
        scene: &Scene,
        asset_bytes: u64,
    ) -> Result<(), RenderError> {
        let frame = &builder.frame;
        if frame.passes.is_empty() {
            self.release_scratch();
            return Ok(());
        }
        if frame.width > device.limits().max_texture_dimension_2d
            || frame.height > device.limits().max_texture_dimension_2d
        {
            return Err(RenderError::Invalid(
                "effect texture exceeds device dimensions".into(),
            ));
        }
        if self.pool_width != frame.width
            || self.pool_height != frame.height
            || self.pool_slots != frame.slots
            || self.pool_sizes != frame.scratch_sizes
        {
            self.invalidate();
            self.pool = (0..8)
                .map(|i| {
                    if frame.slots & (1 << i) != 0 {
                        Some(texture(
                            device,
                            composite,
                            &self.sampler,
                            frame.scratch_sizes[i][0],
                            frame.scratch_sizes[i][1],
                            if i == 7 {
                                wgpu::TextureFormat::Rgba16Float
                            } else if (1..=3).contains(&i) {
                                wgpu::TextureFormat::Rgba8Unorm
                            } else {
                                wgpu::TextureFormat::Rgba8UnormSrgb
                            },
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            self.pool_width = frame.width;
            self.pool_height = frame.height;
            self.pool_slots = frame.slots;
            self.pool_sizes = frame.scratch_sizes;
        }
        self.luts.truncate(scene.curve_luts.len());
        for (index, lut) in scene.curve_luts.iter().enumerate() {
            let bytes: [u8; 1024] =
                std::array::from_fn(|i| (lut[i / 4][i % 4].clamp(0.0, 1.0) * 255.0).round() as u8);
            if index >= self.luts.len() {
                let t = texture(
                    device,
                    composite,
                    &self.sampler,
                    256,
                    1,
                    wgpu::TextureFormat::Rgba8Unorm,
                );
                upload(queue, &t.texture, 256, 1, &bytes);
                self.parameter_resource_upload_bytes += 1024;
                self.luts.push(Lut {
                    texture: t.texture,
                    view: t.view,
                    last: bytes,
                });
            } else if self.luts[index].last != bytes {
                upload(queue, &self.luts[index].texture, 256, 1, &bytes);
                self.parameter_resource_upload_bytes += 1024;
                self.luts[index].last = bytes;
            }
        }
        while self.buffers.len() < frame.passes.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("reusable effect parameters"),
                size: motion_effects::shader::UNIFORM_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.uniform_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            self.buffers.push(Buffer { buffer, group });
        }
        self.bindings.resize_with(frame.passes.len(), || None);
        self.input_resources.resize_with(frame.passes.len(), || None);
        if !frame.sprites.is_empty() {
            queue.write_buffer(&self.sprite_buffer, 0, bytemuck::cast_slice(&frame.sprites));
        }
        for p in &frame.passes {
            let gamma = (1..=3).contains(&p.output);
            let target_kind = if p.output == 7 {
                7
            } else if gamma {
                1
            } else {
                0
            };
            let key = (p.program, target_kind);
            if !self.pipelines.contains_key(&key) {
                device.push_error_scope(wgpu::ErrorFilter::Validation);
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("effect WGSL"),
                    source: wgpu::ShaderSource::Wgsl(
                        builder.programs[p.program as usize]
                            .shader
                            .wgsl
                            .clone()
                            .into(),
                    ),
                });
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[
                        &self.uniform_layout,
                        &self.input_layout,
                        &self.resource_layout,
                    ],
                    push_constant_ranges: &[],
                });
                let sprite = builder.programs[p.program as usize].shader.sprite;
                let additive = builder.programs[p.program as usize].shader.additive;
                const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
                    wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4];
                let vertex_layout = wgpu::VertexBufferLayout {
                    array_stride: 48,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRIBUTES,
                };
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("effect fullscreen pass"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("sdk_vertex"),
                        compilation_options: Default::default(),
                        buffers: if sprite {
                            std::slice::from_ref(&vertex_layout)
                        } else {
                            &[]
                        },
                    },
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("sdk_fragment"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: if p.output == 7 {
                                wgpu::TextureFormat::Rgba16Float
                            } else if gamma {
                                wgpu::TextureFormat::Rgba8Unorm
                            } else {
                                wgpu::TextureFormat::Rgba8UnormSrgb
                            },
                            blend: if sprite {
                                Some(if additive {
                                    wgpu::BlendState {
                                        color: wgpu::BlendComponent {
                                            src_factor: wgpu::BlendFactor::One,
                                            dst_factor: wgpu::BlendFactor::One,
                                            operation: wgpu::BlendOperation::Add,
                                        },
                                        alpha: wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.alpha,
                                    }
                                } else {
                                    wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
                                })
                            } else {
                                None
                            },
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview: None,
                    cache: None,
                });
                device.poll(wgpu::Maintain::Wait);
                if let Some(e) = pollster::block_on(device.pop_error_scope()) {
                    let message = format!(
                        "effect program {}: {e}",
                        builder.programs[p.program as usize].key
                    );
                    self.failed_program = Some((p.program, message.clone()));
                    return Err(RenderError::Invalid(message));
                }
                self.pipelines.insert(key, pipeline);
            }
            if !self.resources.contains_key(&p.program) {
                let program = &builder.programs[p.program as usize];
                if let Some(package) = &program.package {
                    for path in &program.resources {
                        let key = (package.hash.clone(), path.clone());
                        if !self.resource_textures.contains_key(&key) {
                            let rgba = image::load_from_memory(&package.files[path])
                                .map_err(RenderError::Image)?
                                .into_rgba8();
                            let bytes = u64::from(rgba.width()) * u64::from(rgba.height()) * 4;
                            if rgba.width() > device.limits().max_texture_dimension_2d
                                || rgba.height() > device.limits().max_texture_dimension_2d
                            {
                                let message = format!(
                                    "effect resource {path} exceeds device texture dimensions"
                                );
                                self.failed_program = Some((p.program, message.clone()));
                                return Err(RenderError::Invalid(message));
                            }
                            if asset_bytes + self.resource_bytes + bytes > 128 * 1024 * 1024 {
                                self.failed_program = Some((
                                    p.program,
                                    "images and effect resources exceed 128 MiB".into(),
                                ));
                                return Err(RenderError::Invalid(
                                    "images and effect resources exceed 128 MiB".into(),
                                ));
                            }
                            let t = texture(
                                device,
                                composite,
                                &self.sampler,
                                rgba.width(),
                                rgba.height(),
                                wgpu::TextureFormat::Rgba8Unorm,
                            );
                            upload(
                                queue,
                                &t.texture,
                                rgba.width(),
                                rgba.height(),
                                rgba.as_raw(),
                            );
                            self.resource_textures.insert(key, t);
                            self.resource_bytes += bytes;
                        }
                    }
                }
                let mut entries = Vec::new();
                for i in 0..4 {
                    let view = if let (Some(package), Some(path)) =
                        (&program.package, program.resources.get(i))
                    {
                        &self.resource_textures[&(package.hash.clone(), path.clone())].view
                    } else {
                        &self.white.view
                    };
                    entries.push(wgpu::BindGroupEntry {
                        binding: i as u32 * 2,
                        resource: wgpu::BindingResource::TextureView(view),
                    });
                    entries.push(wgpu::BindGroupEntry {
                        binding: i as u32 * 2 + 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    });
                }
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.resource_layout,
                    entries: &entries,
                });
                self.resources.insert(p.program, group);
            }
        }
        Ok(())
    }
    pub fn encode_pass(
        &mut self,
        index: usize,
        frame: &EffectFramePlan,
        assets: &[u64],
        source_key: Option<crate::renderer::TextureKey>,
        external: Option<&wgpu::TextureView>,
        images: &HashMap<crate::renderer::TextureKey, GpuImage>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), RenderError> {
        let p = &frame.passes[index];
        let source_key = if p.sprite {None} else {source_key};
        let external = if p.sprite {None} else {external};
        queue.write_buffer(
            &self.buffers[index].buffer,
            0,
            bytemuck::bytes_of(&p.uniform),
        );
        let external_id = external.map_or(0, |v| v as *const _ as usize);
        let key = (
            p.input,
            p.source,
            p.lut,
            p.program,
            self.epoch,
            source_key,
            external_id,
        );
        if self.bindings[index].as_ref().is_none_or(|v| v.key != key) {
            let view = |id: i32| -> &wgpu::TextureView {
                if id <= crate::mask_plan::SOURCE_TOKEN {
                    let owner=(crate::mask_plan::SOURCE_TOKEN-id) as usize;
                    return &images[&crate::renderer::TextureKey::Mask(frame.draws[owner].layer)].view;
                }
                if id < 0 {
                    if p.sprite {
                        return &images[&crate::renderer::TextureKey::Static(assets[(-id - 1) as usize])].view;
                    }
                    if let Some(view) = external {
                        return view;
                    }
                    // External pass inputs refer to the owning layer's source.
                    // Videos use a live instance texture rather than the white asset.
                    let image = source_key.unwrap_or_else(|| {
                        crate::renderer::TextureKey::Static(assets[(-id - 1) as usize])
                    });
                    &images[&image].view
                } else {
                    &self.pool[id as usize].as_ref().unwrap().view
                }
            };
            let lut = if p.lut >= 0 {
                &self.luts[p.lut as usize].view
            } else {
                &self.identity.view
            };
            let entries = [
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view(p.input)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(view(p.source)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(lut),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ];
            self.bindings[index] = Some(Binding {
                key,
                group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.input_layout,
                    entries: &entries,
                }),
            });
        }
        let target = &self.pool[p.output as usize].as_ref().unwrap().view;
        if p.resource_input != 0 {
            use crate::renderer::TextureKey;
            let image = if p.resource_input < 0 {
                TextureKey::Static(assets[(-p.resource_input-1) as usize])
            } else {
                let draw=&frame.draws[(p.resource_input-1) as usize];
                if p.uniform.params[30][1]>0.5 {TextureKey::EffectInput(draw.layer)}
                else if draw.words[31]==1. {TextureKey::Vector(draw.layer)}
                else if draw.words[24]<0. {TextureKey::Video(draw.layer)}
                else {TextureKey::Static(assets[draw.words[24] as usize])}
            };
            let view=&images.get(&image).ok_or_else(||RenderError::Invalid("effect image input is not available".into()))?.view;
            let pointer=view as *const _ as usize;
            if self.input_resources[index].as_ref().is_none_or(|(program,old,_)|*program!=p.program||*old!=pointer) {
                let mut entries=Vec::new();
                for slot in 0..4 {
                    entries.push(wgpu::BindGroupEntry {binding:slot*2,resource:wgpu::BindingResource::TextureView(if slot==0 {view}else{&self.white.view})});
                    entries.push(wgpu::BindGroupEntry {binding:slot*2+1,resource:wgpu::BindingResource::Sampler(&self.sampler)});
                }
                self.input_resources[index]=Some((p.program,pointer,device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label:Some("effect image input"),layout:&self.resource_layout,entries:&entries
                })));
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("motion-studio effect"),
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
        pass.set_viewport(0.0, 0.0, p.width as f32, p.height as f32, 0.0, 1.0);
        pass.set_pipeline(
            &self.pipelines[&(
                p.program,
                if p.output == 7 {
                    7
                } else if (1..=3).contains(&p.output) {
                    1
                } else {
                    0
                },
            )],
        );
        pass.set_bind_group(0, &self.buffers[index].group, &[]);
        pass.set_bind_group(1, &self.bindings[index].as_ref().unwrap().group, &[]);
        let resources=if p.resource_input!=0 {&self.input_resources[index].as_ref().unwrap().2} else {&self.resources[&p.program]};
        pass.set_bind_group(2, resources, &[]);
        if p.sprite {
            pass.set_vertex_buffer(0, self.sprite_buffer.slice(..));
            pass.draw(0..6, p.sprite_start..p.sprite_start + p.sprite_count);
        } else {
            pass.draw(0..3, 0..1);
        }
        Ok(())
    }
}
