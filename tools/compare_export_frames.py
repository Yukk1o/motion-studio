"""Verify an encoded frame against the host PNG with the file's signaled colors.

Requires Pillow and FFmpeg/FFprobe on PATH. Never override the encoded matrix to
make a test pass: mismatched color signaling must remain a measurable failure.
"""
import argparse
import json
import subprocess
from pathlib import Path
from PIL import Image


def compare(video, reference, frame):
    metadata = json.loads(subprocess.check_output([
        'ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_streams', '-of', 'json', str(video)
    ]))['streams'][0]
    image = Image.open(reference).convert('RGBA')
    assert image.getchannel('A').getextrema() == (255, 255), 'MP4 reference must be composited onto an opaque background'
    image = image.convert('RGB')
    assert (metadata['width'], metadata['height']) == image.size, 'frame dimensions differ'
    raw = subprocess.check_output([
        'ffmpeg', '-v', 'error', '-i', str(video), '-vf', rf'select=eq(n\,{frame})',
        '-frames:v', '1', '-f', 'rawvideo', '-pix_fmt', 'rgb24', 'pipe:1'
    ])
    expected = image.tobytes()
    assert len(raw) == len(expected), 'frame missing or incomplete'
    total = foreground = count = 0
    for i in range(0, len(raw), 3):
        error = sum(abs(raw[i+c] - expected[i+c]) for c in range(3))
        total += error
        if max(expected[i:i+3]) > 50:
            foreground += error
            count += 3
    return dict(frame=frame, width=image.width, height=image.height,
                frames=metadata.get('nb_frames'), color_space=metadata.get('color_space'),
                rgb_mae=total/len(raw), foreground_rgb_mae=foreground/max(count, 1),
                passed=total/len(raw) < 6 and foreground/max(count, 1) < 8)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('video', type=Path)
    parser.add_argument('reference', type=Path)
    parser.add_argument('--frame', type=int, default=0)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    assert args.frame >= 0
    result = compare(args.video, args.reference, args.frame)
    text = json.dumps(result, indent=2) + '\n'
    print(text)
    if args.report:
        args.report.write_text(text, encoding='utf-8')
    if not result['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
