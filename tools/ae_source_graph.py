"""Read-only AEP source graph and authored-property capture, without running AE.

Requires the pinned optional dependencies in ae-source-requirements.txt.
This is research evidence, not an importer or a promise of render compatibility.
All output can contain private paths, text and expressions: keep it outside Git.
"""
from __future__ import annotations

import argparse
from collections import Counter
from enum import Enum
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path, PureWindowsPath
import tempfile

from ae_project_audit import MAX_PROJECT_BYTES, inventory


PARSER_VERSION = "0.18.1"
MAX_PROPERTIES = 200_000
MAX_KEYS = 200_000
MAX_VALUE_ITEMS = 10_000_000
MAX_OUTPUT_BYTES = 128 * 1024 * 1024
MAX_DEPTH = 64


class CaptureLimit(ValueError):
    pass


def load_parser():
    try:
        version = importlib.metadata.version("py-aep")
    except importlib.metadata.PackageNotFoundError as error:
        raise ValueError("install tools/ae-source-requirements.txt in an isolated environment") from error
    if version.removeprefix("v") != PARSER_VERSION:
        raise ValueError(f"py-aep {PARSER_VERSION} required; installed {version}")
    import py_aep
    return py_aep


def select_root(compositions, root_id=None, root_name=None):
    matches = [c for c in compositions if
               (root_id is not None and c["id"] == root_id) or
               (root_id is None and root_name is not None and c["name"] == root_name)]
    if len(matches) != 1:
        raise ValueError(f"root must identify exactly one composition; found {len(matches)}")
    return matches[0]


def source_graph(compositions, root_id):
    """Source references only, including disabled layers; never a visibility graph.

    Postorder memoization counts paths without expanding a potentially exponential
    graph. Repeated source instances count repeatedly. A cyclic/incomplete graph
    has no valid depth or instance count.
    """
    by_id = {}
    for comp in compositions:
        if comp["id"] in by_id:
            raise ValueError(f"duplicate composition ID {comp['id']}")
        by_id[comp["id"]] = comp
    if root_id not in by_id:
        raise ValueError("root composition is absent")
    reachable, missing, cycles, memo, active = set(), [], [], {}, []

    def visit(cid):
        reachable.add(cid)
        if cid in active:
            cycles.append(active[active.index(cid):] + [cid])
            return None
        if cid in memo:
            return memo[cid]
        if len(active) >= MAX_DEPTH:
            raise CaptureLimit("source graph depth budget exceeded")
        active.append(cid)
        depth, instances, complete = 0, 1, True
        comp = by_id[cid]
        if "unreadable" in comp.get("layers", {}):
            complete = False
        else:
            for layer in comp["layers"]:
                sid = layer.get("source_id")
                if sid is None:
                    if "source_id" in layer.get("field_errors", {}):
                        complete = False
                    continue
                if layer.get("source_kind") == "unresolved":
                    missing.append({"composition": cid, "layer": layer["id"], "source": sid})
                    complete = False
                    continue
                if layer.get("source_kind") != "CompItem":
                    continue
                if sid not in by_id:
                    missing.append({"composition": cid, "layer": layer["id"], "source": sid})
                    complete = False
                    continue
                child = visit(sid)
                if child is None:
                    complete = False
                else:
                    depth = max(depth, child[0] + 1)
                    instances += child[1]
        active.pop()
        memo[cid] = (depth, instances) if complete else None
        return memo[cid]

    metric = visit(root_id)
    return {"root_id": root_id, "reachable_composition_ids": sorted(reachable),
            "reachable_count": len(reachable), "missing_sources": missing,
            "cycles": cycles, "complete_source_graph": metric is not None,
            "max_nested_edges": metric[0] if metric else None,
            "expanded_composition_instances": metric[1] if metric else None,
            "includes_disabled_layers": True,
            "excludes_effect_and_expression_dependencies": True}


def resolve_media(original, root, files):
    """Offer a unique longest path-suffix match; do not relink or guess by similarity."""
    original_parts = [s.casefold() for s in PureWindowsPath(original.replace("/", "\\")).parts]
    candidates, best = [], 0
    for path in files:
        parts = [s.casefold() for s in path.relative_to(root).parts]
        score = 0
        for a, b in zip(reversed(original_parts), reversed(parts)):
            if a != b:
                break
            score += 1
        if score and score >= best:
            if score > best:
                candidates, best = [], score
            candidates.append(path.relative_to(root).as_posix())
    return {"status": "unique_candidate" if len(candidates) == 1 else
            "ambiguous" if candidates else "missing", "suffix_components": best,
            "candidates": sorted(candidates), "applied": False}


