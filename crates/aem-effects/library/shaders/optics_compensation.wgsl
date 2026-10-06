fn main_fx(p: vec2<f32>) -> vec4<f32> {
if fx.params[0].x == 0.0 { return sample_input(p); }
let extent = select(select(fx.size.x,fx.size.y,fx.params[2].x == 2.0),length(fx.size.xy),fx.params[2].x == 3.0)*.5;
let angle = radians(fx.params[0].x*.5);
let focal = extent / max(tan(angle),.000001);
let d = p-fx.params[3].xy;let r=length(d);
if r < .000001 { return sample_input(p); }
let n=r/focal;var factor=0.0;
if fx.params[1].x > .5 {
    factor=inverseSqrt(1.0+n*n);
} else {
    if n >= 1.0 {return vec4(0.0);}
    factor=inverseSqrt(1.0-n*n);
}
return sample_input(fx.params[3].xy+d*factor);
}
