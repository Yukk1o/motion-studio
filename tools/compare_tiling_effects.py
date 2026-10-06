"""Numerical evidence only; raw references/projects/images remain outside Git."""
import argparse
import hashlib
import json
from pathlib import Path
from PIL import Image,ImageDraw
from compare_ae_effects import metrics

ROOT=Path(__file__).resolve().parents[1]


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('root',type=Path)
    parser.add_argument('--publish-report',action='store_true')
    args=parser.parse_args()
    root=args.root.resolve()
    cases=json.loads((root/'cases.json').read_text(encoding='utf-8'))
    records=[]
    for case in cases:
        ae=root/f'ae-{case["id"]}.png';app=root/f'motion-{case["id"]}.png'
        with Image.open(ae) as ai,Image.open(app) as mi:
            a,b=ai.convert('RGBA'),mi.convert('RGBA')
            if a.size != (case['width'],case['height']) or b.size != a.size:
                raise ValueError('Reference output dimensions differ')
            result=metrics(a,b)
        records.append(dict(case=case,input_sha256=sha(root/case['input']),ae_sha256=sha(ae),application_sha256=sha(app),
                            metrics=result,case_threshold_pass=all(v['threshold_pass'] for v in result.values())))
    report=dict(ae_version='18.0.1x1',package_version='1.3.0',package_sha256=sha(ROOT/'crates/aem-effects/library/core-effects.msfx'),
                profile=json.loads((ROOT/'tools/effect_metadata/ae18-tiling-ranges.json').read_text(encoding='utf-8'))['profile'],
                rgb_and_alpha_threshold_255=3,status='approximate',verified_effects=0,
                limitations=['24 static samples; no full AE animation/range/device acceptance.',
                             'RGB measured in visible pixels; empty RGB interior masks have zero samples, not evidence of fidelity.',
                             'Raw outputs, fixtures and AE projects are local ignored artifacts.'],cases=records)
    raw=json.dumps(report,ensure_ascii=False,indent=2)+'\n'
    (root/'comparison.json').write_text(raw,encoding='utf-8',newline='\n')
    if args.publish_report:
        (ROOT/'docs/effects/tiling-validation.json').write_text(raw,encoding='utf-8',newline='\n')
    ids=['tile-repeat','tile-mirror','tile-horizontal-phase','tile-small']
    contact=Image.new('RGB',(1024,300),(25,28,32));draw=ImageDraw.Draw(contact)
    for i,id in enumerate(ids):
        with Image.open(root/f'motion-{id}.png') as image:
            contact.paste(image,(i*256,34),image.getchannel('A'))
        draw.text((i*256+12,12),id,fill=(77,218,197))
    contact.save(root/'motion-tile-gallery.png')
    print(f'{sum(r["case_threshold_pass"] for r in records)}/{len(records)} cases within threshold; verified effects remain 0')
    for record in records:
        edge=record['metrics']['edge'];inner=record['metrics']['interior']
        print(record['case']['id'],record['case_threshold_pass'],
              'interior',round(inner['rgb_mae_255'],3),round(inner['alpha_mae_255'],3),
              'edge',round(edge['rgb_mae_255'],3),round(edge['alpha_mae_255'],3))


if __name__=='__main__':main()
