"""Read-only, partial AEP inventory. This is not an AEP importer.

No expressions are executed, AE is not launched, and source files are not changed.
Stored records cannot establish whether an effect is enabled or root-reachable.
Use ae_export_project.jsx in a compatible AE instance for that information.
"""
from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass, field
import hashlib
import json
import mmap
from pathlib import Path
import struct
import subprocess


MAX_PROJECT_BYTES = 512 * 1024 * 1024
MAX_CHUNKS = 2_000_000
MAX_DEPTH = 64
MEDIA_SUFFIXES = {".png", ".jpg", ".jpeg", ".mp4", ".mov", ".mp3", ".wav", ".webm"}
EXPRESSION_FEATURES = {
    "cross_property": ("effect(", ".layer(", "propertyGroup"),
    "per_character": ("textIndex", "textTotal"),
    "property_history": ("valueAtTime", "velocityAtTime", "nearestKey"),
    "loops": ("loopOut(", "loopIn("),
    "random": ("seedRandom(", "random(", "noise("),
}


@dataclass
class Chunk:
    tag: str
    offset: int
    size: int
    kind: str = ""
    children: list[Chunk] = field(default_factory=list)


def parse_chunks(data):
    """Bound every read to its enclosing container, preserving opaque regions."""
    if len(data) < 12 or data[:4] != b"RIFX" or data[8:12] != b"Egg!":
        raise ValueError("expected a big-endian RIFX/Egg! AEP; AEPX is not supported")
    declared_end = 8 + struct.unpack_from(">I", data, 4)[0]
    if declared_end > len(data) or declared_end < 12:
        raise ValueError("truncated or invalid RIFX container")
    opaque, count = [], 0

    def parse(start, end, depth):
        nonlocal count
        chunks = []
        if depth > MAX_DEPTH:
            raise ValueError("AEP inventory depth budget exceeded")
        while start + 8 <= end:
            tag = bytes(data[start:start + 4]).decode("latin1")
            size = struct.unpack_from(">I", data, start + 4)[0]
            next_offset = start + 8 + size
            if next_offset > end:
                opaque.append({"offset": start, "bytes": end - start,
                               "reason": "unrecognized container contents"})
                return chunks
            count += 1
            if count > MAX_CHUNKS:
                raise ValueError("AEP inventory chunk budget exceeded")
            chunk = Chunk(tag, start, size)
            if tag == "LIST":
                if size < 4:
                    raise ValueError("LIST container is missing its type")
                chunk.kind = bytes(data[start + 8:start + 12]).decode("latin1")
                chunk.children = parse(start + 12, next_offset, depth + 1)
            chunks.append(chunk)
            start = next_offset + (size & 1)
        if start < end:
            opaque.append({"offset": start, "bytes": end - start,
                           "reason": "unparsed container tail"})
        return chunks

    chunks = parse(12, declared_end, 0)
    if declared_end < len(data):
        opaque.append({"offset": declared_end, "bytes": len(data) - declared_end,
                       "reason": "bytes after declared RIFX end"})
    return chunks, opaque


def walk(chunks):
    for chunk in chunks:
        yield chunk
        yield from walk(chunk.children)


def chunk_text(data, chunk):
    raw = bytes(data[chunk.offset + 8:chunk.offset + 8 + chunk.size])
    if chunk.tag == "tdsn" and raw[:4] == b"Utf8" and len(raw) >= 8:
        size = struct.unpack_from(">I", raw, 4)[0]
        if size > len(raw) - 8:
            return ""
        raw = raw[8:8 + size]
    return raw.rstrip(b"\0").decode("utf-8", errors="replace")


def first_text(data, children, tag):
    for child in children:
        if child.tag == tag:
            return chunk_text(data, child)
    return ""


