"""Run device tests, retain their artifacts, and reopen the user's editor.

Tests use files/acceptance/<UUID>, never the user's studio/default project.
The runner's textual result is checked because adb may exit 0 after a failed test.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--serial", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--app-apk", type=Path, default=ROOT / "android/app/build/outputs/apk/debug/app-debug.apk")
    parser.add_argument("--test-apk", type=Path, default=ROOT / "android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk")
    parser.add_argument("--classes", default="com.motionstudio.editor.AcceptanceInstrumentedTest,com.motionstudio.editor.EditorGestureTest,com.motionstudio.editor.PropertyOverlayTest,com.motionstudio.editor.MotionInteractionTest,com.motionstudio.editor.CameraSceneAcceptanceTest,com.motionstudio.editor.ProjectReliabilityTest,com.motionstudio.editor.ReferenceFilmTest,com.motionstudio.editor.DeviceReadinessTest,com.motionstudio.editor.PreviewPerformanceTest,com.motionstudio.editor.CameraRigTest")
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

    def acceptance_roots():
        result = run("shell", "run-as", "com.motionstudio.editor", "ls", "files/acceptance", check=False)
        return set(result.stdout.decode("utf-8", errors="replace").splitlines())

    before = project_bytes()
    if before:
        (output / "user-project-before.json").write_bytes(before)
    device = {key: run("shell", "getprop", key).stdout.decode().strip()
              for key in ["ro.product.model", "ro.build.version.sdk", "ro.product.cpu.abilist"]}
    (output / "device.json").write_text(json.dumps(device, indent=2), encoding="utf-8")
    for apk in [args.app_apk, args.test_apk]:
        result = run("install", "-r", str(apk))
        text = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        if "Failure [" in text or "Success" not in text:
            raise RuntimeError(text)
    previous_roots = acceptance_roots()
    try:
        result = run("shell", "am", "instrument", "-w", "-e", "class", args.classes,
                     "com.motionstudio.editor.test/androidx.test.runner.AndroidJUnitRunner", check=False)
        text = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        (output / "instrumentation.txt").write_text(text, encoding="utf-8")
        print(re.sub(r"INSTRUMENTATION_STATUS: perfChunk=[A-Za-z0-9+/=]+", "INSTRUMENTATION_STATUS: performance report retained in instrumentation.txt", text), flush=True)
        archive = output / "acceptance.tar"
        new_roots = sorted(n for n in acceptance_roots() - previous_roots if re.fullmatch(r"[A-Za-z0-9_-]+", n))
        if new_roots:
            with archive.open("wb") as stream:
                subprocess.run([*command, "exec-out", "run-as", "com.motionstudio.editor", "tar", "-c", "-f", "-",
                                *["files/acceptance/" + n for n in new_roots]], stdout=stream, check=True)
        else:
            with tarfile.open(archive, "w"):
                pass
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
