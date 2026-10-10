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

fn basis(n:f32,k:f32)->f32{return cos(PI*(n+0.5)*k/8.0);}
fn pack(c:vec3<f32>)->vec4<f32>{return vec4(clamp((c*127.0+128.0)/255.0,vec3(0.0),vec3(1.0)),1.0);}
fn unpack(c:vec4<f32>)->vec3<f32>{return (c.rgb*255.0-128.0)/127.0;}

fn main_fx(p:vec2<f32>)->vec4<f32>{

if fx.params[2].x<=0.0||fx.params[1].x>=1.0{return sample_source(p);}let local=floor(p-fx.source_region.xy);let block=floor(local/8.0)*8.0+fx.source_region.xy;let pos=local-floor(local/8.0)*8.0;let stage=i32(fx.clock.z);var sum=vec3(0.0);
for(var tap:i32=0;tap<8;tap=tap+1){let k=f32(tap);var q=block+vec2(k,pos.y)+0.5;
 if stage==1||stage==3{q=block+vec2(pos.x,k)+0.5;}
 if stage==0{sum+=sample_input(q).rgb*basis(k,pos.x)/8.0;}else if stage==1{sum+=unpack(sample_input(q))*basis(k,pos.y)/8.0;}
 else{let n=select(pos.x,pos.y,stage==3);let weight=select(2.0,1.0,tap==0);sum+=unpack(sample_input(q))*basis(n,k)*weight;}
}
if stage==1{let quant=(1.0-fx.params[1].x)*0.08*(1.0+(pos.x+pos.y)*0.25);sum=round(sum/quant)*quant;}
if stage<3{return pack(sum);}let original=sample_source(p);return result(original,mix(original.rgb,sum,fx.params[2].x));
}
