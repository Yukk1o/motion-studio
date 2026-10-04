"""Build Motion Studio from any worktree, sharing the existing isolated toolchain."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    parser=argparse.ArgumentParser()
    parser.add_argument("--abis",default="arm64-v8a,x86_64")
    parser.add_argument("--rust-only",action="store_true")
    parser.add_argument("--diagnostics",action="store_true",help="Enable opt-in GPU fault injection for the debug acceptance build")
    parser.add_argument("--task",nargs="+",default=["assembleDebug"])
    args=parser.parse_args()
    diagnostics=args.diagnostics or any(task.endswith("AndroidTest") for task in args.task)
    if diagnostics and any("Release" in task for task in args.task):
        raise RuntimeError("Release tasks must be built without GPU diagnostic injection")
    shared=next((p for p in [ROOT,*ROOT.parents] if (p/".tools/environment.json").exists()),None)
    if shared is None:
        raise RuntimeError("Run tools/bootstrap_android.py in the main worktree first")
    config=json.loads((shared/".tools/environment.json").read_text(encoding="utf-8"))
    toolchain=Path(config["ndk"])/"toolchains/llvm/prebuilt/windows-x86_64/bin"
    env=os.environ.copy()
    env["JAVA_HOME"]=config["java_home"]
    env["ANDROID_HOME"]=config["sdk"]
    env["GRADLE_USER_HOME"]=str(shared/".tools/gradle-cache")
    env["CARGO_TARGET_DIR"]=str(shared/"target")
    env["PATH"]=str(Path(config["java_home"])/"bin")+os.pathsep+str(toolchain)+os.pathsep+env["PATH"]
    installed=subprocess.check_output(["rustup","target","list","--installed"],text=True)
    targets={"arm64-v8a":("aarch64-linux-android","aarch64-linux-android29"),
        "x86_64":("x86_64-linux-android","x86_64-linux-android29")}
    for abi in args.abis.split(","):
        target,clang_target=targets[abi]
        if target not in installed:
            subprocess.run(["rustup","target","add",target],check=True,env=env)
        prefix="CARGO_TARGET_"+target.replace("-","_").upper()
        build_env=env.copy()
        build_env[prefix+"_LINKER"]=str(toolchain/"clang.exe")
        build_env[prefix+"_RUSTFLAGS"]=f"-Clink-arg=--target={clang_target} -Clink-arg=-Wl,-z,max-page-size=16384"
        print(f"Building native runtime for {abi}",flush=True)
        features=["--features","diagnostics"] if diagnostics else []
        subprocess.run(["cargo","build","--locked","-p","aem-android","--target",target,"--release",*features],cwd=ROOT,env=build_env,check=True)
        destination=ROOT/"android/app/src/main/jniLibs"/abi
        destination.mkdir(parents=True,exist_ok=True)
        shutil.copy2(shared/"target"/target/"release/libaem_android.so",destination/"libmotion_engine.so")
    if not args.rust_only:
        android=ROOT/"android"
        sdk=Path(config["sdk"]).as_posix().replace(":","\\:")
        (android/"local.properties").write_text("sdk.dir="+sdk+"\n",encoding="utf-8")
        subprocess.run([config["gradle"],"--no-daemon","--console=plain",*args.task],cwd=android,env=env,check=True)
        print("Android build completed",flush=True)


if __name__=="__main__":
    main()
