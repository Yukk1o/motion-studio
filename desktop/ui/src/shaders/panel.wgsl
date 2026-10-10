// Panel chrome: solid fills and atlas-sampled glyphs.
//
// The viewport transform arrives as a uniform rather than push constants so the
// pipeline needs no optional device feature.

struct SolidVertex {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct GlyphVertex {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> transform: vec4<f32>;

@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

fn to_clip(position: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(position.x * transform.x - 1.0, position.y * transform.w + 1.0, 0.0, 1.0);
}

struct SolidOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

struct GlyphOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn solid_vs(vertex: SolidVertex) -> SolidOutput {
    return SolidOutput(to_clip(vertex.position), vertex.color);
}

@fragment
fn solid_fs(@location(0) color: vec4<f32>) -> @location(0) vec4<f32> {
    return color;
}

@vertex
fn glyph_vs(vertex: GlyphVertex) -> GlyphOutput {
    return GlyphOutput(to_clip(vertex.position), vertex.uv, vertex.color);
}

@fragment
fn glyph_fs(vertex: GlyphOutput) -> @location(0) vec4<f32> {
    // The atlas stores white coverage; tint with the vertex colour.
    let coverage = textureSample(atlas, atlas_sampler, vertex.uv).a;
    return vec4<f32>(vertex.color.rgb, vertex.color.a * coverage);
}
