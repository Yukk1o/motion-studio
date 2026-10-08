"""Owned synthetic projects only; user AEPs/media are never test fixtures."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import ae_source_graph as graph


def comp(cid, name="Comp", layers=None):
    return {"id": cid, "name": name, "layers": layers or []}


def layer(lid, source, enabled=True):
    return {"id": lid, "source_id": source, "source_kind": "CompItem", "enabled": enabled}


class GraphTests(unittest.TestCase):
    def test_disabled_instances_remain_in_source_graph(self):
        result = graph.source_graph([comp(1, layers=[layer(11, 2), layer(12, 2, False)]), comp(2), comp(3)], 1)
        self.assertEqual(result["reachable_composition_ids"], [1, 2])
        self.assertEqual(result["max_nested_edges"], 1)
        self.assertEqual(result["expanded_composition_instances"], 3)

    def test_cycles_missing_and_unreadable_sources_cannot_claim_complete_graph(self):
        cycle = graph.source_graph([comp(1, layers=[layer(11, 2)]), comp(2, layers=[layer(21, 1)])], 1)
        self.assertEqual(cycle["cycles"], [[1, 2, 1]])
        self.assertFalse(cycle["complete_source_graph"])
        self.assertIsNone(cycle["expanded_composition_instances"])
        missing = graph.source_graph([comp(1, layers=[layer(11, 2)])], 1)
        self.assertEqual(missing["missing_sources"], [{"composition": 1, "layer": 11, "source": 2}])
        unreadable = graph.source_graph([comp(1, layers=[{"id": 11, "field_errors": {"source_id": "missing"}}])], 1)
        self.assertFalse(unreadable["complete_source_graph"])
        unresolved = graph.source_graph([comp(1, layers=[{"id": 11, "source_id": 9,
                                                        "source_kind": "unresolved"}])], 1)
        self.assertFalse(unresolved["complete_source_graph"])
        self.assertEqual(unresolved["missing_sources"][0]["source"], 9)

    def test_duplicate_identity_and_ambiguous_names_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate composition"):
            graph.source_graph([comp(1), comp(1)], 1)
        with self.assertRaisesRegex(ValueError, "exactly one"):
            graph.select_root([comp(1, "same"), comp(2, "same")], root_name="same")
        self.assertEqual(graph.select_root([comp(1, "same"), comp(2, "same")], root_id=2)["id"], 2)

    def test_shared_dag_counts_instances_without_expanding_paths(self):
        nodes = [comp(i, layers=[layer(i * 10, i + 1), layer(i * 10 + 1, i + 1)]) for i in range(1, 31)]
        nodes.append(comp(31))
        result = graph.source_graph(nodes, 1)
        self.assertEqual(result["expanded_composition_instances"], 2 ** 31 - 1)
        self.assertEqual(result["max_nested_edges"], 30)
        with patch.object(graph, "MAX_DEPTH", 8):
            with self.assertRaisesRegex(graph.CaptureLimit, "depth budget"):
                graph.source_graph(nodes, 1)

    def test_media_suffix_resolution_never_silently_picks_duplicate_name(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            files = [root / "left" / "photo.png", root / "right" / "photo.png"]
            result = graph.resolve_media("C:/old/photo.png", root, files)
            self.assertEqual(result["status"], "ambiguous")
            exact = graph.resolve_media("C:/old/RIGHT/PHOTO.PNG", root, files)
            self.assertEqual(exact["candidates"], ["right/photo.png"])
            self.assertFalse(exact["applied"])
            self.assertEqual(graph.resolve_media("C:/old/photos.png", root, files)["status"], "missing")

    def test_output_is_atomic_outside_source_and_rejects_non_finite_values(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source" / "owned.aep"
            source.parent.mkdir()
            source.write_bytes(b"owned placeholder")
            with self.assertRaisesRegex(ValueError, "outside"):
                graph.write_capture({}, source, source.parent / "out")
            name = graph.write_capture({"private": "\udcff"}, source, root / "out")
            self.assertIn(b"\\udcff", name.read_bytes())
            original = name.read_bytes()
            with self.assertRaises(ValueError):
                graph.write_capture({"x": float("nan")}, source, root / "out")
            with patch.object(graph.os, "fsync", side_effect=OSError("owned failure case")):
                with self.assertRaisesRegex(OSError, "owned failure"):
                    graph.write_capture({"replacement": True}, source, root / "out")
            self.assertEqual(name.read_bytes(), original)
            self.assertEqual(list(name.parent.glob("*.tmp")), [])


class ParserTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        try:
            cls.parser = graph.load_parser()
        except ValueError as error:
            raise unittest.SkipTest(str(error))

    def test_authored_keys_keep_seconds_and_work_area_differs_from_duration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "inputs" / "owned.aep"
            source.parent.mkdir()
            app = self.parser.new(version="25.6x101")
            parent = app.project.root_folder.add_comp("Main", 640, 360, 1, 10, 23.976)
            parent.work_area_start = 2
            parent.work_area_duration = 4
            expected_duration = parent.duration
            expected_range = {"start": parent.work_area_start, "duration": parent.work_area_duration,
                              "basis": "stored_work_area"}
            child = app.project.root_folder.add_comp("Child", 640, 360, 1, 10, 24)
            parent.add(child)
            parent.add(child).enabled = False
            solid = child.add_solid([0.2, 0.4, 0.6], "Solid")
            opacity = solid.transform["ADBE Opacity"]
            opacity.set_value_at_time(0.625, 20)
            opacity.set_value_at_time(1.75, 80)
            opacity.expression = "throw new Error('must never execute');"
            opacity.expression_enabled = True
            app.project.save(source)
            before = source.read_bytes()
            report = graph.capture(source, root_id=parent.id, properties=True)
            self.assertEqual(source.read_bytes(), before)
            self.assertFalse(report["restorationReady"])
            self.assertEqual(report["root"]["duration"], expected_duration)
            self.assertEqual(report["root"]["reference_range"], expected_range)
            self.assertNotEqual(report["root"]["duration"], report["root"]["reference_range"]["duration"])
            self.assertEqual(report["source_graph"]["expanded_composition_instances"], 3)
            captured_parent = next(c for c in report["compositions"] if c["id"] == parent.id)
            self.assertAlmostEqual(captured_parent["frame_rate"], 23.976, places=4)
            captured_child = next(c for c in report["compositions"] if c["id"] == child.id)

            def find(props):
                for prop in props:
                    if prop["match_name"] == "ADBE Opacity":
                        return prop
                    if prop["kind"] == "group":
                        found = find(prop["children"])
                        if found:
                            return found

            value = find(captured_child["layers"][0]["properties"])
            self.assertEqual([k["time"] for k in value["keys"]], [0.625, 1.75])
            self.assertEqual([k["value"] for k in value["keys"]], [20, 80])
            self.assertTrue(value["expression_enabled"])
            self.assertIn("must never execute", value["expression"])
            self.assertEqual(report["counts"]["keys"], 2)
            footage = next(f for f in report["footages"] if f["name"] == "Solid")
            self.assertEqual(footage["main_source"]["kind"], "SolidSource")
            for captured, expected in zip(footage["main_source"]["color"], [0.2, 0.4, 0.6]):
                self.assertAlmostEqual(captured, expected, places=4)

    def test_capture_budget_failure_does_not_become_an_unreadable_placeholder(self):
        app = self.parser.new()
        parent = app.project.root_folder.add_comp("Main", 640, 360, 1, 10, 30)
        parent.add_solid([0, 0, 0])
        collector = graph.Collector(self.parser)
        with patch.object(graph, "MAX_PROPERTIES", 1):
            with self.assertRaises(graph.CaptureLimit):
                collector.composition(parent, True)

    def test_parser_dangling_source_is_preserved_instead_of_becoming_an_empty_layer(self):
        app = self.parser.new()
        parent = app.project.root_folder.add_comp("Main", 640, 360, 1, 10, 30)
        solid = parent.add_solid([0, 0, 0])
        solid._ldta.source_id = 999999  # Corrupt only this owned in-memory fixture.
        collector = graph.Collector(self.parser)
        captured = collector.composition(parent, False)
        self.assertEqual(captured["layers"][0]["source_id"], 999999)
        self.assertEqual(captured["layers"][0]["source_kind"], "unresolved")
        self.assertFalse(graph.source_graph([captured], parent.id)["complete_source_graph"])

    def test_sourceless_camera_sentinel_does_not_become_a_missing_footage(self):
        app = self.parser.new()
        parent = app.project.root_folder.add_comp("Main", 640, 360, 1, 10, 30)
        camera = parent.add_camera("Camera", [320, 180])
        camera._ldta.source_id = 0xFFFFFFFF
        collector = graph.Collector(self.parser)
        captured = collector.composition(parent, False)
        self.assertEqual(captured["layers"][0]["raw_source_id"], 0xFFFFFFFF)
        self.assertIsNone(captured["layers"][0]["source_id"])
        self.assertTrue(graph.source_graph([captured], parent.id)["complete_source_graph"])


if __name__ == "__main__":
    unittest.main()
