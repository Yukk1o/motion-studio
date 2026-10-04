"""Install isolated, pinned Android build tools; never change system PATH."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / ".tools"
CACHE = TOOLS / "downloads"
SDK = TOOLS / "android-sdk"
GRADLE = "8.11.1"
NDK = "27.0.12077973"


def get(url):
    return urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "AEM-build/0.1"}), timeout=90)


def download(url, name, checksum=None):
    CACHE.mkdir(parents=True, exist_ok=True)
    dest = CACHE / name
    if dest.exists() and (checksum is None or hashlib.sha256(dest.read_bytes()).hexdigest() == checksum):
        print(f"Cached: {name}", flush=True)
        return dest
    temporary = dest.with_suffix(dest.suffix + ".part")
    print(f"Downloading: {name}", flush=True)
    last = time.monotonic()
    with get(url) as source, temporary.open("wb") as output:
        total = 0
        while chunk := source.read(1024 * 1024):
            output.write(chunk)
            total += len(chunk)
            if time.monotonic() - last > 10:
                print(f"  {name}: {total // (1024 * 1024)} MiB", flush=True)
                last = time.monotonic()
    if checksum and hashlib.sha256(temporary.read_bytes()).hexdigest() != checksum:
        raise RuntimeError(f"Checksum mismatch: {name}")
    temporary.replace(dest)
    return dest


def extract(archive, destination):
    destination.mkdir(parents=True, exist_ok=True)
    root = destination.resolve()
    with zipfile.ZipFile(archive) as bundle:
        for item in bundle.infolist():
            path = (root / item.filename).resolve()
            if not path.is_relative_to(root) or (item.external_attr >> 16) & 0o170000 == 0o120000:
                raise RuntimeError("Unsafe archive member")
        bundle.extractall(root)


def main():
    TOOLS.mkdir(exist_ok=True)
    # Check USB devices early, independently of the larger SDK/NDK install.
    adb = SDK / "platform-tools" / "adb.exe"
    if not adb.exists():
        extract(download("https://dl.google.com/android/repository/platform-tools-latest-windows.zip", "platform-tools.zip"), SDK)
    subprocess.run([str(adb), "devices", "-l"], check=True)
    if "--adb-only" in sys.argv:
        return
    jdk_root = TOOLS / "jdk"
    java_homes = list(jdk_root.glob("*/bin/java.exe"))
    if not java_homes:
        with get("https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture=x64&image_type=jdk&os=windows&vendor=eclipse") as result:
            binary = json.load(result)[0]["binary"]["package"]
        extract(download(binary["link"], binary["name"], binary["checksum"]), jdk_root)
        java_homes = list(jdk_root.glob("*/bin/java.exe"))
    java_home = java_homes[0].parents[1]
    gradle_dir = TOOLS / f"gradle-{GRADLE}"
    if not (gradle_dir / "bin" / "gradle.bat").exists():
        base = f"https://services.gradle.org/distributions/gradle-{GRADLE}-bin.zip"
        with get(base + ".sha256") as result:
            checksum = result.read().decode().strip()
        extract(download(base, f"gradle-{GRADLE}.zip", checksum), TOOLS)
    manager = SDK / "cmdline-tools" / "latest" / "bin" / "sdkmanager.bat"
    if not manager.exists():
        package = download("https://dl.google.com/android/repository/commandlinetools-win-15859902_latest.zip", "commandlinetools.zip", "90ae805d20434428bffcb699c290860f19bb5f66a67e6b330067e3de801fb04a")
        staging = TOOLS / "cmdline-staging"
        extract(package, staging)
        manager.parent.parent.parent.mkdir(parents=True, exist_ok=True)
        (staging / "cmdline-tools").rename(manager.parent.parent)
    env = os.environ.copy()
    env["JAVA_HOME"] = str(java_home)
    env["ANDROID_HOME"] = str(SDK)
    env["PATH"] = str(java_home / "bin") + os.pathsep + env["PATH"]
    subprocess.run([str(manager), f"--sdk_root={SDK}", "platform-tools", "platforms;android-35", "build-tools;35.0.0", f"ndk;{NDK}"], input="y\n" * 100, text=True, env=env, check=True)
    subprocess.run(["rustup", "target", "add", "aarch64-linux-android"], check=True)
    config = {"java_home": str(java_home), "sdk": str(SDK), "gradle": str(gradle_dir / "bin" / "gradle.bat"), "ndk": str(SDK / "ndk" / NDK)}
    (TOOLS / "environment.json").write_text(json.dumps(config, indent=2), encoding="utf-8")
    print("Build tools ready: .tools/environment.json", flush=True)


if __name__ == "__main__":
    main()

