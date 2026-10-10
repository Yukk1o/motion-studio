"""Build the SDK 5 world-birth emitter without modifying published scene packages."""
from pathlib import Path
import json
import zipfile
from generate_scene_library import parameter

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / "crates/motion-effects/particle-library"

def main():
    params = [
        parameter("rate", "出生速率", "float", 360, 0, 10000, "particles/s", False),
        parameter("lifetime", "寿命", "float", 2, .001, 120, "s", False),
        parameter("position", "发射位置偏移", "vec3", [0,0,0], -100000,100000,"px"),
        parameter("direction", "喷射方向", "vec3", [0,-1,0], -1,1),
        parameter("speed", "喷射速度", "float", 45,-10000,10000,"px/s"),
        parameter("spread", "随机速度", "float", 12,0,10000,"px/s"),
        parameter("inherit_velocity", "继承发射器速度", "float", .5,0,4,"倍"),
        parameter("gravity", "重力加速度", "vec3", [0,30,0], -10000,10000,"px/s²",False),
        parameter("wind", "风速", "vec3", [0,0,0], -10000,10000,"px/s",False),
        parameter("drag", "空气阻力", "float", .8,0,100,"1/s",False),
        parameter("extent", "发射范围", "vec3", [0,0,0],0,20000,"px"),
        parameter("shape", "发射器形状", "enum", 0,0,2,animate=False,options=["点","盒","球"]),
        parameter("size", "出生尺寸", "float", 14,0,4096,"px"),
        parameter("end_size", "结束尺寸", "float", 2,0,4096,"px"),
        parameter("color", "出生颜色", "color", [.15,.8,1,1],0,1),
        parameter("end_color", "结束颜色", "color", [.8,.15,1,0],0,1),
        parameter("fade", "淡入淡出比例", "float", .04,0,.5),
        parameter("prewarm", "预热", "bool", 0,0,1,animate=False),
    ]
    def section(id,title,*slots): return dict(id=id,title=title,slots=list(slots))
    def controls(*params): return dict(kind="parameters",params=list(params))
    editor = dict(id="com.motionstudio.particle-editor",protocol=1,title="运动粒子编辑器",sections=[
        section("preview","预览",dict(kind="preview",max_size=512),dict(kind="timeline")),
        section("emitter","发射",dict(kind="layer_source"),controls("position","shape","extent","rate","lifetime","prewarm"),dict(kind="seed")),
        section("motion","运动",controls("direction","speed","spread","inherit_velocity","gravity","wind","drag"),dict(kind="note",text="Y 正向下；出生运动支持关键帧。阻力大于 0 时速度逐渐靠近风速。出生速率、寿命和力场首期固定，历史表达式暂不支持。")),
        section("appearance","外观",dict(kind="image_sprite"),controls("size","end_size","fade","color","end_color")),
        section("transform","变换",dict(kind="transform")),
    ])
    effect=dict(id="particle_emitter",name="运动粒子",english_name="Particle Emitter",category="粒子",
                renderer="particle_emitter",blend="additive",params=params,
                passes=[dict(shader="shaders/sprite.wgsl",entry="main_sprite")],native_editor=editor,
                scene=dict(particle_space="world_birth",occlusion=False,elements=[]),
                required_capabilities=["scene_projection","sprite_instances","native_plugin_editor","particle_birth_history"],
                known_differences=["Independent world-birth emitter inspired by common particle workflows; no Trapcode Particular compatibility claim.",
                    "Fixed rate/lifetime; keyed birth appearance; single project-image sprite; no emitter-history expressions, collision, ribbons, auxiliary or fluid simulation."])
    manifest=dict(format_version=1,sdk_version=5,id="com.motionstudio.effects.particles",version="1.0.0",
                  name="Motion Studio Particle Effects",author="Motion Studio",license="MIT",effects=[effect])
    (DEST/"manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8",newline="\n")
    with zipfile.ZipFile(DEST/"particle-effects.msfx","w",compression=zipfile.ZIP_DEFLATED) as archive:
        for name in sorted(["manifest.json","shaders/sprite.wgsl"]):
            data=(DEST/name).read_text(encoding="utf-8").replace("\r\n","\n")
            info=zipfile.ZipInfo(name,date_time=(1980,1,1,0,0,0));info.compress_type=zipfile.ZIP_DEFLATED
            archive.writestr(info,data.encode("utf-8"))

if __name__=="__main__": main()
