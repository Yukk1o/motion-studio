"""Generate Android vector resources from the editor's canonical SVG icons.

Only path geometry and explicit color/stroke attributes are accepted. Run with
--check to verify generated resources without modifying the workspace.
"""
import argparse
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "android/app/src/main/svg"
DESTINATION = ROOT / "android/app/src/main/res/drawable"
ANDROID = "http://schemas.android.com/apk/res/android"
ET.register_namespace("android", ANDROID)


def color(value):
    if value == "none":
        return "@android:color/transparent"
    if value == "currentColor":
        return "#FFEDF1F5"
    raise ValueError(f"Unsupported SVG color: {value}")


def convert(source):
    svg = ET.parse(source).getroot()
    if svg.get("viewBox") != "0 0 24 24":
        raise ValueError(f"{source.name}: expected 24 by 24 viewport")
    prefix = f"{{{ANDROID}}}"
    vector = ET.Element("vector", {
        prefix + "width": "24dp", prefix + "height": "24dp",
        prefix + "viewportWidth": "24", prefix + "viewportHeight": "24",
    })
    if svg.get("data-auto-mirrored") == "true":
        vector.set(prefix + "autoMirrored", "true")
    for element in svg:
        if element.tag.rsplit("}", 1)[-1] != "path":
            raise ValueError(f"{source.name}: only path geometry is supported")
        values = {prefix + "pathData": element.attrib["d"],
                  prefix + "fillColor": color(element.get("fill", svg.get("fill", "none")))}
        stroke = element.get("stroke", svg.get("stroke", "none"))
        if stroke != "none":
            values.update({
                prefix + "strokeColor": color(stroke),
                prefix + "strokeWidth": element.get("stroke-width", svg.get("stroke-width", "1.75")),
                prefix + "strokeLineCap": element.get("stroke-linecap", svg.get("stroke-linecap", "round")),
                prefix + "strokeLineJoin": element.get("stroke-linejoin", svg.get("stroke-linejoin", "round")),
            })
        ET.SubElement(vector, "path", values)
    ET.indent(vector, space="    ")
    return '<?xml version="1.0" encoding="utf-8"?>\n' + ET.tostring(vector, encoding="unicode") + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    files = sorted(SOURCE.glob("*.svg"))
    if not files:
        raise RuntimeError("No editor SVG sources found")
    invalid = []
    for source in files:
        destination = DESTINATION / ("ic_editor_" + source.stem + ".xml")
        generated = convert(source)
        if args.check:
            if not destination.exists() or destination.read_text(encoding="utf-8") != generated:
                invalid.append(destination.name)
        else:
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(generated, encoding="utf-8", newline="\n")
    if invalid:
        raise RuntimeError("Outdated vector resources: " + ", ".join(invalid))
    print(f"{'Verified' if args.check else 'Generated'} {len(files)} editor vector icons")


if __name__ == "__main__":
    main()
