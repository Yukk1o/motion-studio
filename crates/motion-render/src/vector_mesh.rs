//! Both GPU backends consume the same tessellated triangles and linear paints.
use motion_model::vector::{FillRule, LineCap, LineJoin, SampledVector};
use bytemuck::{Pod, Zeroable};
use lyon_tessellation::{
    math::point, path::Path, BuffersBuilder, FillOptions, FillTessellator, FillVertex,
    StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};
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
    let make_path = |fill: bool| {
        let mut b = Path::builder();
        for p in &v.paths {
            if p.nodes.len() < 2 || (fill && !p.closed) {
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
            b.end(p.closed);
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
