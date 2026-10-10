@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vertex {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - vec2(1.0);
    var result: Vertex;
    result.position = vec4(p, 0.0, 1.0);
    result.uv = vec2(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return result;
}
@fragment fn fs(input: Vertex) -> @location(0) vec4<f32> {
    return textureSample(image, image_sampler, input.uv);
}
