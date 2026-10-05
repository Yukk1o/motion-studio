fn main_fx(p: vec2<f32>) -> vec4<f32> {
    let pixel = sample_input(p);
    return vec4(clamp(pixel.rgb * fx.params[0].x, vec3(0.0), vec3(1.0)), pixel.a);
}
