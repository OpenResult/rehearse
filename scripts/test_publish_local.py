import copy
from pathlib import Path
import tempfile
import tomllib
import unittest

from publish_local import MARKER, index_entry, prepare_directory, write_toml


class RegistryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.base = Path(self.temp.name).resolve()
        self.root = self.base / "checkout"
        self.root.mkdir()

    def tearDown(self):
        self.temp.cleanup()

    def test_refuses_unowned_nonempty_directory_without_touching_files(self):
        directory = self.base / "existing"
        directory.mkdir()
        sentinel = directory / "keep"
        sentinel.write_text("user data")
        with self.assertRaisesRegex(ValueError, "nonempty"):
            prepare_directory(directory, self.root)
        self.assertEqual(sentinel.read_text(), "user data")

    def test_only_generated_child_is_cleaned(self):
        directory = self.base / "registry"
        generated = prepare_directory(directory, self.root)
        (generated / "old").write_text("generated")
        (directory / "keep").write_text("user data")
        prepare_directory(directory, self.root)
        self.assertFalse((generated / "old").exists())
        self.assertEqual((directory / "keep").read_text(), "user data")
        self.assertTrue((directory / MARKER).is_file())

    def test_rejects_protected_paths_and_foreign_markers(self):
        for path in [self.root, self.root.parent, Path.home(), Path("/")]:
            with self.assertRaises(ValueError):
                prepare_directory(path, self.root)
        directory = self.base / "registry"
        prepare_directory(directory, self.root)
        with self.assertRaisesRegex(ValueError, "another checkout"):
            prepare_directory(directory, self.base / "another")

    def test_rejects_symlinks_and_parent_traversal(self):
        target = self.base / "target"
        target.mkdir()
        link = self.base / "link"
        link.symlink_to(target, target_is_directory=True)
        for path in [link, link / "child", self.root / ".." / "other"]:
            with self.assertRaises(ValueError):
                prepare_directory(path, self.root)
        directory = self.base / "registry"
        generated = prepare_directory(directory, self.root)
        generated.rmdir()
        generated.symlink_to(target, target_is_directory=True)
        with self.assertRaises(ValueError):
            prepare_directory(directory, self.root)
        generated.unlink()
        (directory / MARKER).unlink()
        (directory / MARKER).symlink_to(target / "marker")
        with self.assertRaises(ValueError):
            prepare_directory(directory, self.root)

    def test_metadata_preserves_renames_targets_versions_and_features(self):
        dependency = {"name": "serde", "rename": "wire", "req": "^1",
                      "features": ["derive"], "optional": True, "uses_default_features": False,
                      "target": "cfg(unix)", "kind": None, "registry": None}
        local = copy.deepcopy(dependency)
        local.update(name="rehearse-macros", rename=None, req="^9.1.2")
        package = {"name": "rehearse", "version": "9.1.2", "dependencies": [dependency, local],
                   "features": {"serde": ["dep:wire"]}, "links": None, "rust_version": "1.85"}
        entry = index_entry(package, "checksum", {"rehearse", "rehearse-macros"})
        self.assertEqual(entry["vers"], "9.1.2")
        self.assertEqual(entry["features"], package["features"])
        self.assertEqual(entry["deps"][0]["package"], "serde")
        self.assertEqual(entry["deps"][0]["name"], "wire")
        self.assertEqual(entry["deps"][0]["target"], "cfg(unix)")
        self.assertIsNotNone(entry["deps"][0]["registry"])
        self.assertIsNone(entry["deps"][1]["registry"])
        self.assertEqual(entry["deps"][1]["req"], "^9.1.2")

    def test_generated_manifest_round_trip(self):
        data = {"package": {"name": "test", "version": "0.3.0", "publish": False},
                "dependencies": {"renamed": {"package": "rehearse", "version": "0.3.0", "features": ["serde"]}},
                "example": [{"name": "demo", "required-features": ["macros"]}], "workspace": {}}
        path = self.base / "Cargo.toml"
        write_toml(path, data)
        self.assertEqual(tomllib.loads(path.read_text()), data)


if __name__ == "__main__":
    unittest.main()
