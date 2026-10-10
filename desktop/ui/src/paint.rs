//! Retained-mode paint command list and its wgpu renderer.
//!
//! Panels emit flat commands into a shared buffer each frame; the renderer
//! uploads one vertex buffer per pass and draws every solid shape and glyph.
//! Keeping the pass count fixed is what lets a dense timeline redraw without
//! allocating or stalling the GPU.

use crate::input::Rect;
use crate::theme::Color;
use wgpu::util::DeviceExt;

/// One drawable shape.
#[derive(Clone, Debug)]
pub enum Shape {
    /// Convex quad in logical pixels, wound for a triangle strip.
    Quad { points: [[f32; 2]; 4] },
    /// Polyline expanded to a strip on the CPU.
    Line { points: Vec<[f32; 2]> },
}

/// Solid fills collected for one frame.
#[derive(Default)]
pub struct PaintList {
    pub shapes: Vec<Shape>,
    pub colors: Vec<Color>,
    pub glyphs: Vec<Glyph>,
}

impl PaintList {
    /// Bound flat UI geometry and adjust cropped glyph UVs to a panel rectangle.
    pub fn clip_to(&mut self, area: Rect) {
        let mut shapes = Vec::new();
        let mut colors = Vec::new();
        for (mut shape, color) in std::mem::take(&mut self.shapes)
            .into_iter()
            .zip(std::mem::take(&mut self.colors))
        {
            let points: &mut [[f32; 2]] = match &mut shape {
                Shape::Quad { points } => points,
                Shape::Line { points } => points,
            };
            let intersects = points.iter().any(|p| {
                p[0] >= area.min[0]
                    && p[0] <= area.max[0]
                    && p[1] >= area.min[1]
                    && p[1] <= area.max[1]
            });
            let spans = points.iter().any(|p| p[0] < area.min[0])
                && points.iter().any(|p| p[0] > area.max[0])
                || points.iter().any(|p| p[1] < area.min[1])
                    && points.iter().any(|p| p[1] > area.max[1]);
            if !intersects && !spans {
                continue;
            }
            for point in points {
                point[0] = point[0].clamp(area.min[0], area.max[0]);
                point[1] = point[1].clamp(area.min[1], area.max[1]);
            }
            shapes.push(shape);
            colors.push(color);
        }
        self.shapes = shapes;
        self.colors = colors;
        self.glyphs.retain_mut(|glyph| {
            let left = glyph.position[0].max(area.min[0]);
            let top = glyph.position[1].max(area.min[1]);
            let right = (glyph.position[0] + glyph.size[0]).min(area.max[0]);
            let bottom = (glyph.position[1] + glyph.size[1]).min(area.max[1]);
            if right <= left || bottom <= top {
                return false;
            }
            let [u0, v0, u1, v1] = glyph.uv;
            glyph.uv = [
                u0 + (u1 - u0) * (left - glyph.position[0]) / glyph.size[0],
                v0 + (v1 - v0) * (top - glyph.position[1]) / glyph.size[1],
                u0 + (u1 - u0) * (right - glyph.position[0]) / glyph.size[0],
                v0 + (v1 - v0) * (bottom - glyph.position[1]) / glyph.size[1],
            ];
            glyph.position = [left, top];
            glyph.size = [right - left, bottom - top];
            true
        });
    }
    pub fn append(&mut self, mut other: Self) {
        self.shapes.append(&mut other.shapes);
        self.colors.append(&mut other.colors);
        self.glyphs.append(&mut other.glyphs);
    }
}

