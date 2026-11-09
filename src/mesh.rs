//! A triangle mesh of the part whose faces carry specify-core's face indices (port of
//! `mesh.py`).
//!
//! A picker shows the part and reports which face was clicked. It must report the index the
//! analysis, questions and intent use, so the mesh is made here, from the same loaded part and
//! face numbering, rather than by a viewer re-reading the STEP file and numbering faces its own
//! way. The mesh carries the analysis binding, so a mesh and an analysis of different files
//! cannot be paired.
//!
//! Triangles are stored face by face: face `i` owns triangles `faces[i][0] .. faces[i][0] +
//! faces[i][1] - 1`, so a hit triangle maps to its face by a search over the starts. Vertices
//! are not shared between faces, which keeps each face's shading and highlight its own. A face
//! that does not triangulate has an empty range and is listed in `missing`, so a picker can say
//! so rather than show a hole.
//!
//! Coordinates are the part's own, the frame the analysis describes; for a part stored inside
//! an assembly that is not necessarily where a CAD viewer places it.
//!
//! What differs from Python: the triangles come from the local stand-in for OpenCascade's
//! `BRepMesh` ([`crate::triangulate`], upstream need U4), so they are not Python's triangles
//! (the format promises faces, not a particular triangulation). The box is the exact geometry's
//! (haecceity's `Part::bounds`), where OpenCascade's `BRepBndLib` box may be loose. Meshing
//! reads the part and changes nothing, so it is independent of what ran before (Python meshes a
//! copy for that reason).

use std::collections::BTreeMap;

use haecceity::geom::{self, V3};
use serde_json::{Value, json};

use crate::part::LoadedPart;
use crate::pyfmt::round_half_even;
use crate::triangulate::{self, FaceMesh};

/// A wire format readers check for: its name stays as first published.
pub const MESH_FORMAT: &str = "pmi-assist-mesh";
pub const MESH_VERSION: u32 = 2;

/// Chordal deflection as a share of the part's diagonal: fine enough that small holes are
/// round, coarse enough that a 300-face part stays a few megabytes.
pub const DEFLECTION: f64 = 0.001;
/// Angular deflection, radians.
pub const ANGULAR_DEFLECTION: f64 = 0.3;

/// Coordinates are written to this many decimals of the model unit (mm).
pub const DECIMALS: i32 = 4;

/// The diagonal of the part's exact bounding box, or 1.0 for a part with no extent (Python's
/// `lo.Distance(hi) or 1.0`). Anchors scale their reach by it too.
pub fn diagonal(loaded: &LoadedPart) -> f64 {
    let d = loaded.part.bounds().diagonal();
    if d > 0.0 && d.is_finite() { d } else { 1.0 }
}

/// The chordal deflection the mesh is made to (mm).
pub fn deflection(loaded: &LoadedPart) -> f64 {
    diagonal(loaded) * DEFLECTION
}

/// Face `face` triangulated to the mesh's deflections, or why it cannot be. A face whose
/// surface haecceity could not resolve is refused here, also for a part a caller rebuilt
/// (`LoadedPart::rebuilt`, whose `Part` no longer lists it).
pub fn face_mesh(loaded: &LoadedPart, face: usize, deflection: f64) -> Result<FaceMesh, String> {
    if loaded.unresolved_faces.contains(&face) || loaded.part.unresolved_faces().contains(&face) {
        return Err("its surface did not resolve".into());
    }
    triangulate::triangulate(&loaded.part, face, deflection, ANGULAR_DEFLECTION)
}

/// The part as JSON-safe triangles, face-ranged by specify-core face index: the
/// `pmi-assist-mesh` version 2 format.
pub fn mesh(loaded: &LoadedPart) -> Value {
    mesh_with(loaded, |face, deflection| {
        face_mesh(loaded, face, deflection)
    })
}

