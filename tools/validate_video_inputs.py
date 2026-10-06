"""Compare full-resolution Android frames with independent source PTS/FFmpeg."""
import argparse
import hashlib
import json
import subprocess
from decimal import Decimal
from pathlib import Path

def run(*args): return subprocess.check_output(args)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--adb',required=True)
    parser.add_argument('--adb-port',type=int,default=5037)
    parser.add_argument('--serial',required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    assert 1<=args.adb_port<=65535
    adb=[args.adb,'-P',str(args.adb_port),'-s',args.serial,'exec-out','run-as','com.motionstudio.editor']
    source=Path(__file__).resolve().parents[1]/'crates/aem-media/tests/fixtures/inputs'
    manifest=json.loads((source/'manifest.json').read_text())
    latest={}
    for path in run(*adb,'find','files/acceptance','-name','input-report.json').decode().splitlines():
        report=json.loads(run(*adb,'cat',path));stamp=int(run(*adb,'stat','-c','%Y',path))
        name=report['fixture']
        if name not in latest or stamp>latest[name][0]:latest[name]=(stamp,path,report)
    assert set(latest)=={'uhd-60.mp4','dci-60.mp4','portrait-60.mp4','hfr-240.mp4','fractional-59.94.mp4','ultrawide-60.mp4','square-60.mp4','tall-60.mp4'}
    args.output.mkdir(parents=True,exist_ok=True)
    results=[]
    for name,(_,path,report) in sorted(latest.items()):
        root=str(Path(path).parent).replace('\\','/');dst=args.output/name;dst.mkdir(exist_ok=True)
        (dst/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        assert hashlib.sha256(run(*adb,'cat',root+'/'+report['asset']['path'])).hexdigest()==manifest[name]['sha256']
        pts=[int(Decimal(f['best_effort_timestamp_time'])*1000000) for f in manifest[name]['frames']]
        for i,sample in enumerate(report['samples']):
            pixels=run(*adb,'cat',root+f'/frame-{i}.rgba');(dst/f'frame-{i}.rgba').write_bytes(pixels)
            index=min(range(len(pts)),key=lambda n:abs(pts[n]-sample['pts_us']))
            assert abs(pts[index]-sample['pts_us'])<=1
            oracle=run('ffmpeg','-v','error','-i',str(source/name),'-vf',f'select=eq(n\\,{index})','-frames:v','1','-f','rawvideo','-pix_fmt','rgba','pipe:1')
            assert len(pixels)==len(oracle)==sample['width']*sample['height']*4
            # Histogram avoids sorting/storing millions of RGB differences.
            histogram=[0]*256
            for offset in range(0,len(pixels),4):
                for c in (0,1,2):histogram[abs(pixels[offset+c]-oracle[offset+c])]+=1
            count=sum(histogram);mae=sum(n*v for n,v in enumerate(histogram))/count
            cumulative=0;p99=0
            for n,v in enumerate(histogram):
                cumulative+=v
                if cumulative>=count*.99:p99=n;break
            value={'fixture':name,'pts_us':sample['pts_us'],'width':sample['width'],'height':sample['height'],'mae':mae,'p99':p99}
            results.append(value);print(json.dumps(value),flush=True)
            assert mae<8 and p99<35,value
    (args.output/'parity.json').write_text(json.dumps({'comparisons':results},indent=2),encoding='utf-8')
    print(json.dumps({'fixtures':len(latest),'frames':len(results),'max_rgb_mae':max(v['mae'] for v in results)},indent=2))

if __name__=='__main__':main()
