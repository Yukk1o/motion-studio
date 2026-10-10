"""Generate Motion Studio's bounded, GLES-compatible official extension shaders.

Run from any directory. Assets are checked in; this script needs only Python's
standard library. Pack with effect_tool after regeneration. Published packages
must be retained when changing this library's version.
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LIB = ROOT / "crates/motion-effects/motion-library"
LIB.mkdir(parents=True, exist_ok=True)
(LIB / "shaders").mkdir(exist_ok=True)
LICENSE = (LIB / "LICENSE.shaders.txt").read_text(encoding="utf-8")
CREDIT = "// Motion Studio kernels; adapted formulas are identified in SOURCES.json.\n/*\n" + LICENSE + "*/\n"

BASE = """
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
"""

# Independently implemented integer-hash gradient noise; no MaterialX code.
NOISE = """
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
"""

effects = []


def param(id, name, default, lo, hi, kind="float", options=None, relative=None, units=""):
    value = list(default) if isinstance(default, (list, tuple)) else [default]
    p = dict(id=id, name=name, kind=kind, default=(value + [0]*4)[:4], min=lo,
             max=hi, step=1 if kind == "enum" else 0.01, animatable=True,
             implemented=True, units=units, options=options or [])
    if relative is not None:
        p["relative_default"] = relative
    return p


def f(id, name, value, lo=0, hi=1):
    return param(id, name, value, lo, hi)


def point(id, name, relative=(0.5,0.5)):
    return param(id,name,[0,0],-32768,32768,"vec2",relative=list(relative),units="pixels")


def color(id, name, value):
    return param(id,name,value,0,1,"color")


def enum(id, name, options, default=0):
    return param(id,name,default,0,len(options)-1,"enum",options)


def emit(id, name, english, category, params, body, helpers="", passes=1,
         resources=None, alpha="straight", working="srgb", edge="transparent",
         padding=None, bounds=None, differences=None, capabilities=None):
    params = [f("effect_opacity","效果不透明度",100,0,100)] + params
    assert len(params) <= 32
    lookup = {p["id"]: i for i,p in enumerate(params)}
    source = CREDIT + BASE + helpers + "\nfn main_fx(p:vec2<f32>)->vec4<f32>{\n" + body + "\n}\n"
    source = re.sub(r"\bpass\b", "stage", source)
    source = re.sub(r"\$([A-Za-z_][A-Za-z_0-9]*)",lambda m:f"fx.params[{lookup[m[1]]}]",source)
    path = "shaders/" + id + ".wgsl"
    (LIB/path).write_text(source,encoding="utf-8",newline="\n")
    if bounds is None:
        bounds = dict(x={"op":"input_origin","component":0}, y={"op":"input_origin","component":1},
                      width={"op":"input_size","component":0},height={"op":"input_size","component":1})
    effect = dict(id=id,name=name,english_name=english,category=category,params=params,
                  passes=[dict(shader=path,entry="main_fx") for _ in range(passes)],
                  resources=resources or [],working_space=working,alpha_mode=alpha,edge_mode=edge,
                  output_bounds=bounds,padding=padding or {"op":"constant","value":0},
                  reference_version="Motion Studio native; unverified",compatibility="approximate",compatibility_profile="motion-native-v1",
                  known_differences=differences or [],required_capabilities=capabilities or ["single_frame","spatial_bounds"])
    effects.append(effect)


emit("solarize","曝光反转","Solarize","调色",[f("threshold","阈值",0.5),f("strength","强度",1)],"""
let c=sample_input(p);let inverted=select(c.rgb,1.0-c.rgb,dot(c.rgb,vec3(0.299,0.587,0.114))>$threshold.x);
return result(c,mix(c.rgb,inverted,$strength.x));""")
emit("vignette","暗角","Vignette","调色",[point("center","中心"),param("radius","半径",[320,320],0.001,32768,"vec2",relative=[0.5,0.5]),f("feather","羽化",0.5,0.001,1),f("amount","明暗",0.65,-1,1)],"""
let c=sample_input(p);let d=length((p-$center.xy)/$radius.xy);let mask=smoothstep(1.0-$feather.x,1.0,d);
return result(c,c.rgb*(1.0-$amount.x*mask));""")
emit("vibrance","自然饱和度","Vibrance","调色",[f("amount","饱和度",0.4,-1,1),f("protection","饱和色保护",1)],"""
let c=sample_input(p);let mx=max(c.r,max(c.g,c.b));let mn=min(c.r,min(c.g,c.b));let avg=(c.r+c.g+c.b)/3.0;
let amt=(mx-avg)*$amount.x*(-3.0)*(1.0-$protection.x*(mx-mn));return result(c,mix(c.rgb,vec3(mx),amt));""",differences=["Adds explicit saturated-color protection to the adapted vibrance formula; no automatic skin classification."])
emit("flip","翻转","Flip","变形",[enum("axis","轴",["水平","垂直","双轴"])],"""
var q=p;let rect=fx.source_region;let axis=i32($axis.x);if axis==0||axis==2{q.x=rect.x+rect.z-(p.x-rect.x);}if axis==1||axis==2{q.y=rect.y+rect.w-(p.y-rect.y);}return sample_input(q);""")
emit("halftone","半色调网点","Halftone","风格化",[f("size","网格尺寸",8,1,512),f("angle","角度",15,-360,360),color("ink","墨色",[0.04,0.04,0.04,1]),color("paper","纸色",[1,1,0.95,1]),enum("shape","图案",["圆点","线条"])],"""
let c=sample_input(p);let a=$angle.x*PI/180.0;let q=turn(p,a)/$size.x;let center=turn((floor(q)+0.5)*$size.x,-a);
let brightness=luma(sample_input(center).rgb);let radius=sqrt(clamp(1.0-brightness,0.0,1.0)/PI);
let distance=select(length(fract(q)-0.5),abs(fract(q).y-0.5),$shape.x==1.0);let aa=max(0.5/$size.x,0.0001);
let mask=1.0-smoothstep(radius-aa,radius+aa,distance);return result(c,mix($paper.rgb,$ink.rgb,mask));""")
emit("dither","抖动量化","Dither","风格化",[f("levels","色阶",8,2,256),f("amount","抖动强度",1),enum("pattern","图案",["Bayer 4×4","蓝噪","白噪"])],"""
let c=sample_input(p);let pixel=vec2<i32>(floor(p));let x=u32(pixel.x)&3u;let y=u32(pixel.y)&3u;
let rank=(((x&1u)^(y&1u))*2u+(y&1u))*4u+(((x>>1u)^(y>>1u))*2u+(y>>1u));var n=(f32(rank)+0.5)/16.0;
if $pattern.x==1.0{n=textureSampleLevel(resource0,resource_sampler0,fract((floor(p)+0.5)/64.0),0.0).r;}else if $pattern.x==2.0{n=rnd(pixel,salt());}
let steps=$levels.x-1.0;return result(c,round(clamp(c.rgb+(n-0.5)*$amount.x/steps,vec3(0.0),vec3(1.0))*steps)/steps);""",resources=["assets/blue-noise.png"])

RAMP = """
fn ramp(t:f32)->vec4<f32>{let v=clamp(t,0.0,1.0)*3.0;let k=min(u32(floor(v)),2u);let a=fx.params[1u+k];let b=fx.params[2u+k];return stop_mix(a,b,v-f32(k));}
"""
ramp_colors=[color("color0","色标 1",[0.05,0.03,0.15,1]),color("color1","色标 2",[0.4,0.1,0.8,1]),color("color2","色标 3",[1,0.35,0.18,1]),color("color3","色标 4",[1,0.95,0.65,1])]
emit("gradient_map","渐变映射","Gradient Map","调色",ramp_colors+[f("black","黑点",0),f("white","白点",1),f("contrast","对比度",1,0,4),f("phase","色标偏移",0,-4,4)],"""
let c=sample_input(p);var t=clamp((luma(c.rgb)-$black.x)/max($white.x-$black.x,0.0001),0.0,1.0);t=clamp((t-0.5)*$contrast.x+0.5,0.0,1.0);
if $phase.x!=0.0{t=fract(t+$phase.x);}return result(c,ramp(t).rgb);""",helpers=RAMP)
emit("gradient","渐变","Gradient","生成",ramp_colors+[enum("shape","形状",["线性","径向","锥形","菱形","HSV 色轮","方向彩虹"]),point("start","起点",(0.25,0.5)),point("end","终点",(0.75,0.5)),f("angle","角度",0,-360,360),enum("repeat","延伸",["钳制","重复","镜像"]),f("phase","相位",0,-100,100)],"""
let axis=$end.xy-$start.xy;let radius=max(length(axis),0.0001);let q=turn(p-$start.xy,-$angle.x*PI/180.0);
var t=dot(p-$start.xy,axis)/max(dot(axis,axis),0.000001);let shape=i32($shape.x);
if shape==1{t=length(q)/radius;}else if shape==2{t=fract(atan2(q.y,q.x)/TAU+0.5);}else if shape==3{t=(abs(q.x)+abs(q.y))/radius;}
else if shape==4{return vec4(hsv(fract(atan2(q.y,q.x)/TAU+0.5+$phase.x),clamp(length(q)/radius,0.0,1.0),1.0),1.0);}
else if shape==5{return vec4(hsv(fract(q.x/radius+$phase.x),1.0,1.0),1.0);}
t+=$phase.x;if $repeat.x==1.0{t=fract(t);}else if $repeat.x==2.0{t=1.0-abs(fract(t*0.5)*2.0-1.0);}return ramp(t);""",helpers=RAMP,differences=["Four evenly spaced, animatable premultiplied color stops; HSV wheel is distinct from directional rainbow."])
positions=[(0.2,0.2),(0.8,0.2),(0.5,0.5),(0.2,0.8),(0.8,0.8)]
palette=[[0.1,0.1,0.8,1],[0.9,0.1,0.5,1],[0.2,0.9,0.7,1],[0.8,0.4,0.1,1],[0.5,0.1,0.8,1]]
point_params=[]
for i,(pos,col) in enumerate(zip(positions,palette)):
    point_params += [point(f"point{i}",f"位置 {i+1}",pos),color(f"color{i}",f"颜色 {i+1}",col)]
POINT_FIELD="""
fn point_field(p:vec2<f32>,moving:bool,speed:f32,drift:f32,power:f32)->vec4<f32>{var sum=vec4(0.0);var weight=0.0;var exact=vec4(0.0);var count=0.0;
for(var index:i32=0;index<5;index=index+1){var point=fx.params[1+index*2].xy;let col=fx.params[2+index*2];
 if moving{let a=fx.clock.x*speed+f32(index)*2.399963+f32(salt()%1024u)*0.01;point+=vec2(cos(a),sin(a*0.83))*drift;}
 let distance=length(p-point);if distance<0.0001{exact+=vec4(col.rgb*col.a,col.a);count+=1.0;}
 let w=1.0/max(pow(distance,power),0.000001);sum+=vec4(col.rgb*col.a,col.a)*w;weight+=w;
}let c=select(sum/max(weight,0.000001),exact/max(count,1.0),count>0.0);return vec4(c.rgb/max(c.a,0.000001),c.a);}
"""
emit("multi_point_gradient","多点渐变","Multi-point Gradient","生成",point_params+[f("power","距离指数",3,1,6)],"return point_field(p,false,0.0,0.0,$power.x);",helpers=POINT_FIELD)
emit("flow_gradient","流动渐变","Flow Gradient","生成",point_params+[f("power","距离指数",3,1,6),f("speed","速度",0.5,-10,10),f("drift","漂移距离",80,0,32768)],"return point_field(p,true,$speed.x,$drift.x,$power.x);",helpers=POINT_FIELD,differences=["Five deterministic animated points; not a free curved mesh or an exact port of upstream MeshGradient."])
grid_colors=[color(f"color{i}",f"顶点 {i//4+1}·{i%4+1}",[i%4/3,i//4/3,0.6,1]) for i in range(16)]
emit("grid_gradient","网格渐变","Grid Gradient","生成",grid_colors,"""
let uv=clamp((p-fx.source_region.xy)/fx.source_region.zw,vec2(0.0),vec2(1.0))*3.0;let cell=min(vec2<u32>(floor(uv)),vec2<u32>(2));let t=uv-vec2<f32>(cell);
let i=1u+cell.y*4u+cell.x;return stop_mix(stop_mix(fx.params[i],fx.params[i+1u],t.x),stop_mix(fx.params[i+4u],fx.params[i+5u],t.x),t.y);""",differences=["Fixed 4×4 regular grid with bilinear color interpolation; vertices and grid curves are not freely movable."])
emit("noise_dissolve","噪声溶解","Noise Dissolve","过渡",[f("progress","进度",0.4),f("scale","噪声尺度",24,1,4096),f("feather","羽化",0.08,0,0.5)],"""
let c=sample_input(p);if $progress.x<=0.0{return c;}if $progress.x>=1.0{return vec4(0.0);}
let n=value_noise(p/$scale.x);let mask=select(select(0.0,1.0,n>=$progress.x),smoothstep($progress.x-$feather.x,$progress.x+$feather.x,n),$feather.x>0.0);
return vec4(c.rgb,c.a*mask);""",helpers=NOISE)
emit("contour_lines","等高线","Contour Lines","风格化",[f("levels","层数",8,1,128),f("width","线宽",0.08,0.001,0.5),color("color","线色",[0.1,0.9,0.7,1]),f("amount","强度",1)],"""
let c=sample_input(p);let distance=abs(fract(luma(c.rgb)*$levels.x+0.5)-0.5);let coverage=1.0-smoothstep($width.x,$width.x+0.02,distance);return result(c,mix(c.rgb,$color.rgb,coverage*$amount.x));""")

# Shared scalar bases. Neighbourhoods are unrolled so the SDK's conservative
# all-loop product does not count mutually exclusive modes as nested loops.
NEIGHBORS=[(x,y) for y in range(-1,2) for x in range(-1,2)]
VORONOI="fn voronoi(q:vec2<f32>)->f32{let cell=vec2<i32>(floor(q));var nearest=100.0;\n"
GABOR="fn gabor(q:vec2<f32>,phase:f32,angle:f32)->f32{let cell=vec2<i32>(floor(q));var sum=0.0;var total=0.0;let dir=vec2(cos(angle),sin(angle));\n"
for x,y in NEIGHBORS:
    VORONOI+=f"{{let c=cell+vec2<i32>({x},{y});let pt=vec2<f32>(c)+vec2(rnd(c,salt()),rnd(c,salt()^12345u));nearest=min(nearest,length(q-pt));}}\n"
    GABOR+=f"{{let c=cell+vec2<i32>({x},{y});let pt=vec2<f32>(c)+vec2(rnd(c,salt()),rnd(c,salt()^12345u));let d=q-pt;let w=exp(-4.0*dot(d,d));sum+=cos(dot(d,dir)*TAU*2.0+phase)*w;total+=w;}}\n"
VORONOI+="return clamp(nearest,0.0,1.0);}\n"
GABOR+="return sum/max(total,0.000001);}\n"
emit("noise_generator","噪声生成器","Noise Generator","生成",[enum("basis","算法",["Perlin","Simplex","Voronoi","Gabor","块噪声","蓝噪"]),f("scale","尺度",80,1,4096),f("octaves","细节",4,1,8),f("speed","演化速度",0.3,-10,10),f("angle","Gabor 方向",0,-360,360),color("low","暗色",[0,0,0,1]),color("high","亮色",[1,1,1,1])],"""
let q=p/$scale.x;let phase=fx.clock.x*$speed.x;var n=0.0;let basis=i32($basis.x);
if basis==0{n=fbm(q,phase,$octaves.x)*0.5+0.5;}else if basis==1{n=simplex(q,phase)*0.5+0.5;}else if basis==2{n=voronoi(q);}else if basis==3{n=gabor(q,phase,$angle.x*PI/180.0)*0.5+0.5;}else if basis==4{n=rnd(vec2<i32>(floor(q)),salt());}else{n=textureSampleLevel(resource0,resource_sampler0,fract((floor(p)+0.5)/64.0),0.0).r;}
return stop_mix($low,$high,clamp(n,0.0,1.0));""",helpers=NOISE+VORONOI+GABOR,resources=["assets/blue-noise.png"],differences=["Independent seeded kernels, not MaterialX-compatible hashes. Blue mode uses a spectrally checked static tile; octaves apply to Perlin."])
emit("curl_noise","旋流噪声","Curl Noise","生成",[f("scale","尺度",80,1,4096),f("speed","演化速度",0.3,-10,10),f("gain","向量编码增益",0.2,0.01,1),enum("output","输出",["RG 向量","流速"])],"""
let v=curl(p/$scale.x,fx.clock.x*$speed.x);if $output.x==1.0{return vec4(vec3(clamp(length(v)*$gain.x,0.0,1.0)),1.0);}return vec4(clamp(vec2(0.5)+v*$gain.x,vec2(0.0),vec2(1.0)),0.5,1.0);""",helpers=NOISE,working="srgb",differences=["RG stores the analytic 2D curl in display-RGB data channels, neutral 0.5; clipping, RGBA8 quantization and texture interpolation can alter the encoded field."])

emit("marble","大理石","Marble","材质",[f("scale","尺度",80,1,4096),f("distortion","扭曲",4,0,20),f("speed","演化",0.1,-10,10),color("base","基色",[0.9,0.88,0.8,1]),color("vein","脉络",[0.12,0.14,0.16,1])],"""
let q=p/$scale.x;let n=fbm(q,fx.clock.x*$speed.x,5.0);let vein=pow(abs(sin(q.x*PI+n*$distortion.x)),0.18);return stop_mix($vein,$base,vein);""",helpers=NOISE)
emit("paper","纸张","Paper","材质",[f("grain","颗粒尺度",2,0.2,64),f("roughness","粗糙度",0.3),f("displacement","纤维位移",0.3,0,16)],"""
let q=p/$grain.x;let v=curl(q,0.0);let c=sample_input(p+v*$displacement.x);let n=value_noise(q);return result(c,c.rgb*(1.0+(n-0.5)*$roughness.x));""",helpers=NOISE)
emit("wool","毛织物","Wool","材质",[f("scale","纤维尺度",4,0.2,256),f("angle","方向",0,-360,360),color("color","颜色",[0.7,0.35,0.24,1]),f("roughness","粗糙度",0.6)],"""
let q=turn(p,-$angle.x*PI/180.0)/$scale.x;let n=value_noise(q*vec2(1.0,0.15));let fiber=pow(abs(sin(q.x*PI+n*4.0)),0.25);return vec4($color.rgb*(0.45+fiber*0.45+n*$roughness.x*0.1),$color.a);""",helpers=NOISE)
WEAVE="""
fn weave(q:vec2<f32>)->vec3<f32>{let cell=vec2<i32>(floor(q));let horizontal=(cell.x+cell.y)%2==0;let f=fract(q)-0.5;let side=select(f.x,f.y,horizontal);let along=select(f.y,f.x,horizontal);let cylinder=sqrt(max(1.0-side*side*4.0,0.0));let fiber=0.92+0.08*cos(along*TAU*12.0);return vec3(cylinder*fiber,select(0.0,1.0,horizontal),side);}
"""
emit("weave","编织","Weave","材质",[f("scale","网格尺寸",12,1,512),f("angle","角度",0,-360,360),color("warp","经线",[0.72,0.56,0.3,1]),color("weft","纬线",[0.2,0.35,0.48,1])],"""
let w=weave(turn(p,-$angle.x*PI/180.0)/$scale.x);let c=mix($weft,$warp,w.y);return vec4(c.rgb*(0.2+w.x*0.8),c.a);""",helpers=WEAVE)
emit("carbon_fiber","碳纤维","Carbon Fiber","材质",[f("scale","编织尺寸",10,1,512),f("angle","角度",45,-360,360),f("gloss","清漆高光",0.6),color("color","颜色",[0.12,0.14,0.16,1])],"""
let q=turn(p,-$angle.x*PI/180.0)/$scale.x;let w=weave(q);let highlight=pow(max(w.x,0.0),18.0)*$gloss.x;let rgb=$color.rgb*(0.3+w.x*0.7)+vec3(highlight*0.3);return vec4(clamp(rgb,vec3(0.0),vec3(1.0)),$color.a);""",helpers=WEAVE,differences=["2D woven surface and clearcoat approximation, not shape-aware physical lighting."])
emit("brushed_metal","拉丝金属","Brushed Metal","材质",[f("grain","拉丝尺度",1,0.2,64),f("angle","方向",0,-360,360),f("roughness","粗糙度",0.4),f("highlight","高光",0.65),color("color","金属色",[0.6,0.65,0.7,1])],"""
let q=turn(p,-$angle.x*PI/180.0)/$grain.x;let n=value_noise(q*vec2(0.012,1.0));let band=pow(max(1.0-abs((p.y/fx.size.y)-0.45)*2.0,0.0),3.0)*$highlight.x;
return vec4(clamp($color.rgb*(0.6+(n-0.5)*$roughness.x)+vec3(band*0.5),vec3(0.0),vec3(1.0)),$color.a);""",helpers=NOISE,differences=["2D directional surface; no scene light or shape-bound normal input."])
emit("swirl_pattern","旋涡纹理","Swirl Pattern","生成",[point("center","中心"),f("scale","尺度",160,1,4096),f("twist","旋转强度",5,-20,20),f("speed","速度",0.3,-10,10),color("low","暗色",[0.06,0.02,0.12,1]),color("high","亮色",[0.55,0.2,0.85,1])],"""
let q=(p-$center.xy)/$scale.x;let uv=turn(q,length(q)*$twist.x+fx.clock.x*$speed.x);let n=fbm(uv*3.0,fx.clock.x*$speed.x,5.0)*0.5+0.5;return stop_mix($low,$high,n);""",helpers=NOISE)
emit("aurora","极光","Aurora","生成",[f("scale","尺度",160,1,4096),f("speed","速度",0.25,-10,10),f("intensity","亮度",0.8,0,4),color("color","光色",[0.1,0.95,0.6,1]),f("height","高度",0.5,0,1)],"""
let uv=(p-fx.source_region.xy)/fx.source_region.zw;let q=p/$scale.x;let time=fx.clock.x*$speed.x;let ridge=$height.x+fbm(vec2(q.x,time),time,5.0)*0.22;
let curtains=pow(clamp(fbm(vec2(q.x*3.0,time*0.7),time,4.0)*0.5+0.5,0.0,1.0),2.0);let light=exp(-abs(uv.y-ridge)*14.0)*curtains*$intensity.x;
return vec4(clamp($color.rgb*light,vec3(0.0),vec3(1.0)),clamp(light,0.0,1.0)*$color.a);""",helpers=NOISE,differences=["Bounded procedural curtain field, independently implemented; no volumetric scene integration."])

emit("bend","弯曲","Bend","变形",[f("strength","曲率",0.4,-1,1),f("falloff","中心平坦区",0.2,0,1),f("angle","轴角度",0,-360,360)],"""
let center=fx.source_region.xy+fx.source_region.zw*0.5;let dir=vec2(cos($angle.x*PI/180.0),sin($angle.x*PI/180.0));let perp=vec2(-dir.y,dir.x);
let half=max(dot(abs(dir),fx.source_region.zw)*0.5,0.001);let delta=p-center;let screen=dot(delta,dir)/half;let m=abs(screen);let f=$falloff.x*0.9;let span=1.0-f;let b=$strength.x*1.2;
if m<=f||abs(b)<0.00001{return sample_input(p);}let a=b*m;let disc=4.0*span*span+8.0*a*(m-f);if disc<0.0{return vec4(0.0);}
// Rationalised quadratic root avoids cancellation as curvature approaches zero.
let u=2.0*(m-f)/(sqrt(disc)+2.0*span);let s=sign(screen)*(f+u*span);let z=b*u*u;
let q=center+dir*s*half+perp*dot(delta,perp)*(2.0-z)/2.0;return sample_input(q);""",differences=["Adapted inverse parabolic-sheet projection; no integration with the scene's 3D mesh or camera."])
emit("stretch","拉伸","Stretch","变形",[point("center","中心"),f("factor","拉伸倍率",1.8,0.1,10),f("width","影响宽度",120,1,32768),f("angle","方向",0,-360,360)],"""
let a=$angle.x*PI/180.0;var q=turn(p-$center.xy,-a);let weight=exp(-q.x*q.x/($width.x*$width.x));q.x/=1.0+($factor.x-1.0)*weight;return sample_input($center.xy+turn(q,a));""",differences=["Local directional stretch with Gaussian falloff, independent of layer transform scale."])


def op(name, a, b):
    return {"op":name,"a":a,"b":b}


def expr(id, component=0):
    return {"op":"parameter","id":id,"component":component}


def constant(v):
    return {"op":"constant","value":v}


def extrema(name, values):
    value=values[0]
    for other in values[1:]:
        value=op(name,value,other)
    return value


corners=[point("tl","左上",(0,0)),point("tr","右上",(1,0)),point("br","右下",(1,1)),point("bl","左下",(0,1))]
left=extrema("min",[expr(x["id"]) for x in corners]);top=extrema("min",[expr(x["id"],1) for x in corners])
right=extrema("max",[expr(x["id"]) for x in corners]);bottom=extrema("max",[expr(x["id"],1) for x in corners])
corner_bounds=dict(x=left,y=top,width=op("max",op("add",right,op("multiply",constant(-1),left)),constant(1)),height=op("max",op("add",bottom,op("multiply",constant(-1),top)),constant(1)))
CORNER="fn cross2(a:vec2<f32>,b:vec2<f32>)->f32{return a.x*b.y-a.y*b.x;}\n"
emit("corner_pin","四角定位","Corner Pin","变形",corners,"""
let a=$tl.xy;let b=$tr.xy;let c=$br.xy;let d=$bl.xy;let edges=array<vec2<f32>,4>(b-a,c-b,d-c,a-d);
let crosses=vec4(cross2(edges[0],edges[1]),cross2(edges[1],edges[2]),cross2(edges[2],edges[3]),cross2(edges[3],edges[0]));
if !(all(crosses>vec4(0.0001))||all(crosses<vec4(-0.0001))){return vec4(0.0);}
let delta=b-c;let other=d-c;let third=a-b+c-d;let det=cross2(delta,other);var g=0.0;var h=0.0;
if dot(third,third)>0.00000001{if abs(det)<0.000001{return vec4(0.0);}g=cross2(third,other)/det;h=cross2(delta,third)/det;}
let col1=b-a+g*b;let col2=d-a+h*d;let aa=col1.x-p.x*g;let bb=col2.x-p.x*h;let cc=col1.y-p.y*g;let dd=col2.y-p.y*h;
let denom=aa*dd-bb*cc;if abs(denom)<0.000001{return vec4(0.0);}let rhs=p-a;let uv=vec2((rhs.x*dd-bb*rhs.y)/denom,(aa*rhs.y-rhs.x*cc)/denom);
if any(uv<vec2(0.0))||any(uv>vec2(1.0)){return vec4(0.0);}return sample_input(fx.source_region.xy+uv*fx.source_region.zw);""",helpers=CORNER,bounds=corner_bounds,differences=["Convex perspective quads; degenerate and self-intersecting quads produce transparent output."])
emit("fluted_glass","条纹玻璃","Fluted Glass","变形",[f("pitch","条纹宽度",24,1,2048),f("refraction","折射",8,-256,256),f("angle","方向",0,-360,360),f("aberration","色散",0.2),f("highlight","高光",0.15),f("speed","速度",0,-10,10)],"""
let a=$angle.x*PI/180.0;let dir=vec2(cos(a),sin(a));let x=dot(p,dir)/$pitch.x+fx.clock.x*$speed.x;let slope=sin(x*TAU);let q=p+dir*slope*$refraction.x;let c=sample_input(q);var rgb=c.rgb;
if $aberration.x>0.0{let split=dir*slope*$refraction.x*$aberration.x;rgb=vec3(sample_input(q-split).r,c.g,sample_input(q+split).b);}rgb+=vec3(pow(max(cos(x*TAU),0.0),12.0)*$highlight.x);return result(c,rgb);""",edge="mirror")
emit("crt_screen","CRT 显像管","CRT Screen","风格化",[f("scanlines","扫描线",0.35),f("pitch","荫罩间距",3,1,128),f("aberration","色散",1.5,0,32),f("curvature","球面曲率",0.1,0,1),f("glow","辉光",0.15),f("vignette","暗角",0.3)],"""
let uv=(p-fx.source_region.xy)/fx.source_region.zw;let d=uv*2.0-1.0;let q=fx.source_region.xy+(d*(1.0+$curvature.x*dot(d,d))*0.5+0.5)*fx.source_region.zw;
let c=sample_input(q);let rgb=vec3(sample_input(q-vec2($aberration.x,0.0)).r,c.g,sample_input(q+vec2($aberration.x,0.0)).b);
let stripe=u32(floor(p.x/$pitch.x))%3u;let mask=select(vec3(0.7,0.7,1.0),select(vec3(0.7,1.0,0.7),vec3(1.0,0.7,0.7),stripe==0u),stripe<2u);
let scan=1.0-$scanlines.x*(0.5+0.5*cos(p.y*PI));let halo=(sample_input(q+vec2(2.0,0.0)).rgb+sample_input(q-vec2(2.0,0.0)).rgb+sample_input(q+vec2(0.0,2.0)).rgb+sample_input(q-vec2(0.0,2.0)).rgb)*0.25;
return result(c,(rgb*mask*scan+halo*$glow.x)*(1.0-$vignette.x*clamp(dot(d,d)*0.5,0.0,1.0)));""",differences=["Includes barrel curvature and a fixed four-neighbour glow approximation; alpha follows warped source."])
emit("vhs","VHS 磁带","VHS","风格化",[f("wobble","抖动",2,0,64),f("smear","色度渗漏",6,-64,64),f("noise","行噪声",0.12),f("speed","速度",1,0,10)],"""
let time=fx.clock.x*$speed.x;let epoch=i32(floor(time*24.0));let row=i32(floor(p.y));let jitter=(rnd(vec2<i32>(row,epoch),salt())-0.5)*$wobble.x;
let q=p+vec2(jitter+sin(p.y*0.03+time*3.0)*$wobble.x,0.0);let c=sample_input(q);var chroma=vec3(0.0);
for(var tap:i32=0;tap<7;tap=tap+1){let s=sample_input(q-vec2(f32(tap)*$smear.x/6.0,0.0));chroma+=s.rgb-vec3(luma(s.rgb));}
let n=(rnd(vec2<i32>(row,epoch),salt()^91u)-0.5)*$noise.x;return result(c,vec3(luma(c.rgb)+n)+chroma/7.0);""",edge="clamp")
atlas_count=f("glyph_count","图集字符数",10,1,256);atlas_count["animatable"]=False
atlas_aspect=f("glyph_aspect","图集字符宽高比",2/3,0.1,10);atlas_aspect["animatable"]=False
emit("ascii","字符画","ASCII","风格化",[f("cell","字符高度",16,4,128),enum("color_mode","颜色",["原图颜色","自定义颜色"]),color("foreground","前景",[0.3,1,0.7,1]),color("background","背景",[0.01,0.02,0.03,1]),atlas_count,atlas_aspect],"""
let count=max(1u,u32($glyph_count.x));let size=vec2($cell.x*$glyph_aspect.x,$cell.x);let grid=floor((p-fx.source_region.xy)/size);let center=fx.source_region.xy+(grid+0.5)*size;
let c=sample_input(p);let shade=sample_input(center);let glyph=min(count-1u,u32(clamp(luma(shade.rgb),0.0,1.0)*f32(count)));let local=fract((p-fx.source_region.xy)/size);
let dimensions=vec2<f32>(textureDimensions(resource0));let cellPixels=dimensions/vec2(f32(count),1.0);let inset=vec2(0.5)/cellPixels;
let uv=(vec2(f32(glyph),0.0)+clamp(local,inset,1.0-inset))/vec2(f32(count),1.0);
let coverage=select(textureSampleLevel(resource0,resource_sampler0,uv,0.0).a,0.0,fx.params[30].x==2.0);let ink=select(shade.rgb,$foreground.rgb,$color_mode.x==1.0);return result(c,mix($background.rgb,ink,coverage));""",resources=["assets/ascii.png"],capabilities=["single_frame","spatial_bounds","image_input"],differences=["Shared font rasterizer can supply a coverage-sorted custom atlas; original ten-glyph bitmap atlas is the default."])

offset=expr("offset");offset_y=expr("offset",1)
pad=op("add",{"op":"ceil","value":expr("radius")},op("max",{"op":"abs","value":offset},{"op":"abs","value":offset_y}))
shadow_bounds={}
for component,key in [(0,"x"),(1,"y")]:
    shadow_bounds[key]=op("add",{"op":"input_origin","component":component},op("multiply",constant(-1),pad))
for component,key in [(0,"width"),(1,"height")]:
    shadow_bounds[key]=op("add",{"op":"input_size","component":component},op("multiply",constant(2),pad))
emit("drop_shadow","投影","Drop Shadow","风格化",[param("offset","偏移",[12,12],-32768,32768,"vec2"),f("radius","模糊半径",12,0,512),color("color","阴影色",[0,0,0,0.65]),enum("mode","输出",["图像和阴影","仅阴影"])],"""
let pass=i32(fx.clock.z);if pass==2{let alpha=sample_input(p).a*$color.a;let shadow=vec4($color.rgb,alpha);if $mode.x==1.0{return shadow;}return over(sample_source(p),shadow);}
let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),pass==1);let radius=$radius.x;let base=select(p-$offset.xy,p,pass==1);if radius<=0.0{return vec4(0.0,0.0,0.0,sample_input(base).a);}
let step=max(1.0,radius/16.0);let sigma=max(radius/3.0,0.15);var sum=0.0;var total=0.0;
for(var tap:i32=-16;tap<=16;tap=tap+1){let d=f32(tap)*step;if abs(d)<=ceil(radius){let w=exp(-0.5*d*d/(sigma*sigma));sum+=sample_input(base+axis*d).a*w;total+=w;}}
return vec4(0.0,0.0,0.0,sum/max(total,0.000001));""",passes=3,bounds=shadow_bounds,differences=["Finite Gaussian support with up to 33 taps per axis; bounds include offset and full blur support."])
emit("channel_blur","通道模糊","Channel Blur","模糊",[f("red","红色半径",4,0,512),f("green","绿色半径",12,0,512),f("blue","蓝色半径",24,0,512)],"""
let c=sample_input(p);let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),fx.clock.z>0.5);let radii=vec3($red.x,$green.x,$blue.x);var rgb=c.rgb;
for(var channel:i32=0;channel<3;channel=channel+1){let radius=radii[channel];if radius<=0.0{continue;}let step=max(1.0,radius/16.0);let sigma=max(radius/3.0,0.15);var sum=0.0;var total=0.0;
 for(var tap:i32=-16;tap<=16;tap=tap+1){let d=f32(tap)*step;if abs(d)<=ceil(radius){let w=exp(-0.5*d*d/(sigma*sigma));let s=sample_input(p+axis*d);sum+=s[channel]*s.a*w;total+=s.a*w;}}
 rgb[channel]=sum/max(total,0.000001);
}return result(c,rgb);""",passes=2,edge="clamp",differences=["Separate Gaussian radius and sampling for each RGB channel; alpha preserved, transparent samples coverage-normalised."])
emit("region_blur","区域模糊","Region Blur","模糊",[enum("mode","模式",["渐进模糊","移轴"]),point("center","清晰中心"),f("angle","方向",90,-360,360),f("width","清晰带半宽",60,0,32768),f("transition","过渡距离",120,1,32768),f("radius","最大半径",24,0,512)],"""
let dir=vec2(cos($angle.x*PI/180.0),sin($angle.x*PI/180.0));let distance=dot(p-$center.xy,dir);let t=select(distance,abs(distance)-$width.x,$mode.x==1.0);
let radius=$radius.x*smoothstep(0.0,$transition.x,t);if radius<=0.0001{return select(sample_input(p),sample_source(p),fx.clock.z>0.5);}
let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),fx.clock.z>0.5);let step=max(1.0,radius/16.0);let sigma=max(radius/3.0,0.15);var sum=vec4(0.0);var total=0.0;
for(var tap:i32=-16;tap<=16;tap=tap+1){let d=f32(tap)*step;if abs(d)<=ceil(radius){let w=exp(-0.5*d*d/(sigma*sigma));let c=sample_input(p+axis*d);sum+=vec4(c.rgb*c.a,c.a)*w;total+=w;}}
let c=sum/max(total,0.000001);return vec4(c.rgb/max(c.a,0.000001),c.a);""",passes=2,edge="clamp",differences=["Separable spatially varying Gaussian approximation; final pass restores exact source pixels inside the focus band."])
emit("bokeh_blur","散景模糊","Bokeh Blur","模糊",[f("radius","半径",16,0,512),enum("aperture","光圈",["圆形","六边形","八边形"]),enum("quality","采样",["16","32","64"],1),f("rotation","旋转",0,-360,360),f("highlights","高光增强",0,0,4)],"""
if $radius.x<=0.0{return sample_input(p);}let count=16i<<u32($quality.x);var sum=vec4(0.0);var total=0.0;
for(var tap:i32=0;tap<64;tap=tap+1){if tap>=count{break;}let angle=f32(tap)*2.39996323+$rotation.x*PI/180.0;var r=sqrt((f32(tap)+0.5)/f32(count));
 if $aperture.x>0.0{let blades=select(6.0,8.0,$aperture.x==2.0);r*=cos(PI/blades)/cos((fract(angle/(TAU/blades)+0.5)-0.5)*TAU/blades);}
 let c=sample_input(p+vec2(cos(angle),sin(angle))*r*$radius.x);let w=1.0+max(luma(c.rgb)-0.7,0.0)*$highlights.x;sum+=vec4(c.rgb*c.a,c.a)*w;total+=w;
}let c=sum/max(total,0.000001);return vec4(c.rgb/max(c.a,0.000001),c.a);""",edge="clamp",differences=["16/32/64 deterministic aperture taps; polygon and circular apertures, no temporal accumulation or cat-eye model."])
emit("displacement_map","置换贴图","Displacement Map","变形",[param("amount","XY 位移",[12,12],-4096,4096,"vec2"),enum("channels","通道",["RG","亮度"]),f("midpoint","中性值",0.5)],"""
let uv=(p-fx.source_region.xy)/fx.source_region.zw;var map=textureSampleLevel(resource0,resource_sampler0,clamp(uv,vec2(0.0),vec2(1.0)),0.0);
// Slots 30/31 are host-owned for SDK 6 image_input effects. Stage is selected by the host,
// before transforms/opacity; source-stage pixels include intrinsic tint.
if fx.params[30].x==1.0{if fx.params[30].w>0.5{map=straight(map);}map=vec4(srgb_encode(map.rgb*fx.params[31].rgb),map.a*fx.params[31].a);}else if fx.params[30].x==2.0{map=vec4(vec3($midpoint.x),0.0);}else if fx.params[30].x==3.0{map=vec4(srgb_encode(map.rgb),map.a);}
var value=map.rg;if $channels.x==1.0{value=vec2(luma(map.rgb));}let delta=(value-vec2($midpoint.x))*map.a*$amount.xy;return sample_input(p+delta);""",resources=["assets/displacement.png"],capabilities=["single_frame","spatial_bounds","image_input"],differences=["Layer input defaults to same-frame masked/effected pixels; optional source stage excludes masks/effects. Transforms/opacity are excluded; input is stretched to the destination source rectangle; inactive source is neutral."])

# Same-frame 8×8 DCT: four separable passes, eight taps per pass. Intermediate
# signed coefficients are packed in RGBA8; this is a visual codec simulation,
# not an encoder. No previous-frame buffer or WebGPU-only compute entry point.
DCT="""
fn basis(n:f32,k:f32)->f32{return cos(PI*(n+0.5)*k/8.0);}
fn pack(c:vec3<f32>)->vec4<f32>{return vec4(clamp((c*127.0+128.0)/255.0,vec3(0.0),vec3(1.0)),1.0);}
fn unpack(c:vec4<f32>)->vec3<f32>{return (c.rgb*255.0-128.0)/127.0;}
"""
emit("compression_artifacts","压缩失真","Compression Artifacts","风格化",[f("quality","画质",0.45),f("strength","强度",1)],"""
if $strength.x<=0.0||$quality.x>=1.0{return sample_source(p);}let local=floor(p-fx.source_region.xy);let block=floor(local/8.0)*8.0+fx.source_region.xy;let pos=local-floor(local/8.0)*8.0;let pass=i32(fx.clock.z);var sum=vec3(0.0);
for(var tap:i32=0;tap<8;tap=tap+1){let k=f32(tap);var q=block+vec2(k,pos.y)+0.5;
 if pass==1||pass==3{q=block+vec2(pos.x,k)+0.5;}
 if pass==0{sum+=sample_input(q).rgb*basis(k,pos.x)/8.0;}else if pass==1{sum+=unpack(sample_input(q))*basis(k,pos.y)/8.0;}
 else{let n=select(pos.x,pos.y,pass==3);let weight=select(2.0,1.0,tap==0);sum+=unpack(sample_input(q))*basis(n,k)*weight;}
}
if pass==1{let quant=(1.0-$quality.x)*0.08*(1.0+(pos.x+pos.y)*0.25);sum=round(sum/quant)*quant;}
if pass<3{return pack(sum);}let original=sample_source(p);return result(original,mix(original.rgb,sum,$strength.x));""",helpers=DCT,passes=4,edge="clamp",differences=["Same-frame separable 8×8 DCT/quantization, RGBA8 coefficient precision; not full JPEG encoding or exact codec parity."])

manifest=dict(format_version=1,sdk_version=6,id="com.motionstudio.effects.motion",version="1.0.0",
              name="Motion Studio 创作效果",author="motion-studio contributors",license="MIT",effects=effects)
(LIB/"manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8",newline="\n")
print(f"Generated {len(effects)} effects in {LIB}")
