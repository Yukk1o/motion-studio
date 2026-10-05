"""Generate the checked-in library from an actual AE parameter capture.
All algorithms start as approximate; comparison reports never promote acceptance automatically.
"""
import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LIB = ROOT / "crates/aem-effects/library"
ENGLISH = dict(zip(
    ["brightness_contrast", "exposure", "hue_saturation", "tint", "tritone", "color_balance", "levels", "curves", "gaussian_blur", "fast_box_blur", "directional_blur", "radial_blur", "sharpen", "unsharp_mask", "mirror", "offset", "bulge", "twirl", "wave_warp", "polar_coordinates"],
    ["Brightness & Contrast", "Exposure", "Hue/Saturation", "Tint", "Tritone", "Color Balance", "Levels", "Curves", "Gaussian Blur", "Fast Box Blur", "Directional Blur", "Radial Blur", "Sharpen", "Unsharp Mask", "Mirror", "Offset", "Bulge", "Twirl", "Wave Warp", "Polar Coordinates"]))
BOOL = {"brightness_contrast": [3], "exposure": [22], "hue_saturation": [7], "color_balance": [10], "gaussian_blur": [3], "fast_box_blur": [4], "bulge": [7]}
ENUM = {"exposure": [1], "hue_saturation": [2], "levels": [1,8,9], "curves": [2], "gaussian_blur": [2], "fast_box_blur": [3], "radial_blur": [3,4], "bulge": [6], "wave_warp": [1,6,8], "polar_coordinates": [2]}
UNSUPPORTED = {"hue_saturation": [2], "bulge": [6], "wave_warp": [6,8]}
COMMON = r'''
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
'''
BODY = {
"brightness_contrast": r'''let b=A0001/150.0;let k=A0002/100.0;var rgb=c.rgb;
 if A0003>0.5{rgb=(rgb-0.5)*(1.0+k)+0.5+A0001/255.0;}else{rgb=mix(rgb,select(vec3(0.0),vec3(1.0),b>=0.0),abs(b));rgb=(rgb-0.5)*select(1.0+k,1.0/max(1.0-k,0.0001),k>=0.0)+0.5;}return vec4(clamp(rgb,vec3(0.0),vec3(1.0)),c.a);''',
"exposure": r'''var rgb=c.rgb;if A0022<0.5{rgb=srgb_decode(rgb);}rgb=pow(max(rgb*exp2(A0003)+A0004,vec3(0.0)),vec3(1.0/max(A0005,0.0001)));
 rgb=pow(max(rgb*exp2(vec3(A0008,A0013,A0018))+vec3(A0009,A0014,A0019),vec3(0.0)),1.0/max(vec3(A0010,A0015,A0020),vec3(0.0001)));
 if A0022<0.5{rgb=srgb_encode(rgb);}return vec4(clamp(rgb,vec3(0.0),vec3(1.0)),c.a);''',
"hue_saturation": r'''var h=rgb_hsl(c.rgb);h.x=fract(h.x+A0004/360.0);let sat=A0005/100.0;h.y=select(h.y*(1.0+sat),mix(h.y,1.0,sat),sat>0.0);
 let light=A0006/100.0;h.z=select(h.z*(1.0+light),mix(h.z,1.0,light),light>0.0);
 if A0007>0.5{h=vec3(A0008/360.0,A0009/100.0,h.z);let v=A0010/100.0;h.z=select(h.z*(1.0+v),mix(h.z,1.0,v),v>0.0);}return vec4(hsl_rgb(h),c.a);''',
"tint": r'''let gray=luminance(c.rgb);let rgb=mix(V0001.rgb,V0002.rgb,gray);return vec4(mix(c.rgb,rgb,A0003/100.0),c.a);''',
"tritone": r'''let gray=luminance(c.rgb);let rgb=select(mix(V0003.rgb,V0002.rgb,gray*2.0),mix(V0002.rgb,V0001.rgb,(gray-0.5)*2.0),gray>=0.5);return vec4(mix(rgb,c.rgb,A0004/100.0),c.a);''',
"color_balance": r'''let l=luminance(c.rgb);let shadows=clamp((0.5-l)*2.0,0.0,1.0);let highs=clamp((l-0.5)*2.0,0.0,1.0);let mid=1.0-shadows-highs;
 var rgb=clamp(c.rgb+(vec3(A0001,A0002,A0003)*shadows+vec3(A0004,A0005,A0006)*mid+vec3(A0007,A0008,A0009)*highs)/100.0,vec3(0.0),vec3(1.0));
 if A0010>0.5{let h=rgb_hsl(rgb);rgb=hsl_rgb(vec3(h.xy,rgb_hsl(c.rgb).z));}return vec4(rgb,c.a);''',
"levels": r'''var rgb=vec3(level(c.r,A0003,A0004,A0005,A0006,A0007),level(c.g,A0003,A0004,A0005,A0006,A0007),level(c.b,A0003,A0004,A0005,A0006,A0007));
 rgb=vec3(level(rgb.r,A0103,A0104,A0105,A0106,A0107),level(rgb.g,A0203,A0204,A0205,A0206,A0207),level(rgb.b,A0303,A0304,A0305,A0306,A0307));return vec4(rgb,level(c.a,A0403,A0404,A0405,A0406,A0407));''',
"curves":r'''return curve_lookup(c);''',
"gaussian_blur":r'''let radius=A0001;if radius<=0.0{return c;}let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),fx.clock.z>0.5);
 if (A0002==2.0&&fx.clock.z>0.5)||(A0002==3.0&&fx.clock.z<0.5){return c;}
 let sigma=max(radius/3.0,0.15);let step=max(1.0,radius/128.0);var sum=vec4(0.0);var weight=0.0;
 for(var i:i32=-128;i<=128;i=i+1){let d=f32(i)*step;if abs(d)<=ceil(radius){let w=exp(-0.5*d*d/(sigma*sigma));sum+=sample_input(p+axis*d)*w;weight+=w;}}return sum/max(weight,0.0001);''',
"fast_box_blur":r'''let radius=A0001;if radius<=0.0||A0002<=0.0{return c;}let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),fx.clock.z>0.5);
 if (A0003==2.0&&fx.clock.z>0.5)||(A0003==3.0&&fx.clock.z<0.5){return c;}
 let spread=radius*sqrt(max(A0002,1.0));let step=max(1.0,spread/128.0);var sum=vec4(0.0);var weight=0.0;
 for(var i:i32=-128;i<=128;i=i+1){let d=f32(i)*step;if abs(d)<=ceil(spread){let w=select(1.0,exp(-1.5*d*d/max(spread*spread,0.0001)),A0002>1.0);sum+=sample_input(p+axis*d)*w;weight+=w;}}return sum/max(weight,0.0001);''',
"directional_blur":r'''if A0002<=0.0{return c;}let a=radians(A0001);let direction=vec2(sin(a),cos(a));var sum=vec4(0.0);for(var i:i32=0;i<128;i=i+1){let t=(f32(i)+0.5)/128.0-0.5;sum+=sample_input(p+direction*A0002*t);}return sum/128.0;''',
"radial_blur":r'''let center=V0002.xy;let amount=A0001;if amount<=0.0{return c;}let count=select(64.0,128.0,A0004>1.5);var sum=vec4(0.0);
 for(var i:i32=0;i<128;i=i+1){if f32(i)<count{let jitter=fract(sin(f32(i)+A0006*12.9898)*43758.5453);let t=(f32(i)+jitter)/count-0.5;var q=center+rotate(p-center,t*amount*0.01);if A0003>1.5{q=center+(p-center)*(1.0+t*amount*0.01);}sum+=sample_input(q);}}return sum/count;''',
"sharpen":r'''let blurred=(sample_input(p+vec2(1.0,0.0))+sample_input(p-vec2(1.0,0.0))+sample_input(p+vec2(0.0,1.0))+sample_input(p-vec2(0.0,1.0)))*0.25;return vec4(clamp(c.rgb+(c.rgb-blurred.rgb)*A0001/100.0,vec3(0.0),vec3(1.0)),c.a);''',
"unsharp_mask":r'''if fx.clock.z>=1.5{let original=sample_source(p);let delta=original.rgb-c.rgb;let sharpened=original.rgb+select(vec3(0.0),delta,abs(delta)>=vec3(A0003))*A0001/100.0;return vec4(clamp(sharpened,vec3(0.0),vec3(1.0)),original.a);}
 let radius=A0002;let sigma=max(radius/3.0,0.15);let axis=select(vec2(1.0,0.0),vec2(0.0,1.0),fx.clock.z>0.5);var sum=vec4(0.0);var weight=0.0;
 for(var i:i32=-128;i<=128;i=i+1){let d=f32(i)*max(1.0,radius/128.0);if abs(d)<=ceil(radius){let w=exp(-0.5*d*d/(sigma*sigma));sum+=sample_input(p+axis*d)*w;weight+=w;}}return sum/max(weight,0.0001);''',
"mirror":r'''let center=V0001.xy;let normal=vec2(cos(radians(A0002)),sin(radians(A0002)));let distance=dot(p-center,normal);let q=select(p,p-2.0*distance*normal,distance>0.0);return sample_input(q);''',
"offset":r'''let shift=V0001.xy-fx.size.xy*0.5;let q=fract((p-shift)/fx.size.xy)*fx.size.xy;return mix(sample_input(q),c,A0002);''',
"bulge":r'''let center=V0003.xy;let radius=max(vec2(A0001,A0002),vec2(0.0001));let v=(p-center)/radius;let r=length(v);if r>=1.0{return c;}
 let taper=select(1.0,max(A0005/max(A0001,A0002),0.05),A0005>0.0);let power=pow(max(1.0-r*r,0.0),taper);let scale=pow(max(r,0.00001),A0004*power);var q=center+(p-center)*scale;if A0007>0.5{q=clamp(q,vec2(0.0),fx.size.xy);}return sample_input(q);''',
"twirl":r'''let center=V0003.xy;let radius=A0002*min(fx.size.x,fx.size.y)/100.0;let distance=length(p-center);if radius<=0.0||distance>=radius{return c;}let a=-radians(A0001)*pow(1.0-distance/radius,2.0);return sample_input(center+rotate(p-center,a));''',
"wave_warp":r'''let a=radians(A0004);let direction=vec2(cos(a),sin(a));let normal=vec2(-direction.y,direction.x);let phase=dot(p,normal)/max(A0003,1.0)+fx.clock.x*A0005+A0007/360.0;
 let t=fract(phase);var wave=sin(phase*6.28318530718);if A0001==2.0{wave=select(-1.0,1.0,t>=0.5);}else if A0001==3.0{wave=1.0-4.0*abs(t-0.5);}else if A0001==4.0{wave=2.0*t-1.0;}else if A0001==5.0{wave=1.0-2.0*t;}else if A0001>=6.0{wave=fract(sin(floor(phase)*12.9898+fx.clock.w)*43758.5453)*2.0-1.0;}return sample_input(p-direction*wave*A0002);''',
"polar_coordinates":r'''let uv=p/fx.size.xy;let center=fx.size.xy*0.5;let v=(p-center)/max(min(fx.size.x,fx.size.y)*0.5,0.0001);var q=vec2((atan2(v.y,v.x)/6.28318530718+0.5)*fx.size.x,length(v)*fx.size.y);
 if A0002>1.5{let angle=(uv.x-0.5)*6.28318530718;q=center+vec2(cos(angle),sin(angle))*uv.y*min(fx.size.x,fx.size.y)*0.5;}return sample_input(mix(p,q,A0001));''',
}

