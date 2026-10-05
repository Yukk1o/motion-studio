
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
let a=radians(fx.params[4].x);let direction=vec2(cos(a),sin(a));let normal=vec2(-direction.y,direction.x);let phase=dot(p,normal)/max(fx.params[3].x,1.0)+fx.clock.x*fx.params[5].x+fx.params[7].x/360.0;
 let t=fract(phase);var wave=sin(phase*6.28318530718);if fx.params[1].x==2.0{wave=select(-1.0,1.0,t>=0.5);}else if fx.params[1].x==3.0{wave=1.0-4.0*abs(t-0.5);}else if fx.params[1].x==4.0{wave=2.0*t-1.0;}else if fx.params[1].x==5.0{wave=1.0-2.0*t;}else if fx.params[1].x>=6.0{wave=fract(sin(floor(phase)*12.9898+fx.clock.w)*43758.5453)*2.0-1.0;}return sample_input(p-direction*wave*fx.params[2].x);
}