/// Mesh with a measured maximum chordal deviation in millimetres. Unlike the picker
/// path, this refuses the entire mesh if any face cannot be triangulated or verified.
/// Measurement uses the *rounded output* vertices, edge midpoints, centroid and
/// three near-corner samples, plus trim-boundary edge midpoints
/// against haecceity's exact surfaces and trim curves. This is sampled, not a continuous proof.
pub fn mesh_with_max_deviation(
    loaded: &LoadedPart,
    max_deviation_mm: f64,
) -> Result<Value, String> {
    validate_max_deviation(max_deviation_mm)?;
    let mut meshes = Vec::with_capacity(loaded.binding.face_count);
    let mut achieved = 0.0_f64;
    let initial = deflection(loaded).min(max_deviation_mm);
    for face in 0..loaded.binding.face_count {
        if loaded.unresolved_faces.contains(&face) || loaded.part.unresolved_faces().contains(&face)
        {
            return Err(format!(
                "face {face}: cannot meet max_deviation_mm: its surface did not resolve"
            ));
        }
        let (mesh, measured) = verified_face(loaded, face, initial, max_deviation_mm, |target| {
            triangulate::triangulate_for_deviation(&loaded.part, face, target, ANGULAR_DEFLECTION)
        })
        .map_err(|why| format!("face {face}: {why}"))?;
        achieved = achieved.max(measured);
        meshes.push(mesh);
    }
    let mut result = mesh_with(loaded, |face, _| Ok(meshes[face].clone()));
    result["edges"] = json!(edges(loaded, initial));
    result["max_deviation_mm"] = json!(max_deviation_mm);
    result["achieved_deviation_mm"] = json!(achieved);
    Ok(result)
}

/// Retain the best measured mesh, and stop when the triangulator exhausts its budget.
/// A later, finer target must never replace a better measurement in a refusal.
fn verified_face(
    loaded: &LoadedPart,
    face: usize,
    initial: f64,
    limit: f64,
    mut triangulate: impl FnMut(f64) -> Result<FaceMesh, String>,
) -> Result<(FaceMesh, f64), String> {
    let mut best: Option<(FaceMesh, f64)> = None;
    let mut target = initial;
    let mut attempts = 0;
    let mut stopped = "8-attempt refinement limit".to_string();
    for _ in 0..8 {
        attempts += 1;
        let mesh = match triangulate(target) {
            Ok(mesh) => mesh,
            Err(why) => {
                stopped = why;
                break;
            }
        };
        let measured = match measured_deviation(loaded, face, &mesh, target) {
            Ok(value) => value,
            Err(why) => {
                stopped = format!("cannot measure deviation: {why}");
                break;
            }
        };
        let converged = mesh.converged;
        if best.as_ref().is_none_or(|(_, value)| measured < *value) {
            best = Some((mesh, measured));
        }
        if best.as_ref().is_some_and(|(_, value)| *value <= limit) {
            return best.ok_or_else(|| "no measured mesh".into());
        }
        if !converged {
            stopped = "triangulator did not converge (vertex or round refinement limit)".into();
            break;
        }
        target *= 0.5;
    }
    let measurement = match best {
        Some((_, value)) => format!("best measured {value} mm"),
        None => "no measurable attempt".into(),
    };
    Err(format!(
        "cannot meet max_deviation_mm {limit} mm after {attempts} attempt(s) \
        ({measurement}; {stopped})"
    ))
}

/// Shared validation for every host interface, including before loading a file.
pub fn validate_max_deviation(max_deviation_mm: f64) -> Result<(), String> {
    if max_deviation_mm.is_finite() && max_deviation_mm > 0.0 {
        Ok(())
    } else {
        Err("max_deviation_mm must be a positive finite number in mm".into())
    }
}

