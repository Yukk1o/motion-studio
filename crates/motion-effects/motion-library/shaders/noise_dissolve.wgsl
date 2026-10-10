// Motion Studio kernels; adapted formulas are identified in SOURCES.json.
/*
MIT License

Copyright (c) 2026 Shader Effects Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

const PI: f32 = 3.141592653589793;
const TAU: f32 = 6.283185307179586;
fn turn(q:vec2<f32>,a:f32)->vec2<f32>{let c=cos(a);let s=sin(a);return vec2(c*q.x-s*q.y,s*q.x+c*q.y);}
fn luma(c:vec3<f32>)->f32{return dot(c,vec3(0.2126,0.7152,0.0722));}
fn result(c:vec4<f32>,rgb:vec3<f32>)->vec4<f32>{return vec4(select(vec3(0.0),clamp(rgb,vec3(0.0),vec3(1.0)),c.a>0.0),c.a);}
fn hash32(x:u32)->u32{var v=x;v=(v^(v>>16u))*0x7feb352du;v=(v^(v>>15u))*0x846ca68bu;return v^(v>>16u);}
fn rnd(q:vec2<i32>,seed:u32)->f32{return f32(hash32(bitcast<u32>(q.x)*0x9e3779b9u^bitcast<u32>(q.y)*0x85ebca6bu^seed)>>8u)/16777216.0;}
fn salt()->u32{return u32(fx.clock.w);}
fn hsv(h:f32,s:f32,v:f32)->vec3<f32>{let c=clamp(abs(fract(h+vec3(0.0,2.0/3.0,1.0/3.0))*6.0-3.0)-1.0,vec3(0.0),vec3(1.0));return v*mix(vec3(1.0),c,s);}
fn over(a:vec4<f32>,b:vec4<f32>)->vec4<f32>{let alpha=a.a+b.a*(1.0-a.a);return vec4((a.rgb*a.a+b.rgb*b.a*(1.0-a.a))/max(alpha,0.000001),alpha);}
fn stop_mix(a:vec4<f32>,b:vec4<f32>,t:f32)->vec4<f32>{let alpha=mix(a.a,b.a,t);return vec4(mix(a.rgb*a.a,b.rgb*b.a,t)/max(alpha,0.000001),alpha);}

fn grad(cell:vec2<i32>,phase:f32)->vec2<f32>{let a=rnd(cell,salt())*TAU+phase;return vec2(cos(a),sin(a));}
// Value and analytic spatial derivatives. Curl is (d/dy, -d/dx).
fn perlin_grad(q:vec2<f32>,phase:f32)->vec3<f32>{
 let i=vec2<i32>(floor(q));let f=fract(q);let u=f*f*f*(f*(f*6.0-15.0)+10.0);let du=30.0*f*f*(f-1.0)*(f-1.0);
 let a=grad(i,phase);let b=grad(i+vec2<i32>(1,0),phase);let c=grad(i+vec2<i32>(0,1),phase);let d=grad(i+vec2<i32>(1,1),phase);
 let va=dot(a,f);let vb=dot(b,f-vec2(1.0,0.0));let vc=dot(c,f-vec2(0.0,1.0));let vd=dot(d,f-vec2(1.0));
 let lo=mix(va,vb,u.x);let hi=mix(vc,vd,u.x);
 let dx=mix(mix(a.x,b.x,u.x)+(vb-va)*du.x,mix(c.x,d.x,u.x)+(vd-vc)*du.x,u.y);
 let dy=mix(mix(a.y,b.y,u.x),mix(c.y,d.y,u.x),u.y)+(hi-lo)*du.y;
 return vec3(mix(lo,hi,u.y),dx,dy);
}
fn perlin(q:vec2<f32>,phase:f32)->f32{return perlin_grad(q,phase).x;}
fn value_noise(q:vec2<f32>)->f32{let i=vec2<i32>(floor(q));let f=fract(q);let u=f*f*(3.0-2.0*f);return mix(mix(rnd(i,salt()),rnd(i+vec2<i32>(1,0),salt()),u.x),mix(rnd(i+vec2<i32>(0,1),salt()),rnd(i+vec2<i32>(1,1),salt()),u.x),u.y);}
fn simplex_corner(i:vec2<i32>,d:vec2<f32>,phase:f32)->f32{let t=max(0.5-dot(d,d),0.0);return t*t*t*t*dot(grad(i,phase),d);}
fn simplex(q:vec2<f32>,phase:f32)->f32{let i=floor(q+dot(q,vec2(0.366025403784)));let a=q-i+dot(i,vec2(0.211324865405));let step=select(vec2(0.0,1.0),vec2(1.0,0.0),a.x>a.y);let b=a-step+0.211324865405;let c=a-1.0+0.42264973081;return 70.0*(simplex_corner(vec2<i32>(i),a,phase)+simplex_corner(vec2<i32>(i+step),b,phase)+simplex_corner(vec2<i32>(i+1.0),c,phase));}
fn fbm(q:vec2<f32>,phase:f32,octaves:f32)->f32{var p=q;var total=0.0;var weight=0.5;var sum=0.0;for(var octave:i32=0;octave<8;octave=octave+1){if f32(octave)>=octaves{break;}total+=perlin(p,phase)*weight;sum+=weight;p=turn(p,0.53)*2.01+vec2(13.7,9.2);weight*=0.5;}return total/max(sum,0.000001);}
fn curl(q:vec2<f32>,phase:f32)->vec2<f32>{let n=perlin_grad(q,phase);return vec2(n.z,-n.y);}

fn main_fx(p:vec2<f32>)->vec4<f32>{

let c=sample_input(p);if fx.params[1].x<=0.0{return c;}if fx.params[1].x>=1.0{return vec4(0.0);}
let n=value_noise(p/fx.params[2].x);let mask=select(select(0.0,1.0,n>=fx.params[1].x),smoothstep(fx.params[1].x-fx.params[3].x,fx.params[1].x+fx.params[3].x,n),fx.params[3].x>0.0);
return vec4(c.rgb,c.a*mask);
}
