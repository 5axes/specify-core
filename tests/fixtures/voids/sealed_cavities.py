"""Generate the sealed interior-void specimen (build123d): a 70 × 50 × 28 block with four
closed ⌀4 × 12 cylindrical cavities, z 8–20, centred at (±30, ±20). No cavity reaches a face."""
import gzip
from pathlib import Path
import sys
import build123d as bd
out = Path(sys.argv[1])
body = bd.Pos(0, 0, 14) * bd.Box(70, 50, 28)
for x in (-30, 30):
    for y in (-20, 20):
        body = body - bd.Pos(x, y, 14) * bd.Cylinder(2, 12)
path = out / "sealed_cavities.step"
bd.export_step(body, path)
path.with_suffix(".step.gz").write_bytes(gzip.compress(path.read_bytes(), mtime=0))
path.unlink()
