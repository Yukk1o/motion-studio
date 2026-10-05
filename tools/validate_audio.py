"""Compare the backend's decoded synthetic sources with independent ffmpeg PCM."""
import argparse
import array
import json
import math
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        raise SystemExit("ffmpeg required for independent verification")
    args.output.mkdir(parents=True, exist_ok=False)
    reports = []
    for name in ("tone-stereo-48000.wav", "tone-mono-44100.mp3", "tone-stereo-48000.m4a"):
        source = ROOT / "crates/aem-media/tests/fixtures" / name
        project = args.output / source.name.replace(".", "-")
        run = subprocess.run([str(args.probe.resolve()), str(source), str(project)], check=True, capture_output=True, text=True, encoding="utf-8")
        task = json.loads(run.stdout)
        metadata = task["metadata"]
        pcm = project / "cache/audio-v1" / (Path(metadata["path"]).stem + ".pcm")
        reference = args.output / (source.name + ".ffmpeg.f32le")
        with reference.open("wb") as output:
            subprocess.run([ffmpeg, "-hide_banner", "-loglevel", "error", "-i", str(source), "-map", "0:a:0", "-f", "f32le", "-c:a", "pcm_f32le", "-"], stdout=output, check=True)
        native = array.array("f", pcm.read_bytes())
        independent = array.array("f", reference.read_bytes())
        if sys.byteorder != "little":
            native.byteswap()
            independent.byteswap()
        assert len(native) <= len(independent), (name, len(native), len(independent))
        errors = [a - b for a, b in zip(native, independent)]
        rms = math.sqrt(sum(e * e for e in errors) / len(errors))
        maximum = max(abs(e) for e in errors)
        report = {"fixture": name, "metadata": metadata, "elapsed_ms": task["elapsed_ms"],
                  "native_sample_values": len(native), "independent_sample_values": len(independent),
                  "rms_error": rms, "max_error": maximum,
                  "independent_tail_padding_frames": (len(independent) - len(native)) // metadata["channels"]}
        reports.append(report)
        (args.output / "decoder-parity.json").write_text(json.dumps({"comparisons": reports}, indent=2), encoding="utf-8")
        assert rms < 0.001 and maximum < 0.01, report
        assert metadata["duration_us"] == 2_000_000, report
    print(json.dumps({"comparisons": reports}, indent=2))


if __name__ == "__main__":
    main()
