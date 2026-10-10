fn luma(c:vec3<f32>)->f32 { return dot(c,vec3(.299,.587,.114)); }
fn rotate(p:vec2<f32>,a:f32)->vec2<f32> { return vec2(cos(a)*p.x-sin(a)*p.y,sin(a)*p.x+cos(a)*p.y); }
fn finish(c:vec4<f32>,rgb:vec3<f32>)->vec4<f32> {
    return vec4(select(clamp(rgb,vec3(0.0),vec3(1.0)),vec3(0.0),c.a==0.0),c.a);
}
fn hash(cell:vec2<i32>,salt:u32)->f32 {
    var v=bitcast<u32>(cell.x)*0x9e3779b9u ^ bitcast<u32>(cell.y)*0x85ebca6bu ^ salt;
    v=(v^(v>>16u))*0x7feb352du;v=(v^(v>>15u))*0x846ca68bu;v=v^(v>>16u);
    return f32(v>>8u)/16777216.0;
}
fn noise(q:vec2<f32>,phase:f32,seed:u32,kind:f32)->f32 {
    let cell=vec2<i32>(floor(q));var f=fract(q);
    if kind==1.0 {f=vec2(0.0);} else if kind>=3.0 {f=f*f*(3.0-2.0*f);}
    let t=sin(phase)*.5+.5;
    let a=mix(hash(cell,seed),hash(cell,seed^0x1234567u),t);
    let b=mix(hash(cell+vec2<i32>(1,0),seed),hash(cell+vec2<i32>(1,0),seed^0x1234567u),t);
    let c=mix(hash(cell+vec2<i32>(0,1),seed),hash(cell+vec2<i32>(0,1),seed^0x1234567u),t);
    let d=mix(hash(cell+vec2<i32>(1,1),seed),hash(cell+vec2<i32>(1,1),seed^0x1234567u),t);
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}
fn cover(distance:f32,feather:f32)->f32 {
    if feather<=0.0 {return select(0.0,1.0,distance>=0.0);}
    return smoothstep(-feather*.5,feather*.5,distance);
}

fn main_fx(p: vec2<f32>) -> vec4<f32> {

let c=sample_input(p);let angle=radians(fx.params[0].x);let offset=vec2(cos(angle),sin(angle))*fx.params[1].x;
let delta=luma(sample_input(p+offset).rgb)-luma(sample_input(p-offset).rgb);
return finish(c,mix(c.rgb+delta*fx.params[2].x/100.0,c.rgb,fx.params[3].x/100.0));

}
