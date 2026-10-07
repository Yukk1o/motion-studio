"""Synthetic parser cases; no user project or third-party material is a fixture."""
import struct
import tempfile
import unittest
from pathlib import Path

from ae_project_audit import inventory_bytes, parse_chunks, run_audit


def chunk(tag, value):
    return tag.encode("ascii") + struct.pack(">I", len(value)) + value + b"\0" * (len(value) & 1)


def group(kind, *children):
    return chunk("LIST", kind.encode("ascii") + b"".join(children))


def aep(*children):
    return chunk("RIFX", b"Egg!" + b"".join(children))


class AuditTests(unittest.TestCase):
    def test_definitions_are_not_instances_and_first_match_name_wins(self):
        identity = chunk("tdmn", b"ADBE Tint")
        definition = group("EfDf", identity)
        effect = group("tdgp", identity, chunk("tdmn", b"ADBE Group End"))
        item = group("Item", chunk("Utf8", "测试合成".encode("utf-8")), chunk("cdta", b""), effect)
        report = inventory_bytes(aep(definition, item))
        self.assertEqual(report["effect_counts"], {"ADBE Tint": 1})
        self.assertEqual(report["compositions"][0]["name"], "测试合成")
        self.assertEqual(report["stored_effect_instances"][0]["composition"], "测试合成")

    def test_unused_definition_and_nested_folder_do_not_duplicate_effects(self):
        identity = chunk("tdmn", b"ADBE Fill")
        item = group("Item", chunk("Utf8", b"child"), chunk("cdta", b""), group("tdgp", identity))
        report = inventory_bytes(aep(group("EfDf", identity), group("Item", group("Sfdr", item))))
        self.assertEqual(len(report["compositions"]), 1)
        self.assertEqual(len(report["stored_effect_instances"]), 1)
        self.assertEqual(inventory_bytes(aep(group("EfDf", identity)))["effect_counts"], {})

    def test_opaque_serialized_regions_are_reported_not_interpreted(self):
        report = inventory_bytes(aep(group("tdgp", b"not RIFF serialized text")))
        self.assertEqual(len(report["opaque_regions"]), 1)

    def test_truncated_rifx_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "truncated"):
            parse_chunks(aep(chunk("Utf8", b"hello"))[:-1])
        with self.assertRaisesRegex(ValueError, "RIFX"):
            parse_chunks(b"not a project")

    def test_expression_strings_are_counted_without_execution(self):
        report = inventory_bytes(aep(chunk("Utf8", b'evil(); effect("x"); textIndex; loopOut()')))
        self.assertEqual(report["expression_feature_candidates"],
                         {"cross_property": 1, "per_character": 1, "loops": 1})

    def test_sources_are_unchanged_and_outputs_cannot_be_in_source(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            source = root / "source"
            source.mkdir()
            project = source / "owned.aep"
            before = aep(chunk("Utf8", b"hello"))
            project.write_bytes(before)
            with self.assertRaisesRegex(ValueError, "outside"):
                run_audit(source, source / "reports", root)
            report = run_audit(source, root / "output", root)
            self.assertEqual(project.read_bytes(), before)
            self.assertEqual(report["projects"][0]["bytes"], len(before))


if __name__ == "__main__":
    unittest.main()
