# Mesh deviation

The default `pmi-assist-mesh` version 2 picker mesh uses a chordal deflection of
0.001 times the exact bounding-box diagonal, with angular deflection 0.3 radians.
All existing fields, omission rules and serialized bytes remain unchanged when
no tolerance is requested.

Request a positive finite `max_deviation_mm`, in millimetres:

- CLI: `specify-core mesh part.step -o part.mesh.json --max-deviation-mm 0.05`.
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
measurement. Every triangle's vertices, three edge midpoints, centroid and the three
permutations of barycentric (2/3, 1/6, 1/6) are projected to the face's exact surface using haecceity's `Surface::parameters` and
`Surface::value`. Analytic projections are closed form; freeform inversion uses
interpolated parameter hints. Distances are measured in 3D, not inferred from the
triangulator's target or convergence flag. This sampled maximum is **not a proof
of the continuous maximum** over every point of an arbitrary triangle or surface.
Boundary triangle-edge midpoints and vertices also contribute their distance to the
exact trim curve, using haecceity's `Curve::parameter`/`value` and its trimmed edge interval.
Triangle edge incidence identifies boundary segments; their unrounded endpoints
associate them with physical trim curves within the larger of four times the current
triangulator target and `1e-7 * bbox diagonal + 1e-8` mm. This association reach is
not an achieved value: every accepted segment is measured against its exact curve.
Nearby corner curves are alternative owners: the smallest sampled segment distance
among candidate curves is retained. Taking the maximum would measure a chord on
one edge against an unrelated neighbouring edge and create false refusals.
The reach accounts for boundary vertices inserted along parameter chords, which
can lie off the trim curve after being mapped onto the surface.
Projection and distance measurement use the rounded emitted coordinates. Degenerate
edges and seams used twice by the same face are excluded. Parameter cuts are
recognized by a full analytic period's extreme parameter lines, or by paired
boundary segments at identical rounded endpoint coordinates. Slightly displaced
cut edges left by omitted slivers are recognized within the same association reach
of those exact parameter lines when they match no physical trim curve; they are
internal cut/gap edges and do not contribute a trim-curve distance.
Association is an optimization, not a validity test. A segment with no gated
candidate is measured against every exact trim interval and uses the nearest one.
If a trim has no associated emitted segment (for example beside an omitted sliver),
its source polyline is thinned at the current target and its rounded vertices and
chord midpoints are projected onto that exact curve; this measured fallback also
contributes to `achieved_deviation_mm`. Neither fallback echoes the target as an
achieved value. Unresolved curves and non-finite projections still refuse clearly.
Closed NURBS curves get two extra one-sided projection seeds at their seam: a
clamped Newton inversion at one endpoint can otherwise miss the nearer branch.
This measures boundary chord sag even on a plane round a circular hole,
whose supporting-surface distance alone would be zero.

The result is **not a watertightness guarantee**. Faces are triangulated and refined
independently: adjacent faces can choose different samples of the same trim curve
and leave gaps even when both sampled bounds pass. No common edge subdivision or
mesh welding is imposed; gaps between faces are not measured. The reported samples do not bound every point of a
trim curve or certify topology. Displayed `edges` polylines still use the initial
deflection and are not included in `achieved_deviation_mm`.

A measured deviation above the request triggers a fresh triangulation with half the deflection,
up to eight attempts per face. Retrying stops immediately when a triangulation
reports `converged=false`; lowering its target cannot add a larger vertex budget.
The refusal reports the **best** measured attempt, the actual attempt count, and
the stopping reason, including when a later triangulation fails or worsens.
Each mesher retains its existing 60,000-vertex and 40-round bounds. If the local
quality path fails, the opt-in path tries haecceity's existing `Part::triangulate`
at the original target. Its edge-labelled boundary points avoid treating internal
cut/sliver-gap edges as trims, and its per-edge discretization avoids local UV
boundary jumps and crossing failures at finer targets. The same surface samples,
exact trim measurements, finite-value checks and stop-on-unconverged rule apply.
The fallback changes neither haecceity nor the default picker path.

Before quality meshing, the default path is checked for unsupported faces. A face
it already lists in `missing` refuses the requested mesh with
`face N: does not triangulate at any supported tolerance (default mesh lists it as missing: ...)`.
This is the pipeline's unsupported-topology policy, not a mathematical assertion
about every possible mesher. Explicitly supported whole pole-loop spheres remain
the exception: their opt-in analytic construction is used as before.
An empty, unresolved, unprojectable or still out-of-tolerance face refuses the **whole mesh** with its face index and reason;
no partial CAM mesh is returned. Coordinate rounding can prevent very fine
requests from succeeding. Without a request, unmeshed faces continue to appear
in `missing` as before. The opt-in path also handles whole spheres whose STEP
boundary consists only of pole vertex loops, using haecceity's full-sphere bounds.

Complete tori and circular cylindrical bands use an opt-in parameter grid. Angular
spacing is chosen independently from the radius of each curved direction (R+r
and r on a torus); cylinders need only the two ends along their straight rulings.
Periodic seams are shared. Trimmed or holed patches use the general triangulator.
The grid still undergoes the same rounded-coordinate measurement and refusal checks.

## Full corpus regression

Run the ignored real-part survey explicitly:

```sh
QUIDDITY_CORPUS_REQUIRED=1 cargo test -j 6 --release --test mesh corpus_requested_deviation -- --ignored --nocapture
```

It walks every STEP/STP file (including gzip) and every part, requests 0.1 mm,
checks finite achieved deviations against the request, and permits refusals only
at faces already missing in the default mesh. Any additional allowed refusal must
have an explicit path, part, face and diagnostic reason in the test; stale entries
fail. There are currently no additional exceptions. This is independent of the
normal oracle corpus subset and never deletes or weakens a known-difference entry.
