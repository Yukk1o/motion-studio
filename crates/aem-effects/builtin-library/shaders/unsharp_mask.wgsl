
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
fn main_fx(p:vec2<f32>)->vec4<f32>{let c=sample_input(p);
if fx.clock.z>=1.5{let original=sample_source(p);let delta=original.rgb-c.rgb;let sharpened=original.rgb+select(vec3(0.0),delta,abs(delta)>=vec3(fx.params[3].x))*fx.params[1].x/100.0;return vec4(clamp(sharpened,vec3(0.0),vec3(1.0)),original.a);}
 let radius=fx.params[2].x;let sigma=max(radius/3.0,0.15);let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),fx.clock.z>0.5);var sum=vec4(0.0);var weight=0.0;
 let n=min(128,i32(ceil(radius)));
 for(var tap:i32=0;tap<=256;tap=tap+1){if tap>2*n{break;}let i=tap-n;let d=f32(i)*max(1.0,radius/128.0);if abs(d)<=ceil(radius){let w=exp(-0.5*d*d/(sigma*sigma));sum+=sample_input(p+axis*d)*w;weight+=w;}}return sum/max(weight,0.0001);
}
