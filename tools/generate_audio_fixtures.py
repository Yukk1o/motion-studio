"""Generate original synthetic audio fixtures; requires ffmpeg on PATH."""
import math
from pathlib import Path
import shutil
import struct
import subprocess
import wave

root = Path(__file__).resolve().parents[1] / "crates/motion-media/tests/fixtures"
root.mkdir(parents=True, exist_ok=True)
ffmpeg = shutil.which("ffmpeg")
if not ffmpeg:
    raise SystemExit("ffmpeg is required to regenerate encoded fixtures")
source = root / "tone-stereo-48000.wav"
with wave.open(str(source), "wb") as out:
    out.setparams((2, 2, 48000, 0, "NONE", "not compressed"))
    for n in range(96000):
        # Distinct frequencies in each channel and a known silent leading 100 ms.
        values = [0 if n < 4800 else int(0.3 * 32767 * math.sin(2 * math.pi * hz * n / 48000)) for hz in (440, 880)]
        out.writeframesraw(struct.pack("<hh", *values))
for name, args in [
    ("tone-mono-44100.mp3", ["-ar", "44100", "-ac", "1", "-c:a", "libmp3lame", "-b:a", "96k"]),
    ("tone-stereo-48000.m4a", ["-c:a", "aac", "-profile:a", "aac_low", "-b:a", "128k"]),
]:
    subprocess.run([ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-i", str(source), *args, str(root / name)], check=True)
(root / "invalid.bin").write_bytes(b"Motion Studio invalid media fixture\n")
print(root)
