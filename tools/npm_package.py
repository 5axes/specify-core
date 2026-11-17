#!/usr/bin/env python3
"""Turn wasm-pack's `pkg/` into the npm package `@draftwright/specify-core-wasm`.

The release asset `specify-core-wasm-<version>.tgz` keeps wasm-pack's output unchanged, because
hosts fetch and verify it. The npm package is that same `pkg/` with only `package.json`
changed: the scoped name and `publishConfig`. The module files (`specify_core_wasm.js`, the
`.wasm` and the declarations) are not touched.

    python3 tools/npm_package.py <pkg dir> [--version X.Y.Z]

With `--version`, refuses a package whose version differs (the release tag's version).
"""

import argparse
import json
import pathlib
import sys

NAME = "@draftwright/specify-core-wasm"
REPOSITORY = "https://github.com/pzfreo/specify-core"
LICENSE = "Apache-2.0"
MODULE_FILES = ["specify_core_wasm_bg.wasm", "specify_core_wasm.js", "specify_core_wasm.d.ts"]


def prepare(pkg: pathlib.Path, version: str | None = None) -> dict:
    path = pkg / "package.json"
    meta = json.loads(path.read_text(encoding="utf-8"))
    if meta.get("name") not in ("specify-core-wasm", NAME):
        raise SystemExit(f"{path}: unexpected package name {meta.get('name')!r}")
    if version is not None and meta.get("version") != version:
        raise SystemExit(f"{path}: version {meta.get('version')!r}, expected {version!r}")
    for name in MODULE_FILES:
        if not (pkg / name).is_file():
            raise SystemExit(f"{pkg}: {name} is missing")
    meta["name"] = NAME
    meta["license"] = LICENSE
    # npm trusted publishing and provenance require repository.url to name this repository.
    meta["repository"] = {"type": "git", "url": REPOSITORY}
    meta["publishConfig"] = {"access": "public", "provenance": True}
    path.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    return meta


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("pkg", type=pathlib.Path)
    parser.add_argument("--version")
    args = parser.parse_args(argv)
    meta = prepare(args.pkg, args.version)
    print(f"{meta['name']}@{meta['version']} ready in {args.pkg}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
