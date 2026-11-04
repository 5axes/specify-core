# Sealed interior voids

`sealed_cavities.step.gz` is a synthetic 70 × 50 × 28 block with four closed ⌀4 × 12
cylindrical cavities (z 8–20, centred at (±30, ±20)), exported by build123d 0.11.1 as one
solid with `BREP_WITH_VOIDS` (four inner `ORIENTED_CLOSED_SHELL`s). The cavity walls are
faces 6/9/12/15 with caps 7-8, 10-11, 13-14 and 16-17; quiddity reports them as
`interior_voids`. `tests/interior_voids.rs` and the Python smoke suite check that review
states each one as an `info` fact with its faces and size. The pinned kernel also reports
the cavities as holes; the tests do not depend on that either way.

The cavities are ⌀4, not ⌀2.5: at exactly (±30, ±20) the pinned kernel's sampling grid
(64 cells on the longest axis) finds no ⌀2.5 cavity, though it finds them 0.1 mm off.

Regenerate with build123d (writes the gzip with mtime 0 into the given directory):

```bash
python sealed_cavities.py tests/fixtures/voids
```