class Collector:
    def __init__(self, parser):
        self.parser = parser
        self.warnings = []
        self.properties = self.keys = self.value_items = 0

    def issue(self, address, code, detail):
        self.warnings.append({"address": address, "code": code, "detail": detail})

    def read(self, obj, fields, address):
        result, errors = {}, {}
        for field in fields:
            try:
                result[field] = self.value(getattr(obj, field), address + "/" + field)
            except CaptureLimit:
                raise
            except Exception as error:
                errors[field] = f"{type(error).__name__}: {error}"
                self.issue(address + "/" + field, "unreadable_field", errors[field])
        if errors:
            result["field_errors"] = errors
        return result

    def value(self, obj, address, depth=0):
        self.value_items += 1
        if self.value_items > MAX_VALUE_ITEMS or depth > MAX_DEPTH:
            raise CaptureLimit(f"property-value capture budget exceeded ({self.properties} properties, {self.keys} keys)")
        if isinstance(obj, Enum):
            return {"name": obj.name, "code": obj.value}
        if obj is None or isinstance(obj, (str, bool, int)):
            return obj
        if isinstance(obj, float):
            if not math.isfinite(obj):
                raise ValueError("non-finite numeric value")
            return obj
        if isinstance(obj, (tuple, list)):
            return [self.value(v, address, depth + 1) for v in obj]
        if isinstance(obj, dict):
            return {str(k): self.value(v, address, depth + 1) for k, v in obj.items()}
        kind = type(obj).__name__
        # Explicit public getters only. Never introspect/evaluate arbitrary host objects.
        fields = {
            "Shape": ["vertices", "in_tangents", "out_tangents", "closed"],
            "TextDocument": ["text", "font", "font_size", "fill_color", "stroke_color",
                             "apply_fill", "apply_stroke", "stroke_width", "tracking", "leading",
                             "justification", "box_text", "box_text_size", "box_text_pos"],
            "Gradient": ["color_stops", "alpha_stops"],
            "GradientColorStop": ["offset", "midpoint", "color"],
            "GradientAlphaStop": ["offset", "midpoint", "alpha"],
            "Curves": ["version", "mode", "uses_points", "channels"],
            "CurvesChannel": ["name", "points", "map"],
            "MarkerValue": ["comment", "duration", "chapter", "url", "frame_target",
                            "cue_point_name", "event_cue_point"],
        }.get(kind)
        if fields is None:
            raise ValueError(f"unsupported value object {kind}")
        return {"type": kind, **self.read(obj, fields, address)}

    def provenance(self, obj):
        # Version-pinned adapter: py-aep synthesizes some unstored AE defaults.
        # Inspect the backing chunk solely to label evidence, never to write it.
        chunk = getattr(obj, "_tdsb", None)
        if chunk is None:
            return "unknown"
        return "parser_synthesized" if chunk.synthetic else "stored"

    def property(self, prop, address, depth=0):
        self.properties += 1
        if self.properties > MAX_PROPERTIES or depth > MAX_DEPTH:
            raise CaptureLimit("property tree capture budget exceeded")
        result = {"address": address, "origin": self.provenance(prop),
                  **self.read(prop, ["name", "match_name", "enabled", "property_index"], address)}
        if isinstance(prop, self.parser.PropertyGroup):
            result["kind"] = "group"
            if isinstance(prop, self.parser.MaskPropertyGroup):
                result["mask"] = self.read(prop, ["mask_mode", "inverted", "roto_bezier",
                                                   "mask_feather_falloff"], address + "/mask")
            result["children"] = self.children(prop, address, depth + 1)
            return result
        result["kind"] = "property"
        result.update(self.read(prop, ["property_value_type", "is_spatial", "can_vary_over_time",
                                      "min_value", "max_value", "units_text",
                                      "expression", "expression_enabled", "value"], address))
        if result.get("property_value_type", {}).get("name") == "CUSTOM_VALUE" and result.get("value") is None:
            result["value_unreadable"] = True
            self.issue(address, "unreadable_custom_value", "plugin value is not decoded")
        result["keys"] = []
        try:
            for index, key in enumerate(prop.keyframes):
                self.keys += 1
                if self.keys > MAX_KEYS:
                    raise CaptureLimit("keyframe capture budget exceeded")
                ka = address + f"/key/{index + 1}"
                item = self.read(key, ["time", "value", "in_interpolation_type", "out_interpolation_type",
                                      "in_spatial_tangent", "out_spatial_tangent", "roving",
                                      "temporal_auto_bezier", "temporal_continuous"], ka)
                for direction in ("in", "out"):
                    try:
                        item[direction + "_temporal_ease"] = [
                            self.read(e, ["speed", "influence"], ka + "/" + direction)
                            for e in getattr(key, direction + "_temporal_ease")]
                    except CaptureLimit:
                        raise
                    except Exception as error:
                        self.issue(ka, "unreadable_ease", f"{type(error).__name__}: {error}")
                        item[direction + "_temporal_ease"] = {"unreadable": True}
                result["keys"].append(item)
        except CaptureLimit:
            raise
        except Exception as error:
            result["keys_unreadable"] = True
            self.issue(address, "unreadable_keys", f"{type(error).__name__}: {error}")
        return result

    def children(self, group, address, depth=0):
        try:
            return [self.property(child, address + f"/{index + 1}", depth)
                    for index, child in enumerate(group)]
        except CaptureLimit:
            raise
        except Exception as error:
            self.issue(address, "unreadable_children", f"{type(error).__name__}: {error}")
            return {"unreadable": True}

    def layer(self, layer, composition, index, properties):
        address = f"comp/{composition}/layer/{layer.id}"
        result = {"index": index + 1, "kind": type(layer).__name__,
                  **self.read(layer, ["id", "name", "enabled", "solo", "locked", "shy",
                                     "start_time", "in_point", "out_point", "stretch"], address)}
        try:
            parent = layer.parent
            result["parent_id"] = layer._ldta.parent_id or None
            if result["parent_id"] is not None and (parent is None or parent.id != result["parent_id"]):
                self.issue(address, "unresolved_parent", str(result["parent_id"]))
            is_av = isinstance(layer, self.parser.AVLayer)
            source = layer.source if is_av else None
            # Camera/light descriptors can carry a nonzero sentinel here;
            # these layer types never own a footage/composition source.
            result["raw_source_id"] = layer._ldta.source_id
            result["source_id"] = (layer._ldta.source_id or None) if is_av else None
            result["source_kind"] = type(source).__name__ if source is not None else None
            if result["source_id"] is not None and (source is None or source.id != result["source_id"]):
                result["source_kind"] = "unresolved"
                self.issue(address, "unresolved_source", str(result["source_id"]))
        except Exception as error:
            result.setdefault("field_errors", {})["source_id"] = f"{type(error).__name__}: {error}"
            self.issue(address, "unreadable_reference", str(error))
        if isinstance(layer, self.parser.AVLayer):
            result.update(self.read(layer, ["three_d_layer", "adjustment_layer", "null_layer", "guide_layer",
                                           "audio_enabled", "effects_active", "collapse_transformation",
                                           "motion_blur", "time_remap_enabled", "blending_mode",
                                           "track_matte_type"], address))
            try:
                matte = layer.track_matte_layer
                result["matte_layer_id"] = matte.id if matte is not None else None
            except Exception as error:
                self.issue(address, "unreadable_matte_reference", str(error))
                result["matte_reference_unreadable"] = True
        result["effects"] = []
        try:
            for effect in layer.effects or []:
                result["effects"].append({"origin": self.provenance(effect),
                                          **self.read(effect, ["match_name", "name", "enabled"], address + "/effect")})
        except CaptureLimit:
            raise
        except Exception as error:
            result["effects_unreadable"] = True
            self.issue(address, "unreadable_effects", str(error))
        if properties:
            result["properties"] = self.children(layer, address)
        return result

    def composition(self, comp, properties):
        address = f"comp/{comp.id}"
        result = self.read(comp, ["id", "name", "width", "height", "frame_rate", "duration",
                                  "pixel_aspect", "display_start_time", "work_area_start",
                                  "work_area_duration", "motion_blur", "preserve_nested_frame_rate"], address)
        try:
            result["layers"] = [self.layer(layer, comp.id, index, properties)
                                for index, layer in enumerate(comp.layers)]
        except CaptureLimit:
            raise
        except Exception as error:
            result["layers"] = {"unreadable": True}
            self.issue(address, "unreadable_layers", f"{type(error).__name__}: {error}")
        return result


