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

fn main_fx(p:vec2<f32>)->vec4<f32>{

let count=max(1u,u32(fx.params[5].x));let size=vec2(fx.params[1].x*fx.params[6].x,fx.params[1].x);let grid=floor((p-fx.source_region.xy)/size);let center=fx.source_region.xy+(grid+0.5)*size;
let c=sample_input(p);let shade=sample_input(center);let glyph=min(count-1u,u32(clamp(luma(shade.rgb),0.0,1.0)*f32(count)));let local=fract((p-fx.source_region.xy)/size);
let dimensions=vec2<f32>(textureDimensions(resource0));let cellPixels=dimensions/vec2(f32(count),1.0);let inset=vec2(0.5)/cellPixels;
let uv=(vec2(f32(glyph),0.0)+clamp(local,inset,1.0-inset))/vec2(f32(count),1.0);
let coverage=select(textureSampleLevel(resource0,resource_sampler0,uv,0.0).a,0.0,fx.params[30].x==2.0);let ink=select(shade.rgb,fx.params[3].rgb,fx.params[2].x==1.0);return result(c,mix(fx.params[4].rgb,ink,coverage));
}
