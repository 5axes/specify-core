"""Run from the pinned text-to-cad checkout with its cadgen venv Python.

Usage: .venv/bin/python /path/to/generate_plate.py /path/to/fixture-directory
No text-to-cad tooling is needed by Specify's tests. See PROVENANCE.md.
"""
from pathlib import Path
import gzip
import os
import subprocess
import sys
import tempfile

PIN = "d18aadfe97869ede0e7f48ee0830e040449cc80d"
SOURCE = Path("models/examples/src/pmi_plate.py")
CONSTRUCTION = "bd.Box(100, 60, 10) - bd.Cylinder(5, 10)"


def main():
    assert subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip() == PIN
    assert f"    body = {CONSTRUCTION}\n" in SOURCE.read_text()
    out = Path(sys.argv[1]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    constructions = {
        "rev1": CONSTRUCTION,
        "rev2": "bd.Box(100, 60, 12) - bd.Cylinder(6, 12)",
        "rev3": CONSTRUCTION + " - bd.Pos(30, 15, 0) * bd.Cylinder(3, 10)",
    }
    with tempfile.TemporaryDirectory(prefix="specify-pmi-plate-") as temporary:
        root = Path(temporary)
        env = dict(os.environ, DO_NOT_TRACK="1", CADGEN_TELEMETRY="0",
                   CADGEN_STATE_DIR=str(root / "state"),
                   CADGEN_CACHE_DIR=str(root / "cache"),
                   CADGEN_ENGINE_DIR=str(root / "engine"))
        for revision, construction in constructions.items():
            folder = root / revision
            folder.mkdir()
            script = folder / "pmi_plate.py"
            # Keep the function, local variable, primitive expression and label.
            # Omit all annotate(...) PMI, face selections and visual material.
            script.write_text(
                'from __future__ import annotations\n'
                'import build123d as bd\nfrom cadgen import step\n\n'
                '@step(out="pmi_plate.step")\ndef pmi_plate():\n'
                f'    body = {construction}\n'
                '    body.label = "plate"\n    return body\n'
            )
            # cadgen's normal @step build/export, cold in this process rather
            # than starting a daemon. Do not assign or rewrite any face names.
            subprocess.run([
                sys.executable, "-c",
                "import sys; from pathlib import Path; "
                "from cadgen.engine.api import build_script; "
                "r = build_script(Path(sys.argv[1]), daemon=False); "
                "assert r.ok, r",
                str(script),
            ], env=env, check=True)
            data = (folder / "pmi_plate.step").read_bytes()
            packed = gzip.compress(data, compresslevel=9, mtime=0)
            (out / f"{revision}.step.gz").write_bytes(packed)
            print(f"{revision}: {len(data)} STEP bytes, {len(packed)} gzip bytes")


if __name__ == "__main__":
    main()
