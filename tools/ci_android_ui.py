"""Run the deterministic editor interaction suite on an isolated CI emulator."""
import json
from pathlib import Path
import re
import subprocess

ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/"android/app/src/androidTest/java/com/motionstudio/editor"
OUTPUT=ROOT/"artifacts/android-ui";OUTPUT.mkdir(parents=True,exist_ok=True)
classes=["PropertyOverlayTest","FrontendLayerControlsTest#separatedKeyDragUsesCompositionTimeAndOnlyCapturedAxis"]
classes += [name for name in ["EditorInertiaTest","LayerSelectionTest","TimelineKeyGestureTest","TransportLayerActionsTest","EffectPreviewBackendTest"] if (SOURCE/(name+".kt")).exists()]
expected=sum(1 if "#" in name else len(re.findall(r"@Test\b",(SOURCE/(name+".kt")).read_text(encoding="utf-8"))) for name in classes)
subprocess.run(["adb","shell","wm","size","1080x1920"],check=True)
subprocess.run(["adb","shell","wm","density","420"],check=True)
for apk in ["debug/app-debug.apk","androidTest/debug/app-debug-androidTest.apk"]:
    subprocess.run(["adb","install","-r",str(ROOT/"android/app/build/outputs/apk"/apk)],check=True)
command=["adb","shell","am","instrument","-w","-e","class",",".join("com.motionstudio.editor."+name for name in classes),"com.motionstudio.editor.effectsacceptance.test/androidx.test.runner.AndroidJUnitRunner"]
with (OUTPUT/"instrumentation.log").open("w",encoding="utf-8") as log:
    process=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,encoding="utf-8")
    for line in process.stdout: print(line,end="",flush=True);log.write(line);log.flush()
    process.wait()
with (OUTPUT/"logcat.txt").open("wb") as log:
    subprocess.run(["adb","logcat","-d","-t","5000"],stdout=log,check=True)
text=(OUTPUT/"instrumentation.log").read_text(encoding="utf-8")
match=re.search(r"OK \((\d+) tests?\)",text)
if process.returncode or not match or int(match[1])!=expected:
    raise RuntimeError("Android interaction suite failed; see instrumentation.log and logcat.txt")
(OUTPUT/"report.json").write_text(json.dumps({"tests":expected,"classes":classes,"api":35,"display":"1080x1920 @420 dpi","device":"GitHub Actions emulator"},indent=2)+"\n",encoding="utf-8")
