fn main_fx(p: vec2<f32>) -> vec4<f32> {
let radius=fx.params[0].x;let d=p-fx.params[1].xy;let distance=length(d);
if radius == 0.0 || distance >= radius || distance < .000001 { return sample_input(p); }
let mapped = asin(clamp(distance/radius,0.0,1.0))*radius*.6366197724;
return sample_input(fx.params[1].xy+d*mapped/distance);
}
