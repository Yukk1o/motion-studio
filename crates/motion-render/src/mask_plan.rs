//! Shared GPU raster/feather/combine plan, serialized for GLES as well as wgpu.
use crate::{
    effect_plan::EffectUniform,
    vector_mesh::{self, VectorVertex},
};
use motion_model::vector::{FillRule, LineCap, LineJoin, SampledPath, SampledVector};
use crate::Scene;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, OnceLock},
};

pub const SOURCE_TOKEN: i32 = -536_870_912;
pub const RECORD_BYTES: usize = 64;
pub const GAUSSIAN: &str = r#"
fn main_fx(p:vec2<f32>)->vec4<f32> {
    let uv=p/fx.size.xy;let sigma=fx.params[0].z;let step=fx.params[0].w;
    var sum=0.0;var weight=0.0;
    for(var i=-16;i<17;i=i+1) {
        let distance=f32(i)*step;
        if abs(distance)<=3.0*sigma+0.0001 {
            let w=exp(-0.5*distance*distance/max(sigma*sigma,0.000001));
            let q=uv+fx.params[0].xy*distance;
            if all(q>=vec2(0.0)) && all(q<=vec2(1.0)) {sum+=textureSampleLevel(input_image,input_sampler,q*fx.size.xy/vec2<f32>(textureDimensions(input_image)),0.0).r*w;}
            weight+=w;
        }
    }
    let value=sum/max(weight,0.000001);return vec4(value,value,value,1.0);
}
"#;
pub const COMBINE: &str = r#"
fn main_fx(p:vec2<f32>)->vec4<f32> {
    let uv=p/fx.size.xy;
    var a=textureSampleLevel(input_image,input_sampler,uv*fx.size.xy/vec2<f32>(textureDimensions(input_image)),0.0).r;
    if fx.params[0].w>=0.0 {a=fx.params[0].w;}
    var b=textureSampleLevel(source_image,source_sampler,uv*fx.size.xy/vec2<f32>(textureDimensions(source_image)),0.0).r;
    if fx.params[0].y>0.5 {b=1.0-b;}b*=fx.params[0].z;
    let mode=fx.params[0].x;var value=a+b*(1.0-a);
    if mode==2.0 {value=a*(1.0-b);}
    if mode==3.0 {value=a*b;}
    if mode==4.0 {value=max(a,b);}
    if mode==5.0 {value=min(a,b);}
    if mode==6.0 {value=a*(1.0-b)+b*(1.0-a);}
    return vec4(value,value,value,1.0);
}
"#;
pub const APPLY: &str = r#"
fn main_fx(p:vec2<f32>)->vec4<f32> {
    let c=sample_input(p);let coverage=sample_source(p).r;
    return vec4(c.rgb*fx.params[0].rgb*fx.params[0].a,c.a*fx.params[0].a)*coverage;
}
"#;
pub fn shaders() -> Result<&'static [Arc<motion_effects::shader::CompiledShader>; 2], String> {
    static SHADERS: OnceLock<Result<[Arc<motion_effects::shader::CompiledShader>; 2], String>> =
        OnceLock::new();
    SHADERS
        .get_or_init(|| {
            Ok([
                Arc::new(
                    motion_effects::shader::compile(GAUSSIAN, "main_fx").map_err(|e| e.to_string())?,
                ),
                Arc::new(
                    motion_effects::shader::compile(COMBINE, "main_fx").map_err(|e| e.to_string())?,
                ),
            ])
        })
        .as_ref()
        .map_err(Clone::clone)
}
#[derive(Clone, Debug)]
pub struct MaskRaster {
    pub layer: usize,
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub fingerprint: u64,
    pub mode: u32,
    pub inverted: bool,
    pub opacity: f32,
    /// Feather in raster pixels; input data remains source pixels.
    pub feather: [f32; 2],
    pub vertices: Arc<Vec<VectorVertex>>,
}
type ShapeKey = (Vec<[f32; 6]>, [f32; 4], f32, f32);
#[derive(Default)]
pub struct MaskCache {
    shapes: HashMap<(u64, u64), (ShapeKey, Arc<Vec<VectorVertex>>, u64)>,
}
impl MaskCache {
    pub fn build(
        &mut self,
        scene: &Scene,
        scale: f32,
        preview: bool,
    ) -> Result<Vec<MaskRaster>, String> {
        self.shapes.retain(|(layer, mask), _| {
            scene
                .layers
                .iter()
                .any(|l| l.id == *layer && l.masks.iter().any(|m| m.id == *mask))
        });
        let mut rasters = vec![];
        for (layer, l) in scene.layers.iter().enumerate() {
            let density = if preview {
                crate::quality::preview_layer_scale(l, scene.width, scene.height, scale)
            } else {
                scale
            };
            for m in l.masks.iter() {
                let key = (m.nodes.clone(), l.source_rect, m.expansion, density);
                let cache = (l.id, m.id);
                if self.shapes.get(&cache).is_none_or(|(k, _, _)| k != &key) {
                    let mut nodes = m.nodes.clone();
                    // Center vertices on the actual raster region, preserving
                    // the source top-left origin even for expanded vector bounds.
                    for n in &mut nodes {
                        n[0] -= l.source_rect[0] + l.source_rect[2] * 0.5;
                        n[1] -= l.source_rect[1] + l.source_rect[3] * 0.5;
                    }
                    let vector = SampledVector {
                        batches: None,
                        root_opacity: 1.,
                        group_parameters: Default::default(),
                        trim: None,
                        dashes: None,
                        paths: vec![SampledPath {
                            closed: true,
                            nodes,
                        }],
                        fill: Some([1.; 4]),
                        fill_rule: FillRule::NonZero,
                        stroke: (m.expansion != 0.).then_some((
                            if m.expansion > 0. {
                                [1.; 4]
                            } else {
                                [0., 0., 0., 1.]
                            },
                            m.expansion.abs() * 2.,
                            LineCap::Round,
                            LineJoin::Round,
                            4.,
                        )),
                    };
                    let vertices = Arc::new(vector_mesh::tessellate(&vector, l.size, density)?);
                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                    bytemuck::cast_slice::<_, u8>(&vertices).hash(&mut hash);
                    self.shapes.insert(cache, (key, vertices, hash.finish()));
                }
                let (_, vertices, fingerprint) = &self.shapes[&cache];
                rasters.push(MaskRaster {
                    layer,
                    id: m.id,
                    width: (l.size[0] * density).ceil().max(1.) as u32,
                    height: (l.size[1] * density).ceil().max(1.) as u32,
                    fingerprint: *fingerprint,
                    mode: m.mode.code(),
                    inverted: m.inverted,
                    opacity: m.opacity,
                    feather: [m.feather[0] * density, m.feather[1] * density],
                    vertices: vertices.clone(),
                });
            }
        }
        if rasters.iter().map(|m| m.vertices.len()).sum::<usize>() > 262144 {
            return Err("frame mask triangle budget exceeds 262144 vertices".into());
        }
        Ok(rasters)
    }
}
pub fn group_fingerprint(masks: &[MaskRaster]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for m in masks {
        (m.id, m.width, m.height, m.fingerprint, m.mode, m.inverted).hash(&mut hash);
        m.opacity.to_bits().hash(&mut hash);
        for f in m.feather {
            f.to_bits().hash(&mut hash);
        }
    }
    hash.finish()
}
pub fn scratch_dimensions(masks: &[MaskRaster]) -> (u32, u32, usize) {
    let mut width = 0;
    let mut height = 0;
    let mut slots = 0;
    for group in masks.chunk_by(|a, b| a.layer == b.layer) {
        width = width.max(group[0].width);
        height = height.max(group[0].height);
        let blur = usize::from(group.iter().any(|m| m.feather.iter().any(|f| *f > 0.)));
        slots = slots.max(1 + blur + (group.len() - 1).min(2));
    }
    (width, height, slots)
}
pub fn uniform(width: u32, height: u32) -> EffectUniform {
    EffectUniform {
        size: [width as f32, height as f32, width as f32, height as f32],
        region: [0., 0., width as f32, height as f32],
        input_region: [0., 0., width as f32, height as f32],
        source_region: [0., 0., width as f32, height as f32],
        clock: [0.; 4],
        mode: [0.; 4],
        output_mode: [0., 0., 1., 1.],
        params: [[0.; 4]; 32],
    }
}
pub fn gaussian_uniform(width: u32, height: u32, axis: usize, feather: f32) -> EffectUniform {
    let mut u = uniform(width, height);
    let sigma = feather * 0.25;
    u.params[0][axis] = 1.
        / if axis == 0 {
            width as f32
        } else {
            height as f32
        };
    u.params[0][2] = sigma;
    u.params[0][3] = (sigma * 3. / 16.).max(1.);
    u
}
pub fn combine_uniform(m: &MaskRaster, first: bool) -> EffectUniform {
    let mut u = uniform(m.width, m.height);
    let initial = if matches!(m.mode, 2 | 3 | 5) { 1. } else { 0. };
    u.params[0] = [
        m.mode as f32,
        u32::from(m.inverted) as f32,
        m.opacity,
        if first { initial } else { -1. },
    ];
    u
}
pub fn write(masks: &[MaskRaster], out: &mut [u8], records: usize, vertices: usize) {
    let mut cursor = vertices;
    for (index, m) in masks.iter().enumerate() {
        let words = [
            m.layer as u32,
            m.id as u32,
            (m.id >> 32) as u32,
            m.width,
            m.height,
            cursor as u32,
            m.vertices.len() as u32,
            m.fingerprint as u32,
            (m.fingerprint >> 32) as u32,
            m.mode,
            u32::from(m.inverted),
            m.opacity.to_bits(),
            m.feather[0].to_bits(),
            m.feather[1].to_bits(),
            0,
            0,
        ];
        out[records + index * RECORD_BYTES..records + (index + 1) * RECORD_BYTES]
            .copy_from_slice(bytemuck::cast_slice(&words));
        let bytes = bytemuck::cast_slice(&m.vertices);
        out[cursor..cursor + bytes.len()].copy_from_slice(bytes);
        cursor += bytes.len();
    }
}