def generate(capture:Path):
    data=json.loads(capture.read_text(encoding="utf-8"))
    assert data["version"].startswith("18.0.1") and not data["errors"]
    LIB.mkdir(parents=True,exist_ok=True);(LIB/"shaders").mkdir(exist_ok=True)
    reference=ROOT/"crates/aem-effects/reference";reference.mkdir(exist_ok=True)
    (reference/"ae2021-parameters.json").write_text(json.dumps(data,ensure_ascii=False,indent=2)+"\n",encoding="utf-8",newline="\n")
    effects=[]
    for record in data["effects"]:
        eid=record["id"];params=[]
        for prop in record["children"]:
            value=prop.get("defaultValue");suffix=prop["matchName"].split("-")[-1]
            if not suffix.isdigit():continue
            number=int(suffix)
            if value is None and not(eid=="curves" and number==1):continue
            kind="float"
            if isinstance(value,list):kind="color" if len(value)==4 else "vec2" if len(value)==2 else "vec3"
            if number in BOOL.get(eid,[]):kind="bool"
            if number in ENUM.get(eid,[]):kind="enum"
            if eid=="curves" and number==1:kind="curve";value=0
            vector=(value if isinstance(value,list) else [value])+[0]*4
            minimum=prop.get("min");maximum=prop.get("max")
            if minimum is None:minimum=-1000000
            if maximum is None:maximum=1000000
            if kind=="curve":minimum=0;maximum=1
            options=[str(i) for i in range(int(minimum),int(maximum)+1)] if kind=="enum" else []
            p=dict(id=f"p{number:04}",name=prop["name"],kind=kind,default=vector[:4],min=minimum,max=maximum,step=1 if kind in ("enum","bool") or maximum-minimum>10 else .01,units=prop.get("units") or "",animatable=bool(prop.get("animatable")),options=options,reference_match_name=prop["matchName"],implemented=number not in UNSUPPORTED.get(eid,[]))
            if kind=="vec2":p["relative_default"]=[vector[0]/256,vector[1]/256]
            params.append(p)
        if eid=="levels":
            for channel in range(1,5):
                for p in list(params):
                    number=int(p['id'][1:])
                    if 3<=number<=7:
                        v=dict(p);v['id']=f"p{channel:02}{number:02}";v['name']=["","红色 · ","绿色 · ","蓝色 · ","Alpha · "][channel]+p['name'];v['animatable']=True;params.append(v)
        params.append(dict(id="effect_opacity",name="效果不透明度",kind="float",default=[100,0,0,0],min=0,max=100,step=1,units="百分比",animatable=True,options=[],reference_match_name="ADBE Effect Mask Opacity",implemented=True))
        params.sort(key=lambda p:p['id'])
        slots={p['id']:i for i,p in enumerate(params)}
        def subst(match):
            prefix,number=match.group(1),match.group(2)
            return f"fx.params[{slots['p'+number]}]"+(".x" if prefix=="A" else "")
        body=re.sub(r"\b([AV])(\d{4})\b",subst,BODY[eid])
        extra="fn level(v:f32,black:f32,white:f32,gamma:f32,low:f32,high:f32)->f32 {let t=clamp((v-black)/select(0.00001,white-black,abs(white-black)>0.00001),0.0,1.0);return mix(low,high,pow(t,1.0/max(gamma,0.00001)));}\n" if eid=="levels" else ""
        source=COMMON+extra+"fn main_fx(p:vec2<f32>)->vec4<f32>{let c=sample_input(p);\n"+body+"\n}\n"
        (LIB/"shaders"/(eid+".wgsl")).write_text(source,encoding="utf-8",newline="\n")
        index=list(ENGLISH).index(eid);category="调色" if index<8 else "模糊与锐化" if index<14 else "扭曲"
        passes=2 if eid in ("gaussian_blur","fast_box_blur") else 3 if eid=="unsharp_mask" else 1
        edge_param={"gaussian_blur":"p0003","fast_box_blur":"p0004"}.get(eid)
        padding={"op":"constant","value":0}
        if eid in ("gaussian_blur","fast_box_blur"):padding={"op":"ceil","value":{"op":"parameter","id":"p0001"}}
        if eid=="fast_box_blur":padding={"op":"ceil","value":{"op":"multiply","a":{"op":"parameter","id":"p0001"},"b":{"op":"parameter","id":"p0002"}}}
        if eid in ("gaussian_blur","fast_box_blur"):
            padding={"op":"multiply","a":padding,"b":{"op":"add","a":{"op":"constant","value":1},"b":{"op":"multiply","a":{"op":"constant","value":-1},"b":{"op":"parameter","id":edge_param}}}}
        if eid=="directional_blur":padding={"op":"ceil","value":{"op":"parameter","id":"p0002"}}
        if eid=="wave_warp":padding={"op":"ceil","value":{"op":"abs","value":{"op":"parameter","id":"p0002"}}}
        notes=["独立算法实现；尚未通过完整 AE 参考用例，不能声明精确还原。"]
        if any(p['kind']=='enum' for p in params):notes.append("枚举选项暂保留 AE 数值编号；原生菜单名称尚待采集。")
        if eid=="hue_saturation":notes.append("首期只支持主通道；AE 自定义通道范围与范围关键帧尚不支持。")
        if eid=="levels":notes.append("独立保存主/R/G/B/Alpha 通道数值；AE 自定义直方图界面不属于可渲染参数。")
        if eid=="curves":notes.append("曲线对象采用分段插值与 LUT；AE 私有曲线数据格式未导入。")
        if eid=="fast_box_blur":notes.append("多次迭代使用等方差核近似，需要对照后确认差异。")
        if eid=="wave_warp":notes.append("非默认固定/抗锯齿选项尚不支持；波形5使用反向锯齿、6及以上使用确定性噪声近似，圆形系列与平滑噪声尚待补齐。")
        effects.append(dict(id=eid,name=record["name"],english_name=ENGLISH[eid],category=category,params=params,passes=[dict(shader=f"shaders/{eid}.wgsl",entry="main_fx") for _ in range(passes)],resources=[],padding=padding,edge_mode="clamp" if eid in ("sharpen","unsharp_mask") else "transparent",edge_param=edge_param,working_space="srgb",alpha_mode="premultiplied" if eid in ("gaussian_blur","fast_box_blur","directional_blur","radial_blur") else "straight",compatibility="approximate",compatibility_profile="ae2021-srgb8-v1",reference_match_name=record["matchName"],reference_version="18.0.1",known_differences=notes,required_capabilities=["single_frame","multipass","dynamic_bounds","color_profile"]+(["param_lut"] if eid=="curves" else [])))
    manifest=dict(format_version=1,sdk_version=1,id="com.motionstudio.effects.ae2021",version="1.0.0",name="motion-studio · AE 2021 常用效果",author="motion-studio contributors",license="MIT",effects=effects)
    (LIB/"manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8",newline="\n")

if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("capture",type=Path);generate(parser.parse_args().capture)
