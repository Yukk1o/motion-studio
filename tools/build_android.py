"""Build Motion Studio with shared tools and worktree-isolated native outputs."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]


def tool_config():
    shared=next((p for p in [ROOT,*ROOT.parents] if (p/".tools/environment.json").exists()),None)
    if shared is not None:
        return shared,json.loads((shared/".tools/environment.json").read_text(encoding="utf-8"))
    sdk=os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    java=os.environ.get("JAVA_HOME")
    gradle=shutil.which("gradle")
    if not (sdk and java and gradle):
        raise RuntimeError("Run tools/bootstrap_android.py, or configure JAVA_HOME, ANDROID_HOME and Gradle")
    ndk=os.environ.get("ANDROID_NDK_HOME") or str(Path(sdk)/"ndk/27.0.12077973")
    return ROOT,{"java_home":java,"sdk":sdk,"gradle":gradle,"ndk":ndk}


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    parser=argparse.ArgumentParser()
    parser.add_argument("--abis",default="arm64-v8a,x86_64")
    parser.add_argument("--rust-only",action="store_true")
    parser.add_argument("--diagnostics",action="store_true",help="Enable opt-in GPU fault injection for the debug acceptance build")
    parser.add_argument("--effects-acceptance",action="store_true",help="Use an isolated debug application ID for effects tests")
    parser.add_argument("--task",nargs="+",default=["assembleDebug"])
    parser.add_argument("--target-dir",type=Path,help="Override the native output directory")
    parser.add_argument("--codegen-units",type=int,choices=range(1,257),help="Override project-crate release codegen units for memory-constrained local builds")
    args=parser.parse_args()
    benchmark=any("Benchmark" in task for task in args.task)
    diagnostics=args.diagnostics or any(task.endswith("AndroidTest") and "Benchmark" not in task for task in args.task)
    if diagnostics and any(any(kind in task for kind in ("Release","Benchmark","Preview")) for task in args.task):
        raise RuntimeError("Release/Benchmark/Preview tasks must be built without GPU diagnostic injection")
    targets={"arm64-v8a":("aarch64-linux-android","aarch64-linux-android29"),
        "x86_64":("x86_64-linux-android","x86_64-linux-android29")}
    abis=args.abis.split(",")
    if not abis or any(abi not in targets for abi in abis):
        raise RuntimeError("Supported ABIs: arm64-v8a,x86_64")
    shared,config=tool_config()
    target_dir=args.target_dir.resolve() if args.target_dir else shared/"target"/("main" if ROOT==shared else ROOT.name)
    host={"win32":"windows-x86_64","linux":"linux-x86_64","darwin":"darwin-x86_64"}.get(sys.platform)
    if host is None: raise RuntimeError("Unsupported NDK host platform")
    extension=".exe" if sys.platform=="win32" else ""
    toolchain=Path(config["ndk"])/"toolchains/llvm/prebuilt"/host/"bin"
    # Windows NDK distributions do not always include libclang for bindgen.
    # Keep the pinned build-only wheel in the shared private tool directory.
    if sys.platform=="win32":
        libclang=shared/".tools/python-libclang/clang/native"
        if not (libclang/"libclang.dll").exists():
            subprocess.run([sys.executable,"-m","pip","install","--no-cache-dir","--target",str(shared/".tools/python-libclang"),"libclang==18.1.1"],check=True)
    else:
        libclang=Path(os.environ.get("LIBCLANG_PATH",str(toolchain.parent/"lib")))
    env=os.environ.copy()
    env["JAVA_HOME"]=config["java_home"]
    env["ANDROID_HOME"]=config["sdk"]
    env["GRADLE_USER_HOME"]=os.environ.get("GRADLE_USER_HOME",str(shared/".tools/gradle-cache"))
    env["CARGO_TARGET_DIR"]=str(target_dir)
    env["PATH"]=str(Path(config["java_home"])/"bin")+os.pathsep+str(toolchain)+os.pathsep+env["PATH"]
    installed=subprocess.check_output(["rustup","target","list","--installed"],text=True)
    for abi in abis:
        target,clang_target=targets[abi]
        if target not in installed:
            subprocess.run(["rustup","target","add",target],check=True,env=env)
        prefix="CARGO_TARGET_"+target.replace("-","_").upper()
        build_env=env.copy()
        build_env[prefix+"_LINKER"]=str(toolchain/("clang"+extension))
        build_env[prefix+"_RUSTFLAGS"]=f"-Clink-arg=--target={clang_target} -Clink-arg=-Wl,-z,max-page-size=16384"
        # Native dependencies (QuickJS and the JS parser's stack guard) use cc-rs.
        # Explicit target flags prevent it selecting the host MSVC compiler.
        cc_target=target.replace("-","_")
        build_env["CC_"+cc_target]=str(toolchain/("clang"+extension))
        build_env["AR_"+cc_target]=str(toolchain/("llvm-ar"+extension))
        build_env["CFLAGS_"+cc_target]=f"--target={clang_target}"
        build_env["LIBCLANG_PATH"]=str(libclang)
        sysroot=(toolchain.parent/"sysroot").as_posix()
        build_env["BINDGEN_EXTRA_CLANG_ARGS"]=f'--target={clang_target} --sysroot="{sysroot}"'
        print(f"Building native runtime for {abi}",flush=True)
        features=["--features","diagnostics"] if diagnostics else []
        overrides=[]
        if args.codegen_units:
            for package in ("aem-core","aem-effects","aem-render","aem-android"):
                overrides.extend(["--config",f'profile.release.package.{package}.codegen-units={args.codegen_units}'])
        subprocess.run(["cargo","build","--locked","-p","aem-android","--target",target,"--release",*features,*overrides],cwd=ROOT,env=build_env,check=True)
        destination=ROOT/"android/app/src/main/jniLibs"/abi
        destination.mkdir(parents=True,exist_ok=True)
        shutil.copy2(target_dir/target/"release/libaem_android.so",destination/"libmotion_engine.so")
    if not args.rust_only:
        android=ROOT/"android"
        sdk=Path(config["sdk"]).as_posix().replace(":","\\:")
        (android/"local.properties").write_text("sdk.dir="+sdk+"\n",encoding="utf-8")
        properties=["-PperformanceTest=true"] if benchmark else []
        if args.effects_acceptance: properties.append("-PeffectsAcceptance=true")
        subprocess.run([config["gradle"],"--no-daemon","--console=plain",*properties,*args.task],cwd=android,env=env,check=True)
        print("Android build completed",flush=True)


if __name__=="__main__":
    main()
