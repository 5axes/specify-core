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
/// Measurement uses the *rounded output* vertices, three edge midpoints and centroid
/// against haecceity's exact surface. This is a sampled deviation, not a continuous proof.
pub fn mesh_with_max_deviation(
    loaded: &LoadedPart,
    max_deviation_mm: f64,
) -> Result<Value, String> {
    validate_max_deviation(max_deviation_mm)?;
    let mut meshes = Vec::with_capacity(loaded.binding.face_count);
    let mut achieved = 0.0_f64;
    let initial = deflection(loaded).min(max_deviation_mm);
    for face in 0..loaded.binding.face_count {
        let mut target = initial;
        let mut accepted = None;
        let mut measured = f64::INFINITY;
        // Bound retries independently of triangulate's own vertex/round limits. A
        // quantisation floor or an unrefinable boundary must terminate with a reason.
        for _ in 0..8 {
            if loaded.unresolved_faces.contains(&face)
                || loaded.part.unresolved_faces().contains(&face)
            {
                return Err(format!(
                    "face {face}: cannot meet max_deviation_mm: its surface did not resolve"
                ));
            }
            let mesh = triangulate::triangulate_for_deviation(
                &loaded.part,
                face,
                target,
                ANGULAR_DEFLECTION,
            )
            .map_err(|why| format!("face {face}: cannot meet max_deviation_mm: {why}"))?;
            measured = measured_deviation(loaded, face, &mesh)
                .map_err(|why| format!("face {face}: cannot measure max_deviation_mm: {why}"))?;
            if measured <= max_deviation_mm {
                accepted = Some(mesh);
                break;
            }
            target *= 0.5;
        }
        let mesh = accepted.ok_or_else(|| {
            format!(
                "face {face}: cannot meet max_deviation_mm {max_deviation_mm} mm after 8 attempts \
             (measured {measured} mm; coordinate rounding or refinement limit)"
            )
        })?;
        achieved = achieved.max(measured);
        meshes.push(mesh);
    }
    let mut result = mesh_with(loaded, |face, _| Ok(meshes[face].clone()));
    result["edges"] = json!(edges(loaded, initial));
    result["max_deviation_mm"] = json!(max_deviation_mm);
    result["achieved_deviation_mm"] = json!(achieved);
    Ok(result)
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
fn measured_deviation(loaded: &LoadedPart, face: usize, mesh: &FaceMesh) -> Result<f64, String> {
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
