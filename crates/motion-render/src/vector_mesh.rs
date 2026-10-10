//! Both GPU backends consume the same tessellated triangles and linear paints.
use bytemuck::{Pod, Zeroable};
use lyon_tessellation::{
    math::point, path::Path, BuffersBuilder, FillOptions, FillTessellator, FillVertex,
    StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};
use motion_model::vector::{FillRule, LineCap, LineJoin, SampledVector};
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct VectorVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}
#[derive(Clone, Debug)]
pub struct VectorMesh {
    pub layer: usize,
    pub width: u32,
    pub height: u32,
    pub fingerprint: u64,
    pub vertices: std::sync::Arc<Vec<VectorVertex>>,
    pub commands: std::sync::Arc<Vec<RasterCommand>>,
    pub root_opacity: f32,
    pub origin: [f32; 2],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct RasterCommand {
    /// 0: draw triangles, 1: begin isolated group, 2: composite isolated group.
    pub kind: u32,
    pub start: u32,
    pub end: u32,
    pub opacity: f32,
}
impl VectorMesh {
    pub fn scope_depth(&self) -> usize {
        let (mut depth, mut maximum) = (0usize, 0usize);
        for command in self.commands.iter() {
            if command.kind == 1 { depth += 1; maximum = maximum.max(depth); }
            else if command.kind == 2 { depth -= 1; }
        }
        maximum
    }
}
pub struct RasterGeometry { pub vertices: Vec<VectorVertex>, pub commands: Vec<RasterCommand>, pub root_opacity: f32 }
pub fn group_bounds(v: &SampledVector) -> Option<[f32; 4]> {
    let batches = v.batches.as_ref()?;
    let mut min = glam::Vec2::splat(f32::INFINITY);
    let mut max = glam::Vec2::splat(f32::NEG_INFINITY);
    for batch in batches {
        let matrix = glam::Mat3::from_cols_array(&batch.transform);
        let radius = batch.vector.stroke.map_or(0., |s| s.1*0.5*if s.3==LineJoin::Miter{s.4}else{1.});
        let pad = (matrix.x_axis.truncate().abs()+matrix.y_axis.truncate().abs())*radius+glam::Vec2::ONE;
        for n in batch.vector.paths.iter().flat_map(|p|&p.nodes) {
            for p in [glam::Vec2::new(n[0],n[1]),glam::Vec2::new(n[0]+n[2],n[1]+n[3]),glam::Vec2::new(n[0]+n[4],n[1]+n[5])] {
                let p=matrix.transform_point2(p);min=min.min(p-pad);max=max.max(p+pad);
            }
        }
    }
    if !min.is_finite() { return Some([-0.5,-0.5,1.,1.]); }
    Some([min.x,min.y,max.x-min.x,max.y-min.y])
}
pub fn rasterize(v: &SampledVector, size: [f32; 2], scale: f32, origin: [f32; 2]) -> Result<RasterGeometry, String> {
    let Some(batches) = &v.batches else {
        return Ok(RasterGeometry { vertices: tessellate(v, size, scale)?, commands: vec![], root_opacity: 1. });
    };
    let mut vertices = Vec::new();
    let mut commands = Vec::new();
    let mut scopes: Vec<motion_model::vector::groups::PaintScope> = Vec::new();
    for batch in batches {
        let matrix = glam::Mat3::from_cols_array(&batch.transform);
        let mut triangles = tessellate(&batch.vector,size,scale)?;
        for vertex in &mut triangles {
            let local=glam::Vec2::new(vertex.position[0]*size[0]*0.5,-vertex.position[1]*size[1]*0.5);
            let point=matrix.transform_point2(local)-glam::Vec2::from_array(origin);
            vertex.position=[point.x*2./size[0],-point.y*2./size[1]];
        }
        let visible:Vec<_>=triangles.chunks_exact(3).filter(|triangle| !(0..2).any(|axis|
            triangle.iter().all(|v|v.position[axis]< -1.) || triangle.iter().all(|v|v.position[axis]>1.)))
            .flat_map(|t|t.iter().copied()).collect();
        if visible.is_empty() {continue;}
        let common = scopes.iter().zip(&batch.scopes).take_while(|(a,b)| a == b).count();
        for scope in scopes.drain(common..).rev() {
            commands.push(RasterCommand { kind: 2, start: 0, end: 0, opacity: scope.opacity });
        }
        for scope in batch.scopes.iter().skip(common) {
            commands.push(RasterCommand { kind: 1, start: 0, end: 0, opacity: scope.opacity });
            scopes.push(scope.clone());
        }
        let start = vertices.len() as u32;
        vertices.extend(visible);
        if vertices.len() > 262144 { return Err("vector triangle vertex limit exceeded (262144)".into()); }
        commands.push(RasterCommand { kind: 0, start, end: vertices.len() as u32, opacity: 1. });
    }
    for scope in scopes.into_iter().rev() { commands.push(RasterCommand {kind:2,start:0,end:0,opacity:scope.opacity}); }
    Ok(RasterGeometry { vertices, commands, root_opacity: v.root_opacity })
}
fn paint(mut c: [f32; 4]) -> [f32; 4] {
    for i in 0..3 {
        c[i] = crate::renderer::srgb_to_linear(c[i]) * c[3];
    }
    c
}
pub fn tessellate(
    v: &SampledVector,
    size: [f32; 2],
    scale: f32,
) -> Result<Vec<VectorVertex>, String> {
    if v.batches.is_some() {
        return Err("grouped vector raster execution is not available yet".into());
    }
    let trimmed = v
        .trim
        .as_ref()
        .map(|t| motion_model::vector::path_ops::trim(&v.paths, t))
        .transpose()
        .map_err(|e| e.to_string())?;
    let paths = trimmed.as_deref().unwrap_or(&v.paths);
    let dashed = if v.stroke.is_some_and(|s| s.1 > 0.) {
        v.dashes
            .as_ref()
            .map(|d| motion_model::vector::path_ops::dash(paths, d))
            .transpose()
            .map_err(|e| e.to_string())?
    } else {
        None
    };
    let make_path = |fill: bool| {
        let mut b = Path::builder();
        for p in if fill {
            paths
        } else {
            dashed.as_deref().unwrap_or(paths)
        } {
            if p.nodes.len() < 2 || (fill && !p.closed && v.trim.is_none()) {
                continue;
            }
            let first = p.nodes[0];
            b.begin(point(first[0], first[1]));
            let count = p.nodes.len();
            for i in 1..count + usize::from(p.closed) {
                let prev = p.nodes[(i - 1) % count];
                let n = p.nodes[i % count];
                if prev[4] == 0. && prev[5] == 0. && n[2] == 0. && n[3] == 0. {
                    b.line_to(point(n[0], n[1]));
                } else {
                    b.cubic_bezier_to(
                        point(prev[0] + prev[4], prev[1] + prev[5]),
                        point(n[0] + n[2], n[1] + n[3]),
                        point(n[0], n[1]),
                    );
                }
            }
            b.end(p.closed || fill);
        }
        b.build()
    };
    let position = |p: lyon_tessellation::math::Point| [p.x * 2. / size[0], -p.y * 2. / size[1]];
    let mut buffers: VertexBuffers<VectorVertex, u32> = VertexBuffers::new();
    let tolerance = (0.2 / scale).max(0.05);
    if let Some(color) = v.fill {
        let color = paint(color);
        let options = FillOptions::default()
            .with_tolerance(tolerance)
            .with_fill_rule(match v.fill_rule {
                FillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
                FillRule::NonZero => lyon_tessellation::FillRule::NonZero,
            });
        FillTessellator::new()
            .tessellate_path(
                &make_path(true),
                &options,
                &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| VectorVertex {
                    position: position(v.position()),
                    color,
                }),
            )
            .map_err(|e| format!("vector fill: {e:?}"))?;
    }
    if let Some((color, width, cap, join, miter)) = v.stroke.filter(|s| s.1 > 0.) {
        let color = paint(color);
        let cap = match cap {
            LineCap::Butt => lyon_tessellation::LineCap::Butt,
            LineCap::Round => lyon_tessellation::LineCap::Round,
            LineCap::Square => lyon_tessellation::LineCap::Square,
        };
        let join = match join {
            LineJoin::Miter => lyon_tessellation::LineJoin::Miter,
            LineJoin::Round => lyon_tessellation::LineJoin::Round,
            LineJoin::Bevel => lyon_tessellation::LineJoin::Bevel,
        };
        let options = StrokeOptions::default()
            .with_tolerance(tolerance)
            .with_line_width(width)
            .with_line_cap(cap)
            .with_line_join(join)
            .with_miter_limit(miter);
        StrokeTessellator::new()
            .tessellate_path(
                &make_path(false),
                &options,
                &mut BuffersBuilder::new(&mut buffers, |v: StrokeVertex| VectorVertex {
                    position: position(v.position()),
                    color,
                }),
            )
            .map_err(|e| format!("vector stroke: {e:?}"))?;
    }
    if buffers.indices.len() > 262144 {
        return Err("vector triangle vertex limit exceeded (262144)".into());
    }
    Ok(buffers
        .indices
        .iter()
        .map(|i| buffers.vertices[*i as usize])
        .collect())
}
