"""Regenerate local AE input fixtures and snapshots; generated images stay ignored."""
import json
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT/'artifacts/ae-tiling/18.0.1'


def main():
    OUT.mkdir(parents=True,exist_ok=True)
    data=json.loads((ROOT/'tools/effect_metadata/ae18-tiling-ranges.json').read_text(encoding='utf-8'))
    identities={e['id']:e['matchName'] for e in data['effects']}
    grid=Image.new('RGBA',(64,64))
    grid.putdata([(int(x*255/63),int(y*255/63),220 if (x//8+y//8)%2 else 30,255)
                  for y in range(64) for x in range(64)])
    grid.save(OUT/'grid.png')
    alpha=grid.copy()
    alpha.putdata([(*grid.getpixel((x,y))[:3],0 if x<8 or y<8 or x>=56 or y>=56 else (128 if x<32 else 255))
                   for y in range(64) for x in range(64)])
    alpha.save(OUT/'alpha.png')
    cases=[]
    def case(id,effect,properties,transparent=False):
        match=identities[effect]
        cases.append(dict(id=id,effect=effect,matchName=match,input='alpha.png' if transparent else 'grid.png',
                          width=256,height=256,frame=0,
                          properties={match+'-'+str(k).zfill(4):v for k,v in properties.items()}))
    case('tile-default','motion_tile',{})
    case('tile-repeat','motion_tile',{4:400,5:400})
    case('tile-mirror','motion_tile',{4:400,5:200,6:1})
    case('tile-small','motion_tile',{2:50,3:50,4:400,5:400})
    case('tile-phase','motion_tile',{4:400,5:400,7:90})
    case('tile-horizontal-phase','motion_tile',{4:400,5:400,7:90,8:1})
    case('tile-mirror-phase','motion_tile',{4:400,5:400,6:1,7:90})
    case('tile-center','motion_tile',{1:[40,24],4:400,5:400})
    case('tile-alpha','motion_tile',{2:50,3:50,4:400,5:400,6:1},True)
    case('tile-crop','motion_tile',{4:50,5:200})
    case('tile-zero','motion_tile',{2:0,4:400,5:400})
    case('spherize','spherize',{1:28})
    case('spherize-off','spherize',{},True)
    case('optics','optics_compensation',{1:90})
    case('optics-reverse','optics_compensation',{1:90,2:1})
    case('lens','cc_lens',{})
    case('lens-negative','cc_lens',{2:75,3:-100})
    case('radial-fast','cc_radial_fast_blur',{})
    case('radial-dark','cc_radial_fast_blur',{2:40,3:3},True)
    case('choker','simple_choker',{2:3},True)
    case('choker-expand','simple_choker',{2:-3},True)
    case('choker-matte','simple_choker',{1:2,2:3},True)
    case('solid','solid_composite',{},True)
    case('solid-alpha','solid_composite',{1:70,2:[.2,.4,.8,1],3:40},True)
    (OUT/'cases.json').write_text(json.dumps(cases,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
    print(f'{len(cases)} local reference cases; {OUT}')


if __name__ == '__main__':
    main()
