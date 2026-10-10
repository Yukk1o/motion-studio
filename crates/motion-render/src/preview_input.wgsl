struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
struct Flags { straight: u32, p0: u32, p1: u32, p2: u32 }
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> flags: Flags;
@vertex fn vertex_main(@builtin(vertex_index) index: u32) -> Vertex {
    let p=vec2(f32((index<<1u)&2u)*2.0-1.0,f32(index&2u)*2.0-1.0);
    var out:Vertex;out.position=vec4(p,0.0,1.0);out.uv=vec2(p.x*0.5+0.5,0.5-p.y*0.5);return out;
}
@fragment fn fragment_main(input:Vertex)->@location(0) vec4<f32> {
    if flags.straight==0u { return textureSample(source,source_sampler,input.uv); }
    let size=vec2<i32>(textureDimensions(source));
    let point=input.uv*vec2<f32>(size)-0.5;
    let base=vec2<i32>(floor(point));let weight=fract(point);
    let a=load_premultiplied(base,size);
    let b=load_premultiplied(base+vec2(1,0),size);
    let c=load_premultiplied(base+vec2(0,1),size);
    let d=load_premultiplied(base+vec2(1,1),size);
    return mix(mix(a,b,weight.x),mix(c,d,weight.x),weight.y);
}
fn load_premultiplied(point:vec2<i32>,size:vec2<i32>)->vec4<f32> {
    let color=textureLoad(source,clamp(point,vec2(0),size-vec2(1)),0);
    return vec4(color.rgb*color.a,color.a);
}
