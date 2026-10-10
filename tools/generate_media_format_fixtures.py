"""Transcode our own synthetic pulse/colour fixtures; no third-party media.

Requires host ffmpeg/ffprobe. Records source codecs, native rates and packet times.
"""
from pathlib import Path
import hashlib, json, subprocess

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'crates/motion-media/tests/fixtures'
OUT = FIXTURES / 'formats'
OUT.mkdir(parents=True, exist_ok=True)

def run(*args):
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', *map(str,args)], check=True)

audio = FIXTURES / 'tone-stereo-48000.wav'
audio_cases = [
    ('pcm8.wav','pcm_u8',8000), ('pcm24.wav','pcm_s24le',96000),
    ('pcm32.wav','pcm_s32le',192000), ('float32.wav','pcm_f32le',32000),
    ('float64.wav','pcm_f64le',11025), ('lossless.flac','flac',96000),
    ('lossless.m4a','alac',44100), ('vorbis.ogg','libvorbis',22050),
    ('linear.aiff','pcm_s16be',48000), ('opus.ogg','libopus',48000),
    ('adts.aac','aac',16000),
]
for name,codec,rate in audio_cases:
    run('-i',audio,'-c:a',codec,'-ar',rate,OUT/name)

video = FIXTURES / 'video/sound-24fps.mp4'
run('-i',video,'-c','copy',OUT/'avc.mov')
run('-i',video,'-c','copy',OUT/'avc.mkv')
hevc=['-c:v','libx265','-pix_fmt','yuv420p','-x265-params','log-level=error:pools=1:frame-threads=1:keyint=24','-colorspace','bt709','-color_primaries','bt709','-color_trc','bt709','-color_range','tv','-c:a','copy']
run('-i',video,*hevc,'-tag:v','hvc1',OUT/'hevc.mp4')
run('-i',OUT/'hevc.mp4','-c','copy',OUT/'hevc.mkv')
run('-i',video,'-c:v','libvpx','-b:v','300k','-pix_fmt','yuv420p','-g','24','-c:a','libvorbis',OUT/'vp8.webm')
run('-i',video,'-vf','scale=in_color_matrix=bt709:out_color_matrix=bt601','-c:v','libvpx','-b:v','300k','-pix_fmt','yuv420p','-g','24','-colorspace','smpte170m','-color_primaries','smpte170m','-color_trc','smpte170m','-c:a','libvorbis',OUT/'vp8-601.webm')
run('-i',video,'-c:v','libvpx-vp9','-b:v','300k','-pix_fmt','yuv420p','-g','24','-colorspace','bt709','-color_range','tv','-c:a','libopus',OUT/'vp9.webm')
run('-i',OUT/'vp9.webm','-itsoffset','0.25','-i',audio,'-map','0:v:0','-map','1:a:0','-c:v','copy','-c:a','libopus',OUT/'vp9-delayed.webm')
# Unsupported profiles must fail before a lossy implicit downconversion.
run('-i',video,'-an','-c:v','libx265','-pix_fmt','yuv420p10le','-x265-params','log-level=error:pools=1:frame-threads=1','-tag:v','hvc1',OUT/'reject-hevc10.mp4')
run('-i',video,'-an','-c:v','libvpx-vp9','-pix_fmt','yuv420p10le',OUT/'reject-vp9-10.webm')

manifest={}
for path in sorted(OUT.iterdir()):
    if path.suffix=='.json':continue
    result=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_streams','-show_format','-show_entries','stream=index,codec_name,profile,sample_rate,channels,duration,width,height,pix_fmt,color_space,color_transfer,color_primaries,color_range:format=duration,start_time','-of','json',str(path)]))
    manifest[path.name]={'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'bytes':path.stat().st_size,**result}
    # VP8's decoder defaults to BT.601 and may override explicit WebM colour
    # tags. For this authored BT.709 fixture the oracle honours the container.
    if path.name=='vp8.webm':manifest[path.name]['oracle_color_matrix']='bt709'
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
print(json.dumps({name:data['bytes'] for name,data in manifest.items()},indent=2))
