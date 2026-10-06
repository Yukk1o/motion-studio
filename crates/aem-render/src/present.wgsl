@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> mode: vec4<u32>;
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@vertex fn vertex_main(@builtin(vertex_index) index: u32) -> Out {
    let p=vec2<f32>(f32((index<<1u)&2u),f32(index&2u))*2.0-vec2<f32>(1.0);
    var out:Out;
    out.position=vec4(p,0.0,1.0);
    out.uv=vec2(p.x*0.5+0.5,0.5-p.y*0.5);
    return out;
}
fn encode(v:vec3<f32>)->vec3<f32> {
    return select(1.055*pow(max(v,vec3(0.0)),vec3(1.0/2.4))-0.055,
        12.92*v,v<=vec3(0.0031308));
}
@fragment fn fragment_main(input:Out)->@location(0) vec4<f32> {
    let c=textureSample(source,source_sampler,input.uv);
    if mode.x==1u { return c; }
    return vec4(encode(c.rgb),c.a);
}
