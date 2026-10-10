#!/usr/bin/env python3
"""Build the Motion Studio desktop application.

The workspace shares its engine with the Android app, so a desktop build links
the same `motion-host` session the phone build uses. Two feature switches control
what the resulting binary can do:

* ``--ffmpeg`` enables video probing and decoding through libav. Without it the
  shell still edits and previews, and reports the missing
  capability instead of failing silently.
* ``--debug`` produces an unoptimised build for development.

Libav is discovered through ``FFMPEG_DIR`` (an include/lib installation prefix)
or pkg-config, matching ffmpeg-sys-next. Explicit include/lib paths remain
accepted for installations with that layout.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Matches android/app/build.gradle.kts so both applications report the same
# version for the same commit.
DEFAULT_VERSION = os.environ.get("MOTION_VERSION_NAME", "0.1.0-desktop.0")

# Windows builds pin the GPU to a feature level that every shipping desktop
# driver meets; Vulkan and Metal still reach their own paths through wgpu.
DEFAULT_TARGET_DIR = REPO / "target"


def run(command: list[str], env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, check=True, env=env)


def ffmpeg_environment(args: argparse.Namespace) -> dict[str, str]:
    """Resolve libav include and library directories for the linker."""
    env = dict(os.environ)
    prefix = env.get("FFMPEG_DIR")
    include = args.ffmpeg_include or env.get("FFMPEG_INCLUDE_DIR") or (str(Path(prefix) / "include") if prefix else None)
    lib = args.ffmpeg_lib or env.get("FFMPEG_LIB_DIR") or (str(Path(prefix) / "lib") if prefix else None)
    if include is None or lib is None:
        pkg_config = shutil.which("pkg-config")
        if pkg_config:
            if include is None:
                found = subprocess.run(
                    [pkg_config, "--variable=includedir", "libavcodec"],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                if found.returncode == 0:
                    include = found.stdout.strip()
            if lib is None:
                found = subprocess.run(
                    [pkg_config, "--variable=libdir", "libavcodec"],
                    capture_output=True,
                    text=True,
                    check=False,
                )
                if found.returncode == 0:
                    lib = found.stdout.strip()
    if not include or not lib:
        raise SystemExit(
            "libav was not found.\n"
            "  Install FFmpeg development libraries, or pass --ffmpeg-include and --ffmpeg-lib,\n"
            "  or build without --ffmpeg to produce a shell without video import and decoding."
        )
    env["FFMPEG_INCLUDE_DIR"] = include
    env["FFMPEG_LIB_DIR"] = lib
    include_path, lib_path = Path(include).resolve(), Path(lib).resolve()
    if include_path.name == "include" and lib_path.name == "lib" and include_path.parent == lib_path.parent:
        env["FFMPEG_DIR"] = str(include_path.parent)
        env["PATH"] = str(include_path.parent / "bin") + os.pathsep + env.get("PATH", "")
    env["PKG_CONFIG_PATH"] = os.pathsep.join(
        [str(Path(lib) / "pkgconfig"), env.get("PKG_CONFIG_PATH", "")]
    ).strip(os.pathsep)
    print(f"libav headers: {include}\nlibav libraries: {lib}", flush=True)
    return env


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--task",
        default="run",
        help="cargo task: run (default), build, or check",
    )
    parser.add_argument(
        "--ffmpeg",
        action="store_true",
        help="enable libav video import and decoding (MP4 encoder is pending)",
    )
    parser.add_argument("--ffmpeg-include", help="libav header directory")
    parser.add_argument("--ffmpeg-lib", help="libav library directory")
    parser.add_argument("--debug", action="store_true", help="unoptimised build")
    parser.add_argument("--target", help="rustc target triple, for example x86_64-apple-darwin")
    parser.add_argument(
        "--target-dir",
        default=str(DEFAULT_TARGET_DIR),
        help="cargo target directory",
    )
    parser.add_argument(
        "--package-dir",
        default=os.environ.get("MOTION_PROJECT"),
        help="project directory to open on start",
    )
    parser.add_argument("--no-default-features", action="store_true")
    parser.add_argument("--locked", action="store_true", default=True)
    parser.add_argument(
        "--report",
        help="write a JSON build report to this path",
    )
    parser.add_argument("cargo_args", nargs="*", help="extra arguments passed to cargo")
    args = parser.parse_args()

    env = dict(os.environ)
    if args.ffmpeg:
        env = ffmpeg_environment(args)

    binary = "motion-studio"
    match args.task:
        case "run":
            cargo_task = ["run", "--package", "motion-desktop", "--bin", binary]
        case "build":
            cargo_task = ["build", "--package", "motion-desktop", "--bin", binary]
        case "check":
            cargo_task = ["check", "--package", "motion-desktop"]
        case other:
            raise SystemExit(f"unknown task: {other}")

    if args.ffmpeg:
        cargo_task += ["--features", "ffmpeg"]
    if args.no_default_features:
        cargo_task += ["--no-default-features"]
    if args.locked:
        cargo_task += ["--locked"]
    if args.debug:
        cargo_task += ["--profile", "dev"]
    else:
        cargo_task += ["--release"]
    if args.target:
        cargo_task += ["--target", args.target]
    cargo_task += ["--target-dir", args.target_dir]
    if args.package_dir:
        env["MOTION_PROJECT"] = args.package_dir
    cargo_task += args.cargo_args

    run(["cargo", *cargo_task], env)

    if args.report:
        report_path = Path(args.report)
        report_path.parent.mkdir(parents=True, exist_ok=True)
        executable = None
        if args.task == "build":
            target_root = Path(args.target_dir)
            if args.target:
                target_root /= args.target
            profile = "debug" if args.debug else "release"
            windows = "windows" in args.target if args.target else sys.platform == "win32"
            filename = binary + (".exe" if windows else "")
            source = target_root / profile / filename
            destination = report_path.parent / filename
            if source.resolve() != destination.resolve():
                shutil.copy2(source, destination)
            executable = str(destination)
            shutil.copy2(REPO / "LICENSE", report_path.parent / "LICENSE")
        report = {
            "task": args.task,
            "ffmpeg": args.ffmpeg,
            "release": not args.debug,
            "target": args.target,
            "version": DEFAULT_VERSION,
            "executable": executable,
            "video_encoder": False,
        }
        report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
