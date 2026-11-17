import json
import pathlib
import tempfile
import unittest

import npm_package


def write_pkg(root: pathlib.Path, **meta) -> pathlib.Path:
    pkg = root / "pkg"
    pkg.mkdir()
    for name in npm_package.MODULE_FILES:
        (pkg / name).write_text("x", encoding="utf-8")
    base = {
        "name": "specify-core-wasm",
        "version": "0.1.17",
        "license": "Apache-2.0",
        "repository": {"type": "git", "url": "https://github.com/pzfreo/specify-core"},
        "main": "specify_core_wasm.js",
    }
    base.update(meta)
    (pkg / "package.json").write_text(json.dumps(base), encoding="utf-8")
    return pkg


class NpmPackageTests(unittest.TestCase):
    def test_scopes_the_name_and_keeps_the_module(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg = write_pkg(pathlib.Path(tmp))
            npm_package.prepare(pkg, "0.1.17")
            meta = json.loads((pkg / "package.json").read_text(encoding="utf-8"))
            self.assertEqual(meta["name"], "@draftwright/specify-core-wasm")
            self.assertEqual(meta["publishConfig"], {"access": "public", "provenance": True})
            self.assertEqual(meta["repository"]["url"], "https://github.com/pzfreo/specify-core")
            self.assertEqual(meta["main"], "specify_core_wasm.js")
            self.assertEqual(meta["version"], "0.1.17")

    def test_refuses_a_version_other_than_the_tag(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg = write_pkg(pathlib.Path(tmp))
            with self.assertRaises(SystemExit):
                npm_package.prepare(pkg, "0.1.18")

    def test_refuses_a_missing_module_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            pkg = write_pkg(pathlib.Path(tmp))
            (pkg / "specify_core_wasm_bg.wasm").unlink()
            with self.assertRaises(SystemExit):
                npm_package.prepare(pkg)


if __name__ == "__main__":
    unittest.main()
