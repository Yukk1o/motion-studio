fn main_fx(p: vec2<f32>) -> vec4<f32> {
let c=sample_input(p);if fx.params[1].x == 0.0 { return c; }
var sum=vec4(0.0);var peak=vec4(0.0);var trough=vec4(1.0);
for(var i=0;i<64;i=i+1) {
    let q=fx.params[0].xy+(p-fx.params[0].xy)*(1.0-fx.params[1].x*.01*f32(i)/63.0);
    let v=sample_input(q);let weighted=vec4(v.rgb*v.a,v.a);
    sum+=weighted;peak=max(peak,weighted);trough=min(trough,weighted);
}
var result=sum/64.0;
if fx.params[2].x == 2.0 {result=peak;} if fx.params[2].x == 3.0 {result=trough;}
return vec4(select(vec3(0.0),result.rgb/max(result.a,.000001),result.a > .000001),result.a);
}
