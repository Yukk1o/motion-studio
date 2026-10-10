//! Bounded, independent source textures for authoring previews.
//! Every exposed view samples linear premultiplied RGBA. No CPU GPU-readback.
use crate::{image_resources::Resolution, video_gpu::VideoGpu, Yuv420Frame};
use wgpu::util::DeviceExt;

pub const DEFAULT_INPUT_BUDGET: u64 = 64 * 1024 * 1024;
pub struct PreviewInputTexture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    budget: u64,
    output: Option<wgpu::Texture>,
    view: Option<wgpu::TextureView>,
    size: (u32, u32),
    rgba: Option<wgpu::Texture>,
    rgba_size: (u32, u32),
    video: Option<VideoGpu>,
    copy_layout: wgpu::BindGroupLayout,
    copy_pipeline: wgpu::RenderPipeline,
    copy_sampler: wgpu::Sampler,
    copy_uniform: wgpu::Buffer,
    copy_group: Option<wgpu::BindGroup>,
}
impl PreviewInputTexture {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, budget: u64) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("authoring source copy"),
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
            label: Some("authoring source color contract"),
            source: wgpu::ShaderSource::Wgsl(include_str!("preview_input.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("authoring source resize"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let copy_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let copy_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("source alpha convention"),
            contents: bytemuck::cast_slice(&[0u32; 4]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            device,
            queue,
            budget,
            output: None,
            view: None,
            size: (0, 0),
            rgba: None,
            rgba_size: (0, 0),
            video: None,
            copy_layout: layout,
            copy_pipeline: pipeline,
            copy_sampler,
            copy_uniform,
            copy_group: None,
        }
    }
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.view.as_ref()
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.size
    }
    pub fn memory_bytes(&self) -> u64 {
        bytes(self.size) + bytes(self.rgba_size) + self.video.as_ref().map_or(0, VideoGpu::bytes)
    }
    fn validate_size(&self, width: u32, height: u32) -> Result<(), String> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > limit || height > limit {
            return Err("preview input dimensions exceed device limits".into());
        }
        Ok(())
    }
    fn output_size(&self, width: u32, height: u32, max_edge: u32) -> Result<(u32, u32), String> {
        self.validate_size(width, height)?;
        if !(1..=crate::image_resources::MAX_PREVIEW_EDGE).contains(&max_edge) {
            return Err("invalid preview input raster edge".into());
        }
        Ok(Resolution::Preview(max_edge).dimensions(width, height))
    }
    fn budget(&self, output: (u32, u32), source_bytes: u64) -> Result<(), String> {
        if bytes(output)
            .checked_add(source_bytes)
            .is_none_or(|total| total > self.budget)
        {
            return Err("preview input exceeds GPU budget".into());
        }
        Ok(())
    }
    fn texture(&self, size: (u32, u32)) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("independent authoring source"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }
    fn prepare_output(&mut self, size: (u32, u32)) {
        if self.size != size {
            let texture = self.texture(size);
            self.view = Some(texture.create_view(&Default::default()));
            self.output = Some(texture);
            self.size = size;
        }
    }
    /// Image-resource Pixels already use linear-premultiplied sRGB storage.
    /// Raw codec RGBA is straight sRGB; pass false to premultiply on the GPU.
    pub fn upload_rgba(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
        premultiplied: bool,
        max_edge: u32,
    ) -> Result<(), String> {
        let output = self.output_size(width, height, max_edge)?;
        if pixels.len() as u64 != u64::from(width) * u64::from(height) * 4 {
            return Err("preview RGBA byte count mismatch".into());
        }
        let direct = premultiplied && output == (width, height);
        self.budget(output, if direct { 0 } else { bytes((width, height)) })?;
        self.video = None;
        if direct {
            self.copy_group = None;
            self.rgba = None;
            self.rgba_size = (0, 0);
            self.prepare_output(output);
            write(
                &self.queue,
                self.output.as_ref().unwrap(),
                width,
                height,
                pixels,
            );
        } else {
            if self.rgba_size != (width, height) {
                self.copy_group = None;
                self.rgba = Some(self.texture((width, height)));
                self.rgba_size = (width, height);
            }
            write(
                &self.queue,
                self.rgba.as_ref().unwrap(),
                width,
                height,
                pixels,
            );
            self.prepare_output(output);
            let flags = [u32::from(!premultiplied), 0, 0, 0];
            self.queue
                .write_buffer(&self.copy_uniform, 0, bytemuck::cast_slice(&flags));
            if self.copy_group.is_none() {
                let view = self.rgba.as_ref().unwrap().create_view(&Default::default());
                let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.copy_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.copy_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.copy_uniform.as_entire_binding(),
                        },
                    ],
                });
                self.copy_group = Some(group);
            }
            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("authoring source GPU resize"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: self.view.as_ref().unwrap(),
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
                pass.set_pipeline(&self.copy_pipeline);
                pass.set_bind_group(0, self.copy_group.as_ref().unwrap(), &[]);
                pass.draw(0..3, 0..1);
            }
            self.queue.submit(Some(encoder.finish()));
        }
        Ok(())
    }
    pub fn upload_yuv(&mut self, frame: &Yuv420Frame, max_edge: u32) -> Result<(), String> {
        frame.validate().map_err(|error| error.to_string())?;
        self.validate_size(frame.width, frame.height)?;
        let (width, height) = frame.display_size();
        let output = self.output_size(width, height, max_edge)?;
        self.budget(output, frame.bytes() as u64)?;
        self.copy_group = None;
        self.rgba = None;
        self.rgba_size = (0, 0);
        self.prepare_output(output);
        let converter = self.video.get_or_insert_with(|| {
            VideoGpu::new(&self.device, wgpu::TextureFormat::Rgba8UnormSrgb)
        });
        converter.convert_resized(
            &self.device,
            &self.queue,
            0,
            frame,
            self.view.as_ref().unwrap(),
            output,
        );
        Ok(())
    }
    pub fn transparent(&mut self) -> Result<(), String> {
        self.upload_rgba(1, 1, &[0; 4], true, 1)
    }
}
fn bytes(size: (u32, u32)) -> u64 {
    u64::from(size.0) * u64::from(size.1) * 4
}
fn write(queue: &wgpu::Queue, texture: &wgpu::Texture, width: u32, height: u32, pixels: &[u8]) {
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
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}
