#!/usr/bin/env python3
"""Pack declared Motion Studio effect files; optional full SDK validation."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tempfile
import zipfile

MAX_PACKAGE = 16 * 1024 * 1024
MAX_SOURCE = 256 * 1024
MAX_MANIFEST = 512 * 1024


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def relative_name(value: object) -> str:
    require(isinstance(value, str) and bool(value), "file reference must be a nonempty string")
    require(not any(c in value for c in ("\\", ":", "\0")), f"invalid package path: {value}")
    parts = value.split("/")
    require(not PurePosixPath(value).is_absolute() and all(p not in ("", ".", "..") for p in parts), f"invalid package path: {value}")
    return value


def finite_json(value: object) -> None:
    if isinstance(value, float):
        require(math.isfinite(value), "manifest contains a non-finite number")
    elif isinstance(value, dict):
        for child in value.values():
            finite_json(child)
    elif isinstance(value, list):
        for child in value:
            finite_json(child)


def pack(source: Path, output: Path, validator: Path | None = None) -> dict:
    base = source.resolve(strict=True)
    require(base.is_dir(), "source must be a directory")
    manifest_path = (base / "manifest.json").resolve(strict=True)
    require(manifest_path.is_relative_to(base), "manifest resolves outside source")
    require(manifest_path.stat().st_size <= MAX_MANIFEST, "manifest exceeds 512 KiB")
    with manifest_path.open("rb") as stream:
        raw_manifest = stream.read(MAX_MANIFEST + 1)
    require(len(raw_manifest) <= MAX_MANIFEST, "manifest exceeds 512 KiB")
    manifest = json.loads(raw_manifest, parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"non-finite JSON: {value}")))
    finite_json(manifest)
    require(isinstance(manifest, dict) and manifest.get("format_version") == 1, "unsupported package format")
    sdk = manifest.get("sdk_version")
    require(type(sdk) is int and 1 <= sdk <= 6, "sdk_version must be 1–6")
    require(isinstance(manifest.get("id"), str) and re.fullmatch(r"[A-Za-z0-9_.-]+", manifest["id"]) is not None, "invalid plugin ID")
    require(isinstance(manifest.get("version"), str) and re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", manifest["version"]) is not None, "version must use SemVer")
    effects = manifest.get("effects")
    require(isinstance(effects, list) and 1 <= len(effects) <= (128 if sdk >= 6 else 64), "package exceeds SDK effect count")
    names = {"manifest.json"}
    ids = set()
    for effect in effects:
        require(isinstance(effect, dict) and isinstance(effect.get("id"), str), "effect requires an ID")
        require(effect["id"] not in ids, "duplicate effect ID")
        ids.add(effect["id"])
        params = effect.get("params", [])
        passes = effect.get("passes", [])
        resources = effect.get("resources", [])
        require(isinstance(params, list) and len(params) <= 32, "effect exceeds 32 parameters")
        require(isinstance(passes, list) and 1 <= len(passes) <= 8, "effect requires 1–8 passes")
        require(isinstance(resources, list) and len(resources) <= 4, "effect exceeds 4 resources")
        if "image_input" in effect.get("required_capabilities", []):
            require(sdk >= 6 and len(params) <= 30 and len(resources) == 1 and effect.get("renderer", "image") == "image", "image_input requires SDK 6, one resource and slots 30/31 reserved")
        for shader_pass in passes:
            require(isinstance(shader_pass, dict), "invalid pass")
            name = relative_name(shader_pass.get("shader"))
            require(name.endswith(".wgsl"), "pass source must be WGSL")
            names.add(name)
        for resource in resources:
            names.add(relative_name(resource))
        editor = effect.get("editor")
        if editor is not None:
            require(isinstance(editor, dict) and isinstance(editor.get("files", []), list), "invalid editor files")
            names.update(relative_name(name) for name in editor.get("files", []))
    require(len(names) <= 256, "package exceeds 256 files")
    require(len({name.lower() for name in names}) == len(names), "duplicate case-insensitive file names")
    files = []
    total = 0
    for name in sorted(names):
        path = (base / name).resolve(strict=True)
        require(path.is_relative_to(base) and path.is_file(), f"resource resolves outside source or is not a file: {name}")
        size = path.stat().st_size
        require(size <= (MAX_MANIFEST if name == "manifest.json" else MAX_SOURCE if name.endswith(".wgsl") else MAX_PACKAGE), f"file exceeds budget: {name}")
        total += size
        require(total <= MAX_PACKAGE, "expanded package exceeds 16 MiB")
        if name == "manifest.json":
            data = raw_manifest
        else:
            with path.open("rb") as stream:
                data = stream.read(size + 1)
        require(len(data) == size, f"resource changed during packaging: {name}")
        files.append((name, data))
    destination = output.resolve()
    require(destination.suffix.lower() == ".msfx", "output must use .msfx")
    destination.parent.mkdir(parents=True, exist_ok=True)
    handle = tempfile.NamedTemporaryFile(prefix=".msfx-", suffix=".tmp", dir=destination.parent, delete=False)
    temporary = Path(handle.name)
    handle.close()
    try:
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            for name, data in files:
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.create_system = 3
                info.external_attr = 0o100644 << 16
                archive.writestr(info, data)
        require(temporary.stat().st_size <= MAX_PACKAGE, "compressed package exceeds 16 MiB")
        if validator is not None:
            subprocess.run([str(validator.resolve(strict=True)), "check", str(temporary)], check=True)
        digest = hashlib.sha256(temporary.read_bytes()).hexdigest()
        os.replace(temporary, destination)
        return {"path": str(destination), "plugin": manifest["id"], "version": manifest["version"], "sha256": digest, "files": len(files), "expanded_bytes": total, "validation": "sdk" if validator else "structure_only"}
    finally:
        temporary.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--validator", type=Path, help="existing effect_tool executable for complete SDK validation")
    args = parser.parse_args()
    try:
        print(json.dumps(pack(args.source, args.output, args.validator), ensure_ascii=False, indent=2))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"pack_msfx: {error}\n")


if __name__ == "__main__":
    main()