def capture(source, root_id=None, root_name=None, properties=False, media_root=None):
    source = source.resolve()
    if not 12 <= source.stat().st_size <= MAX_PROJECT_BYTES:
        raise ValueError("AEP size outside capture budget")
    partial = inventory(source)  # Validate the outer container and retain opaque regions.
    parser = load_parser()
    app = parser.parse(source)
    collector = Collector(parser)
    compositions = [collector.composition(comp, properties) for comp in app.project.compositions]
    root = select_root(compositions, root_id, root_name)
    graph = source_graph(compositions, root["id"])
    media_files = sorted(p for p in media_root.rglob("*") if p.is_file()) if media_root else []
    footages = []
    for footage in app.project.footages:
        address = f"footage/{footage.id}"
        item = collector.read(footage, ["id", "name", "width", "height", "duration", "frame_rate", "file",
                                        "has_audio", "has_video", "use_proxy"], address)
        try:
            main_source = footage.main_source
            item["main_source"] = {"kind": type(main_source).__name__,
                **collector.read(main_source, ["has_alpha", "alpha_mode", "invert_alpha", "premul_color",
                                               "is_still", "native_frame_rate", "conform_frame_rate", "loop"],
                                 address + "/main_source")}
            if isinstance(main_source, parser.SolidSource):
                item["main_source"].update(collector.read(main_source, ["color"], address + "/main_source"))
            if item.get("use_proxy"):
                collector.issue(address, "proxy_not_captured", "active proxy needs separate source capture")
        except CaptureLimit:
            raise
        except Exception as error:
            item["main_source"] = {"unreadable": True}
            collector.issue(address, "unreadable_media_source", str(error))
        if media_root and item.get("file"):
            item["local_resolution"] = resolve_media(item["file"], media_root, media_files)
        footages.append(item)
    after = hashlib.sha256(source.read_bytes()).hexdigest()
    if after != partial["sha256"]:
        raise ValueError("source changed during capture; snapshot discarded")
    reachable = set(graph["reachable_composition_ids"])
    effects = Counter(fx.get("match_name", "unreadable") for comp in compositions if comp["id"] in reachable
                      for layer in comp["layers"] if isinstance(layer, dict)
                      for fx in layer.get("effects", []))
    return {"schema": "motion-studio-ae-source-1", "state": "captured_static",
            "restorationReady": False, "source": {"file": source.name, "sha256": after, "bytes": source.stat().st_size},
            "parser": {"name": "py-aep", "version": PARSER_VERSION, "saved_ae_version": app.version},
            "project": collector.read(app.project, ["bits_per_channel", "working_space", "linear_blending",
                                                     "linearize_working_space"], "project"),
            "root": {"id": root["id"], "name": root["name"], "duration": root.get("duration"),
                     "reference_range": {"start": root.get("work_area_start"),
                                         "duration": root.get("work_area_duration"), "basis": "stored_work_area"}},
            "source_graph": graph, "compositions": compositions, "footages": footages,
            "source_reachable_effect_record_counts": dict(sorted(effects.items())),
            "counts": {"compositions": len(compositions), "properties": collector.properties, "keys": collector.keys},
            "properties_requested": properties, "warnings": collector.warnings,
            "opaque_regions": partial["opaque_regions"],
            "limits": ["Binary parser results require differential validation against AE.",
                       "Source reachability includes disabled/off-range layers and is not visibility.",
                       "Effect layer inputs, expression references and time remapping are not resolved by the graph.",
                       "Parser-synthesized property defaults are explicitly distinguished from stored data.",
                       "Parameter ranges may include parser defaults; they are not AE-verified hard ranges.",
                       "Complex values capture selected fields; unreadable custom data and remaining text styles need follow-up.",
                       "No expression is executed, no media is relinked, no project is written or rendered."]}


