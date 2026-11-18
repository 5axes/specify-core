#!/usr/bin/env python3
"""Turn wasm-pack's `pkg/` into the npm package `@draftwright/specify-core-wasm`.

The release asset `specify-core-wasm-<version>.tgz` is wasm-pack's output plus the repository's
NOTICE (release.yml copies it in), and hosts fetch and verify it. The npm package is that same
`pkg/` with only `package.json` changed: the scoped name, `publishConfig` and `NOTICE` in `files` (npm always packs LICENSE
and README, but not NOTICE). The module files (`specify_core_wasm.js`, the `.wasm` and the
declarations) are not touched.

    python3 tools/npm_package.py <pkg dir> [--version X.Y.Z] [--check-pack]

With `--version`, refuses a package whose version differs (the release tag's version). With
`--check-pack`, runs `npm pack --dry-run` and refuses a tarball without LICENSE, NOTICE or a
module file.
"""

import argparse
import json
import pathlib
import subprocess
import sys

NAME = "@draftwright/specify-core-wasm"
REPOSITORY = "https://github.com/pzfreo/specify-core"
LICENSE = "Apache-2.0"
MODULE_FILES = ["specify_core_wasm_bg.wasm", "specify_core_wasm.js", "specify_core_wasm.d.ts"]
LEGAL_FILES = ["LICENSE", "NOTICE"]


def prepare(pkg: pathlib.Path, version: str | None = None) -> dict:
    path = pkg / "package.json"
    meta = json.loads(path.read_text(encoding="utf-8"))
    if meta.get("name") not in ("specify-core-wasm", NAME):
        raise SystemExit(f"{path}: unexpected package name {meta.get('name')!r}")
    if version is not None and meta.get("version") != version:
        raise SystemExit(f"{path}: version {meta.get('version')!r}, expected {version!r}")
    for name in MODULE_FILES + LEGAL_FILES:
        if not (pkg / name).is_file():
            raise SystemExit(f"{pkg}: {name} is missing")
    meta["name"] = NAME
    meta["license"] = LICENSE
    # npm trusted publishing and provenance require repository.url to name this repository.
    meta["repository"] = {"type": "git", "url": REPOSITORY}
    meta["publishConfig"] = {"access": "public", "provenance": True}
    files = meta.setdefault("files", [])
    if "NOTICE" not in files:
        files.append("NOTICE")
    path.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    return meta


def check_pack(pkg: pathlib.Path) -> list[str]:
    """The files `npm pack` would publish; refuses a tarball without the legal or module files."""
    # An absolute path: npm reads a relative `release/pkg` as the GitHub repository release/pkg.
    out = subprocess.run(
        ["npm", "pack", "--dry-run", "--json", str(pkg.resolve())],
        check=True, capture_output=True, text=True,
    ).stdout
    packed = [entry["path"] for entry in json.loads(out)[0]["files"]]
    missing = [name for name in LEGAL_FILES + MODULE_FILES if name not in packed]
    if missing:
        raise SystemExit(f"{pkg}: npm pack leaves out {', '.join(missing)}")
    return packed


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("pkg", type=pathlib.Path)
    parser.add_argument("--version")
    parser.add_argument("--check-pack", action="store_true")
    args = parser.parse_args(argv)
    meta = prepare(args.pkg, args.version)
    print(f"{meta['name']}@{meta['version']} ready in {args.pkg}")
    if args.check_pack:
        print("packs: " + ", ".join(sorted(check_pack(args.pkg))))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