/// Maximum sampled distance of the emitted triangles to their face's exact surface.
/// Freeform inversion is seeded with each sample's interpolated surface parameters;
/// analytic surfaces use the kernel's closed-form closest-point projection.
fn measured_deviation(
    loaded: &LoadedPart,
    face: usize,
    mesh: &FaceMesh,
    target: f64,
) -> Result<f64, String> {
    if mesh.triangles.is_empty() {
        return Err("no triangles".into());
    }
    let surface = &loaded.part.faces[face].surface;
    let mut worst = 0.0_f64;
    for triangle in &mesh.triangles {
        let corners = triangle.map(|i| point(mesh.points[i]));
        let uv = triangle.map(|i| mesh.uv[i]);
        for weights in [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.5, 0.5, 0.0],
            [0.0, 0.5, 0.5],
            [0.5, 0.0, 0.5],
            [1.0 / 3.0; 3],
            [2.0 / 3.0, 1.0 / 6.0, 1.0 / 6.0],
            [1.0 / 6.0, 2.0 / 3.0, 1.0 / 6.0],
            [1.0 / 6.0, 1.0 / 6.0, 2.0 / 3.0],
        ] {
            let mut sample = [0.0; 3];
            let mut hint = (0.0, 0.0);
            for i in 0..3 {
                sample = geom::add(sample, geom::scale(corners[i], weights[i]));
                hint.0 += uv[i].0 * weights[i];
                hint.1 += uv[i].1 * weights[i];
            }
            let (u, v) = surface
                .parameters(sample, Some(hint))
                .ok_or("surface projection unavailable")?;
            let distance = geom::dist(sample, surface.value(u, v));
            if !distance.is_finite() {
                return Err("surface projection returned a non-finite distance".into());
            }
            worst = worst.max(distance);
        }
    }
    Ok(worst.max(boundary_deviation(loaded, face, mesh, target)?))
}

/// The exact trim curves, rather than the untrimmed supporting surface, bound a
/// face's polygon. Artificial parameter cuts and seams are not physical trim edges.
fn boundary_deviation(
    loaded: &LoadedPart,
    face: usize,
    mesh: &FaceMesh,
    target: f64,
) -> Result<f64, String> {
    let part = &loaded.part;
    let mut uses = BTreeMap::new();
    for &(edge, _) in part.faces[face].loops.iter().flat_map(|lp| &lp.edges) {
        *uses.entry(edge).or_insert(0_usize) += 1;
    }
    let tolerance = 1e-7 * diagonal(loaded) + 1e-8;
    // Inserted boundary vertices follow parameter chords mapped onto the surface,
    // so they need not lie exactly on the trim curve. Reach only associates them;
    // their actual curve distances are measured below, never assumed from target.
    let association = tolerance.max(4.0 * target);
    let mut curves = Vec::new();
    for (edge, count) in uses {
        if count > 1 {
            continue;
        } // a periodic seam, not a trim
        if loaded.unresolved_edges.contains(&edge) || part.unresolved_edges().contains(&edge) {
            return Err(format!("boundary edge {edge}: its curve did not resolve"));
        }
        let e = &part.edges[edge];
        if e.samples
            .windows(2)
            .map(|w| geom::dist(w[0], w[1]))
            .sum::<f64>()
            <= tolerance
        {
            continue; // a degenerate edge carries no chord
        }
        let (a, b) = haecceity::sampling::edge_interval(
            &e.curve,
            e.start,
            e.end,
            e.same_sense,
            e.vertices.0 == e.vertices.1,
        );
        let mut bounds = geom::Bounds::empty();
        for &point in &e.samples {
            bounds.add(point);
        }
        curves.push((edge, &e.curve, (a.min(b), a.max(b)), bounds));
    }
    if curves.is_empty() {
        return Ok(0.0);
    }
    let mut segments = BTreeMap::new();
    for &[a, b, c] in &mesh.triangles {
        for (i, j) in [(a, b), (b, c), (c, a)] {
            *segments.entry((i.min(j), i.max(j))).or_insert(0_usize) += 1;
        }
    }
    // Match paired cuts by emitted coordinates. Analytic cut lines below also
    // identify cuts with unequal subdivisions or displaced sliver boundaries.
    let key =
        |vertex: usize| point(mesh.points[vertex]).map(|x| if x == 0.0 { 0 } else { x.to_bits() });
    let segment_key = |a: usize, b: usize| {
        let (a, b) = (key(a), key(b));
        if a <= b { (a, b) } else { (b, a) }
    };
    let mut paired = BTreeMap::new();
    for (&(a, b), &count) in &segments {
        if count == 1 {
            *paired.entry(segment_key(a, b)).or_insert(0_usize) += 1;
        }
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for &(u, v) in &mesh.uv {
        for (axis, x) in [(0, u), (1, v)] {
            lo[axis] = lo[axis].min(x);
            hi[axis] = hi[axis].max(x);
        }
    }
    let period = part.faces[face].surface.periodic();
    let parameter_cut = |a: usize, b: usize, reach: f64| {
        [(0, period.0), (1, period.1)]
            .into_iter()
            .any(|(axis, periodic)| {
                periodic
                    && ((hi[axis] - lo[axis]) - std::f64::consts::TAU).abs() <= 1e-6
                    && [a, b].into_iter().all(|vertex| {
                        let (u, v) = mesh.uv[vertex];
                        let at = |cut| {
                            if axis == 0 {
                                part.faces[face].surface.value(cut, v)
                            } else {
                                part.faces[face].surface.value(u, cut)
                            }
                        };
                        geom::dist(mesh.points[vertex], at(lo[axis]))
                            .min(geom::dist(mesh.points[vertex], at(hi[axis])))
                            <= reach
                    })
            })
    };
    let project = |curve: &geom::Curve, interval: (f64, f64), p: V3| {
        let mut t = curve.parameter(p);
        if let Some(period) = curve.period() {
            t += period * (((interval.0 + interval.1) * 0.5 - t) / period).round();
        }
        curve.value(t.clamp(interval.0, interval.1))
    };
    // A vertex can be shared by several trim edges. Cache all of its associations.
    let mut matches = BTreeMap::<usize, Vec<usize>>::new();
    let mut covered = vec![false; curves.len()];
    let mut worst = 0.0_f64;
    for ((a, b), count) in segments {
        if count != 1 || parameter_cut(a, b, tolerance) {
            continue;
        }
        for vertex in [a, b] {
            matches.entry(vertex).or_insert_with(|| {
                curves
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (_, curve, interval, bounds))| {
                        let p = mesh.points[vertex];
                        (bounds.contains(p, association + haecceity::sampling::CHORD_TOLERANCE)
                            && geom::dist(p, project(curve, *interval, p)) <= association)
                            .then_some(i)
                    })
                    .collect()
            });
        }
        let middle = geom::scale(geom::add(point(mesh.points[a]), point(mesh.points[b])), 0.5);
        let mut distance = f64::INFINITY;
        for &i in &matches[&a] {
            if !matches[&b].contains(&i) {
                continue;
            }
            let (_, curve, interval, _) = &curves[i];
            let off = [point(mesh.points[a]), middle, point(mesh.points[b])]
                .into_iter()
                .try_fold(0.0_f64, |worst, sample| {
                    let off = geom::dist(sample, project(curve, *interval, sample));
                    if off.is_finite() {
                        Ok(worst.max(off))
                    } else {
                        Err("boundary projection returned a non-finite distance")
                    }
                })?;
            distance = distance.min(off);
            covered[i] = true;
        }
        if distance.is_finite() {
            worst = worst.max(distance);
        } else if paired.get(&segment_key(a, b)).copied().unwrap_or(0) < 2
            && !parameter_cut(a, b, association)
        {
            return Err("cannot associate a boundary segment with an exact trim curve or periodic parameter cut".into());
        }
    }
    for (i, found) in covered.iter().enumerate() {
        if !found {
            return Err(format!(
                "boundary edge {} cannot be associated with emitted triangles",
                curves[i].0
            ));
        }
    }
    Ok(worst)
}

