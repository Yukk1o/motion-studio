
fn luminance(c:vec3<f32>)->f32{return dot(c,vec3(0.299,0.587,0.114));}
fn rgb_hsl(c:vec3<f32>)->vec3<f32>{
    let hi=max(c.r,max(c.g,c.b));let lo=min(c.r,min(c.g,c.b));let d=hi-lo;let l=(hi+lo)*0.5;
    if d<0.000001{return vec3(0.0,0.0,l);} var h=0.0;
    if hi==c.r{h=(c.g-c.b)/d;}else if hi==c.g{h=(c.b-c.r)/d+2.0;}else{h=(c.r-c.g)/d+4.0;}
    return vec3(fract(h/6.0),d/max(1.0-abs(2.0*l-1.0),0.000001),l);
}
fn hsl_rgb(hsl:vec3<f32>)->vec3<f32>{
    let h=fract(hsl.x);let s=clamp(hsl.y,0.0,1.0);let l=clamp(hsl.z,0.0,1.0);
    let c=(1.0-abs(2.0*l-1.0))*s;let base=clamp(abs(fract(h+vec3(0.0,2.0/3.0,1.0/3.0))*6.0-3.0)-1.0,vec3(0.0),vec3(1.0));
    return (base-0.5)*c+l;
}
fn rotate(p:vec2<f32>,a:f32)->vec2<f32>{return vec2(cos(a)*p.x-sin(a)*p.y,sin(a)*p.x+cos(a)*p.y);}
fn main_fx(p:vec2<f32>)->vec4<f32>{
let uv=(p-fx.input_region.xy)/fx.input_region.zw;
let center=fx.input_region.xy+fx.input_region.zw*0.5;
let radius=max(min(fx.input_region.z,fx.input_region.w)*0.5,0.0001);
let v=(p-center)/radius;
var q=fx.input_region.xy+vec2((atan2(v.y,v.x)/6.28318530718+0.5)*fx.input_region.z,length(v)*fx.input_region.w);
if fx.params[2].x>1.5{let angle=(uv.x-0.5)*6.28318530718;q=center+vec2(cos(angle),sin(angle))*uv.y*radius;}
return sample_input(mix(p,q,fx.params[1].x));
}
