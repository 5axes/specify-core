# Mesh deviation

The default `pmi-assist-mesh` version 2 picker mesh uses a chordal deflection of
0.001 times the exact bounding-box diagonal, with angular deflection 0.3 radians.
All existing fields, omission rules and serialized bytes remain unchanged when
no tolerance is requested.

Request a positive finite `max_deviation_mm`, in millimetres:

- CLI: `specify-core mesh part.step -o part.mesh.json --max_deviation_mm 0.05`.
- Serve: `{"op":"analyse", "step":"part.step", "max_deviation_mm":0.05}`.
- Python: `api.mesh(step, max_deviation_mm=0.05)` (path or loaded part).
- WASM: `engine.snapshot(0.05)`; the argument is `max_deviation_mm`.
- Rust: `api::mesh_with_max_deviation(&loaded, Some(0.05))`, or the file variant
  `api::mesh_file_with_max_deviation(path, part, Some(0.05))`.

Absent/`None`/`null` uses the default path. A requested result adds two numeric
keys: `max_deviation_mm` (the request) and `achieved_deviation_mm` (the largest
measured distance across all faces). Mesh version stays 2: the existing fields
retain their meaning and readers consume named fields, ignoring unknown keys.
The additional keys occur only on opt-in requests; strict readers must allow them.
No crate version changes.

Each face starts with the smaller of the request and the default deflection.
The emitted coordinates are rounded to the existing four decimal places before
measurement. Every triangle's vertices, three edge midpoints and centroid are
projected to the face's exact surface using haecceity's `Surface::parameters` and
`Surface::value`. Analytic projections are closed form; freeform inversion uses
interpolated parameter hints. Distances are measured in 3D, not inferred from the
triangulator's target or convergence flag. This sampled maximum is **not a proof
of the continuous maximum** over every point of an arbitrary triangle or surface.
Boundary polylines use the initial deflection; the reported deviation describes
the triangles, not the displayed edge polylines.

A measured deviation above the request triggers a fresh triangulation with half the deflection,
up to eight attempts per face. The triangulator's existing 60,000-vertex and
40-round bounds remain in force. An empty, unresolved, unprojectable or still
out-of-tolerance face refuses the **whole mesh** with its face index and reason;
no partial CAM mesh is returned. Coordinate rounding can prevent very fine
requests from succeeding. Without a request, unmeshed faces continue to appear
in `missing` as before. The opt-in path also handles whole spheres whose STEP
boundary consists only of pole vertex loops, using haecceity's full-sphere bounds.
