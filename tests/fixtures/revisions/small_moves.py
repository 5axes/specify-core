# Small moves for Specify's diff (build123d 0.11.0): a 100 × 60 × 10 plate with unequal anchor
# holes, Ø6 at (20, 15) and Ø8 at (85, 45), and one feature moved between the revisions:
#   hole_nudged:  a Ø5 hole at (60, 20) moves 0.1 mm in X;
#   pocket_moved: a 30 × 14 × 4 pocket centred at (50, 30) moves 3 mm in X;
#   slot_slid:    a 16 × 6 through slot along Y at (35, 30) slides 4 mm in X.
# Run: python small_moves.py OUTDIR, then gzip -9 -n OUTDIR/*.step.
import sys
from build123d import Box, Cylinder, Pos, SlotOverall, extrude, export_step, Plane, Align

MIN = (Align.MIN, Align.MIN, Align.MIN)


def plate():
    p = Box(100, 60, 10, align=MIN)
    for x, y, d in [(20, 15, 6), (85, 45, 8)]:
        p -= Pos(x, y, 5) * Cylinder(d / 2, 10)
    return p


def hole(x):
    return plate() - Pos(x, 20, 5) * Cylinder(2.5, 10)


def pocket(x):
    return plate() - Pos(x, 30, 8) * Box(30, 14, 4)


def slot(x):
    s = extrude(Plane.XY * Pos(x, 30) * SlotOverall(16, 6, rotation=90), amount=10)
    return plate() - s


out = sys.argv[1]
for name, make, old, new in [
    ("hole_nudged", hole, 60, 60.1),
    ("pocket_moved", pocket, 50, 53),
    ("slot_slid", slot, 35, 39),
]:
    export_step(make(old), f"{out}/{name}.old.step")
    export_step(make(new), f"{out}/{name}.new.step")
