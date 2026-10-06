//! SDK 1: host-owned fullscreen vertex, resource bindings and 16-byte parameter slots.
use crate::{ensure, Error, Result};
use naga::{
    back::glsl,
    valid::{Capabilities, ValidationFlags, Validator},
    ShaderStage,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const UNIFORM_BYTES: usize = 7 * 16 + 32 * 16;
pub const HEADER: &str = r#"
struct EffectUniform {
    size: vec4<f32>, region: vec4<f32>, input_region: vec4<f32>, source_region: vec4<f32>,
    clock: vec4<f32>, mode: vec4<f32>, output_mode: vec4<f32>, params: array<vec4<f32>,32>,
};
@group(0) @binding(0) var<uniform> fx: EffectUniform;
@group(1) @binding(0) var input_image: texture_2d<f32>;
@group(1) @binding(1) var input_sampler: sampler;
@group(1) @binding(2) var source_image: texture_2d<f32>;
@group(1) @binding(3) var source_sampler: sampler;
@group(1) @binding(4) var curve_image: texture_2d<f32>;
@group(1) @binding(5) var curve_sampler: sampler;
@group(2) @binding(0) var resource0: texture_2d<f32>;
@group(2) @binding(1) var resource_sampler0: sampler;
@group(2) @binding(2) var resource1: texture_2d<f32>;
@group(2) @binding(3) var resource_sampler1: sampler;
@group(2) @binding(4) var resource2: texture_2d<f32>;
@group(2) @binding(5) var resource_sampler2: sampler;
@group(2) @binding(6) var resource3: texture_2d<f32>;
@group(2) @binding(7) var resource_sampler3: sampler;
struct EffectVertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn sdk_vertex(@builtin(vertex_index) index: u32) -> EffectVertex {
    let points = array<vec2<f32>,3>(vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    let p = points[index]; var out: EffectVertex;
    out.position = vec4(p,0.0,1.0); out.uv = vec2(p.x*0.5+0.5,0.5-p.y*0.5); return out;
}
fn edge_uv(uv: vec2<f32>) -> vec2<f32> {
    if fx.mode.x == 2.0 { return fract(uv); }
    if fx.mode.x == 3.0 { return 1.0-abs(fract(uv*0.5)*2.0-1.0); }
    return clamp(uv,vec2(0.0),vec2(1.0));
}
fn sample_input(p: vec2<f32>) -> vec4<f32> {
    let uv = (p-fx.input_region.xy)/fx.input_region.zw;
    var coord=edge_uv(uv);
    if fx.mode.w<0.5 {
        let dimensions=vec2<f32>(textureDimensions(input_image));let pixels=ceil(fx.input_region.zw*fx.output_mode.w);
        coord*=pixels/dimensions;
        if fx.mode.x!=0.0 {coord=clamp(coord,vec2(0.5)/dimensions,(pixels-0.5)/dimensions);}
    }
    let c = textureSampleLevel(input_image,input_sampler,coord,0.0);
    return select(c,vec4(0.0),fx.mode.x == 0.0 && (any(uv < vec2(0.0)) || any(uv > vec2(1.0))));
}
fn sample_source(p: vec2<f32>) -> vec4<f32> {
    let uv = (p-fx.source_region.xy)/fx.source_region.zw;
    var coord=edge_uv(uv);
    if fx.mode.w<0.5 {
        let dimensions=vec2<f32>(textureDimensions(source_image));let pixels=ceil(fx.source_region.zw*fx.output_mode.w);
        coord*=pixels/dimensions;
        if fx.mode.x!=0.0 {coord=clamp(coord,vec2(0.5)/dimensions,(pixels-0.5)/dimensions);}
    }
    let c = textureSampleLevel(source_image,source_sampler,coord,0.0);
    return select(c,vec4(0.0),fx.mode.x == 0.0 && (any(uv < vec2(0.0)) || any(uv > vec2(1.0))));
}
fn curve_lookup(v: vec4<f32>) -> vec4<f32> {
    let u=(clamp(v,vec4(0.0),vec4(1.0))*255.0+0.5)/256.0;
    return vec4(textureSampleLevel(curve_image,curve_sampler,vec2(u.r,0.5),0.0).r,
        textureSampleLevel(curve_image,curve_sampler,vec2(u.g,0.5),0.0).g,
        textureSampleLevel(curve_image,curve_sampler,vec2(u.b,0.5),0.0).b,
        textureSampleLevel(curve_image,curve_sampler,vec2(u.a,0.5),0.0).a);
}
fn srgb_encode(v: vec3<f32>) -> vec3<f32> {
    return select(1.055*pow(max(v,vec3(0.0)),vec3(1.0/2.4))-0.055,12.92*v,v<=vec3(0.0031308));
}
fn srgb_decode(v: vec3<f32>) -> vec3<f32> {
    return select(pow((max(v,vec3(0.0))+0.055)/1.055,vec3(2.4)),v/12.92,v<=vec3(0.04045));
}
fn straight(c: vec4<f32>) -> vec4<f32> { return vec4(select(vec3(0.0),c.rgb/max(c.a,0.000001),c.a>0.000001),c.a); }
fn convert_pixel(pixel: vec4<f32>, src_mode: vec2<f32>, dst_mode: vec2<f32>) -> vec4<f32> {
    var c=pixel; if src_mode.y == 0.0 { c=straight(c); }
    if src_mode.x != dst_mode.x { if dst_mode.x == 1.0 { c=vec4(srgb_encode(c.rgb),c.a); } else { c=vec4(srgb_decode(c.rgb),c.a); } }
    if dst_mode.y == 0.0 { c=vec4(c.rgb*c.a,c.a); } return c;
}
"#;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlslShader {
    pub vertex: String,
    pub fragment: String,
    /// GLSL uniform block name -> SDK (group,binding).
    pub blocks: BTreeMap<String, [u32; 2]>,
    /// Combined sampler name -> SDK texture (group,binding).
    pub textures: BTreeMap<String, [u32; 2]>,
}
#[derive(Clone, Debug)]
pub struct CompiledShader {
    pub wgsl: String,
    pub glsl: GlslShader,
    pub sprite: bool,
    pub additive: bool,
}

fn portable_source(source: &str) -> Result<()> {
    ensure(source.len() <= 256 * 1024, "shader source exceeds 256 KiB")?;
    let comments = regex::Regex::new(r"(?s)/\*.*?\*/|//[^\n]*").unwrap();
    let text = comments.replace_all(source, " ");
    let forbidden =
        regex::Regex::new(r"\b(?:while|loop)\b|@(?:group|binding|compute|vertex|fragment)\b")
            .unwrap();
    ensure(
        !forbidden.is_match(&text),
        "shader must use host entry points/bindings and statically bounded for loops",
    )?;
    let loops = regex::Regex::new(r"\bfor\s*\(([^)]*)\)").unwrap();
    let bounds=regex::Regex::new(r"^\s*var\s+([a-zA-Z_][a-zA-Z0-9_]*)(?:\s*:\s*(?:i32|u32))?\s*=\s*(-?\d+)[iu]?\s*;\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*(<|<=)\s*(-?\d+)[iu]?\s*;\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\+\s*1[iu]?\s*$").unwrap();
    let mut work = 1u64;
    let body = loops.replace_all(&text, "");
    for lp in loops.captures_iter(&text) {
        let b = bounds.captures(&lp[1]).ok_or_else(|| {
            Error::Invalid("for loop requires literal bounds and unit increment".into())
        })?;
        ensure(
            b[1] == b[3] && b[1] == b[6] && b[1] == b[7],
            "inconsistent loop variable",
        )?;
        let writes = regex::Regex::new(&format!(
            r"\b{}\s*(?:=[^=]|[+*/%-]=|\+\+|--)|&\s*\b{}\b",
            &b[1], &b[1]
        ))
        .unwrap();
        ensure(
            !writes.is_match(&body),
            "loop counters cannot be written or borrowed in loop bodies",
        )?;
        let start: i64 = b[2]
            .parse()
            .map_err(|_| Error::Invalid("invalid loop bound".into()))?;
        let end: i64 = b[5]
            .parse()
            .map_err(|_| Error::Invalid("invalid loop bound".into()))?;
        let count = (end - start + i64::from(&b[4] == "<=")).max(0) as u64;
        ensure(count <= 1024, "loop exceeds 1024 iterations")?;
        work = work.saturating_mul(count.max(1));
        ensure(work <= 65536, "shader static loop work exceeds limit")?;
    }
    Ok(())
}

pub fn compile(source: &str, entry: &str) -> Result<CompiledShader> {
    compile_mode(source, entry, false, false)
}
pub fn compile_mode(
    source: &str,
    entry: &str,
    sprite: bool,
    additive: bool,
) -> Result<CompiledShader> {
    compile_internal(source, entry, sprite, additive, false)
}
/// SDK 3 rectangles may write directly to the host's composition target.
pub(crate) fn compile_rect_image(source: &str, entry: &str) -> Result<CompiledShader> {
    compile_internal(source, entry, false, false, true)
}
fn compile_internal(
    source: &str,
    entry: &str,
    sprite: bool,
    additive: bool,
    convert_output: bool,
) -> Result<CompiledShader> {
    portable_source(source)?;
    ensure(
        regex::Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$")
            .unwrap()
            .is_match(entry),
        "invalid shader entry name",
    )?;
    let wgsl = if sprite {
        let start = HEADER.find("struct EffectVertex").unwrap();
        let end = HEADER.find("fn edge_uv").unwrap();
        let header = format!("{}{}{}", &HEADER[..start], SPRITE_VERTEX, &HEADER[end..]);
        format!("{header}\n{source}\n@fragment fn sdk_fragment(input:SpriteVertex)->@location(0) vec4<f32>{{return {entry}(input.uv,input.color,input.style);}}\n")
    } else if convert_output {
        format!("{HEADER}\n{source}\n@fragment fn sdk_fragment(input:EffectVertex)->@location(0) vec4<f32>{{let p=fx.region.xy+input.uv*fx.region.zw;let c=mix(sample_source(p),{entry}(p),fx.output_mode.z);if all(fx.mode.yz==fx.output_mode.xy) {{return c;}} return convert_pixel(c,fx.mode.yz,fx.output_mode.xy);}}\n")
    } else {
        format!("{HEADER}\n{source}\n@fragment fn sdk_fragment(input:EffectVertex)->@location(0) vec4<f32>{{let p=fx.region.xy+input.uv*fx.region.zw;return mix(sample_source(p),{entry}(p),fx.output_mode.z);}}\n")
    };
    let module =
        naga::front::wgsl::parse_str(&wgsl).map_err(|e| Error::Invalid(e.emit_to_string(&wgsl)))?;
    ensure(
        module.global_variables.len() == 15,
        "plugin shaders cannot declare additional global bindings",
    )?;
    ensure(
        module
            .functions
            .iter()
            .map(|(_, f)| f.expressions.len())
            .sum::<usize>()
            <= 65536,
        "shader expression budget exceeded",
    )?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .map_err(|e| Error::Invalid(format!("WGSL validation: {e}")))?;
    let options = glsl::Options {
        version: glsl::Version::new_gles(300),
        ..Default::default()
    };
    let mut result = GlslShader {
        vertex: String::new(),
        fragment: String::new(),
        blocks: BTreeMap::new(),
        textures: BTreeMap::new(),
    };
    for (stage, ep) in [
        (ShaderStage::Vertex, "sdk_vertex"),
        (ShaderStage::Fragment, "sdk_fragment"),
    ] {
        let mut output = String::new();
        let pipeline = glsl::PipelineOptions {
            shader_stage: stage,
            entry_point: ep.into(),
            multiview: None,
        };
        let mut writer = glsl::Writer::new(
            &mut output,
            &module,
            &info,
            &options,
            &pipeline,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .map_err(|e| Error::Invalid(format!("GLSL ES 300: {e}")))?;
        let reflection = writer
            .write()
            .map_err(|e| Error::Invalid(format!("GLSL ES 300: {e}")))?;
        for (handle, name) in reflection.uniforms {
            if let Some(b) = &module.global_variables[handle].binding {
                output = output.replace(
                    &format!("uniform {name} {{"),
                    &format!("layout(std140) uniform {name} {{"),
                );
                result.blocks.insert(name, [b.group, b.binding]);
            }
        }
        for (name, mapping) in reflection.texture_mapping {
            if let Some(b) = &module.global_variables[mapping.texture].binding {
                result.textures.insert(name, [b.group, b.binding]);
            }
        }
        if stage == ShaderStage::Vertex {
            result.vertex = output;
        } else {
            result.fragment = output;
        }
    }
    Ok(CompiledShader {
        wgsl,
        glsl: result,
        sprite,
        additive,
    })
}

/// Screen-space rectangles projected by the host; 48 bytes per instance.
pub const SPRITE_VERTEX: &str = r#"
struct SpriteVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>, @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) style: vec4<f32>,
};
@vertex fn sdk_vertex(@builtin(vertex_index) index: u32,
    @location(0) rect: vec4<f32>, @location(1) color: vec4<f32>, @location(2) style: vec4<f32>) -> SpriteVertex {
    let points = array<vec2<f32>,6>(vec2(-0.5,0.5),vec2(-0.5,-0.5),vec2(0.5,0.5),vec2(0.5,0.5),vec2(-0.5,-0.5),vec2(0.5,-0.5));
    let p=points[index]; var out:SpriteVertex;
    out.position=vec4(rect.xy+p*rect.zw,0.0,1.0);
    out.uv=vec2(p.x+0.5,0.5-p.y); out.color=color; out.style=style; return out;
}
"#;
