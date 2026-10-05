"""Build deterministic inputs and cases for ae_render_references.jsx."""
import json
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/"artifacts/ae-reference/18.0.1"
VALUES={
"brightness_contrast":{"0001":35,"0002":30},"exposure":{"0003":.75,"0004":.02,"0005":1.1},
"hue_saturation":{"0004":35,"0005":25},"tint":{"0001":[.05,.1,.2,0],"0002":[.9,.4,.2,0],"0003":75},
"tritone":{"0002":[.2,.6,.4,0],"0004":15},"color_balance":{"0004":25,"0005":-15,"0010":1},
"levels":{"0003":.15,"0004":.85,"0005":1.2},"curves":{},"gaussian_blur":{"0001":12},
"fast_box_blur":{"0001":4,"0002":3},"directional_blur":{"0001":35,"0002":14},
"radial_blur":{"0001":12},"sharpen":{"0001":40},"unsharp_mask":{"0001":80,"0002":2,"0003":.01},
"mirror":{"0001":[128,128]},"offset":{"0001":[160,110],"0002":.1},"bulge":{"0001":70,"0002":70,"0004":.7},
"twirl":{"0001":90,"0002":45},"wave_warp":{"0002":6,"0003":32,"0005":.5,"0007":45},
"polar_coordinates":{"0001":.6},
}
def main():
    OUT.mkdir(parents=True,exist_ok=True)
    for kind in ("gradient","checker","alpha_edge"):
        pixels=[]
        for y in range(256):
            for x in range(256):
                if kind=="gradient":p=(x,y,255-x,255)
                elif kind=="checker":p=((240,30,80,255) if (x//16+y//16)%2 else (20,220,160,255))
                else:p=(x,255-y,200,255 if 40<=x<216 and 40<=y<216 else 0)
                pixels.append(p)
        image=Image.new("RGBA",(256,256));image.putdata(pixels);image.save(OUT/(kind+".png"))
    reference=json.loads((OUT/"parameters.json").read_text(encoding="utf-8"));cases=[]
    for effect in reference["effects"]:
        for kind in ("gradient","checker","alpha_edge"):
            for variant in ("default","changed"):
                if effect['id']=="curves" and variant=="changed":continue
                properties={p['matchName']:VALUES[effect['id']][p['matchName'].split('-')[-1]] for p in effect['children'] if variant=="changed" and p['matchName'].split('-')[-1] in VALUES[effect['id']]}
                cases.append(dict(id=effect['id']+"-"+kind+"-"+variant,effect=effect['id'],matchName=effect['matchName'],input=kind+".png",properties=properties,width=256,height=256,frame=0))
    (OUT/"cases.json").write_text(json.dumps(cases,ensure_ascii=False,indent=2),encoding="utf-8")
if __name__=="__main__":main()
