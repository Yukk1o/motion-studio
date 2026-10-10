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

pub(crate) const TEXTURE_BUDGET: u64 = 128 * 1024 * 1024;
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
pub(crate) type Result<T> = std::result::Result<T, RenderError>;

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
    pub(crate) _texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) bytes: u64,
    pub(crate) size: (u32, u32),
    pub(crate) video_stamp: Option<(u64, u64)>,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TextureKey {
    Static(u64),
    Video(u64),
    Vector(u64),
    Mask(u64),
    EffectInput(u64),
}
fn texture_key(layer: &aem_core::DrawLayer) -> TextureKey {
    if layer.vector.is_some() {
        TextureKey::Vector(layer.id)
    } else if layer.video.is_some() || layer.composition {
        TextureKey::Video(layer.id)
    } else {
        TextureKey::Static(layer.asset.unwrap_or(0))
    }
}
fn nested_size(parent:&Scene,child:&Scene,width:u32,height:u32)->(u32,u32) {
    let scale=(f64::from(width)/f64::from(parent.width)).min(f64::from(height)/f64::from(parent.height));
    ((f64::from(child.width)*scale).ceil().max(1.) as u32,(f64::from(child.height)*scale).ceil().max(1.) as u32)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub cpu_prepare_us: u64,
    pub draw_calls: u32,
    pub texture_bytes: u64,
    pub parameter_upload_bytes: u64,
    pub parameter_resource_upload_bytes: u64,
    pub instance_upload_bytes: u64,
    pub particles_alive: u32,
    pub particles_visible: u32,
    pub particles_culled: u32,
}

impl RenderStats {
    fn add(&mut self,other:Self) {
        self.cpu_prepare_us+=other.cpu_prepare_us;self.draw_calls+=other.draw_calls;
        self.parameter_upload_bytes+=other.parameter_upload_bytes;self.parameter_resource_upload_bytes+=other.parameter_resource_upload_bytes;
        self.instance_upload_bytes+=other.instance_upload_bytes;self.particles_alive+=other.particles_alive;
        self.particles_visible+=other.particles_visible;self.particles_culled+=other.particles_culled;
        self.texture_bytes=self.texture_bytes.max(other.texture_bytes);
    }
}