def inventory_bytes(data):
    chunks, opaque = parse_chunks(data)
    definitions = set()
    for chunk in walk(chunks):
        if chunk.kind == "EfDf":
            match = first_text(data, chunk.children, "tdmn")
            if match:
                definitions.add(match)
    compositions, effects, features = [], [], Counter()
    property_names = Counter()

    def visit(chunk, composition=None, in_item=False):
        if chunk.kind == "Item":
            in_item = True
            if any(c.tag == "cdta" for c in chunk.children):
                composition = first_text(data, chunk.children, "Utf8")
                compositions.append({"name": composition, "offset": chunk.offset})
        if chunk.tag == "tdmn":
            property_names[chunk_text(data, chunk)] += 1
        if chunk.kind == "tdgp" and in_item:
            # The LAST tdmn is usually ADBE Group End. Identity is the FIRST.
            match = first_text(data, chunk.children, "tdmn")
            if match in definitions:
                effects.append({"match_name": match, "composition": composition,
                                "offset": chunk.offset})
        if chunk.tag == "Utf8":
            text = chunk_text(data, chunk)
            for name, tokens in EXPRESSION_FEATURES.items():
                if any(token in text for token in tokens):
                    features[name] += 1
        for child in chunk.children:
            visit(child, composition, in_item)

    for chunk in chunks:
        visit(chunk)
    return {
        "evidence": "partial_static_inventory",
        "limits": ["Stored effects may be disabled or outside the chosen root.",
                   "Chunk offsets are evidence locations, not stable project IDs.",
                   "Timing, keyframes, parameters, fonts and references need an AE export.",
                   "Expression feature counts are string candidates, not executed expressions."],
        "effect_definitions": sorted(definitions), "compositions": compositions,
        "stored_effect_instances": effects,
        "effect_counts": dict(sorted(Counter(e["match_name"] for e in effects).items())),
        "expression_feature_candidates": dict(features),
        "property_name_counts": dict(property_names),
        "opaque_regions": opaque,
    }


def inventory(path):
    size = path.stat().st_size
    if not 12 <= size <= MAX_PROJECT_BYTES:
        raise ValueError("AEP file size is outside inventory budget")
    with path.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
        report = inventory_bytes(data)
        report.update(file=path.name, bytes=size, sha256=hashlib.sha256(data).hexdigest())
        return report


def catalogue(repo):
    effects = {}
    for manifest in sorted((repo / "crates/aem-effects").glob("*/manifest.json")):
        package = json.loads(manifest.read_text(encoding="utf-8"))
        for effect in package["effects"]:
            match = effect.get("reference_match_name")
            if match:
                effects.setdefault(match, []).append({
                    "package": package["id"], "version": package["version"],
                    "effect": effect["id"], "compatibility": effect.get("compatibility"),
                    "known_differences": effect.get("known_differences", []),
                    "unimplemented_parameters": [p["id"] for p in effect["params"]
                                                 if not p.get("implemented", True)],
                })
    return effects


def probe_media(root, ffprobe):
    records = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.suffix.lower() not in MEDIA_SUFFIXES:
            continue
        record = {"path": path.relative_to(root).as_posix(), "bytes": path.stat().st_size}
        try:
            run = subprocess.run([ffprobe, "-v", "error", "-show_entries",
                "stream=codec_name,codec_type,width,height,avg_frame_rate,sample_rate,channels:format=duration",
                "-of", "json", str(path)], capture_output=True, timeout=30, check=True)
            record["probe"] = json.loads(run.stdout)
        except (OSError, subprocess.SubprocessError, ValueError) as error:
            record["error"] = str(error)
        records.append(record)
    return records


def run_audit(source, output, repo, ffprobe=None):
    source, output = source.resolve(), output.resolve()
    if output == source or source in output.parents:
        raise ValueError("write the report outside the user's source directory")
    projects = []
    known = catalogue(repo)
    for path in sorted(source.glob("*.aep")):
        record = inventory(path)
        record["host_effect_candidates"] = {
            match: known.get(match, []) for match in record["effect_counts"]}
        projects.append(record)
    if not projects:
        raise ValueError("no .aep files found")
    report = {"schema": "motion-studio-ae-audit-1", "source": str(source),
              "projects": projects, "media": probe_media(source, ffprobe) if ffprobe else [],
              "media_probe_requested": bool(ffprobe)}
    output.mkdir(parents=True, exist_ok=True)
    (output / "project-audit.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path, help="private output directory, outside source")
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--ffprobe", help="optional ffprobe executable for media metadata")
    args = parser.parse_args()
    report = run_audit(args.source, args.output, args.repo, args.ffprobe)
    for project in report["projects"]:
        print(f'{project["file"]}: {len(project["compositions"])} stored compositions, '
              f'{len(project["stored_effect_instances"])} effects / {len(project["effect_counts"])} types, '
              f'{len(project["opaque_regions"])} opaque regions')
    print(f'Report: {args.output.resolve() / "project-audit.json"}')


if __name__ == "__main__":
    main()
