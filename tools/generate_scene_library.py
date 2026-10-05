"""Build deterministic SDK 2 scene packages; no proprietary plugin assets."""
from pathlib import Path
import json
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "crates/aem-effects/scene-library"

def parameter(id, name, kind, default, low, high, units="", animate=True, options=None):
    values = default if isinstance(default, list) else [default]
    return dict(id=id, name=name, kind=kind, default=values+[0]*(4-len(values)),
                min=low, max=high, step=0.01, units=units, animatable=animate,
                options=options or [])

def main():
    (DEST / "shaders").mkdir(parents=True, exist_ok=True)
    (DEST / "ui").mkdir(exist_ok=True)
    # One appearance shader works with all host-generated sprites. Plugins can
    # replace this shader, including sampling their own packaged PNG resources.
    shader = '''fn main_sprite(uv:vec2<f32>,c:vec4<f32>,style:vec4<f32>)->vec4<f32>{
    let p=(uv-0.5)*2.0;let r=length(p);var coverage=0.0;
    if style.x<0.5 {coverage=exp(-r*r*6.0)*(1.0-smoothstep(0.8,1.0,r));}
    else if style.x<1.5 {coverage=exp(-pow((r-0.65)*18.0,2.0));}
    else if style.x<2.5 {coverage=(1.0-smoothstep(0.65,0.85,r))*0.35;}
    else if style.x<3.5 {coverage=exp(-p.y*p.y*120.0)*(1.0-smoothstep(0.1,1.0,abs(p.x)));}
    else {let angle=atan2(p.y,p.x);let spokes=pow(abs(cos(angle*style.y*0.5)),24.0);coverage=exp(-r*4.0)*spokes*(1.0-smoothstep(0.8,1.0,r));}
    let fringe=vec3(1.0+style.z*p.x,1.0,1.0-style.z*p.x);
    return vec4(c.rgb*fringe*coverage,c.a*coverage);
}'''
    (DEST / "shaders/sprite.wgsl").write_text(shader, encoding="utf-8", newline="\n")
    editor = dict(id="com.motionstudio.scene-editor", protocol=1, title="场景效果编辑器",
                  entry="ui/editor.html", files=["ui/editor.html","ui/editor.js","ui/editor.css"])
    def effect(id, name, english, renderer, params, scene, blend="alpha"):
        return dict(id=id,name=name,english_name=english,category="粒子" if renderer=="particles" else "镜头光效",
                    renderer=renderer,blend=blend,params=params,passes=[dict(shader="shaders/sprite.wgsl",entry="main_sprite")],
                    editor=editor,scene=scene,required_capabilities=["scene_projection","sprite_instances","plugin_editor"] + (["alpha_occlusion"] if renderer=="lens_flare" else []),
                    known_differences=["Independent Motion Studio effect; no Optical Flares/Particular compatibility claim."])
    lens_params = [parameter("position","光源位置","vec3",[540,960,0],-100000,100000,"px"),
                   parameter("intensity","亮度","float",1,0,32),parameter("scale","尺寸","float",100,0,1000,"%"),
                   parameter("attenuation","距离衰减","bool",0,0,1,animate=False),
                   parameter("reference_distance","参考距离","float",1000,1,100000,"px"),
                   parameter("occlusion_radius","遮挡采样半径","float",3,0,128,"px")]
    lens_params[0]["center_default"] = True
    elements = []
    for i,(shape,offset,size,color,strength) in enumerate([
        ("glow",0,[220,220],[1,.65,.25,1],1), ("halo",0,[380,380],[.3,.6,1,1],.3),
        ("streak",0,[1000,120],[.25,.6,1,1],.7), ("star",0,[280,280],[1,.8,.5,1],.6),
        ("ghost",.7,[70,70],[.4,1,.7,1],.3), ("ghost",1.3,[110,110],[.4,.55,1,1],.25),
        ("ghost",1.8,[180,180],[1,.4,.3,1],.12)]):
        elements.append(dict(id=i+1,shape=shape,enabled=True,offset=offset,size=size,color=color,intensity=strength,rays=8,chromatic=.08))
    effects = [effect("lens_flare","镜头光效","Lens Flare","lens_flare",lens_params,dict(occlusion=False,elements=elements),"additive")]
    for id,name,english,speed,spread,gravity,extent,size,color,end_color,blend in [
        ("starfield","星空","Starfield",0,0,[0,0,0],[1800,2600,1800],8,[.8,.9,1,1],[.6,.75,1,1],"additive"),
        ("sparks","火花","Sparks",260,180,[0,240,0],[0,0,0],12,[1,.8,.2,1],[1,.15,.02,0],"additive"),
        ("dust","尘埃","Dust",8,18,[0,0,0],[1300,1900,1200],18,[.8,.85,1,.3],[.8,.85,1,.2],"alpha"),
        ("snow","飘雪","Snow",-70,25,[0,8,0],[1800,2600,1000],10,[1,1,1,.9],[1,1,1,.8],"alpha"),
        ("energy","能量粒子","Energy Particles",90,140,[0,0,0],[240,240,240],28,[.1,.7,1,1],[.7,.2,1,0],"additive")]:
        params=[parameter("rate","出生速率","float",80,0,10000,"particles/s",False),
                parameter("lifetime","寿命","float",5,.01,120,"s",False),
                parameter("speed","纵向速度","float",speed,-10000,10000,"px/s",False),
                parameter("spread","随机速度","float",spread,0,10000,"px/s",False),
                parameter("gravity","重力","vec3",gravity,-10000,10000,"px/s²",False),
                parameter("extent","发射范围","vec3",extent,0,20000,"px",False),
                parameter("shape","发射器形状","enum",0 if id=="sparks" else 1,0,2,animate=False,options=["点","盒","球"]),
                parameter("size","出生尺寸","float",size,0,4096,"px"),
                parameter("end_size","结束尺寸","float",size*.5,0,4096,"px"),
                parameter("color","出生颜色","color",color,0,1),
                parameter("end_color","结束颜色","color",end_color,0,1),
                parameter("fade","淡入淡出比例","float",.15,0,.5),
                parameter("prewarm","预热","bool",1,0,1,animate=False)]
        effects.append(effect(id,name,english,"particles",params,dict(occlusion=False,elements=[]),blend))
    manifest=dict(format_version=1,sdk_version=2,id="com.motionstudio.effects.scene",version="1.0.0",name="Motion Studio Scene Effects",author="Motion Studio",license="MIT",effects=effects)
    (DEST / "manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8",newline="\n")
    names=["manifest.json","shaders/sprite.wgsl",*editor["files"]]
    # Match Git's LF checkout so Windows and other hosts produce the same hash.
    for name in editor["files"]:
        path = DEST / name
        path.write_text(path.read_text(encoding="utf-8"), encoding="utf-8", newline="\n")
    with zipfile.ZipFile(DEST / "scene-effects.msfx","w",compression=zipfile.ZIP_DEFLATED) as archive:
        for name in sorted(names):
            info=zipfile.ZipInfo(name,date_time=(1980,1,1,0,0,0));info.compress_type=zipfile.ZIP_DEFLATED
            archive.writestr(info,(DEST / name).read_bytes())

if __name__ == "__main__": main()
