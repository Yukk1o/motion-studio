"""Prepare signing, validate APKs and publish gated GitHub releases."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import zipfile

ROOT=Path(__file__).resolve().parents[1]
SEMVER=re.compile(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?")


def release_kind(ref):
    if ref=="refs/heads/main": return "preview","0.1.0"
    if ref.startswith("refs/tags/"):
        tag=ref.removeprefix("refs/tags/")
        if SEMVER.fullmatch(tag): return "release",tag[1:]
    raise ValueError("Publishing requires main or a vMAJOR.MINOR.PATCH[-prerelease] tag")


def version_code(raw):
    if not raw.isascii() or not raw.isdecimal() or not 1<=int(raw)<=2100000000:
        raise ValueError("Android version code must be 1..2100000000")
    return int(raw)


def run(*args,**kwargs):
    return subprocess.run(args,check=True,text=True,encoding="utf-8",**kwargs)


def prepare():
    kind,version=release_kind(os.environ["GITHUB_REF"])
    code=version_code(os.environ["GITHUB_RUN_NUMBER"])
    prefix=kind.upper()
    names=[prefix+suffix for suffix in ("_KEYSTORE_BASE64","_KEYSTORE_PASSWORD","_KEY_ALIAS","_KEY_PASSWORD")]
    values=[os.environ.get(name,"") for name in names]
    if not all(values): raise RuntimeError("Missing signing secrets: "+", ".join(name for name,value in zip(names,values) if not value))
    data=base64.b64decode(values[0],validate=True)
    if not data or len(data)>1024*1024: raise ValueError("Invalid keystore size")
    folder=Path(os.environ["RUNNER_TEMP"])/"motion-signing"
    folder.mkdir(mode=0o700,exist_ok=True)
    key=folder/"keystore.p12"
    key.write_bytes(data);key.chmod(0o600)
    additions={"MOTION_KEYSTORE_FILE":str(key),"MOTION_KEYSTORE_PASSWORD":values[1],"MOTION_KEY_ALIAS":values[2],"MOTION_KEY_PASSWORD":values[3],"MOTION_VERSION_CODE":str(code),"MOTION_VERSION_NAME":version}
    env=os.environ|additions
    run("keytool","-list","-keystore",str(key),"-alias",values[2],"-storepass:env","MOTION_KEYSTORE_PASSWORD",env=env,stdout=subprocess.DEVNULL)
    if any("\n" in value or "\r" in value for value in additions.values()): raise ValueError("Invalid multiline signing configuration")
    with Path(os.environ["GITHUB_ENV"]).open("a",encoding="utf-8") as output:
        for name,value in additions.items(): output.write(name+"="+value+"\n")
    print("Signing configured for",kind,"version",version,"code",code)


def validate_badging(text,code,version):
    package=re.search(r"^package: name='([^']+)' versionCode='([^']+)' versionName='([^']+)'",text,re.MULTILINE)
    if not package or package.groups()!=("com.motionstudio.editor",str(code),version):
        raise ValueError("APK package/version differs from this release")
    if re.search(r"^application-debuggable",text,re.MULTILINE): raise ValueError("A debuggable APK cannot be published")


def package():
    kind,version=release_kind(os.environ["GITHUB_REF"])
    code=version_code(os.environ["GITHUB_RUN_NUMBER"])
    if kind=="preview": version+="-preview."+str(code)
    apk=ROOT/"android/app/build/outputs/apk"/kind/("app-"+kind+".apk")
    sdk=Path(os.environ["ANDROID_HOME"])/"build-tools/35.0.0"
    run(str(sdk/"apksigner"),"verify","--verbose","--print-certs",str(apk))
    text=run(str(sdk/"aapt"),"dump","badging",str(apk),capture_output=True).stdout
    validate_badging(text,code,version)
    with zipfile.ZipFile(apk) as archive:
        for abi in ["arm64-v8a","x86_64"]:
            if archive.getinfo("lib/"+abi+"/libmotion_engine.so").file_size<1024:
                raise ValueError("Missing native runtime for "+abi)
    folder=ROOT/"artifacts/release";folder.mkdir(parents=True,exist_ok=True)
    name="MotionStudio-"+("preview" if kind=="preview" else version)+".apk"
    output=folder/name;output.write_bytes(apk.read_bytes())
    digest=hashlib.sha256(output.read_bytes()).hexdigest()
    (folder/"SHA256SUMS").write_text(digest+"  "+name+"\n",encoding="utf-8")
    (folder/"build-info.json").write_text(json.dumps({"commit":os.environ["GITHUB_SHA"],"ref":os.environ["GITHUB_REF"],"versionCode":code,"versionName":version,"kind":kind,"apk":name,"sha256":digest,"abis":["arm64-v8a","x86_64"],"run":os.environ["GITHUB_RUN_ID"]},indent=2)+"\n",encoding="utf-8")
    print("Verified signed APK:",name,digest)


def gh(*args,check=True):
    return subprocess.run(["gh",*args],check=check,capture_output=True,text=True,encoding="utf-8")


def publish():
    folder=ROOT/"artifacts/release"
    info=json.loads((folder/"build-info.json").read_text(encoding="utf-8"))
    kind,version=release_kind(os.environ["GITHUB_REF"])
    if info["commit"]!=os.environ["GITHUB_SHA"] or info["ref"]!=os.environ["GITHUB_REF"] or info["kind"]!=kind:
        raise ValueError("Release artifact does not belong to this workflow revision")
    apk=folder/info["apk"]
    if apk.parent!=folder or hashlib.sha256(apk.read_bytes()).hexdigest()!=info["sha256"]:
        raise ValueError("Release artifact checksum mismatch")
    repo=os.environ["GITHUB_REPOSITORY"];commit=info["commit"]
    if not re.fullmatch(r"[0-9a-f]{40}",commit): raise ValueError("Invalid commit")
    tag="preview" if kind=="preview" else "v"+version
    prerelease=kind=="preview" or "-" in version
    if kind=="preview":
        head=gh("api","repos/"+repo+"/git/ref/heads/main","--jq",".object.sha").stdout.strip()
        if head!=commit:
            print("A newer main commit exists; this build will not replace its preview")
            return
        exists=gh("api","repos/"+repo+"/git/ref/tags/preview",check=False)
        if exists.returncode==0:
            gh("api","--method","PATCH","repos/"+repo+"/git/refs/tags/preview","-f","sha="+commit,"-F","force=true")
        else:
            if "404" not in exists.stderr: raise RuntimeError(exists.stderr)
            gh("api","--method","POST","repos/"+repo+"/git/refs","-f","ref=refs/tags/preview","-f","sha="+commit)
    existing=gh("release","view",tag,"--repo",repo,"--json","isDraft,url",check=False)
    if existing.returncode!=0 and "not found" not in existing.stderr.lower(): raise RuntimeError(existing.stderr)
    release=json.loads(existing.stdout) if existing.returncode==0 else None
    if kind=="release" and release and not release["isDraft"]:
        with tempfile.TemporaryDirectory() as temporary:
            gh("release","download",tag,"--repo",repo,"--pattern","build-info.json","--dir",temporary)
            previous=json.loads((Path(temporary)/"build-info.json").read_text(encoding="utf-8"))
            if previous["commit"]!=commit or previous["sha256"]!=info["sha256"]:
                raise RuntimeError("A published version cannot be replaced; create a new version tag")
        print("This exact release is already published:",release["url"])
        return
    notes=folder/"release-notes.md"
    notes.write_text(("main 分支自动预览构建。\n\n" if kind=="preview" else "版本标签自动构建。\n\n")+"版本："+info["versionName"]+"\n\n提交：`"+commit+"`\n\n包含 arm64-v8a 与 x86_64。APK 已验证签名、包名、版本及非调试状态。Rust/GPU、Android 构建和指定界面交互检查通过后发布；模拟器检查不替代真机性能与完整媒体验收。下载 APK 后可使用 SHA256SUMS 校验文件。\n",encoding="utf-8")
    title="自动预览版" if kind=="preview" else "Motion Studio "+version
    if not release:
        gh("release","create",tag,"--repo",repo,"--verify-tag","--title",title,"--notes-file",str(notes),"--draft")
    gh("release","upload",tag,*map(str,[apk,folder/"SHA256SUMS",folder/"build-info.json"]),"--repo",repo,"--clobber")
    gh("release","edit",tag,"--repo",repo,"--title",title,"--notes-file",str(notes),"--draft=false","--prerelease="+str(prerelease).lower(),"--latest="+str(not prerelease).lower())
    print(gh("release","view",tag,"--repo",repo,"--json","url","--jq",".url").stdout.strip())


if __name__=="__main__":
    parser=argparse.ArgumentParser();parser.add_argument("action",choices=["prepare","package","publish"])
    {"prepare":prepare,"package":package,"publish":publish}[parser.parse_args().action]()
