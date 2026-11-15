"""For #181: a top-filleted Ø65 boss shortened 13 → 8 mm, keeping its R3 top fillet
(build123d 0.11.0). Run: python top_filleted_boss.py OUTDIR. All sizes are mm.
"""
import gzip
from pathlib import Path
import sys
from build123d import Align, Box, Cylinder, export_step, fillet


def boss(height):
    base = Box(120, 120, 10, align=(Align.CENTER, Align.CENTER, Align.MAX))
    part = base + Cylinder(32.5, height, align=(Align.CENTER, Align.CENTER, Align.MIN))
    top = [e for e in part.edges() if abs(e.center().Z - height) < 1e-3]
    return fillet(top, 3.0)


out = Path(sys.argv[1])
for which, height in (("old", 13.0), ("new", 8.0)):
    step = out / f"boss_shortened.{which}.step"
    export_step(boss(height), str(step))
    (out / f"{step.name}.gz").write_bytes(gzip.compress(step.read_bytes(), 9, mtime=0))
    step.unlink()
