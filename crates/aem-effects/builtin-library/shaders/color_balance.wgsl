
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
let l=luminance(c.rgb);let shadows=clamp((0.5-l)*2.0,0.0,1.0);let highs=clamp((l-0.5)*2.0,0.0,1.0);let mid=1.0-shadows-highs;
 var rgb=clamp(c.rgb+(vec3(fx.params[1].x,fx.params[2].x,fx.params[3].x)*shadows+vec3(fx.params[4].x,fx.params[5].x,fx.params[6].x)*mid+vec3(fx.params[7].x,fx.params[8].x,fx.params[9].x)*highs)/100.0,vec3(0.0),vec3(1.0));
 if fx.params[10].x>0.5{let h=rgb_hsl(rgb);rgb=hsl_rgb(vec3(h.xy,rgb_hsl(c.rgb).z));}return vec4(rgb,c.a);
}
