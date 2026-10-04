"""Exercise the real Android window at different dp sizes and font settings.

Uses isolated acceptance projects. Restores every display/font override even
when a test fails; emulator evidence must not be reported as physical phones.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tarfile
import time

ROOT = Path(__file__).resolve().parents[1]
PROFILES = {
    "portrait-320": ("1080x1920", 540, 1.0),
    "portrait-360-large": ("1080x1920", 480, 1.3),
    "portrait-360-largest": ("1080x1920", 480, 1.8),
    "portrait-411": ("1080x1920", 420, 1.0),
    "landscape-640": ("1920x1080", 480, 1.0),
    "landscape-640-large": ("1920x1080", 480, 1.3),
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profiles", nargs="+", choices=PROFILES, default=list(PROFILES))
    args = parser.parse_args()
    shared = next(p for p in [ROOT, *ROOT.parents] if (p / ".tools/environment.json").exists())
    config = json.loads((shared / ".tools/environment.json").read_text(encoding="utf-8"))
    adb = [str(Path(config["sdk"]) / "platform-tools/adb.exe"), "-s", args.serial]
    args.output.mkdir(parents=True, exist_ok=True)

    def run(*items, check=True, timeout=180):
        return subprocess.run([*adb, *map(str, items)], capture_output=True, check=check, timeout=timeout)

    def shell(*items):
        return run("shell", *items).stdout.decode("utf-8", errors="replace").strip()

    def roots():
        result = run("shell", "run-as", "com.motionstudio.editor", "ls", "files/acceptance", check=False)
        return set(result.stdout.decode().splitlines())

    original = {"size": shell("wm", "size"), "density": shell("wm", "density"),
                "fontScale": shell("settings", "get", "system", "font_scale")}
    (args.output / "settings-before.json").write_text(json.dumps(original, indent=2), encoding="utf-8")
    results = []
    try:
        for name in args.profiles:
            size, density, font = PROFILES[name]
            shell("wm", "size", size)
            shell("wm", "density", density)
            shell("settings", "put", "system", "font_scale", font)
            time.sleep(1)
            before = roots()
            print(f"Testing {name}: {size}, {density} dpi, font {font}", flush=True)
            folder = args.output / name
            folder.mkdir(parents=True, exist_ok=True)
            result = run("shell", "am", "instrument", "-w", "-e", "class",
                         "com.motionstudio.editor.DeviceReadinessTest#a10LayoutProfileKeepsControlsTouchableAndNumericInputPrecise",
                         "com.motionstudio.editor.test/androidx.test.runner.AndroidJUnitRunner")
            log = (result.stdout + result.stderr).decode("utf-8", errors="replace")
            (folder / "instrumentation.txt").write_text(log, encoding="utf-8")
            new = sorted(n for n in roots() - before if re.fullmatch(r"device-[a-f0-9-]+", n))
            if new:
                archive = folder / "acceptance.tar"
                with archive.open("wb") as stream:
                    subprocess.run([*adb, "exec-out", "run-as", "com.motionstudio.editor", "tar", "-c", "-f", "-",
                                    *["files/acceptance/" + n for n in new]], stdout=stream, check=True)
                with tarfile.open(archive) as bundle:
                    bundle.extractall(folder / "acceptance", filter="data")
            reports = list(folder.rglob("layout-profile-report.json"))
            passed = result.returncode == 0 and "OK (1 test)" in log and "FAILURES!!!" not in log
            observed = json.loads(reports[-1].read_text(encoding="utf-8")) if reports else None
            if observed:
                passed = passed and abs(observed["fontScale"] - font) < 0.01
                passed = passed and observed["densityDpi"] == density
                passed = passed and (observed["screenWidthDp"] > observed["screenHeightDp"]) == (name.startswith("landscape"))
            else:
                passed = False
            results.append({"profile": name, "passed": passed, "observed": observed})
            print(log, flush=True)
    finally:
        for item in ["size", "density"]:
            override = re.search(r"Override (?:size|density):\s*(\S+)", original[item])
            shell("wm", item, override.group(1) if override else "reset")
        if original["fontScale"] == "null":
            shell("settings", "delete", "system", "font_scale")
        else:
            shell("settings", "put", "system", "font_scale", original["fontScale"])
        restored = {"size": shell("wm", "size"), "density": shell("wm", "density"),
                    "fontScale": shell("settings", "get", "system", "font_scale")}
        (args.output / "settings-after.json").write_text(json.dumps(restored, indent=2), encoding="utf-8")
        (args.output / "summary.json").write_text(json.dumps({"serial": args.serial,
            "settingsRestored": restored == original, "profiles": results}, indent=2), encoding="utf-8")
        run("shell", "am", "start", "-n", "com.motionstudio.editor/.MainActivity", check=False)
    if restored != original or len(results) != len(args.profiles) or not all(p["passed"] for p in results):
        raise RuntimeError("Layout checks failed; see profile evidence and summary.json")


if __name__ == "__main__":
    main()
