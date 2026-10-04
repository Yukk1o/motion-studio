use aem_core::{Project, Scene, MAX_LAYERS};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use std::{collections::HashMap, num::NonZeroU64, path::Path, sync::mpsc, time::Instant};

const TEXTURE_BUDGET: u64 = 128 * 1024 * 1024;
const CAPTURE_PIXEL_LIMIT: u64 = 16 * 1024 * 1024;
const DRAW_SIZE: u64 = std::mem::size_of::<DrawUniform>() as u64;
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no compatible GPU adapter")]
    Adapter,
    #[error(transparent)]
    Device(#[from] wgpu::RequestDeviceError),
    #[error(transparent)]
    Image(#[from] image::ImageError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("GPU validation: {0}")]
    Invalid(String),
    #[error("GPU readback failed: {0}")]
    Readback(String),
}
type Result<T> = std::result::Result<T, RenderError>;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawUniform {
    mvp: [[f32; 4]; 4],
    color: [f32; 4],
    extent_opacity: [f32; 4],
}
struct GpuImage {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    bytes: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub cpu_prepare_us: u64,
    pub draw_calls: u32,
    pub texture_bytes: u64,
    pub parameter_upload_bytes: u64,
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
    pipeline: wgpu::RenderPipeline,
    image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    uniform_stride: usize,
    upload: Vec<u8>,
    images: HashMap<u64, GpuImage>,
    texture_bytes: u64,
    pub target_format: wgpu::TextureFormat,
}

impl Renderer {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        format: wgpu::TextureFormat,
    ) -> Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RenderError::Adapter)?;
        let adapter_info = adapter.get_info();
        let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("AEM GPU"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await?;
        let alignment = device.limits().min_uniform_buffer_offset_alignment as usize;
        let uniform_stride = (DRAW_SIZE as usize).div_ceil(alignment) * alignment;
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("AEM reusable layer parameters"),
            size: (uniform_stride * MAX_LAYERS) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("AEM parameter layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(DRAW_SIZE),
                },
                count: None,
            }],
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("AEM parameter group"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer,
                    offset: 0,
                    size: NonZeroU64::new(DRAW_SIZE),
                }),
            }],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("AEM image layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("AEM linear image sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("AEM own planar compositor"),
            source: wgpu::ShaderSource::Wgsl(include_str!("plane.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("AEM planar pipeline"),
            bind_group_layouts: &[&uniform_layout, &image_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("AEM planar compositor"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let mut renderer = Self {
            device,
            queue,
            adapter_info,
            pipeline,
            image_layout,
            sampler,
            uniform_buffer,
            uniform_group,
            uniform_stride,
            upload: vec![0; uniform_stride * MAX_LAYERS],
            images: HashMap::new(),
            texture_bytes: 0,
            target_format: format,
        };
        renderer.upload_image(0, 1, 1, &[255; 4])?;
        Ok(renderer)
    }
    pub async fn headless() -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        Self::new(&instance, None, TARGET_FORMAT).await
    }
    pub fn texture_bytes(&self) -> u64 {
        self.texture_bytes
    }
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
    pub fn upload_image(&mut self, id: u64, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
        let bytes = u64::from(width) * u64::from(height) * 4;
        if width == 0
            || height == 0
            || width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
            || rgba.len() as u64 != bytes
        {
            return Err(RenderError::Invalid(
                "invalid image dimensions or pixel length".into(),
            ));
        }
        let previous = self.images.get(&id).map_or(0, |t| t.bytes);
        if self.texture_bytes - previous + bytes > TEXTURE_BUDGET {
            return Err(RenderError::Invalid(
                "decoded preview textures exceed 128 MiB".into(),
            ));
        }
        let mut premultiplied = rgba.to_vec();
        for pixel in premultiplied.chunks_exact_mut(4) {
            let alpha = pixel[3] as f32 / 255.0;
            for channel in &mut pixel[..3] {
                *channel = (linear_to_srgb(srgb_to_linear(*channel as f32 / 255.0) * alpha) * 255.0)
                    .round() as u8;
            }
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("AEM cached image"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &premultiplied,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("AEM cached texture binding"),
            layout: &self.image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.images.insert(
            id,
            GpuImage {
                _texture: texture,
                bind_group,
                bytes,
            },
        );
        self.texture_bytes = self.texture_bytes - previous + bytes;
        Ok(())
    }
    pub fn synchronize_assets(&mut self, project: &Project, root: &Path) -> Result<()> {
        for asset in &project.assets {
            if self.images.contains_key(&asset.id) {
                continue;
            }
            let reader = ImageReader::open(root.join(&asset.path))?.with_guessed_format()?;
            let (width, height) = reader.into_dimensions()?;
            let bytes = u64::from(width) * u64::from(height) * 4;
            if width != asset.width
                || height != asset.height
                || bytes > TEXTURE_BUDGET
                || self.texture_bytes + bytes > TEXTURE_BUDGET
            {
                return Err(RenderError::Invalid(
                    "asset metadata mismatch or texture budget exceeded".into(),
                ));
            }
            let decoded = ImageReader::open(root.join(&asset.path))?
                .decode()?
                .into_rgba8();
            self.upload_image(asset.id, width, height, decoded.as_raw())?;
        }
        let remove: Vec<_> = self
            .images
            .keys()
            .copied()
            .filter(|id| *id != 0 && !project.assets.iter().any(|a| a.id == *id))
            .collect();
        for id in remove {
            self.texture_bytes -= self.images.remove(&id).unwrap().bytes;
        }
        Ok(())
    }
    pub fn clear_assets(&mut self) {
        self.images.retain(|id, _| *id == 0);
        self.texture_bytes = self.images.get(&0).map_or(0, |t| t.bytes);
    }
    pub fn draw(
        &mut self,
        scene: &Scene,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<RenderStats> {
        if width == 0 || height == 0 || scene.layers.len() > MAX_LAYERS {
            return Err(RenderError::Invalid(
                "invalid render target or layer count".into(),
            ));
        }
        let started = Instant::now();
        for (index, layer) in scene.layers.iter().enumerate() {
            let asset = layer.asset.unwrap_or(0);
            if !self.images.contains_key(&asset) {
                return Err(RenderError::Invalid(format!(
                    "image asset {asset} is not uploaded"
                )));
            }
            let mut color = layer.color;
            for c in &mut color[..3] {
                *c = srgb_to_linear(*c);
            }
            let uniform = DrawUniform {
                mvp: (scene.camera.view_projection * layer.model).to_cols_array_2d(),
                color,
                extent_opacity: [layer.size[0], layer.size[1], layer.opacity, 0.0],
            };
            let offset = index * self.uniform_stride;
            self.upload[offset..offset + DRAW_SIZE as usize]
                .copy_from_slice(bytemuck::bytes_of(&uniform));
        }
        let upload_bytes = scene.layers.len() * self.uniform_stride;
        if upload_bytes > 0 {
            self.queue
                .write_buffer(&self.uniform_buffer, 0, &self.upload[..upload_bytes]);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("AEM frame"),
            });
        let alpha = scene.background[3];
        let clear = wgpu::Color {
            r: (srgb_to_linear(scene.background[0]) * alpha) as f64,
            g: (srgb_to_linear(scene.background[1]) * alpha) as f64,
            b: (srgb_to_linear(scene.background[2]) * alpha) as f64,
            a: alpha as f64,
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("AEM composition"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let scale =
                (width as f32 / scene.width as f32).min(height as f32 / scene.height as f32);
            let vw = scene.width as f32 * scale;
            let vh = scene.height as f32 * scale;
            pass.set_viewport(
                (width as f32 - vw) / 2.0,
                (height as f32 - vh) / 2.0,
                vw,
                vh,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.pipeline);
            for (index, layer) in scene.layers.iter().enumerate() {
                pass.set_bind_group(
                    0,
                    &self.uniform_group,
                    &[(index * self.uniform_stride) as u32],
                );
                pass.set_bind_group(1, &self.images[&layer.asset.unwrap_or(0)].bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(RenderStats {
            cpu_prepare_us: started.elapsed().as_micros() as u64,
            draw_calls: scene.layers.len() as u32,
            texture_bytes: self.texture_bytes,
            parameter_upload_bytes: upload_bytes as u64,
        })
    }
    pub fn capture_target(&self, width: u32, height: u32) -> Result<CaptureTarget> {
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > CAPTURE_PIXEL_LIMIT
            || width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
        {
            return Err(RenderError::Invalid("invalid capture size".into()));
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("AEM reusable offscreen frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let row_stride = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("AEM bounded frame readback"),
            size: u64::from(row_stride) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(CaptureTarget {
            texture,
            view,
            buffer,
            width,
            height,
            row_stride,
        })
    }
    /// Readback is for screenshots and a measured reference export path, never preview.
    pub fn capture(
        &mut self,
        scene: &Scene,
        target: &CaptureTarget,
    ) -> Result<(Vec<u8>, RenderStats)> {
        let stats = self.draw(scene, &target.view, target.width, target.height)?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("AEM screenshot copy"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &target.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(target.row_stride),
                    rows_per_image: Some(target.height),
                },
            },
            wgpu::Extent3d {
                width: target.width,
                height: target.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let (send, recv) = mpsc::sync_channel(1);
        target
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.device.poll(wgpu::Maintain::Wait);
        recv.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let mut pixels = Vec::with_capacity((target.width * target.height * 4) as usize);
        {
            let mapped = target.buffer.slice(..).get_mapped_range();
            for row in mapped.chunks_exact(target.row_stride as usize) {
                pixels.extend_from_slice(&row[..target.width as usize * 4]);
            }
        }
        target.buffer.unmap();
        if matches!(
            self.target_format,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
        ) {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }
        // PNG uses straight alpha. GPU compositing used linear premultiplied alpha.
        for rgba in pixels.chunks_exact_mut(4) {
            let alpha = rgba[3] as f32 / 255.0;
            if alpha == 0.0 {
                rgba[..3].fill(0);
            } else if alpha < 1.0 {
                for c in &mut rgba[..3] {
                    *c = (linear_to_srgb(
                        (srgb_to_linear(*c as f32 / 255.0) / alpha).clamp(0.0, 1.0),
                    ) * 255.0)
                        .round() as u8;
                }
            }
        }
        Ok((pixels, stats))
    }
}

pub struct CaptureTarget {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    buffer: wgpu::Buffer,
    pub width: u32,
    pub height: u32,
    row_stride: u32,
}
fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
