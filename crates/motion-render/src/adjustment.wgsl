struct Draw { inverse:mat4x4<f32>, region:vec4<f32>, mask_opacity:vec4<f32>, uv_size:vec4<f32> }
@group(0) @binding(0) var<uniform> draw:Draw;
@group(1) @binding(0) var base:texture_2d<f32>;
@group(1) @binding(1) var base_sampler:sampler;
@group(2) @binding(0) var filtered:texture_2d<f32>;
@group(2) @binding(1) var filtered_sampler:sampler;
struct Out { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn fullscreen(@builtin(vertex_index) i:u32)->Out {
    let p=vec2<f32>(f32((i<<1u)&2u),f32(i&2u))*2.-vec2<f32>(1.);
    var o:Out;o.position=vec4<f32>(p,0.,1.);o.uv=vec2<f32>(p.x*.5+.5,.5-p.y*.5);return o;
}
@fragment fn adjust(o:Out)->@location(0) vec4<f32> {
    let size=draw.uv_size.zw;
    let pixel=o.uv*size;
    let local=(draw.inverse*vec4<f32>(pixel.x-size.x*.5,size.y*.5-pixel.y,0.,1.)).xy;
    let distance=draw.mask_opacity.xy*.5-abs(local);
    let aa=max(fwidth(local),vec2<f32>(.0001));
    let coverage=clamp(distance/aa+.5,vec2<f32>(0.),vec2<f32>(1.));
    let uv=(pixel-draw.region.xy)/draw.region.zw;
    let b=textureSample(base,base_sampler,o.uv);
    var f=textureSample(filtered,filtered_sampler,uv*draw.uv_size.xy);
    if any(uv<vec2<f32>(0.)) || any(uv>vec2<f32>(1.)) {f=vec4<f32>(0.);}
    return mix(b,f,coverage.x*coverage.y*draw.mask_opacity.z);
}
@fragment fn present(o:Out)->@location(0) vec4<f32> {return textureSample(base,base_sampler,o.uv);}