pub struct Renderer {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
    pipeline: wgpu::RenderPipeline,
    additive_pipeline: wgpu::RenderPipeline,
    masked_pipeline: wgpu::RenderPipeline,
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
    layer_gpu: crate::layer_gpu::LayerGpu,
    mask_gpu: crate::mask_gpu::MaskGpu,
    video_gpu: Option<crate::video_gpu::VideoGpu>,
    pub video_upload_bytes: u64,
    pub video_uploads: u64,
    pub video_gpu_conversions: u64,
    asset_order: Vec<u64>,
    image_sources: HashMap<u64, crate::image_resources::Source>,
    image_decode: crate::image_resources::DecodeTask,
    image_active: std::collections::BTreeSet<u64>,
    image_access: HashMap<u64, u64>,
    image_clock: u64,
    image_prefetch: Vec<u64>,
    image_prefetch_failed: std::collections::HashSet<u64>,
    image_cache_enabled: bool,
    pub image_decodes: u64,
    pub image_proxy_cache_hits: u64,
    pub image_upload_bytes: u64,
    pub image_memory_cache_hits: u64,
    pub image_prefetches: u64,
    pub effect_diagnostics: Vec<String>,
    composition_images: std::collections::HashSet<u64>,
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
        let supported = adapter.limits();
        let base = if supported.max_compute_workgroups_per_dimension == 0 {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else { wgpu::Limits::downlevel_defaults() };
        let limits = base.using_resolution(supported);
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
            // Invalid-command follow-up errors must not overwrite the root cause.
            uncaptured
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(|| format!("GPU error: {error}"));
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
        let create_pipeline = |blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };
        let pipeline = create_pipeline(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let additive_pipeline = create_pipeline(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.alpha,
        });
        let mask_shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("masked source composite"),source:wgpu::ShaderSource::Wgsl(include_str!("plane_mask.wgsl").into())});
        let masked_pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:Some("masked layer without effects"),layout:Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {label:None,bind_group_layouts:&[&uniform_layout,&image_layout,&image_layout],push_constant_ranges:&[]})),
            vertex:wgpu::VertexState {module:&mask_shader,entry_point:Some("vertex_main"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout {array_stride:std::mem::size_of::<GeometryVertex>() as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2]}]},
            fragment:Some(wgpu::FragmentState {module:&mask_shader,entry_point:Some("fragment_main"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState {format,blend:Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})]}),
            primitive:wgpu::PrimitiveState {cull_mode:None,..Default::default()},depth_stencil:None,multisample:Default::default(),multiview:None,cache:None});
        let effect_gpu = crate::effect_gpu::EffectGpu::new(&device, &queue, &image_layout)?;
        let layer_gpu =
            crate::layer_gpu::LayerGpu::new(&device, &uniform_layout, &image_layout, format);
        let mask_gpu=crate::mask_gpu::MaskGpu::new(&device,&image_layout)?;
        let mut renderer = Self {
            adapter,
            device,
            queue,
            adapter_info,
            pipeline,
            additive_pipeline,
            masked_pipeline,
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
            composition_images: Default::default(),
            gpu_failure,
            effect_gpu,
            layer_gpu,
            mask_gpu,
            video_gpu: None,
            video_upload_bytes: 0,
            video_uploads: 0,
            video_gpu_conversions: 0,
            asset_order: Vec::new(),
            image_sources: HashMap::new(),
            image_decode: Default::default(),
            image_active: Default::default(),
            image_access: Default::default(),
            image_clock: 0,
            image_prefetch: Vec::new(),
            image_prefetch_failed: Default::default(),
            image_cache_enabled: false,
            image_decodes: 0,
            image_proxy_cache_hits: 0,
            image_upload_bytes: 0,
            image_memory_cache_hits: 0,
            image_prefetches: 0,
            effect_diagnostics: Vec::new(),
        };
        renderer.effect_gpu.builder.device_dimension =
            renderer.device.limits().max_texture_dimension_2d.min(8192);
        renderer.upload_image(0, 1, 1, &[255; 4])?;
        Ok(renderer)
    }
    pub async fn headless() -> Result<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        Self::new(&instance, None, TARGET_FORMAT).await
    }
    pub fn effect_plan_builds(&self) -> u64 {
        self.effect_gpu.builder.preview_plan_builds
    }
    pub fn set_scratch_budget(&mut self, bytes: u64) -> Result<()> {
        self.effect_gpu.builder.set_scratch_budget(bytes).map_err(RenderError::Invalid)
    }
    pub fn effect_plan_cache_hits(&self) -> u64 {
        self.effect_gpu.builder.preview_cache_hits
    }
    pub fn texture_bytes(&self) -> u64 {
        self.texture_bytes
            + self.video_plane_bytes()
            + self.effect_gpu.state.bytes()
            + self.layer_gpu.bytes()
            + self.mask_gpu.scratch_bytes()
    }
    fn video_plane_bytes(&self) -> u64 {
        self.video_gpu.as_ref().map_or(0, |v| v.bytes())
    }
    pub fn set_effect_registry(&mut self, registry: aem_effects::Registry) {
        self.effect_gpu.set_registry(registry);
    }
    pub fn preflight_effects(&mut self, scene: &Scene, width: u32, height: u32) -> Result<()> {
        for node in &scene.nested {
            let (w,h)=nested_size(scene,&node.scene,width,height);
            self.preflight_effects(&node.scene,w,h)?;
        }
        self.effect_gpu
            .builder
            .build(scene, &self.asset_order, width, height, true)
            .map_err(RenderError::Invalid)?;
        self.prepare_effect_images();
        self.effect_gpu.state.prepare(
            &self.device,
            &self.queue,
            &self.image_layout,
            &self.effect_gpu.builder,
            scene,
            self.texture_bytes + self.video_plane_bytes(),
        )?;
        if self.texture_bytes + self.video_plane_bytes() + self.effect_gpu.state.resource_bytes
            > TEXTURE_BUDGET
        {
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
        self.video_uploads += 1;
        self.video_upload_bytes += rgba.len() as u64;
        self.images
            .get_mut(&TextureKey::Video(object))
            .unwrap()
            .video_stamp = Some((source, pts));
        Ok(())
    }
    pub fn upload_video_yuv(
        &mut self,
        object: u64,
        source: u64,
        pts: u64,
        frame: &crate::Yuv420Frame,
    ) -> Result<()> {
        self.check_health()?;
        frame.validate()?;
        let (width, height) = frame.display_size();
        if width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
        {
            return Err(RenderError::Invalid("video exceeds GPU dimensions".into()));
        }
        let key = TextureKey::Video(object);
        if self
            .images
            .get(&key)
            .is_some_and(|i| i.video_stamp == Some((source, pts)) && i.size == (width, height))
        {
            return Ok(());
        }
        let bytes = u64::from(width) * u64::from(height) * 4;
        let previous = self.images.get(&key).map_or(0, |i| i.bytes);
        let reinterpret = self
            .adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::VIEW_FORMATS);
        let format = if reinterpret {
            wgpu::TextureFormat::Rgba8Unorm
        } else {
            TARGET_FORMAT
        };
        let planes = self
            .video_gpu
            .as_ref()
            .map_or(frame.bytes() as u64, |v| v.replacement_bytes(object, frame));
        self.trim_image_cache((bytes + planes).saturating_sub(previous + self.video_plane_bytes()));
        if self.texture_bytes - previous + bytes + planes + self.effect_gpu.state.resource_bytes
            > TEXTURE_BUDGET
        {
            return Err(RenderError::Invalid(
                "video planes and images exceed 128 MiB".into(),
            ));
        }
        if self.images.get(&key).is_none_or(|i| {
            i.size != (width, height)
                || i._texture.format() != format
                || !i
                    ._texture
                    .usage()
                    .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        }) {
            // Write encoded RGB bytes to an UNORM attachment, then sample the
            // sRGB view in the same compositing space as existing RGBA uploads.
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("GPU converted video"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                view_formats: if reinterpret { &[TARGET_FORMAT] } else { &[] },
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST,
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(TARGET_FORMAT),
                ..Default::default()
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("converted video image"),
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
                key,
                GpuImage {
                    _texture: texture,
                    view,
                    bind_group,
                    bytes,
                    size: (width, height),
                    video_stamp: None,
                },
            );
            self.texture_bytes = self.texture_bytes - previous + bytes;
            self.effect_gpu.invalidate();
        }
        let target = self.images[&key]
            ._texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(format),
                ..Default::default()
            });
        let converter = self
            .video_gpu
            .get_or_insert_with(|| crate::video_gpu::VideoGpu::new(&self.device, format));
        converter.convert(&self.device, &self.queue, object, frame, &target);
        self.images.get_mut(&key).unwrap().video_stamp = Some((source, pts));
        self.video_uploads += 1;
        self.video_upload_bytes += frame.bytes() as u64;
        self.video_gpu_conversions += 1;
        Ok(())
    }
    pub fn retain_video_instances(&mut self, scene: &Scene) {
        fn collect(scene:&Scene,ids:&mut std::collections::HashSet<u64>,compositions:&mut std::collections::HashSet<u64>){for l in &scene.layers{if l.video.is_some()||l.composition{ids.insert(l.id);}if l.composition{compositions.insert(l.id);}}for n in &scene.nested{collect(&n.scene,ids,compositions);}}
        let mut live=Default::default();let mut compositions=Default::default();collect(scene,&mut live,&mut compositions);
        let stale: Vec<_> = self
            .images
            .keys()
            .filter_map(|key| match key {
                TextureKey::Video(object)
                    if !live.contains(object)||self.composition_images.contains(object)!=compositions.contains(object) =>
                {
                    Some(*object)
                }
                _ => None,
            })
            .collect();
        for object in stale {
            self.texture_bytes -= self
                .images
                .remove(&TextureKey::Video(object))
                .unwrap()
                .bytes;
            if let Some(video) = &mut self.video_gpu {
                video.remove(object);
            }
            self.effect_gpu.invalidate();
        }
        self.composition_images.retain(|id|compositions.contains(id));
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
        self.trim_image_cache_except(bytes.saturating_sub(previous), Some(id));
        if self.texture_bytes - previous
            + bytes
            + self.video_plane_bytes()
            + self.effect_gpu.state.resource_bytes
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
        if let TextureKey::Static(asset) = id {
            self.effect_gpu
                .builder
                .set_alpha(asset, width, height, rgba)
                .map_err(RenderError::Invalid)?;
        }
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
        if let TextureKey::Static(asset) = id {
            if !self.asset_order.contains(&asset) {
                self.asset_order.push(asset);
            }
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
                || self.texture_bytes + self.video_plane_bytes() + bytes > TEXTURE_BUDGET
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
                TextureKey::Video(_) => true,
                TextureKey::Vector(id) => !project
                    .layers
                    .iter()
                    .any(|l| l.id == *id && matches!(l.content, aem_core::Content::Vector { .. })),
                TextureKey::Mask(_) => true,
                TextureKey::EffectInput(_) => true,
            })
            .collect();
        for id in remove {
            if let TextureKey::Static(asset) = id {
                self.effect_gpu.builder.alpha_images.remove(&asset);
            }
            self.texture_bytes -= self.images.remove(&id).unwrap().bytes;
            if let TextureKey::Video(object) = id {
                if let Some(video) = &mut self.video_gpu {
                    video.remove(object);
                }
            }
            if let TextureKey::Static(asset) = id {
                self.asset_order.retain(|v| *v != asset);
            }
            self.effect_gpu.invalidate();
        }
        Ok(())
    }
    /// Register the complete, stable asset table without decoding/uploading
    /// inactive images. Source files in a project are immutable; replacing a
    /// resource must use a new path (including when retaining its asset ID).
    pub fn configure_assets(&mut self, project: &Project, root: &Path) -> Result<()> {
        let mut sources = HashMap::new();
        for asset in &project.assets {
            let source = crate::image_resources::Source::new(root, asset)
                .map_err(RenderError::Invalid)?;
            sources.insert(asset.id, source);
        }
        let stale: Vec<_> = self.images.keys().filter_map(|key| match key {
            TextureKey::Static(id) if *id != 0 && self.image_sources.get(id) != sources.get(id) => Some(*id),
            _ => None,
        }).collect();
        for id in stale { self.remove_static_image(id); }
        self.image_decode.cancel();
        self.image_active.clear();
        self.image_prefetch.clear();
        self.image_prefetch_failed.clear();
        // Resource edits and project switches may reuse object/source IDs and
        // timestamps. Never carry a decoded video or vector across a new catalog.
        let dynamic: Vec<_> = self.images.keys().copied()
            .filter(|key| !matches!(key, TextureKey::Static(_))).collect();
        for key in dynamic {
            self.texture_bytes -= self.images.remove(&key).unwrap().bytes;
            self.effect_gpu.invalidate();
        }
        if let Some(video) = &mut self.video_gpu { video.clear(); }
        self.composition_images.clear();
        self.image_sources = sources;
        self.asset_order = std::iter::once(0).chain(project.assets.iter().map(|a| a.id)).collect();
        Ok(())
    }
    fn remove_static_image(&mut self, id: u64) {
        self.image_access.remove(&id);
        if let Some(image) = self.images.remove(&TextureKey::Static(id)) {
            self.texture_bytes -= image.bytes;
            self.effect_gpu.builder.alpha_images.remove(&id);
            self.effect_gpu.invalidate();
        }
    }
    pub fn image_dimensions(&self, id: u64) -> Option<(u32, u32)> {
        self.images.get(&TextureKey::Static(id)).map(|i| i.size)
    }
    pub fn asset_ids(&self) -> &[u64] { &self.asset_order }
    pub fn image_idle_bytes(&self) -> u64 {
        self.image_access.keys().filter(|id| !self.image_active.contains(id))
            .filter_map(|id| self.images.get(&TextureKey::Static(*id))).map(|i| i.bytes).sum()
    }
    pub fn set_image_prefetch(&mut self, assets: Vec<u64>) { self.image_prefetch = assets; }
    /// Idle textures are expendable, and share (rather than extend) the source
    /// texture budget with videos, nested compositions and plugin resources.
    fn trim_image_cache(&mut self, additional: u64) {
        self.trim_image_cache_except(additional, None);
    }
    fn trim_image_cache_except(&mut self, additional: u64, replacing: Option<TextureKey>) {
        let limit = if self.image_cache_enabled { crate::image_resources::IDLE_TEXTURE_BYTES } else { 0 };
        let mut idle = self.image_idle_bytes();
        let mut total = self.texture_bytes + self.video_plane_bytes() + self.effect_gpu.state.resource_bytes + additional;
        let mut victims: Vec<_> = self.image_access.iter()
            .filter(|(id, _)| !self.image_active.contains(id) && replacing != Some(TextureKey::Static(**id)))
            .map(|(id, time)| (*time, *id)).collect();
        victims.sort_unstable();
        for (_, id) in victims {
            if idle <= limit && total <= TEXTURE_BUDGET { break; }
            let bytes = self.images.get(&TextureKey::Static(id)).map_or(0, |i| i.bytes);
            self.remove_static_image(id);
            idle -= bytes;
            total = total.saturating_sub(bytes);
        }
    }
    fn touch_image(&mut self, id: u64) {
        self.image_clock = self.image_clock.saturating_add(1);
        self.image_access.insert(id, self.image_clock);
    }
    fn prepare_effect_images(&mut self) {
        let extra = self.effect_gpu.state.pending_resource_bytes(&self.effect_gpu.builder);
        self.trim_image_cache(extra);
    }
    /// Preview polls a single bounded decode worker. Full capture/export uses
    /// synchronous preparation on its output worker, always at source size.
    pub fn prepare_scene_assets(
        &mut self,
        scene: &Scene,
        resolution: crate::image_resources::Resolution,
        asynchronous: bool,
    ) -> Result<bool> {
        use crate::image_resources::{decode, scene_assets, Resolution};
        let wanted = scene_assets(scene);
        let mut required = 4u64;
        for id in &wanted {
            let source = self.image_sources.get(id)
                .ok_or_else(|| RenderError::Invalid(format!("image asset {id} is not registered")))?;
            let (w, h) = resolution.dimensions(source.width, source.height);
            if w > self.device.limits().max_texture_dimension_2d || h > self.device.limits().max_texture_dimension_2d {
                return Err(RenderError::Invalid(format!("image {id} exceeds device texture dimensions")));
            }
            required += u64::from(w) * u64::from(h) * 4;
        }
        if required > TEXTURE_BUDGET {
            return Err(RenderError::Invalid("active image working set exceeds 128 MiB; full output requires original resolution".into()));
        }
        self.image_cache_enabled = matches!(resolution, Resolution::Preview(_));
        for id in wanted.difference(&self.image_active) {
            if self.image_access.contains_key(id) && self.images.get(&TextureKey::Static(*id))
                .is_some_and(|i| resolution.dimensions(self.image_sources[id].width, self.image_sources[id].height) == i.size) {
                self.image_memory_cache_hits += 1;
            }
        }
        self.image_active = wanted.clone();
        for id in &wanted { if self.images.contains_key(&TextureKey::Static(*id)) { self.touch_image(*id); } }
        // Wrong-resolution images cannot survive a quality switch. Idle preview
        // images otherwise stay resident until LRU or resource pressure evicts.
        let stale: Vec<_> = self.images.keys().filter_map(|key| match key {
            TextureKey::Static(id) if *id != 0 && (self.image_sources.get(id)
                .is_none_or(|s| resolution.dimensions(s.width, s.height) != self.images[key].size)) => Some(*id),
            _ => None,
        }).collect();
        for id in stale { self.remove_static_image(id); }
        self.trim_image_cache(0);
        let demand_missing = wanted.iter().any(|id| !self.images.contains_key(&TextureKey::Static(*id)));
        let candidates: std::collections::BTreeSet<_> = self.image_prefetch.iter().copied()
            .filter(|id| !wanted.contains(id)).take(crate::image_resources::MAX_PREFETCH_IMAGES).collect();
        let allowed = if self.image_cache_enabled && asynchronous && !demand_missing {
            wanted.union(&candidates).copied().collect()
        } else { wanted.clone() };
        self.image_decode.cancel_unwanted(&allowed, resolution);
        if let Some((source, mode, speculative, result)) = self.image_decode.poll() {
            if mode == resolution && allowed.contains(&source.id) && self.image_sources.get(&source.id) == Some(&source)
                && !self.images.contains_key(&TextureKey::Static(source.id)) {
                match result {
                Ok(pixels) => {
                    // Prefetch must never evict useful recent images or consume
                    // resources needed by an already active video/effect.
                    let cost = pixels.rgba.len() as u64;
                    if !wanted.contains(&source.id) && (cost + self.image_idle_bytes() > crate::image_resources::IDLE_TEXTURE_BYTES
                        || self.texture_bytes + self.video_plane_bytes() + self.effect_gpu.state.resource_bytes + cost > TEXTURE_BUDGET) { return Ok(true); }
                self.upload_pixels(TextureKey::Static(source.id), pixels.width, pixels.height, &pixels.rgba, false)?;
                self.image_decodes += u64::from(!pixels.cached);
                self.image_proxy_cache_hits += u64::from(pixels.cached);
                self.image_upload_bytes += pixels.rgba.len() as u64;
                self.touch_image(source.id);
                }
                Err(error) if error == crate::image_resources::CANCELLED => {}
                Err(error) if !speculative => return Err(RenderError::Invalid(format!("image {}: {error}", source.id))),
                Err(_) => { self.image_prefetch_failed.insert(source.id); }
                }
            }
        }
        for id in wanted {
            if self.images.contains_key(&TextureKey::Static(id)) { continue; }
            let source = self.image_sources[&id].clone();
            if asynchronous {
                if !self.image_decode.busy() {
                    self.image_decode.start(source, resolution, false).map_err(RenderError::Invalid)?;
                }
                return Ok(false);
            }
            let pixels = decode(&source, resolution)
                .map_err(|e| RenderError::Invalid(format!("image {id}: {e}")))?;
            self.upload_pixels(TextureKey::Static(id), pixels.width, pixels.height, &pixels.rgba, false)?;
            self.image_decodes += u64::from(!pixels.cached);
            self.image_proxy_cache_hits += u64::from(pixels.cached);
            self.image_upload_bytes += pixels.rgba.len() as u64;
            self.touch_image(id);
        }
        Ok(true)
    }
    /// Call after active image/video uploads. Speculation never precedes demand
    /// preparation, and at most one cancellable decode allocation exists.
    pub fn prefetch_scene_assets(&mut self, resolution: crate::image_resources::Resolution) -> Result<()> {
        if !self.image_cache_enabled || self.image_decode.busy() { return Ok(()); }
        for id in self.image_prefetch.clone().into_iter().filter(|id| !self.image_active.contains(id))
            .take(crate::image_resources::MAX_PREFETCH_IMAGES) {
            if self.images.contains_key(&TextureKey::Static(id)) { continue; }
            if self.image_prefetch_failed.contains(&id) { continue; }
            let Some(source) = self.image_sources.get(&id) else { continue; };
            let (w, h) = resolution.dimensions(source.width, source.height);
            let bytes = u64::from(w) * u64::from(h) * 4;
            if w > self.device.limits().max_texture_dimension_2d || h > self.device.limits().max_texture_dimension_2d
                || bytes + self.image_idle_bytes() > crate::image_resources::IDLE_TEXTURE_BYTES
                || self.texture_bytes + self.video_plane_bytes() + self.effect_gpu.state.resource_bytes + bytes > TEXTURE_BUDGET { continue; }
            self.image_decode.start(source.clone(), resolution, true).map_err(RenderError::Invalid)?;
            self.image_prefetches += 1;
            break;
        }
        Ok(())
    }
    pub fn clear_assets(&mut self) {
        self.image_decode.cancel();
        self.image_active.clear();
        self.image_access.clear();
        self.image_prefetch.clear();
        self.image_prefetch_failed.clear();
        self.mask_gpu.clear();
        self.composition_images.clear();
        if let Some(video) = &mut self.video_gpu {
            video.clear();
        }
        self.effect_gpu.builder.alpha_images.clear();
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
        let previous_video_gpu = self.video_gpu.take();
        let mut previous = std::mem::take(&mut self.images);
        let previous_alpha = std::mem::take(&mut self.effect_gpu.builder.alpha_images);
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
            self.video_gpu = previous_video_gpu;
            self.effect_gpu.builder.alpha_images = previous_alpha;
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
        self.draw_internal(scene,view,width,height,false)
    }
    fn draw_internal(&mut self,scene:&Scene,view:&wgpu::TextureView,width:u32,height:u32,preview:bool)->Result<RenderStats> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Motion Studio frame"),
            });
        let mut stats = self.encode_internal(scene, view, width, height, &mut encoder, None, preview)?;
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
        self.encode_internal(scene, view, width, height, encoder, timestamps, false)
    }
    /// Interactive preview, with conservative 2D effect density. `encode`,
    /// `draw`, and `capture` retain their formal-output behavior.
    pub fn encode_preview(
        &mut self,
        scene: &Scene,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> Result<RenderStats> {
        self.encode_internal(scene, view, width, height, encoder, timestamps, true)
    }
    fn encode_internal(
        &mut self,
        scene: &Scene,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
        preview: bool,
    ) -> Result<RenderStats> {
        self.check_health()?;
        let mut child_stats=RenderStats::default();let mut child_diagnostics=Vec::new();
        for node in &scene.nested {
            let (w,h)=nested_size(scene,&node.scene,width,height);
            let key=TextureKey::Video(node.layer);
            if self.images.get(&key).is_none_or(|image|image.size!=(w,h)) {
                let bytes=u64::from(w)*u64::from(h)*4;
                let previous=self.images.get(&key).map_or(0,|image|image.bytes);
                self.trim_image_cache(bytes.saturating_sub(previous));
                if self.texture_bytes-previous+self.video_plane_bytes()+self.effect_gpu.state.resource_bytes+bytes>TEXTURE_BUDGET {
                    return Err(RenderError::Invalid(format!("composition {}: nested textures exceed 128 MiB",node.composition)));
                }
                let target=self.render_target(w,h)?;
                let group=self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label:Some("Composition reference"),layout:&self.image_layout,
                    entries:&[wgpu::BindGroupEntry{binding:0,resource:wgpu::BindingResource::TextureView(&target.view)},
                        wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::Sampler(&self.sampler)}],
                });
                self.images.insert(key,GpuImage {_texture:target.texture,view:target.view,bind_group:group,bytes,size:(w,h),video_stamp:None});
                self.texture_bytes=self.texture_bytes-previous+bytes;
                self.composition_images.insert(node.layer);
                self.effect_gpu.invalidate();
            }
            let view=self.images[&key].view.clone();
            let stats=self.draw_internal(&node.scene,&view,w,h,preview).map_err(|e|RenderError::Invalid(node.scene.diagnostic(&e.to_string())))?;
            child_stats.add(stats);child_diagnostics.extend(self.effect_diagnostics.iter().map(|v|node.scene.diagnostic(v)));
        }
        let mut stats=self.encode_local(scene,view,width,height,encoder,timestamps,preview)?;
        stats.add(child_stats);self.effect_diagnostics.extend(child_diagnostics);Ok(stats)
    }
    fn encode_local(&mut self,scene:&Scene,view:&wgpu::TextureView,width:u32,height:u32,encoder:&mut wgpu::CommandEncoder,timestamps:Option<wgpu::RenderPassTimestampWrites<'_>>,preview:bool)->Result<RenderStats> {
        if width == 0 || height == 0 || scene.layers.len() > MAX_LAYERS {
            return Err(RenderError::Invalid(
                "invalid render target or layer count".into(),
            ));
        }
        let started = Instant::now();
        if scene.effects.iter().any(|e| e.enabled) || scene.layers.iter().any(|l|!l.masks.is_empty())
            || scene
                .layers
                .iter()
                .any(|l| l.vector.is_some() || l.adjustment)
        {
            return self.encode_effects(scene, view, width, height, encoder, timestamps, preview);
        }
        // The last mask may have been disabled without changing source media.
        // Releasing its coverage must also happen on the direct draw path.
        let obsolete_masks:Vec<_>=self.images.keys().copied().filter(|k|matches!(k,TextureKey::Mask(_))).collect();
        if !obsolete_masks.is_empty() {
            for key in obsolete_masks {self.texture_bytes-=self.images.remove(&key).unwrap().bytes;}
            self.mask_gpu.clear();self.effect_gpu.state.invalidate();
        }
        self.effect_diagnostics.clear();
        if self.layer_gpu.prepare_vectors(
            &self.device,
            encoder,
            &self.image_layout,
            &self.sampler,
            scene,
            &[],
            &mut self.images,
            &mut self.texture_bytes,
            self.effect_gpu.state.resource_bytes,
        )? {
            self.effect_gpu.state.invalidate();
        }
        if self.composition_images.is_empty() { self.effect_gpu.state.release_scratch(); }
        self.layer_gpu.prepare_accumulators(
            &self.device,
            &self.image_layout,
            &self.sampler,
            self.target_format,
            None,
        );
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
            texture_bytes: self.texture_bytes + self.video_plane_bytes(),
            parameter_upload_bytes: (upload_bytes
                + self.geometry_upload.len() * std::mem::size_of::<GeometryVertex>())
                as u64,
            parameter_resource_upload_bytes: 0,
            instance_upload_bytes: 0,
            particles_alive: 0,
            particles_visible: 0,
            particles_culled: 0,
        })
    }
    fn prepare_input_snapshots(&mut self)->Result<()> {
        let frame=&self.effect_gpu.builder.frame;
        let specs=crate::effect_plan::image_input_order(frame).map_err(RenderError::Invalid)?.into_iter().map(|i| {
            let draw=&frame.draws[i];let pass=&frame.passes[draw.pass_end-1];(TextureKey::EffectInput(draw.layer),pass.width,pass.height)
        }).collect::<Vec<_>>();
        let stale=self.images.keys().copied().filter(|key|matches!(key,TextureKey::EffectInput(_))&&!specs.iter().any(|v|v.0==*key)).collect::<Vec<_>>();
        for key in stale {self.texture_bytes-=self.images.remove(&key).unwrap().bytes;self.effect_gpu.invalidate();}
        let needed=specs.iter().filter(|(key,w,h)|self.images.get(key).is_none_or(|image|image.size!=(*w,*h)))
            .map(|(_,w,h)|u64::from(*w)*u64::from(*h)*4).sum();
        self.trim_image_cache(needed);
        for (key,width,height) in specs {
            if self.images.get(&key).is_some_and(|image|image.size==(width,height)){continue;}
            let bytes=u64::from(width)*u64::from(height)*4;
            if self.texture_bytes+self.video_plane_bytes()+self.effect_gpu.state.resource_bytes+bytes>TEXTURE_BUDGET {
                return Err(RenderError::Invalid("effect image snapshots exceed source texture budget".into()));
            }
            let texture=self.device.create_texture(&wgpu::TextureDescriptor {label:Some("same-frame effect image input"),size:wgpu::Extent3d{width,height,depth_or_array_layers:1},
                mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:TARGET_FORMAT,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
            let view=texture.create_view(&Default::default());
            let bind_group=self.device.create_bind_group(&wgpu::BindGroupDescriptor {label:None,layout:&self.image_layout,entries:&[
                wgpu::BindGroupEntry{binding:0,resource:wgpu::BindingResource::TextureView(&view)},wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::Sampler(&self.sampler)}]});
            if let Some(old)=self.images.insert(key,GpuImage{_texture:texture,view,bind_group,bytes,size:(width,height),video_stamp:None}){self.texture_bytes-=old.bytes;}
            self.texture_bytes+=bytes;self.effect_gpu.invalidate();
        }
        Ok(())
    }
    fn encode_effects(
        &mut self,
        scene: &Scene,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        encoder: &mut wgpu::CommandEncoder,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
        preview: bool,
    ) -> Result<RenderStats> {
        let started = Instant::now();
        loop {
            if preview {
                self.effect_gpu.builder.build_preview(scene, &self.asset_order, width, height)
            } else {
                self.effect_gpu.builder.build(scene, &self.asset_order, width, height, false)
            }
            .map_err(RenderError::Invalid)?;
            self.prepare_effect_images();
            self.prepare_input_snapshots()?;
            match self.effect_gpu.state.prepare(
                &self.device,
                &self.queue,
                &self.image_layout,
                &self.effect_gpu.builder,
                scene,
                self.texture_bytes + self.video_plane_bytes(),
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
        let vector_bytes = self.layer_gpu.pending_vector_bytes(scene, &self.effect_gpu.builder.frame.vectors, &self.images);
        let mask_bytes = self.effect_gpu.builder.frame.masks
            .chunk_by(|a,b|a.layer==b.layer)
            .map(|group| {
                let first=&group[0];
                let cost=u64::from(first.width)*u64::from(first.height);
                let resident=self.images.get(&TextureKey::Mask(scene.layers[first.layer].id)).map_or(0,|image|image.bytes);
                cost.saturating_sub(resident)
            }).sum::<u64>();
        self.trim_image_cache(vector_bytes+mask_bytes);
        let mask_other=self.video_plane_bytes()+self.effect_gpu.state.resource_bytes;
        let frame = &self.effect_gpu.builder.frame;
        let masks_changed=self.mask_gpu.prepare(&self.device,&self.queue,encoder,&self.image_layout,scene,&frame.masks,
            &mut self.images,&mut self.texture_bytes,mask_other)?;
        if masks_changed {
            self.effect_gpu.state.invalidate();
        }
        let vectors_changed = self.layer_gpu.prepare_vectors(
            &self.device,
            encoder,
            &self.image_layout,
            &self.sampler,
            scene,
            &frame.vectors,
            &mut self.images,
            &mut self.texture_bytes,
            self.effect_gpu.state.resource_bytes,
        )?;
        let has_adjustment = frame
            .draws
            .iter()
            .any(|d| d.words[31] == 2. && d.pass_start < d.pass_end);
        let render_scale = (width as f32 / scene.width as f32)
            .min(height as f32 / scene.height as f32)
            .min(1.)
            .max(0.001);
        let composition_size = [
            (scene.width as f32 * render_scale).ceil() as u32,
            (scene.height as f32 * render_scale).ceil() as u32,
        ];
        let accumulators_changed = self.layer_gpu.prepare_accumulators(
            &self.device,
            &self.image_layout,
            &self.sampler,
            self.target_format,
            has_adjustment.then_some(composition_size),
        );
        if masks_changed || vectors_changed || accumulators_changed {
            self.effect_gpu.state.invalidate();
            self.effect_gpu.state.prepare(
                &self.device,
                &self.queue,
                &self.image_layout,
                &self.effect_gpu.builder,
                scene,
                self.texture_bytes + self.video_plane_bytes(),
            )?;
        }
        self.geometry_upload.clear();
        self.geometry_upload
            .extend(frame.vertices.iter().map(|v| GeometryVertex {
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
        self.effect_diagnostics.clone_from(&frame.diagnostics);
        for (i, draw) in frame.draws.iter().enumerate() {
            let w = &draw.words;
            let uniform = DrawUniform {
                mvp: std::array::from_fn(|c| std::array::from_fn(|r| w[c * 4 + r])),
                color: w[16..20].try_into().unwrap(),
                extent_opacity: [w[20], w[21], w[22], 0.0],
                uv_scale: [w[25], w[26], scene.width as f32, scene.height as f32],
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
                    view: if has_adjustment {
                        &self.layer_gpu.accumulators[0].view
                    } else {
                        view
                    },
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if has_adjustment {
                            wgpu::Color::TRANSPARENT
                        } else {
                            clear
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: writes,
                occlusion_query_set: None,
            });
        }
        let mut materialized = None;
        let mut executed_passes = 0;
        let mut composition_draws = 0;
        let mut accumulator = 0;
        // Preserve each referenced source before another layer reuses slot 0.
        // This is GPU-only and independent of the composition's stacking order.
        for i in crate::effect_plan::image_input_order(frame).map_err(RenderError::Invalid)? {
            let draw=&frame.draws[i];
            for p in draw.pass_start..draw.pass_end {
                self.effect_gpu.state.encode_pass(p,frame,&self.asset_order,
                    Some(texture_key(&scene.layers[i])),None,&self.images,&self.device,&self.queue,encoder)?;
                executed_passes+=1;
            }
            let output=&frame.passes[draw.pass_end-1];
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo{texture:&self.effect_gpu.state.pool[0].as_ref().unwrap().texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},
                wgpu::TexelCopyTextureInfo{texture:&self.images[&TextureKey::EffectInput(draw.layer)]._texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},
                wgpu::Extent3d{width:output.width,height:output.height,depth_or_array_layers:1});
        }
        for (batch_index, batch) in frame.batches.iter().enumerate() {
            let i = batch.layer;
            let draw = &frame.draws[i];
            if materialized!=Some(i) {
                if let Some(cached)=self.images.get(&TextureKey::EffectInput(draw.layer)) {
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo{texture:&cached._texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},
                        wgpu::TexelCopyTextureInfo{texture:&self.effect_gpu.state.pool[0].as_ref().unwrap().texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},
                        wgpu::Extent3d{width:cached.size.0,height:cached.size.1,depth_or_array_layers:1});
                    materialized=Some(i);
                }
            }
            for p in draw.pass_start..draw.pass_end {
                if materialized == Some(i) {
                    break;
                }
                self.effect_gpu.state.encode_pass(
                    p,
                    frame,
                    &self.asset_order,
                    (scene.layers[i].video.is_some() || scene.layers[i].vector.is_some() || scene.layers[i].composition)
                        .then(|| texture_key(&scene.layers[i])),
                    (scene.layers[i].adjustment && has_adjustment)
                        .then(|| &self.layer_gpu.accumulators[accumulator].view),
                    &self.images,
                    &self.device,
                    &self.queue,
                    encoder,
                )?;
                executed_passes += 1;
            }
            materialized = Some(i);
            if scene.layers[i].adjustment {
                if draw.pass_start == draw.pass_end || draw.words[22] <= 0. {
                    continue;
                }
                let next = 1 - accumulator;
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("adjust lower composite"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &self.layer_gpu.accumulators[next].view,
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
                    pass.set_pipeline(&self.layer_gpu.adjustment);
                    pass.set_bind_group(
                        0,
                        &self.uniform_group,
                        &[(i * self.uniform_stride) as u32],
                    );
                    pass.set_bind_group(
                        1,
                        &self.layer_gpu.accumulators[accumulator].composite,
                        &[],
                    );
                    pass.set_bind_group(
                        2,
                        &self.effect_gpu.state.pool[0].as_ref().unwrap().composite,
                        &[],
                    );
                    pass.draw(0..3, 0..1);
                }
                composition_draws += 1;
                accumulator = next;
                continue;
            }
            let writes = if !has_adjustment && batch_index + 1 == frame.batches.len() {
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
                    view: if has_adjustment {
                        &self.layer_gpu.accumulators[accumulator].view
                    } else {
                        view
                    },
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
            let (vw, vh) = if has_adjustment {
                (composition_size[0] as f32, composition_size[1] as f32)
            } else {
                let scale =
                    (width as f32 / scene.width as f32).min(height as f32 / scene.height as f32);
                (scene.width as f32 * scale, scene.height as f32 * scale)
            };
            pass.set_viewport(
                if has_adjustment {
                    0.
                } else {
                    (width as f32 - vw) / 2.0
                },
                if has_adjustment {
                    0.
                } else {
                    (height as f32 - vh) / 2.0
                },
                vw,
                vh,
                0.0,
                1.0,
            );
            let masked=draw.words[27]<0. && !scene.layers[i].masks.is_empty();
            pass.set_pipeline(if masked {&self.masked_pipeline} else if draw.words[23] > 0.5 {
                &self.additive_pipeline
            } else {
                &self.pipeline
            });
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_bind_group(0, &self.uniform_group, &[(i * self.uniform_stride) as u32]);
            let group = if draw.words[27] >= 0.0 {
                &self.effect_gpu.state.pool[0].as_ref().unwrap().composite
            } else {
                &self.images[&texture_key(&scene.layers[i])].bind_group
            };
            pass.set_bind_group(1, group, &[]);
            if masked {pass.set_bind_group(2,&self.images[&TextureKey::Mask(scene.layers[i].id)].bind_group,&[]);}
            pass.draw(batch.vertices.clone(), 0..1);
            composition_draws += 1;
        }
        if has_adjustment {
            let writes = timestamps
                .as_ref()
                .map(|t| wgpu::RenderPassTimestampWrites {
                    query_set: t.query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: t.end_of_pass_write_index,
                });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present adjusted composition"),
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
            let scale =
                (width as f32 / scene.width as f32).min(height as f32 / scene.height as f32);
            let (vw, vh) = (scene.width as f32 * scale, scene.height as f32 * scale);
            pass.set_viewport(
                (width as f32 - vw) * 0.5,
                (height as f32 - vh) * 0.5,
                vw,
                vh,
                0.,
                1.,
            );
            pass.set_pipeline(&self.layer_gpu.present);
            pass.set_bind_group(0, &self.uniform_group, &[0]);
            pass.set_bind_group(1, &self.layer_gpu.accumulators[accumulator].composite, &[]);
            pass.draw(0..3, 0..1);
            composition_draws += 1;
        }
        Ok(RenderStats {
            cpu_prepare_us: started.elapsed().as_micros() as u64,
            draw_calls: (composition_draws + executed_passes) as u32,
            texture_bytes: self.texture_bytes
                + self.video_plane_bytes()
                + self.effect_gpu.state.bytes()
                + self.layer_gpu.bytes(),
            parameter_upload_bytes: (bytes
                + executed_passes * aem_effects::shader::UNIFORM_BYTES
                + frame.vertices.len() * 20) as u64,
            instance_upload_bytes: (frame.sprites.len() * 48) as u64,
            particles_alive: frame.generator_stats.alive,
            particles_visible: frame.generator_stats.visible,
            particles_culled: frame.generator_stats.culled,
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
