
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
fn level(v:f32,black:f32,white:f32,gamma:f32,low:f32,high:f32)->f32 {let t=clamp((v-black)/select(0.00001,white-black,abs(white-black)>0.00001),0.0,1.0);return mix(low,high,pow(t,1.0/max(gamma,0.00001)));}
fn main_fx(p:vec2<f32>)->vec4<f32>{let c=sample_input(p);
var rgb=vec3(level(c.r,fx.params[2].x,fx.params[3].x,fx.params[4].x,fx.params[5].x,fx.params[6].x),level(c.g,fx.params[2].x,fx.params[3].x,fx.params[4].x,fx.params[5].x,fx.params[6].x),level(c.b,fx.params[2].x,fx.params[3].x,fx.params[4].x,fx.params[5].x,fx.params[6].x));
 rgb=vec3(level(rgb.r,fx.params[9].x,fx.params[10].x,fx.params[11].x,fx.params[12].x,fx.params[13].x),level(rgb.g,fx.params[14].x,fx.params[15].x,fx.params[16].x,fx.params[17].x,fx.params[18].x),level(rgb.b,fx.params[19].x,fx.params[20].x,fx.params[21].x,fx.params[22].x,fx.params[23].x));return vec4(rgb,level(c.a,fx.params[24].x,fx.params[25].x,fx.params[26].x,fx.params[27].x,fx.params[28].x));
}
