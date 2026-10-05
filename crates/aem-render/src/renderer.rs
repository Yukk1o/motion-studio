use aem_core::{PlaneCompositor, Project, Scene, MAX_LAYERS};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use std::{
    collections::HashMap,
    num::NonZeroU64,
    path::Path,
    sync::{mpsc, Arc, Mutex},
    time::Instant,
};

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
    uv_scale: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GeometryVertex {
    position: [f32; 3],
    uv: [f32; 2],
}
pub(crate) struct GpuImage {
    _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    bytes: u64,
    size: (u32, u32),
    video_stamp: Option<(u64, u64)>,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum TextureKey {
    Static(u64),
    Video(u64),
}
fn texture_key(layer: &aem_core::DrawLayer) -> TextureKey {
    if layer.video.is_some() {
        TextureKey::Video(layer.id)
    } else {
        TextureKey::Static(layer.asset.unwrap_or(0))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub cpu_prepare_us: u64,
    pub draw_calls: u32,
    pub texture_bytes: u64,
    pub parameter_upload_bytes: u64,
    pub parameter_resource_upload_bytes: u64,
}

pub struct Renderer {
    pub adapter: wgpu::Adapter,
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
    compositor: PlaneCompositor,
    vertex_buffer: wgpu::Buffer,
    geometry_upload: Vec<GeometryVertex>,
    images: HashMap<TextureKey, GpuImage>,
    texture_bytes: u64,
    pub target_format: wgpu::TextureFormat,
    gpu_failure: Arc<Mutex<Option<String>>>,
    effect_gpu: crate::effect_gpu::EffectGpu,
    asset_order: Vec<u64>,
    pub effect_diagnostics: Vec<String>,
}

impl Renderer {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        format: wgpu::TextureFormat,
    ) -> Result<Self> {
        Self::new_profiled(instance, surface, format, false).await
    }
    pub async fn new_profiled(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        format: wgpu::TextureFormat,
        timestamps: bool,
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
                    required_features: if timestamps {
                        adapter.features() & wgpu::Features::TIMESTAMP_QUERY
                    } else {
                        wgpu::Features::empty()
                    },
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await?;
        let gpu_failure = Arc::new(Mutex::new(None));
        let lost = gpu_failure.clone();
        device.set_device_lost_callback(move |reason, message| {
            *lost.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(format!("GPU device lost: {reason:?}: {message}"));
        });
        let uncaptured = gpu_failure.clone();
        device.on_uncaptured_error(Box::new(move |error| {
            *uncaptured.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(format!("GPU error: {error}"));
        }));
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
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Motion Studio bounded planar geometry"),
            size: 65_536 * std::mem::size_of::<GeometryVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GeometryVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2],
                }],
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
        let effect_gpu = crate::effect_gpu::EffectGpu::new(&device, &queue, &image_layout)?;
        let mut renderer = Self {
            adapter,
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
            compositor: PlaneCompositor::new(),
            vertex_buffer,
            geometry_upload: Vec::with_capacity(MAX_LAYERS * 12),
            images: HashMap::new(),
            texture_bytes: 0,
            target_format: format,
            gpu_failure,
            effect_gpu,
            asset_order: Vec::new(),
            effect_diagnostics: Vec::new(),
        };
        renderer.effect_gpu.builder.device_dimension =
            renderer.device.limits().max_texture_dimension_2d.min(8192);
        renderer.upload_image(0, 1, 1, &[255; 4])?;
        Ok(renderer)
    }
    pub async fn headless() -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        Self::new(&instance, None, TARGET_FORMAT).await
    }
    pub fn texture_bytes(&self) -> u64 {
        self.texture_bytes + self.effect_gpu.state.bytes()
    }
    pub fn set_effect_registry(&mut self, registry: aem_effects::Registry) {
        self.effect_gpu.set_registry(registry);
    }
    pub fn preflight_effects(&mut self, scene: &Scene, width: u32, height: u32) -> Result<()> {
        self.effect_gpu
            .builder
            .build(scene, &self.asset_order, width, height, true)
            .map_err(RenderError::Invalid)?;
        self.effect_gpu.state.prepare(
            &self.device,
            &self.queue,
            &self.image_layout,
            &self.effect_gpu.builder,
            scene,
            self.texture_bytes,
        )?;
        if self.texture_bytes + self.effect_gpu.state.resource_bytes > TEXTURE_BUDGET {
            return Err(RenderError::Invalid(
                "images and plugin resources exceed 128 MiB".into(),
            ));
        }
        Ok(())
    }
    pub fn gpu_error(&self) -> Option<String> {
        self.gpu_failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn check_health(&self) -> Result<()> {
        if let Some(error) = self.gpu_error() {
            Err(RenderError::Invalid(error))
        } else {
            Ok(())
        }
    }
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
    pub fn upload_image(&mut self, id: u64, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
        self.upload_pixels(TextureKey::Static(id), width, height, rgba, true)
    }
    pub fn upload_video_frame(
        &mut self,
        object: u64,
        source: u64,
        pts: u64,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        self.check_health()?;
        if u64::from(width) * u64::from(height) * 4 != rgba.len() as u64 {
            return Err(RenderError::Invalid("video pixel length mismatch".into()));
        }
        if self
            .images
            .get(&TextureKey::Video(object))
            .is_some_and(|i| i.video_stamp == Some((source, pts)) && i.size == (width, height))
        {
            return Ok(());
        }
        self.upload_pixels(TextureKey::Video(object), width, height, rgba, false)?;
        self.images
            .get_mut(&TextureKey::Video(object))
            .unwrap()
            .video_stamp = Some((source, pts));
        Ok(())
    }
    fn upload_pixels(
        &mut self,
        id: TextureKey,
        width: u32,
        height: u32,
        rgba: &[u8],
        premultiply: bool,
    ) -> Result<()> {
        self.check_health()?;
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
        if self.texture_bytes - previous + bytes + self.effect_gpu.state.resource_bytes
            > TEXTURE_BUDGET
        {
            return Err(RenderError::Invalid(
                "decoded preview textures exceed 128 MiB".into(),
            ));
        }
        let mut premultiplied = if premultiply {
            rgba.to_vec()
        } else {
            Vec::new()
        };
        for pixel in premultiplied.chunks_exact_mut(4) {
            let alpha = pixel[3] as f32 / 255.0;
            for channel in &mut pixel[..3] {
                *channel = (linear_to_srgb(srgb_to_linear(*channel as f32 / 255.0) * alpha) * 255.0)
                    .round() as u8;
            }
        }
        let pixels = if premultiply {
            premultiplied.as_slice()
        } else {
            rgba
        };
        if let Some(image) = self.images.get(&id).filter(|i| i.size == (width, height)) {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &image._texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
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
            return Ok(());
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
            pixels,
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
                view,
                bind_group,
                bytes,
                size: (width, height),
                video_stamp: None,
            },
        );
        if !self.asset_order.contains(&id) {
            self.asset_order.push(id);
        }
        self.effect_gpu.invalidate();
        self.texture_bytes = self.texture_bytes - previous + bytes;
        Ok(())
    }
    pub fn synchronize_assets(&mut self, project: &Project, root: &Path) -> Result<()> {
        for asset in &project.assets {
            if self.images.contains_key(&TextureKey::Static(asset.id)) {
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
            .filter(|id| match id {
                TextureKey::Static(id) => *id != 0 && !project.assets.iter().any(|a| a.id == *id),
                TextureKey::Video(id) => !project
                    .layers
                    .iter()
                    .any(|l| l.id == *id && matches!(l.content, aem_core::Content::Video { .. })),
            })
            .collect();
        for id in remove {
            self.texture_bytes -= self.images.remove(&id).unwrap().bytes;
            self.asset_order.retain(|v| *v != id);
            self.effect_gpu.invalidate();
        }
        Ok(())
    }
    pub fn clear_assets(&mut self) {
        self.images.retain(|id, _| *id == TextureKey::Static(0));
        self.asset_order.retain(|id| *id == 0);
        self.effect_gpu.invalidate();
        self.texture_bytes = self
            .images
            .get(&TextureKey::Static(0))
            .map_or(0, |t| t.bytes);
    }
    /// New projects may reuse asset IDs from another directory. Load into a
    /// separate cache and keep the visible project's resources on failure.
    pub fn replace_assets(&mut self, project: &Project, root: &Path) -> Result<()> {
        let mut previous = std::mem::take(&mut self.images);
        let previous_bytes = self.texture_bytes;
        let previous_order = self.asset_order.clone();
        self.asset_order = vec![0];
        self.effect_gpu.invalidate();
        self.images.insert(
            TextureKey::Static(0),
            previous
                .remove(&TextureKey::Static(0))
                .expect("solid texture exists"),
        );
        self.texture_bytes = 4;
        if let Err(error) = self.synchronize_assets(project, root) {
            previous.insert(
                TextureKey::Static(0),
                self.images
                    .remove(&TextureKey::Static(0))
                    .expect("solid texture exists"),
            );
            self.images = previous;
            self.texture_bytes = previous_bytes;
            self.asset_order = previous_order;
            self.effect_gpu.invalidate();
            return Err(error);
        }
        Ok(())
    }
    pub fn draw(
        &mut self,
        scene: &Scene,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<RenderStats> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Motion Studio frame"),
            });
        let mut stats = self.encode(scene, view, width, height, &mut encoder, None)?;
        let submitted = Instant::now();
        self.queue.submit(Some(encoder.finish()));
        stats.cpu_prepare_us += submitted.elapsed().as_micros() as u64;
        self.check_health()?;
        Ok(stats)
    }
    pub fn encode(
        &mut self,
        scene: &Scene,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> Result<RenderStats> {
        self.check_health()?;
        if width == 0 || height == 0 || scene.layers.len() > MAX_LAYERS {
            return Err(RenderError::Invalid(
                "invalid render target or layer count".into(),
            ));
        }
        let started = Instant::now();
        if scene.effects.iter().any(|e| e.enabled) {
            return self.encode_effects(scene, view, width, height, encoder, timestamps);
        }
        self.effect_diagnostics.clear();
        self.effect_gpu.state.release_scratch();
        self.compositor
            .prepare(scene)
            .map_err(|e| RenderError::Invalid(e.to_string()))?;
        self.geometry_upload.clear();
        self.geometry_upload
            .extend(self.compositor.vertices.iter().map(|v| GeometryVertex {
                position: v.position,
                uv: v.uv,
            }));
        if !self.geometry_upload.is_empty() {
            self.queue.write_buffer(
                &self.vertex_buffer,
                0,
                bytemuck::cast_slice(&self.geometry_upload),
            );
        }
        for (index, layer) in scene.layers.iter().enumerate() {
            let asset = texture_key(layer);
            if !self.images.contains_key(&asset) {
                return Err(RenderError::Invalid(format!("draw image is not uploaded")));
            }
            let mut color = layer.color;
            for c in &mut color[..3] {
                *c = srgb_to_linear(*c);
            }
            let uniform = DrawUniform {
                mvp: layer.view_projection.to_cols_array_2d(),
                color,
                extent_opacity: [layer.size[0], layer.size[1], layer.opacity, 0.0],
                uv_scale: [1.0, 1.0, 0.0, 0.0],
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
                timestamp_writes: timestamps,
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
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            for batch in &self.compositor.batches {
                let index = batch.layer;
                let layer = &scene.layers[index];
                pass.set_bind_group(
                    0,
                    &self.uniform_group,
                    &[(index * self.uniform_stride) as u32],
                );
                pass.set_bind_group(1, &self.images[&texture_key(layer)].bind_group, &[]);
                pass.draw(batch.vertices.clone(), 0..1);
            }
        }
        Ok(RenderStats {
            cpu_prepare_us: started.elapsed().as_micros() as u64,
            draw_calls: self.compositor.batches.len() as u32,
            texture_bytes: self.texture_bytes,
            parameter_upload_bytes: (upload_bytes
                + self.geometry_upload.len() * std::mem::size_of::<GeometryVertex>())
                as u64,
            parameter_resource_upload_bytes: 0,
        })
    }
    fn encode_effects(
        &mut self,
        scene: &Scene,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> Result<RenderStats> {
        let started = Instant::now();
        loop {
            self.effect_gpu
                .builder
                .build(scene, &self.asset_order, width, height, false)
                .map_err(RenderError::Invalid)?;
            match self.effect_gpu.state.prepare(
                &self.device,
                &self.queue,
                &self.image_layout,
                &self.effect_gpu.builder,
                scene,
                self.texture_bytes,
            ) {
                Ok(()) => break,
                Err(error) => {
                    if let Some((program, message)) = self.effect_gpu.state.failed_program.take() {
                        self.effect_gpu
                            .builder
                            .program_errors
                            .insert(program, message);
                        continue;
                    }
                    self.effect_diagnostics = vec![error.to_string()];
                    return Err(error);
                }
            }
        }
        let frame = &self.effect_gpu.builder.frame;
        self.effect_diagnostics.clone_from(&frame.diagnostics);
        for (i, draw) in frame.draws.iter().enumerate() {
            let w = &draw.words;
            let uniform = DrawUniform {
                mvp: std::array::from_fn(|c| std::array::from_fn(|r| w[c * 4 + r])),
                color: w[16..20].try_into().unwrap(),
                extent_opacity: [w[20], w[21], w[22], 0.0],
                uv_scale: [w[25], w[26], 0.0, 0.0],
            };
            self.upload[i * self.uniform_stride..i * self.uniform_stride + DRAW_SIZE as usize]
                .copy_from_slice(bytemuck::bytes_of(&uniform));
        }
        let bytes = frame.draws.len() * self.uniform_stride;
        if bytes > 0 {
            self.queue
                .write_buffer(&self.uniform_buffer, 0, &self.upload[..bytes]);
        }
        let alpha = scene.background[3];
        let clear = wgpu::Color {
            r: (srgb_to_linear(scene.background[0]) * alpha) as f64,
            g: (srgb_to_linear(scene.background[1]) * alpha) as f64,
            b: (srgb_to_linear(scene.background[2]) * alpha) as f64,
            a: alpha as f64,
        };
        {
            let writes = timestamps
                .as_ref()
                .map(|t| wgpu::RenderPassTimestampWrites {
                    query_set: t.query_set,
                    beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                    end_of_pass_write_index: None,
                });
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("effect composition clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: writes,
                occlusion_query_set: None,
            });
        }
        for (i, draw) in frame.draws.iter().enumerate() {
            for p in draw.pass_start..draw.pass_end {
                self.effect_gpu.state.encode_pass(
                    p,
                    frame,
                    &self.asset_order,
                    &self.images,
                    &self.device,
                    &self.queue,
                    encoder,
                )?;
            }
            let writes = if i + 1 == frame.draws.len() {
                timestamps
                    .as_ref()
                    .map(|t| wgpu::RenderPassTimestampWrites {
                        query_set: t.query_set,
                        beginning_of_pass_write_index: None,
                        end_of_pass_write_index: t.end_of_pass_write_index,
                    })
            } else {
                None
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("effect composition layer"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: writes,
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
            pass.set_bind_group(0, &self.uniform_group, &[(i * self.uniform_stride) as u32]);
            let group = if draw.words[27] >= 0.0 {
                &self.effect_gpu.state.pool[0].as_ref().unwrap().composite
            } else {
                &self.images[&self.asset_order[draw.words[24] as usize]].bind_group
            };
            pass.set_bind_group(1, group, &[]);
            pass.draw(0..6, 0..1);
        }
        Ok(RenderStats {
            cpu_prepare_us: started.elapsed().as_micros() as u64,
            draw_calls: (frame.draws.len() + frame.passes.len()) as u32,
            texture_bytes: self.texture_bytes + self.effect_gpu.state.bytes(),
            parameter_upload_bytes: (bytes
                + frame.passes.len() * aem_effects::shader::UNIFORM_BYTES)
                as u64,
            parameter_resource_upload_bytes: std::mem::take(
                &mut self.effect_gpu.state.parameter_resource_upload_bytes,
            ),
        })
    }
    pub fn render_target(&self, width: u32, height: u32) -> Result<RenderTarget> {
        self.check_health()?;
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Ok(RenderTarget {
            texture,
            view,
            width,
            height,
        })
    }
    pub fn capture_target(&self, width: u32, height: u32) -> Result<CaptureTarget> {
        let target = self.render_target(width, height)?;
        let row_stride = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("AEM bounded frame readback"),
            size: u64::from(row_stride) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(CaptureTarget {
            texture: target.texture,
            view: target.view,
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
        self.preflight_effects(scene, target.width, target.height)?;
        let stats = self.draw(scene, &target.view, target.width, target.height)?;
        Ok((self.read_target(target)?, stats))
    }
    pub fn read_target(&self, target: &CaptureTarget) -> Result<Vec<u8>> {
        self.check_health()?;
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
        Ok(pixels)
    }
}

/// Convert straight-alpha sRGB bytes for correct filtered GPU compositing.
/// This work belongs to resource loading, never the preview frame loop.
pub fn premultiply_pixels(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = pixel[3] as f32 / 255.0;
        for c in &mut pixel[..3] {
            *c = (linear_to_srgb(srgb_to_linear(*c as f32 / 255.0) * alpha) * 255.0).round() as u8;
        }
    }
}

pub struct RenderTarget {
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
}
impl RenderTarget {
    pub fn texture_bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
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
pub(crate) fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
pub(crate) fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