/// One positioned glyph.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    pub uv: [f32; 4],
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: Color,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SolidVertex {
    position: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlyphVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

const SOLID_CAPACITY: usize = 1 << 16;
const GLYPH_CAPACITY: usize = 1 << 17;

/// Draw a filled rectangle.
pub fn rect(list: &mut PaintList, area: Rect, color: Color) {
    if area.width() <= 0.0 || area.height() <= 0.0 || color.0[3] <= 0.0 {
        return;
    }
    let min = area.min;
    let max = area.max;
    // Counter-clockwise in screen space so both pipelines can use one winding.
    list.shapes.push(Shape::Quad {
        points: [min, [max[0], min[1]], max, [min[0], max[1]]],
    });
    list.colors.push(color);
}

/// Stroke a rectangle outline of `thickness` logical pixels.
pub fn diamond(list: &mut PaintList, center: [f32; 2], radius: f32, color: Color) {
    let [x, y] = center;
    list.shapes.push(Shape::Quad {
        points: [
            [x, y - radius],
            [x + radius, y],
            [x, y + radius],
            [x - radius, y],
        ],
    });
    list.colors.push(color);
}

/// Stroke a rectangle outline of `thickness` logical pixels.
pub fn stroke(list: &mut PaintList, area: Rect, color: Color, thickness: f32) {
    if thickness <= 0.0 {
        return;
    }
    rect(
        list,
        Rect::new(area.min[0], area.min[1], area.width(), thickness),
        color,
    );
    rect(
        list,
        Rect::new(
            area.min[0],
            area.max[1] - thickness,
            area.width(),
            thickness,
        ),
        color,
    );
    rect(
        list,
        Rect::new(area.min[0], area.min[1], thickness, area.height()),
        color,
    );
    rect(
        list,
        Rect::new(
            area.max[0] - thickness,
            area.min[1],
            thickness,
            area.height(),
        ),
        color,
    );
}

pub fn line(list: &mut PaintList, points: Vec<[f32; 2]>, color: Color) {
    if points.len() < 2 || color.0[3] <= 0.0 {
        return;
    }
    list.shapes.push(Shape::Line { points });
    list.colors.push(color);
}

/// Alpha checkerboard for transparent pixels.
pub fn checkerboard(list: &mut PaintList, area: Rect, light: Color, dark: Color, size: f32) {
    let columns = (area.width() / size).ceil() as i32;
    let rows = (area.height() / size).ceil() as i32;
    for row in 0..rows.max(0) {
        for column in 0..columns.max(0) {
            let color = if (row + column) % 2 == 0 { light } else { dark };
            let min_x = area.min[0] + column as f32 * size;
            let min_y = area.min[1] + row as f32 * size;
            let width = (area.min[0] + (column as f32 + 1.0) * size).min(area.max[0]) - min_x;
            let height = (area.min[1] + (row as f32 + 1.0) * size).min(area.max[1]) - min_y;
            rect(list, Rect::new(min_x, min_y, width, height), color);
        }
    }
}

fn expand_quad(points: [[f32; 2]; 4]) -> [[f32; 2]; 6] {
    [
        points[0], points[1], points[2], points[0], points[2], points[3],
    ]
}

fn expand_line(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(points.len().saturating_sub(1) * 6);
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = dx.hypot(dy);
        if length <= f32::EPSILON {
            continue;
        }
        let nx = -dy / length * 0.5;
        let ny = dx / length * 0.5;
        out.extend_from_slice(&expand_quad([
            [a[0] + nx, a[1] + ny],
            [b[0] + nx, b[1] + ny],
            [b[0] - nx, b[1] - ny],
            [a[0] - nx, a[1] - ny],
        ]));
    }
    out
}
fn upload_color(mut rgba: [f32; 4], srgb: bool) -> [f32; 4] {
    if srgb {
        for component in &mut rgba[..3] {
            let value = *component;
            *component = if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            };
        }
    }
    rgba
}

/// GPU resources for the two-pass panel renderer.
pub struct Painter {
    srgb_target: bool,
    solid_pipeline: wgpu::RenderPipeline,
    glyph_pipeline: wgpu::RenderPipeline,
    solid_layout: wgpu::BindGroupLayout,
    glyph_layout: wgpu::BindGroupLayout,
    solid_uniform: wgpu::Buffer,
    glyph_uniform: wgpu::Buffer,
    solid_buffer: wgpu::Buffer,
    glyph_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
    atlas: Option<wgpu::Texture>,
    atlas_view: Option<wgpu::TextureView>,
    atlas_size: [u32; 2],
    /// Solid vertices submitted for the last frame.
    pub last_solid_vertices: usize,
    /// Glyph vertices submitted for the last frame.
    pub last_glyph_vertices: usize,
}

