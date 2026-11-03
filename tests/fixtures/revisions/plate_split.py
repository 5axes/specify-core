"""Generate the split-bottom workflow regression (build123d)."""
import gzip
from pathlib import Path
import sys
import build123d as bd
out = sys.argv[1]
def plate(groove: bool):
    body = bd.Box(100, 60, 10) - bd.Pos(-30, 0, 0) * bd.Cylinder(4, 10) - bd.Pos(30, 0, 0) * bd.Cylinder(4, 10)
    if groove:  # a groove across the bottom: the bottom face becomes two
        body = body - bd.Pos(0, 0, -5) * bd.Box(6, 60, 4)
    return body
for which, groove in [("old", False), ("new", True)]:
    path = Path(out) / f"plate_split.{which}.step"
    bd.export_step(plate(groove), path)
    path.with_suffix(".step.gz").write_bytes(gzip.compress(path.read_bytes(), mtime=0))
    path.unlink()