/// [`mesh`] with the face triangulation supplied, so a test can make a face fail.
pub fn mesh_with(
    loaded: &LoadedPart,
    mut triangulate: impl FnMut(usize, f64) -> Result<FaceMesh, String>,
) -> Value {
    let bounds = loaded.part.bounds();
    let deflection = deflection(loaded);

    let mut positions: Vec<f64> = Vec::new();
    let mut indices: Vec<usize> = Vec::new();
    let mut ranges: Vec<[usize; 2]> = Vec::new();
    let mut missing: Vec<usize> = Vec::new();
    for face in 0..loaded.binding.face_count {
        let start = indices.len() / 3;
        let triangles = match triangulate(face, deflection) {
            Ok(m) if !m.triangles.is_empty() => m,
            _ => {
                ranges.push([start, 0]);
                missing.push(face);
                continue;
            }
        };
        let base = positions.len() / 3;
        for &p in &triangles.points {
            positions.extend(point(p));
        }
        for t in &triangles.triangles {
            indices.extend(t.iter().map(|&k| base + k));
        }
        ranges.push([start, indices.len() / 3 - start]);
    }

    json!({
        "format": MESH_FORMAT,
        "version": MESH_VERSION,
        "binding": loaded.binding,
        "bbox": [point(bounds.min), point(bounds.max)],
        "positions": positions,
        "indices": indices,
        "faces": ranges,
        "missing": missing,
        "edges": edges(loaded, deflection),
    })
}

