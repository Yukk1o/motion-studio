"""Assemble the one distributed builtin package from maintained source groups.

The original library/core-effects.msfx is a published 1.4.1 dependency; it must
not be rewritten. Pack builtin-library with effect_tool after generation.
"""
import copy
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
EFFECTS=ROOT/"crates/aem-effects"
OUT=EFFECTS/"builtin-library"
OUT.mkdir(exist_ok=True)
groups=[EFFECTS/"library",EFFECTS/"motion-library"]
manifest=copy.deepcopy(json.loads((groups[0]/"manifest.json").read_text(encoding="utf-8")))
manifest.update(sdk_version=6,version="2.0.0",name="Motion Studio 内置效果",effects=[])
seen=set()
files={}
for root in groups:
    source=json.loads((root/"manifest.json").read_text(encoding="utf-8"))
    for effect in source["effects"]:
        assert effect["id"] not in seen,effect["id"]
        seen.add(effect["id"]);manifest["effects"].append(effect)
        for name in [p["shader"] for p in effect["passes"]]+effect.get("resources",[]):
            assert name.startswith(("shaders/","assets/")) and all(part not in ("..",".") for part in Path(name).parts),name
            data=(root/name).read_bytes()
            assert name not in files or files[name]==data,"conflicting resource: "+name
            files[name]=data
assert len(manifest["effects"])==95
for name,data in files.items():
    path=OUT/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
(OUT/"manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8",newline="\n")
print(f"Assembled {len(seen)} builtin effects; {len(files)} resources")
