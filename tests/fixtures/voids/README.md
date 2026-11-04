# Sealed interior voids

`sealed_cavities.step.gz` is a synthetic 70 × 50 × 28 block with four closed ⌀2.5 × 12
cylindrical cavities (z 8–20, centred at (±30, ±20)), exported by build123d 0.13.0 as one
solid with `BREP_WITH_VOIDS` (four inner `ORIENTED_CLOSED_SHELL`s). The cavity walls are
faces 6/9/12/15 with caps 7-8, 10-11, 13-14 and 16-17. quiddity 0.4.7 reports each one only
as `interior_voids`, with an exact `closed_shell` record (`openings: []`, volume
π × 1.25² × 12 ≈ 58.905 mm³), and never as a hole. `tests/interior_voids.rs` and the Python
smoke suite check that, and that review states each one as an `info` fact with its faces,
size and volume.

The cavities were ⌀4 until quiddity 0.4.7: at exactly (±30, ±20) the 0.4.6 sampling grid (64
cells on the longest axis) found no ⌀2.5 cavity. 0.4.7 proves sealed voids from the shell
topology, so the grid's resolution no longer matters.

Regenerate with build123d (writes the gzip with mtime 0 into the given directory; the STEP
header's timestamp still changes, nothing else does):

```bash
python sealed_cavities.py tests/fixtures/voids
```
