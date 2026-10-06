fn main_fx(p: vec2<f32>) -> vec4<f32> {
let c=sample_input(p);let source=sample_source(p);let amount=fx.params[1].x;
var inner=c;var outer=c;
let radius=abs(amount);let base=floor(radius);let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),fx.clock.z > .5);
for(var i=-100;i<=100;i=i+1) {
    if abs(f32(i)) <= ceil(radius) {
        let v=sample_input(p+axis*f32(i));
        if select(v.a < outer.a,v.a > outer.a,amount < 0.0) {outer=v;}
        if abs(f32(i)) <= base && select(v.a < inner.a,v.a > inner.a,amount < 0.0) {inner=v;}
    }
}
let matte=mix(inner,outer,fract(radius));
if fx.clock.z < .5 {return matte;}
if fx.params[0].x == 2.0 {return vec4(vec3(matte.a),1.0);}
return vec4(select(matte.rgb,source.rgb,source.a > 0.0),matte.a);
}