/// A point rounded to [`DECIMALS`], as Python's `round`.
fn point(p: V3) -> [f64; 3] {
    p.map(|x| round_half_even(x, DECIMALS))
}

/// The boundaries between faces as flat polylines, each edge's samples thinned to the
/// deflection (Python samples the exact curve with `GCPnts_QuasiUniformDeflection`, chordal
/// deflection only). A seam (an edge a face uses twice, where a cylinder closes on itself) is no
/// boundary, and drawing it would make a bore look like two faces. A degenerate edge (no length)
/// and an edge whose curve did not resolve (only its chord is known) are left out.
pub fn edges(loaded: &LoadedPart, deflection: f64) -> Vec<Vec<f64>> {
    let part = &loaded.part;
    let mut out = Vec::new();
    for (e, edge) in part.edges.iter().enumerate() {
        if loaded.unresolved_edges.contains(&e) || part.unresolved_edges().contains(&e) {
            continue;
        }
        if is_seam(loaded, e) {
            continue;
        }
        let length: f64 = edge
            .samples
            .windows(2)
            // windows(2) yields exactly two samples.
            .map(|w| geom::dist(w[0], w[1]))
            .sum();
        if edge.samples.len() < 2 || length <= 1e-9 * deflection {
            continue;
        }
        let kept = triangulate::thin(&edge.samples, &[], None, deflection, None);
        out.push(
            kept.into_iter()
                // thin() returns indices from 0..edge.samples.len().
                .flat_map(|k| point(edge.samples[k]))
                .collect(),
        );
    }
    out
}

/// Whether edge `e` is a seam: some face's loops use it twice (OpenCascade's
/// `BRep_Tool::IsClosed(edge, face)`).
fn is_seam(loaded: &LoadedPart, e: usize) -> bool {
    let part = &loaded.part;
    // e enumerates part.edges; the kernel builds one adjacency entry per edge.
    part.edge_faces()[e].iter().any(|&f| {
        // The kernel constructs edge_faces from validated topology face indices.
        part.faces[f]
            .loops
            .iter()
            .flat_map(|l| &l.edges)
            .filter(|&&(edge, _)| edge == e)
            .count()
            >= 2
    })
}

#[cfg(test)]
mod tolerance_tests {
    use super::*;

    fn sphere() -> LoadedPart {
        crate::part::load_bytes(
            "sphere.step",
            include_bytes!("../tests/fixtures/parts/mesh_sphere.step").to_vec(),
        )
        .unwrap()
    }

    fn offset_mesh(offset: f64, converged: bool) -> FaceMesh {
        FaceMesh {
            points: vec![
                [60.0 - offset, 0.0, 0.0],
                [60.0 - offset, 0.01, 0.0],
                [60.0 - offset, 0.0, 0.01],
            ],
            uv: vec![(0.0, 0.0); 3],
            triangles: vec![[0, 1, 2]],
            converged,
            folded_dropped: (0, 0.0),
        }
    }

    #[test]
    fn exhausted_triangulation_stops_after_one_attempt() {
        let mut calls = 0;
        let error = verified_face(&sphere(), 0, 0.1, 0.1, |_| {
            calls += 1;
            Ok(offset_mesh(1.0, false))
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(error.contains("best measured 1 mm"), "{error}");
        assert!(error.contains("did not converge"), "{error}");
    }

    #[test]
    fn refusal_keeps_best_measurement_when_later_mesh_worsens_or_fails() {
        for later_error in [false, true] {
            let mut calls = 0;
            let error = verified_face(&sphere(), 0, 0.1, 0.1, |_| {
                calls += 1;
                if calls == 1 {
                    Ok(offset_mesh(1.0, true))
                } else if later_error {
                    Err("vertex refinement limit".into())
                } else {
                    Ok(offset_mesh(2.0, false))
                }
            })
            .unwrap_err();
            assert_eq!(calls, 2);
            assert!(error.contains("best measured 1 mm"), "{error}");
        }
    }
}
