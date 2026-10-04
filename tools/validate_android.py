"""Run device tests, retain their artifacts, and reopen the user's editor.

Tests use files/acceptance/<UUID>, never the user's studio/default project.
The runner's textual result is checked because adb may exit 0 after a failed test.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--classes", default="com.motionstudio.editor.AcceptanceInstrumentedTest,com.motionstudio.editor.EditorGestureTest,com.motionstudio.editor.PropertyOverlayTest")
    args = parser.parse_args()
    shared = next(p for p in [ROOT, *ROOT.parents] if (p / ".tools/environment.json").exists())
    config = json.loads((shared / ".tools/environment.json").read_text(encoding="utf-8"))
    adb = Path(config["sdk"]) / "platform-tools/adb.exe"
    command = [str(adb), "-s", args.serial]
    output = args.output or shared / "artifacts/device-validation" / args.serial
    output.mkdir(parents=True, exist_ok=True)

    def run(*arguments, check=True):
        return subprocess.run([*command, *arguments], capture_output=True, check=check)

    def project_bytes():
        result = run("exec-out", "run-as", "com.motionstudio.editor", "cat", "files/studio/default/project.json", check=False)
        return result.stdout if result.returncode == 0 and result.stdout.startswith(b"{") else None

    before = project_bytes()
    if before:
        (output / "user-project-before.json").write_bytes(before)
    device = {key: run("shell", "getprop", key).stdout.decode().strip()
              for key in ["ro.product.model", "ro.build.version.sdk", "ro.product.cpu.abilist"]}
    (output / "device.json").write_text(json.dumps(device, indent=2), encoding="utf-8")
    for apk in [ROOT / "android/app/build/outputs/apk/debug/app-debug.apk",
                ROOT / "android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"]:
        result = run("install", "-r", str(apk))
        text = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        if "Failure [" in text or "Success" not in text:
            raise RuntimeError(text)
    try:
        result = run("shell", "am", "instrument", "-w", "-e", "class", args.classes,
                     "com.motionstudio.editor.test/androidx.test.runner.AndroidJUnitRunner", check=False)
        text = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        (output / "instrumentation.txt").write_text(text, encoding="utf-8")
        print(text, flush=True)
        archive = output / "acceptance.tar"
        with archive.open("wb") as stream:
            subprocess.run([*command, "exec-out", "run-as", "com.motionstudio.editor", "tar", "-c", "-f", "-", "files/acceptance"], stdout=stream, check=True)
        with tarfile.open(archive) as bundle:
            bundle.extractall(output / "acceptance", filter="data")
        after = project_bytes()
        preserved = before is None or before == after
        (output / "user-project-preservation.json").write_text(json.dumps({
            "preserved": preserved,
            "beforeSha256": hashlib.sha256(before).hexdigest() if before else None,
            "afterSha256": hashlib.sha256(after).hexdigest() if after else None,
        }, indent=2), encoding="utf-8")
        if result.returncode != 0 or "OK (" not in text or "FAILURES!!!" in text:
            raise RuntimeError("Android tests failed; see instrumentation.txt")
        if not preserved:
            raise RuntimeError("The user's saved project changed during isolated tests")
    finally:
        run("shell", "am", "start", "-n", "com.motionstudio.editor/.MainActivity", check=False)


if __name__ == "__main__":
    main()
