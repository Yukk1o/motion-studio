"""Generate original SDR patterns for source-resolution/frame-rate acceptance."""
from pathlib import Path
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT/'crates/aem-media/tests/fixtures/inputs'
OUT.mkdir(parents=True,exist_ok=True)
CASES = [
    ('uhd-60.mp4',3840,2160,'60','0.25'),
    ('dci-60.mp4',4096,2160,'60','0.15'),
    ('dci-120.mp4',4096,2160,'120','0.1'),
    ('portrait-60.mp4',2160,3840,'60','0.2'),
    ('hfr-240.mp4',1920,1080,'240','0.25'),
    ('fractional-59.94.mp4',384,216,'60000/1001','0.4'),
    ('ultrawide-60.mp4',3840,480,'60','0.15'),
    ('square-60.mp4',2560,2560,'60','0.15'),
    ('tall-60.mp4',480,3840,'60','0.15'),
    ('too-wide.mp4',4160,64,'30','0.08'),
    ('too-fast.mp4',128,72,'300','0.05'),
]
manifest = {}
for name,w,h,rate,duration in CASES:
    path=OUT/name
    encoder=(['-c:v','libx265','-preset','ultrafast','-crf','28','-x265-params','pools=1:frame-threads=1:log-level=error','-tag:v','hvc1']
        if name=='dci-60.mp4' else ['-c:v','libx264','-preset','ultrafast','-crf','28','-threads','1'])
    subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y','-f','lavfi','-i',f'testsrc2=size={w}x{h}:rate={rate}',
        '-t',duration,'-an',*encoder,'-pix_fmt','yuv420p',
        '-colorspace','bt709','-color_primaries','bt709','-color_trc','bt709','-color_range','tv','-movflags','+faststart',str(path)],check=True)
    probe=json.loads(subprocess.check_output(['ffprobe','-v','error','-select_streams','v:0','-show_streams','-show_frames',
        '-show_entries','stream=codec_name,profile,level,width,height,avg_frame_rate,nb_frames:frame=best_effort_timestamp_time','-of','json',str(path)]))
    manifest[name]={'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'bytes':path.stat().st_size,**probe}
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
print(json.dumps({k:v['bytes'] for k,v in manifest.items()},indent=2))
