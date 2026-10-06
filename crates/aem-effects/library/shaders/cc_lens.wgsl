fn main_fx(p: vec2<f32>) -> vec4<f32> {
let radius = length(fx.size.xy)*fx.params[1].x*.005;
let d=p-fx.params[0].xy;let r=length(d);
if radius == 0.0 || r >= radius { return vec4(0.0); }
let n=r/radius;
let k=fx.params[2].x*.01;
let factor=1.0-k*n*n;
let pixel=sample_input(fx.params[0].xy+d*factor);
let coverage=clamp((radius-r)*fx.output_mode.w+.5,0.0,1.0);
return vec4(pixel.rgb,pixel.a*coverage);
}