impl Painter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("panel shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/panel.wgsl").into()),
        });
        let uniform_entry = || wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let solid_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("solid layout"),
            entries: &[uniform_entry()],
        });
        let glyph_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glyph layout"),
            entries: &[
                uniform_entry(),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // The transform is passed through a uniform buffer rather than push
        // constants: push constants need a device feature the panel pipeline
        // should not depend on, and one uniform per pass is enough here.
        let layout_for = |label: &str, bind: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[bind],
                push_constant_ranges: &[],
            })
        };
        let solid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("panel solids"),
            layout: Some(&layout_for("solid pipeline", &solid_layout)),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("solid_vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SolidVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("solid_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("panel glyphs"),
            layout: Some(&layout_for("glyph pipeline", &glyph_layout)),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("glyph_vs"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlyphVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x4,
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("glyph_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let uniform_bytes = bytemuck::bytes_of(&[0.0f32, 0.0, 1.0, 1.0]);
        Self {
            srgb_target: format.is_srgb(),
            solid_pipeline,
            glyph_pipeline,
            solid_layout,
            glyph_layout,
            solid_uniform: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("solid uniforms"),
                contents: uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }),
            glyph_uniform: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glyph uniforms"),
                contents: uniform_bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }),
            solid_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("solid vertices"),
                size: (SOLID_CAPACITY * std::mem::size_of::<SolidVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            glyph_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glyph vertices"),
                size: (GLYPH_CAPACITY * std::mem::size_of::<GlyphVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("glyph sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            atlas: None,
            atlas_view: None,
            atlas_size: [0, 0],
            last_solid_vertices: 0,
            last_glyph_vertices: 0,
        }
    }

    /// Upload a glyph atlas bitmap. Only needed when new glyphs were rasterised.
    pub fn upload_atlas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) {
        if self.atlas_size != [width, height] || self.atlas.is_none() {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("glyph atlas"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.atlas_view = Some(texture.create_view(&Default::default()));
            self.atlas = Some(texture);
            self.atlas_size = [width, height];
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: self.atlas.as_ref().unwrap(),
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

    /// Record commands and draw them over `target`.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        list: &PaintList,
        viewport: [f32; 2],
    ) {
        let mut solids: Vec<SolidVertex> = Vec::with_capacity(list.shapes.len() * 6);
        for (shape, color) in list.shapes.iter().zip(list.colors.iter()) {
            let rgba = upload_color(color.0, self.srgb_target);
            match shape {
                Shape::Quad { points } => {
                    for vertex in expand_quad(*points) {
                        solids.push(SolidVertex {
                            position: vertex,
                            color: rgba,
                        });
                    }
                }
                Shape::Line { points } => {
                    for vertex in expand_line(points) {
                        solids.push(SolidVertex {
                            position: vertex,
                            color: rgba,
                        });
                    }
                }
            }
        }
        let glyphs: Vec<GlyphVertex> = list
            .glyphs
            .iter()
            .flat_map(|glyph| {
                let rgba = upload_color(glyph.color.0, self.srgb_target);
                let min = glyph.position;
                let max = [
                    glyph.position[0] + glyph.size[0],
                    glyph.position[1] + glyph.size[1],
                ];
                [
                    (min, [glyph.uv[0], glyph.uv[1]]),
                    ([max[0], min[1]], [glyph.uv[2], glyph.uv[1]]),
                    (max, [glyph.uv[2], glyph.uv[3]]),
                    (min, [glyph.uv[0], glyph.uv[1]]),
                    (max, [glyph.uv[2], glyph.uv[3]]),
                    ([min[0], max[1]], [glyph.uv[0], glyph.uv[3]]),
                ]
                .into_iter()
                .map(|(position, uv)| GlyphVertex {
                    position,
                    uv,
                    color: rgba,
                })
                .collect::<Vec<_>>()
            })
            .collect();
        self.last_solid_vertices = solids.len();
        self.last_glyph_vertices = glyphs.len();
        if !solids.is_empty() {
            queue.write_buffer(&self.solid_buffer, 0, bytemuck::cast_slice(&solids));
        }
        if !glyphs.is_empty() {
            queue.write_buffer(&self.glyph_buffer, 0, bytemuck::cast_slice(&glyphs));
        }
        // Logical pixels to clip space: x to the right, y downward.
        queue.write_buffer(
            &self.solid_uniform,
            0,
            bytemuck::bytes_of(&[2.0 / viewport[0], 0.0, 0.0, -2.0 / viewport[1]]),
        );
        queue.write_buffer(
            &self.glyph_uniform,
            0,
            bytemuck::bytes_of(&[2.0 / viewport[0], 0.0, 0.0, -2.0 / viewport[1]]),
        );
        let solid_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("solid bind group"),
            layout: &self.solid_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.solid_uniform.as_entire_binding(),
            }],
        });
        let glyph_bind = self.atlas_view.as_ref().map(|view| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glyph bind group"),
                layout: &self.glyph_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.glyph_uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("panels"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if solids.is_empty() && glyphs.is_empty() {
            return;
        }
        if !solids.is_empty() {
            pass.set_pipeline(&self.solid_pipeline);
            pass.set_bind_group(0, &solid_bind, &[]);
            pass.set_vertex_buffer(0, self.solid_buffer.slice(..));
            pass.draw(0..solids.len() as u32, 0..1);
        }
        if let Some(bind) = glyph_bind {
            if !glyphs.is_empty() {
                pass.set_pipeline(&self.glyph_pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.set_vertex_buffer(0, self.glyph_buffer.slice(..));
                pass.draw(0..glyphs.len() as u32, 0..1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_clipping_crops_text_uvs_and_keeps_geometry_inside() {
        let mut list = PaintList::default();
        rect(
            &mut list,
            Rect::new(-5.0, -5.0, 20.0, 20.0),
            Color::grey(30),
        );
        list.glyphs.push(Glyph {
            uv: [0.0, 0.0, 1.0, 1.0],
            position: [-5.0, 0.0],
            size: [20.0, 10.0],
            color: Color::grey(255),
        });
        list.clip_to(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(list.glyphs[0].uv, [0.25, 0.0, 0.75, 1.0]);
        assert_eq!(list.glyphs[0].size, [10.0, 10.0]);
        assert!(
            matches!(&list.shapes[0],Shape::Quad{points} if points.iter().all(|p|p[0]>=0.0&&p[0]<=10.0&&p[1]>=0.0&&p[1]<=10.0))
        );
    }

    #[test]
    fn degenerate_rectangles_emit_nothing() {
        let mut list = PaintList::default();
        rect(
            &mut list,
            Rect::new(0.0, 0.0, 0.0, 10.0),
            Color::rgb(1, 2, 3),
        );
        rect(
            &mut list,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::rgba(1, 2, 3, 0.0),
        );
        assert!(list.shapes.is_empty());
    }

    #[test]
    fn a_quad_becomes_six_triangle_list_vertices() {
        let mut list = PaintList::default();
        rect(&mut list, Rect::new(0.0, 0.0, 4.0, 4.0), Color::grey(20));
        assert_eq!(list.shapes.len(), 1);
        let mut solids = Vec::new();
        for (shape, color) in list.shapes.iter().zip(list.colors.iter()) {
            if let Shape::Quad { points } = shape {
                solids.extend(expand_quad(*points));
            }
            assert_eq!(color.0[3], 1.0);
        }
        assert_eq!(solids.len(), 6);
    }

    #[test]
    fn polylines_expand_to_triangles_without_dropping_the_last_segment() {
        let line = expand_line(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
        assert_eq!(line.len(), 12);
        assert!(line.iter().any(|p| (p[1] - 10.0).abs() < 0.01));
        assert!(expand_line(&[[1.0, 1.0], [1.0, 1.0]]).is_empty());
    }

    #[test]
    fn stroke_covers_four_edges() {
        let mut list = PaintList::default();
        stroke(
            &mut list,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::grey(80),
            1.0,
        );
        assert_eq!(list.shapes.len(), 4);
        let mut list = PaintList::default();
        stroke(
            &mut list,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::grey(80),
            0.0,
        );
        assert!(list.shapes.is_empty());
    }

    #[test]
    fn checkerboard_tiles_are_clipped_to_the_requested_area() {
        let mut list = PaintList::default();
        checkerboard(
            &mut list,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Color::grey(70),
            Color::grey(50),
            8.0,
        );
        // Two columns by two rows, all four clipped to the 10x10 area.
        assert_eq!(list.shapes.len(), 4);
        let areas: Vec<f32> = list
            .shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Quad { points } => {
                    let width = points[2][0] - points[0][0];
                    let height = points[2][1] - points[0][1];
                    Some(width * height)
                }
                Shape::Line { .. } => None,
            })
            .collect();
        assert_eq!(areas.iter().sum::<f32>(), 100.0);
    }
}