def write_capture(report, source, output):
    source, output = source.resolve(), output.resolve()
    if output == source.parent or source.parent in output.parents:
        raise ValueError("capture output must be outside the source directory")
    content = json.dumps(report, ensure_ascii=True, allow_nan=False, separators=(",", ":")).encode("utf-8")
    if len(content) > MAX_OUTPUT_BYTES:
        raise CaptureLimit("capture output budget exceeded")
    output.mkdir(parents=True, exist_ok=True)
    name = output / "ae-source-capture.json"
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=output, delete=False, suffix=".tmp") as stream:
            temporary = Path(stream.name)
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, name)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return name


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path, help="private directory outside the input directory")
    root = parser.add_mutually_exclusive_group(required=True)
    root.add_argument("--root-id", type=int)
    root.add_argument("--root-name")
    parser.add_argument("--properties", action="store_true", help="also capture authored values, keys and expressions")
    parser.add_argument("--media-root", type=Path, help="offer local path candidates without applying relinks")
    args = parser.parse_args()
    if args.media_root:
        args.media_root = args.media_root.resolve()
        if not args.media_root.is_dir():
            parser.error("--media-root must be a directory")
    report = capture(args.source, args.root_id, args.root_name, args.properties, args.media_root)
    name = write_capture(report, args.source, args.output)
    graph = report["source_graph"]
    print(f"Captured {report['counts']['compositions']} compositions; {graph['reachable_count']} source-reachable")
    print(f"Properties {report['counts']['properties']}, keys {report['counts']['keys']}, warnings {len(report['warnings'])}")
    print(f"Private report: {name}")


if __name__ == "__main__":
    main()
