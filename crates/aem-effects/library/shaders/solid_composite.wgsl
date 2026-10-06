fn main_fx(p: vec2<f32>) -> vec4<f32> {
let c=sample_input(p);let a=c.a*fx.params[0].x*.01;let b=fx.params[2].x*.01*fx.params[1].a;
let alpha=a+b*(1.0-a);
let rgb=c.rgb*a+fx.params[1].rgb*b*(1.0-a);
return vec4(select(vec3(0.0),rgb/max(alpha,.000001),alpha > .000001),alpha);
}
