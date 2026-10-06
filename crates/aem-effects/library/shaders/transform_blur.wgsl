// All coordinates are untransformed layer pixels; all loops have literal bounds.
fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }
fn turn(p: vec2<f32>, a: f32) -> vec2<f32> {
    return vec2(cos(a)*p.x-sin(a)*p.y, sin(a)*p.x+cos(a)*p.y);
}
fn hash32(value: u32) -> u32 {
    var v = value; v = (v ^ (v >> 16u))*0x7feb352du;
    v = (v ^ (v >> 15u))*0x846ca68bu; return v ^ (v >> 16u);
}
fn noise(cell: vec2<i32>, epoch: i32, salt: u32) -> f32 {
    let h = hash32(bitcast<u32>(cell.x)*0x9e3779b9u ^ bitcast<u32>(cell.y)*0x85ebca6bu
        ^ bitcast<u32>(epoch)*0xc2b2ae35u ^ bitcast<u32>(fx.clock.w) ^ salt);
    return f32(h >> 8u)/16777216.0;
}
fn smooth_noise(time: f32, salt: u32) -> f32 {
    let epoch = i32(floor(time)); let f = fract(time); let w = f*f*(3.0-2.0*f);
    return mix(noise(vec2<i32>(0), epoch, salt), noise(vec2<i32>(0), epoch+1, salt), w)*2.0-1.0;
}
fn bright(c: vec4<f32>, threshold: f32) -> vec4<f32> {
    let level = luma(c.rgb/max(c.a, 0.000001));
    let weight = max(level-threshold, 0.0)/max(level, 0.000001);
    return c*weight;
}
fn edge_light(p: vec2<f32>, threshold: f32) -> vec4<f32> {
    let left = sample_input(p-vec2(1.0, 0.0)); let right = sample_input(p+vec2(1.0, 0.0));
    let up = sample_input(p-vec2(0.0, 1.0)); let down = sample_input(p+vec2(0.0, 1.0));
    let edge = clamp(length(vec2(luma(right.rgb-left.rgb), luma(down.rgb-up.rgb))), 0.0, 1.0);
    let c = sample_input(p); let rgb = max(max(left.rgb, right.rgb), max(up.rgb, down.rgb));
    let alpha = max(max(left.a, right.a), max(up.a, down.a));
    return vec4(rgb, alpha)*max(edge-threshold, 0.0);
}
fn combine_light(source: vec4<f32>, light: vec4<f32>, strength: f32, color: vec3<f32>, affect_alpha: f32) -> vec4<f32> {
    let glow = max(light.rgb*color*strength, vec3(0.0));
    let coverage = clamp(max(glow.r, max(glow.g, glow.b)), 0.0, 1.0)*affect_alpha;
    let alpha = source.a+(1.0-source.a)*coverage;
    // Premultiplied linear output, including emitted light outside the source.
    return vec4(min(source.rgb+glow, vec3(alpha)), alpha);
}

fn main_fx(p: vec2<f32>) -> vec4<f32> {

var sum = vec4(0.0);
for(var i:i32=0;i<32;i=i+1) {
    let t = f32(i)/31.0-0.5;
    let shift = fx.params[1].xy+fx.params[4].xy*t;
    let angle = radians(fx.params[2].x+fx.params[5].x*t);
    let zoom = max(.05,fx.params[3].x*(1.0+fx.params[6].x*t));
    let q = fx.params[0].xy+turn(p-fx.params[0].xy-shift,-angle)/zoom;
    if all(q>=fx.input_region.xy)&&all(q<=fx.input_region.xy+fx.input_region.zw) {sum += sample_input(q);}
}
return sum/32.0;

}
