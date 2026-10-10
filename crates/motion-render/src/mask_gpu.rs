use crate::{
    effect_gpu::FxTexture,
    mask_plan::{self, MaskRaster},
    renderer::{GpuImage, TextureKey},
    RenderError,
};
use std::collections::{HashMap, HashSet};
use wgpu::util::DeviceExt;

struct Uniform {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}
pub(crate) struct MaskGpu {
    raster: wgpu::RenderPipeline,
    filters: Vec<wgpu::RenderPipeline>,
    uniform_layout: wgpu::BindGroupLayout,
    input_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: Vec<Uniform>,
    scratch: Vec<FxTexture>,
    scratch_size: (u32, u32),
    fingerprints: HashMap<u64, u64>,
    meshes: HashMap<(u64,u64),(u64,wgpu::Buffer)>,
    pub uploads: u64,
}
fn texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    w: u32,
    h: u32,
) -> FxTexture {
    crate::effect_gpu::texture(device, layout, sampler, w, h, wgpu::TextureFormat::R8Unorm)
}
impl MaskGpu {
    pub fn new(
        device: &wgpu::Device,
        image_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self, RenderError> {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mask coverage"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mask uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(624),
                },
                count: None,
            }],
        });
        let mut entries = vec![];
        for binding in 0..4 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: if binding % 2 == 0 {
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }
                } else {
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
                },
                count: None,
            });
        }
        let input_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mask inputs"),
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask filters"),
            bind_group_layouts: &[&uniform_layout, &input_layout],
            push_constant_ranges: &[],
        });
        let filters = mask_plan::shaders()
            .map_err(RenderError::Invalid)?
            .iter()
            .map(|shader| {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("portable mask filter"),
                    source: wgpu::ShaderSource::Wgsl(shader.wgsl.clone().into()),
                });
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("mask scalar filter"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("sdk_vertex"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("sdk_fragment"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::R8Unorm,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview: None,
                    cache: None,
                })
            })
            .collect();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mask path paint"),
            source: wgpu::ShaderSource::Wgsl(include_str!("vector.wgsl").into()),
        });
        let raster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mask path raster"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[],
                    push_constant_ranges: &[],
                }),
            ),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let _ = image_layout;
        Ok(Self {
            raster,
            filters,
            uniform_layout,
            input_layout,
            sampler,
            uniforms: vec![],
            scratch: vec![],
            scratch_size: (0, 0),
            fingerprints: HashMap::new(),
            meshes: HashMap::new(),
            uploads: 0,
        })
    }
    pub fn scratch_bytes(&self) -> u64 {
        u64::from(self.scratch_size.0) * u64::from(self.scratch_size.1) * self.scratch.len() as u64
    }
    pub fn clear(&mut self) {
        self.scratch.clear();
        self.scratch_size = (0, 0);
        self.fingerprints.clear();
        self.meshes.clear();
    }
    fn filter(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        index: &mut usize,
        pipeline: usize,
        u: crate::effect_plan::EffectUniform,
        input: &wgpu::TextureView,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
    ) {
        if *index >= self.uniforms.len() {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mask pass uniform"),
                size: 624,
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
            self.uniforms.push(Uniform { buffer, group });
        }
        let slot = &self.uniforms[*index];
        *index += 1;
        queue.write_buffer(&slot.buffer, 0, bytemuck::bytes_of(&u));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mask pass inputs"),
            layout: &self.input_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mask feather/combine"),
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
        pass.set_viewport(0., 0., u.size[0], u.size[1], 0., 1.);
        pass.set_pipeline(&self.filters[pipeline]);
        pass.set_bind_group(0, &slot.group, &[]);
        pass.set_bind_group(1, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        image_layout: &wgpu::BindGroupLayout,
        scene: &motion_core::Scene,
        masks: &[MaskRaster],
        images: &mut HashMap<TextureKey, GpuImage>,
        bytes: &mut u64,
        other: u64,
    ) -> Result<bool, RenderError> {
        let active: HashSet<_> = masks.iter().map(|m| scene.layers[m.layer].id).collect();
        let active_meshes:HashSet<_>=masks.iter().map(|m|(scene.layers[m.layer].id,m.id)).collect();
        self.meshes.retain(|key,_|active_meshes.contains(key));
        let mut changed = false;
        let stale: Vec<_> = images
            .keys()
            .copied()
            .filter(|k| matches!(k,TextureKey::Mask(id) if !active.contains(id)))
            .collect();
        for key in stale {
            *bytes -= images.remove(&key).unwrap().bytes;
            changed = true;
        }
        self.fingerprints.retain(|id, _| active.contains(id));
        if masks.is_empty() {
            self.clear();
            return Ok(changed);
        }
        let (w, h, slots) = mask_plan::scratch_dimensions(masks);
        if (w, h) != self.scratch_size || slots != self.scratch.len() {
            self.scratch = (0..slots)
                .map(|_| texture(device, image_layout, &self.sampler, w, h))
                .collect();
            self.scratch_size = (w, h);
        }
        let mut uniform_index = 0;
        for group in masks.chunk_by(|a, b| a.layer == b.layer) {
            let owner = scene.layers[group[0].layer].id;
            let key = TextureKey::Mask(owner);
            let signature = mask_plan::group_fingerprint(group);
            if self.fingerprints.get(&owner) == Some(&signature) && images.contains_key(&key) {
                continue;
            }
            let first = &group[0];
            let cost = u64::from(first.width) * u64::from(first.height);
            let previous = images.get(&key).map_or(0, |i| i.bytes);
            if *bytes - previous + cost + other > 128 * 1024 * 1024 {
                return Err(RenderError::Invalid(format!(
                    "layer {owner}: source and mask textures exceed 128 MiB"
                )));
            }
            if images
                .get(&key)
                .is_none_or(|i| i.size != (first.width, first.height))
            {
                let t = texture(
                    device,
                    image_layout,
                    &self.sampler,
                    first.width,
                    first.height,
                );
                images.insert(
                    key,
                    GpuImage {
                        _texture: t.texture,
                        view: t.view,
                        bind_group: t.composite,
                        bytes: cost,
                        size: (first.width, first.height),
                        video_stamp: None,
                    },
                );
                *bytes = *bytes - previous + cost;
            }
            let blur = group.iter().any(|m| m.feather.iter().any(|f| *f > 0.));
            let accumulator_start = 1 + usize::from(blur);
            let mut previous_slot = None;
            for (i, m) in group.iter().enumerate() {
                let mesh_key=(owner,m.id);
                if self.meshes.get(&mesh_key).is_none_or(|(fingerprint,_)|*fingerprint!=m.fingerprint) {
                let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mask path triangles"),
                    contents: if m.vertices.is_empty() {
                        &[0; 24]
                    } else {
                        bytemuck::cast_slice(&m.vertices)
                    },
                    usage: wgpu::BufferUsages::VERTEX,
                });
                self.meshes.insert(mesh_key,(m.fingerprint,vertices));
                }
                let vertices=&self.meshes[&mesh_key].1;
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("mask source path"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &self.scratch[0].view,
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
                    pass.set_viewport(0., 0., m.width as f32, m.height as f32, 0., 1.);
                    pass.set_pipeline(&self.raster);
                    pass.set_vertex_buffer(0, vertices.slice(..));
                    pass.draw(0..m.vertices.len() as u32, 0..1);
                }
                let mut coverage = 0;
                for axis in 0..2 {
                    if m.feather[axis] <= 0. {
                        continue;
                    }
                    let next = 1 - coverage;
                    let input = self.scratch[coverage].view.clone();
                    let target = self.scratch[next].view.clone();
                    self.filter(
                        device,
                        queue,
                        encoder,
                        &mut uniform_index,
                        0,
                        mask_plan::gaussian_uniform(m.width, m.height, axis, m.feather[axis]),
                        &input,
                        &input,
                        &target,
                    );
                    coverage = next;
                }
                let source = self.scratch[coverage].view.clone();
                let input = previous_slot.map_or_else(
                    || source.clone(),
                    |index: usize| self.scratch[index].view.clone(),
                );
                let slot = accumulator_start + (i % 2);
                let target = if i + 1 == group.len() {
                    images[&key].view.clone()
                } else {
                    self.scratch[slot].view.clone()
                };
                self.filter(
                    device,
                    queue,
                    encoder,
                    &mut uniform_index,
                    1,
                    mask_plan::combine_uniform(m, i == 0),
                    &input,
                    &source,
                    &target,
                );
                previous_slot = Some(slot);
            }
            self.fingerprints.insert(owner, signature);
            self.uploads += 1;
            changed = true;
        }
        Ok(changed)
    }
}
