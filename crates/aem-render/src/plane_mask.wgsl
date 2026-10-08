struct Draw { mvp:mat4x4<f32>, color:vec4<f32>, extent_opacity:vec4<f32>, uv_scale:vec4<f32> };
@group(0) @binding(0) var<uniform> draw:Draw;
@group(1) @binding(0) var image:texture_2d<f32>;
@group(1) @binding(1) var image_sampler:sampler;
@group(2) @binding(0) var mask:texture_2d<f32>;
@group(2) @binding(1) var mask_sampler:sampler;
struct VertexOutput { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> };
@vertex fn vertex_main(@location(0) position:vec3<f32>,@location(1) uv:vec2<f32>)->VertexOutput {
    var out:VertexOutput;out.position=draw.mvp*vec4(position,1.0);out.uv=uv*draw.uv_scale.xy;return out;
}
@fragment fn fragment_main(input:VertexOutput)->@location(0) vec4<f32> {
    let c=textureSample(image,image_sampler,input.uv);let coverage=textureSample(mask,mask_sampler,input.uv).r;
    let opacity=draw.color.a*draw.extent_opacity.z;
    return vec4(c.rgb*draw.color.rgb*opacity,c.a*opacity)*coverage;
}
