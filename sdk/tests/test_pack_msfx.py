import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile

SDK = Path(__file__).resolve().parents[1]
SKILL = SDK / "motion-studio-plugin"
spec = importlib.util.spec_from_file_location("pack_msfx", SKILL / "scripts/pack_msfx.py")
packer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packer)


class PackageWorkflowTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.source = self.root / "effect"
        shutil.copytree(SKILL / "assets/effect-template", self.source)
        self.output = self.root / "gain.msfx"

    def tearDown(self):
        self.temporary.cleanup()

    def manifest(self, change):
        path = self.source / "manifest.json"
        value = json.loads(path.read_text(encoding="utf-8"))
        change(value)
        path.write_text(json.dumps(value), encoding="utf-8")

    def test_pack_is_repeatable_and_includes_only_declared_files(self):
        (self.source / "private.txt").write_text("do not publish")
        first = packer.pack(self.source, self.output)
        with zipfile.ZipFile(self.output) as package:
            self.assertEqual(set(package.namelist()), {"manifest.json", "shaders/gain.wgsl"})
            manifest = json.loads(package.read("manifest.json"))
            self.assertEqual(manifest["effects"][0]["native_editor"]["protocol"], 1)
        self.assertEqual(first["sha256"], packer.pack(self.source, self.output)["sha256"])

    def test_invalid_reference_preserves_previous_package(self):
        packer.pack(self.source, self.output)
        before = self.output.read_bytes()
        self.manifest(lambda value: value["effects"][0]["passes"][0].update(shader="../private.wgsl"))
        with self.assertRaises(ValueError):
            packer.pack(self.source, self.output)
        self.assertEqual(self.output.read_bytes(), before)

    def test_case_aliases_are_rejected(self):
        self.manifest(lambda value: value["effects"][0]["passes"].append({"shader":"shaders/GAIN.wgsl","entry":"main_fx"}))
        with self.assertRaisesRegex(ValueError, "case-insensitive"):
            packer.pack(self.source, self.output)

    def test_resource_resolution_must_stay_within_package(self):
        outside = self.root / "outside.wgsl"
        outside.write_text("not a package resource")
        target = self.source / "shaders/gain.wgsl"
        target.unlink()
        try:
            target.symlink_to(outside)
        except OSError as error:
            if sys.platform != "win32":
                raise
            # Directory junctions exercise the same resolved-path contract
            # without requiring Windows' symbolic-link privilege.
            directory = self.root / "external-files"
            directory.mkdir()
            shutil.copyfile(outside, directory / "outside.wgsl")
            junction = self.source / "external"
            self.assertTrue(directory.resolve().is_relative_to(self.root.resolve()))
            self.assertTrue(junction.resolve().is_relative_to(self.root.resolve()))
            quote = lambda path: "'" + str(path).replace("'", "''") + "'"
            subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-Command",
                f"New-Item -ItemType Junction -Path {quote(junction)} -Target {quote(directory)} -ErrorAction Stop | Out-Null"], check=True)
            self.manifest(lambda value: value["effects"][0]["passes"][0].update(shader="external/outside.wgsl"))
        with self.assertRaisesRegex(ValueError, "outside source"):
            packer.pack(self.source, self.output)

    def test_nonfinite_json_and_non_msfx_output_are_rejected(self):
        self.manifest(lambda value: value["effects"][0]["params"][0].update(max=float("inf")))
        with self.assertRaises(ValueError):
            packer.pack(self.source, self.output)
        shutil.copyfile(SKILL / "assets/effect-template/manifest.json", self.source / "manifest.json")
        with self.assertRaisesRegex(ValueError, "output must use"):
            packer.pack(self.source, self.root / "gain.zip")

    def test_missing_validator_does_not_replace_published_output(self):
        packer.pack(self.source, self.output)
        before = self.output.read_bytes()
        with self.assertRaises(OSError):
            packer.pack(self.source, self.output, self.root / "missing-validator")
        self.assertEqual(self.output.read_bytes(), before)
        self.assertFalse(list(self.root.glob(".msfx-*.tmp")))


if __name__ == "__main__":
    unittest.main()
