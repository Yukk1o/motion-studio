"""Compare actual Android PCM/frame output with host FFmpeg at source PTS.

Reads only reports from MediaFormatApiTest and uses our original fixtures.
Artifacts belong in an ignored directory or a task-owned temporary directory.
"""
import argparse, array, hashlib, json, math, subprocess, sys
from decimal import Decimal
from pathlib import Path

def run(*args): return subprocess.check_output(args)

def floats(data):
    result=array.array('f',data)
    if sys.byteorder!='little':result.byteswap()
    return result

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--adb',required=True)
    parser.add_argument('--adb-port',type=int,default=5037,help='Dedicated adb server port when other work shares the host')
    parser.add_argument('--serial',required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    assert 1<=args.adb_port<=65535
    adb=[args.adb,'-P',str(args.adb_port),'-s',args.serial,'exec-out','run-as','com.motionstudio.editor']
    fixtures=Path(__file__).resolve().parents[1]/'crates/aem-media/tests/fixtures/formats'
    manifest=json.loads((fixtures/'manifest.json').read_text())
    args.output.mkdir(parents=True,exist_ok=True)
    latest={}
    for kind in ('audio','video'):
        for path in run(*adb,'find','files/acceptance','-name',kind+'-report.json').decode().splitlines():
            if '/media-formats-' not in path:continue
            report=json.loads(run(*adb,'cat',path))
            name=report['fixture'];stamp=int(run(*adb,'stat','-c','%Y',path))
            if name not in latest or stamp>latest[name][0]:latest[name]=(stamp,path,kind,report)
    results=[];failures=[]
    for name,(_,path,kind,report) in sorted(latest.items()):
        root=str(Path(path).parent).replace('\\','/');dst=args.output/name;dst.mkdir(exist_ok=True)
        (dst/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        if kind=='video':
            pts=json.loads(run('ffprobe','-v','error','-select_streams','v:0','-show_frames','-show_entries','frame=best_effort_timestamp_time','-of','json',str(fixtures/name)))['frames']
            pts=[int(Decimal(f['best_effort_timestamp_time'])*1000000) for f in pts]
            for i,sample in enumerate(report['samples']):
                frame=run(*adb,'cat',root+f'/frame-{i}.rgba');(dst/f'frame-{i}.rgba').write_bytes(frame)
                # MediaExtractor truncates some rational MP4 times to us;
                # ffprobe formats them rounded to six decimals. Allow only 1 us.
                index=min(range(len(pts)),key=lambda n:abs(pts[n]-sample['pts_us']))
                assert abs(pts[index]-sample['pts_us'])<=1,(name,sample['pts_us'],pts[index])
                vf=f'select=eq(n\\,{index})'
                if 'oracle_color_matrix' in manifest[name]:vf+=',scale=in_color_matrix='+manifest[name]['oracle_color_matrix']
                oracle=run('ffmpeg','-v','error','-i',str(fixtures/name),'-vf',vf,'-frames:v','1','-f','rawvideo','-pix_fmt','rgba','pipe:1')
                assert len(frame)==len(oracle),(name,len(frame),len(oracle))
                errors=[abs(a-b) for n,(a,b) in enumerate(zip(frame,oracle)) if n%4!=3];errors.sort()
                value={'fixture':name,'kind':'frame','pts_us':sample['pts_us'],'rgb_mae':sum(errors)/len(errors),'rgb_p99':errors[int(.99*len(errors))],'rgb_max':max(errors)}
                results.append(value)
                if value['rgb_mae']>=8 or value['rgb_p99']>=35:failures.append(value)
            asset=report['metadata']['audio']
        else:asset=report['asset']
        owned=run(*adb,'cat',root+'/'+asset['path'])
        assert hashlib.sha256(owned).hexdigest()==manifest[name]['sha256'],('owned source mismatch',name)
        stem=Path(asset['path']).stem
        pcm=run(*adb,'cat',root+f'/cache/audio-v1/{stem}.pcm');(dst/'source.f32').write_bytes(pcm)
        reference=run('ffmpeg','-v','error','-copyts','-i',str(fixtures/name),'-map','0:a:0','-af','aresample=async=1:first_pts=0','-ar',str(asset['sample_rate']),'-f','f32le','-c:a','pcm_f32le','pipe:1')
        native=floats(pcm);independent=floats(reference);n=min(len(native),len(independent))
        assert n>0
        errors=[a-b for a,b in zip(native,independent)]
        codec=next(s['codec_name'] for s in manifest[name]['streams'] if 'sample_rate' in s)
        # Android may retain one final compressed packet's padding. Validate
        # length separately rather than silently comparing only a short prefix.
        allowed_frames={'aac':1024,'opus':960,'vorbis':math.ceil(asset['sample_rate']*.002)}.get(codec,0)
        delta=len(native)-len(independent)
        value={'fixture':name,'kind':'audio','native_values':len(native),'ffmpeg_values':len(independent),'length_difference_frames':delta/asset['channels'],'allowed_length_difference_frames':allowed_frames,'rms_error':math.sqrt(sum(e*e for e in errors)/n),'max_error':max(abs(e) for e in errors)}
        results.append(value)
        if abs(delta)>allowed_frames*asset['channels'] or value['rms_error']>=.003 or value['max_error']>=.04:failures.append(value)
    (args.output/'parity.json').write_text(json.dumps({'comparisons':results},indent=2),encoding='utf-8')
    assert not failures,failures
    assert len(latest)==19,('missing completed format reports',list(latest))
    print(json.dumps({'fixtures':len(latest),'frame_comparisons':sum(v['kind']=='frame' for v in results),'audio_comparisons':sum(v['kind']=='audio' for v in results),'max_audio_rms':max(v['rms_error'] for v in results if v['kind']=='audio'),'max_frame_mae':max(v['rgb_mae'] for v in results if v['kind']=='frame')},indent=2))

if __name__=='__main__':main()
