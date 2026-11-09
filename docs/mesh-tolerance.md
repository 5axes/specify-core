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
Measurement uses the emitted coordinates, rounded to the existing four decimal
places. Every triangle's vertices, three edge midpoints, centroid, the three
permutations of barycentric (2/3, 1/6, 1/6), and its circumcentre projected into
the closed triangle are projected to the exact supporting surface with
haecceity's `Surface::parameters` and `Surface::value`. Analytic projections are
closed form; freeform inversion uses interpolated parameter hints. Distances are
measured in 3D, independently of the triangulator target and convergence flag.
For spheres, tori and B-spline surfaces the sampled maximum must be at most 95%
of the request before acceptance; the reported number is the measured distance,
not this safety margin. This remains a **sampled maximum, not a proof of the
continuous maximum** on an arbitrary surface.

Triangle-edge incidence identifies emitted boundary segments. Each segment's
rounded endpoints and midpoint are measured against an exact trimmed curve, using
`Curve::parameter`/`value` and the edge's parameter interval. Kernel edge labels
identify candidate owners when available. Otherwise all physical trim curves are
candidates, including for segments with an unlabelled endpoint. The smallest
maximum of these three distances selects the nearest candidate. No association
failure substitutes the requested target or a source polyline for a measurement.
If a source trim has no assigned mesh segment, its exact-curve sample points are
measured against the actual emitted boundary segments. These reverse coverage
distances also contribute to `achieved_deviation_mm`.

Degenerate source edges and twice-used seam edges are not physical trims.
A physical trim owner is established by kernel labels, or by both unrounded
endpoints projecting to the same trim within `1e-7 * bbox diagonal + 1e-8` mm.
Interior periodic cuts are excluded only when **both** sides have no physical
trim owner and their oppositely directed UV intervals cover each other across
one verified surface period (constant-coordinate agreement within one millionth
of the period). This permits different subdivisions. Coordinate duplication alone
never excludes a segment; a doubled real trim remains measured. Zero-length
rounded segments have no chord sag; their vertices still undergo surface checks.
The opt-in local triangulator retains its tiny triangles at parameter
seams and poles, rather than deleting them and creating unmeasured gaps.
Closed NURBS curves receive two extra one-sided projection seeds at their seam
(see upstream need U16). Unresolved curves, missing emitted boundary coverage and
non-finite projections refuse the mesh. Rounded triangle areas must sum to within
1% of haecceity's exact-face boundary-quadrature area; otherwise the face is not
verified and is refined or refused. This coverage check detects significant
holes/overlap but does not certify topology or locate defects.

The nearest-curve rule measures distance to the boundary union. In a narrow face,
a chord intended for trim A may be closer to trim B. In particular, faces narrower
than about four times the triangulator target can put both trims within a
plausible matching reach. The unlabelled fallback considers the entire boundary
union rather than imposing a reach gate; without reliable edge labels, its
smaller distance can understate deviation from the intended trim A. The value does not certify
trim ownership, an unsampled trim extremum, or a continuous Hausdorff bound.
Boundary sampling does detect chord sag on a planar face round a hole, where the
supporting-surface distance alone is zero.

The result is **not a watertightness guarantee**. Faces are triangulated and refined
independently: adjacent faces can choose different samples of the same trim curve
and leave gaps even when both sampled bounds pass. No common edge subdivision or
mesh welding is imposed; gaps between faces are not measured. The reported samples do not bound every point of a
trim curve or certify topology. Displayed `edges` polylines still use the initial
deflection and are not included in `achieved_deviation_mm`.

A measured deviation above the acceptance bound triggers a fresh triangulation
with half the deflection, up to eight attempts per face. Area failures reduce
the deflection by a factor of ten to resolve narrow faces without spending all
attempts at an unsuitable scale. Retrying stops immediately when a triangulation
reports `converged=false`; lowering its target cannot add a larger vertex budget.
The refusal reports the **best** measured attempt, the actual attempt count, and
the stopping reason, including when a later triangulation fails or worsens.
Each mesher retains its existing 60,000-vertex and 40-round bounds. If the local
quality path fails, the opt-in path tries haecceity's existing `Part::triangulate`
at the original target. Its edge-labelled boundary points identify physical trims,
and its per-edge discretization avoids local UV boundary jumps and crossing failures at finer targets. The same surface samples,
exact trim measurements, finite-value checks and stop-on-unconverged rule apply.
The fallback changes neither haecceity nor the default picker path.

There is no default-meshing pre-pass. Only when quality verification fails is the
default path checked for unsupported faces. A face it lists in `missing` refuses
the requested mesh with
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
