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

/// A sampled, measured mesh in mm. The default picker path is deliberately separate.
pub fn mesh_with_max_deviation(
    loaded: &LoadedPart,
    max_deviation_mm: f64,
) -> Result<Value, String> {
    validate_max_deviation(max_deviation_mm)?;
    let diagonal = diagonal(loaded);
    let initial = (diagonal * DEFLECTION).min(max_deviation_mm);
    let mut meshes = Vec::with_capacity(loaded.binding.face_count);
    let mut achieved = 0.0_f64;
    for face in 0..loaded.binding.face_count {
        let verified = (|| {
            let check = FaceCheck::new(loaded, face, diagonal)?;
            let local = refine_measured(initial, max_deviation_mm, check.margin(), |target| {
                let mesh = triangulate::triangulate_for_deviation(
                    &loaded.part,
                    face,
                    target,
                    ANGULAR_DEFLECTION,
                )?;
                let measured = check.measure(&mesh, None)?;
                Ok((mesh, measured))
            });
            local.or_else(|local_reason| {
                refine_measured(initial, max_deviation_mm, check.margin(), |target| {
                    let k = loaded
                        .part
                        .triangulate(face, target, ANGULAR_DEFLECTION)
                        .map_err(|e| e.to_string())?;
                    let mesh = FaceMesh {
                        points: k.points,
                        uv: k.uv,
                        triangles: k.triangles,
                        converged: k.converged,
                        folded_dropped: k.folded_dropped,
                    };
                    let measured = check.measure(&mesh, Some(&k.boundary))?;
                    Ok((mesh, measured))
                })
                .map_err(|why| format!("{local_reason}; exact-edge kernel fallback: {why}"))
            })
        })()
        .map_err(|why| refusal(loaded, face, why))?;
        achieved = achieved.max(verified.1);
        meshes.push(verified.0);
    }
    let mut result = mesh_with_deflection(loaded, initial, |face, _| Ok(meshes[face].clone()));
    result["max_deviation_mm"] = json!(max_deviation_mm);
    result["achieved_deviation_mm"] = json!(achieved);
    Ok(result)
}

// This extra work is only done when verification has actually refused a mesh.
fn refusal(loaded: &LoadedPart, face: usize, why: String) -> String {
    let default_deflection = deflection(loaded);
    for f in std::iter::once(face).chain((0..loaded.binding.face_count).filter(|&f| f != face)) {
        let sphere = loaded.part.faces.get(f).is_some_and(|f| {
            matches!(f.surface, geom::Surface::Sphere { .. })
                && !f.loops.is_empty()
                && f.loops.iter().all(|lp| lp.vertex.is_some())
        });
        if sphere
            && !loaded.unresolved_faces.contains(&f)
            && !loaded.part.unresolved_faces().contains(&f)
        {
            continue;
        }
        let missing = match face_mesh(loaded, f, default_deflection) {
            Ok(m) if !m.triangles.is_empty() => continue,
            Ok(_) => "no triangles".into(),
            Err(e) => e,
        };
        return format!(
            "face {f}: does not triangulate at any supported tolerance (default mesh lists it as missing: {missing})"
        );
    }
    format!("face {face}: {why}")
}

#[derive(Clone, Copy)]
struct Measured {
    deviation: f64,
    area_error: f64,
}
const AREA_TOLERANCE: f64 = 0.01;

fn refine_measured(
    initial: f64,
    limit: f64,
    margin: f64,
    mut attempt: impl FnMut(f64) -> Result<(FaceMesh, Measured), String>,
) -> Result<(FaceMesh, f64), String> {
    let (mut target, mut best, mut attempts) = (initial, f64::INFINITY, 0);
    let mut stopped = "8-attempt refinement limit".to_string();
    for _ in 0..8 {
        attempts += 1;
        let (mesh, measured) = match attempt(target) {
            Ok(v) => v,
            Err(e) => {
                stopped = e;
                break;
            }
        };
        best = best.min(measured.deviation);
        if measured.area_error <= AREA_TOLERANCE && measured.deviation <= margin * limit {
            return Ok((mesh, measured.deviation));
        }
        stopped = format!(
            "area error {:.6}% (maximum {}%); deviation verification bound {} mm",
            100.0 * measured.area_error,
            100.0 * AREA_TOLERANCE,
            margin * limit
        );
        if !mesh.converged {
            stopped.push_str("; triangulator did not converge (vertex or round refinement limit)");
            break;
        }
        target *= if measured.area_error > AREA_TOLERANCE {
            0.1
        } else {
            0.5
        };
    }
    let best = if best.is_finite() {
        format!("best measured {best} mm")
    } else {
        "no measurable attempt".into()
    };
    Err(format!(
        "cannot meet max_deviation_mm {limit} mm after {attempts} attempt(s) ({best}; {stopped})"
    ))
}

pub fn validate_max_deviation(max_deviation_mm: f64) -> Result<(), String> {
    if max_deviation_mm.is_finite() && max_deviation_mm > 0.0 {
        Ok(())
    } else {
        Err("max_deviation_mm must be a positive finite number in mm".into())
    }
}

struct Trim {
    edge: usize,
    interval: (f64, f64),
}
struct FaceCheck<'a> {
    loaded: &'a LoadedPart,
    face: usize,
    area: f64,
    curves: Vec<Trim>,
    tolerance: f64,
    periods: [Option<f64>; 2],
}
type BoundaryLabels = [Option<haecceity::mesh::BoundaryPoint>];
struct BoundarySegment {
    points: [V3; 2],
    uv: [(f64, f64); 2],
    deviation: f64,
    owner: usize,
    physical: bool,
    cut: bool,
}

impl<'a> FaceCheck<'a> {
    fn new(loaded: &'a LoadedPart, face: usize, diagonal: f64) -> Result<Self, String> {
        if loaded.unresolved_faces.contains(&face) || loaded.part.unresolved_faces().contains(&face)
        {
            return Err("its surface did not resolve".into());
        }
        let part = &loaded.part;
        let area = part
            .face_moments(face)
            .map(|m| m.area)
            .filter(|a| a.is_finite() && *a > 0.0)
            .ok_or("no positive exact face area for coverage verification")?;
        let mut uses = BTreeMap::new();
        for &(edge, _) in part.faces[face].loops.iter().flat_map(|lp| &lp.edges) {
            *uses.entry(edge).or_insert(0_usize) += 1;
        }
        let mut curves = Vec::new();
        for (edge, count) in uses {
            if count > 1 {
                continue;
            }
            if loaded.unresolved_edges.contains(&edge) || part.unresolved_edges().contains(&edge) {
                return Err(format!("boundary edge {edge}: its curve did not resolve"));
            }
            let e = &part.edges[edge];
            if e.samples
                .windows(2)
                .map(|w| geom::dist(w[0], w[1]))
                .sum::<f64>()
                <= 1e-8
            {
                continue;
            }
            let (a, b) = haecceity::sampling::edge_interval(
                &e.curve,
                e.start,
                e.end,
                e.same_sense,
                e.vertices.0 == e.vertices.1,
            );
            curves.push(Trim {
                edge,
                interval: (a.min(b), a.max(b)),
            });
        }
        Ok(Self {
            loaded,
            face,
            area,
            curves,
            tolerance: 1e-7 * diagonal + 1e-8,
            periods: triangulate::mesh_periods(&part.faces[face].surface),
        })
    }
    fn margin(&self) -> f64 {
        if matches!(
            self.loaded.part.faces[self.face].surface,
            geom::Surface::Sphere { .. }
                | geom::Surface::Torus { .. }
                | geom::Surface::Freeform { .. }
        ) {
            0.95
        } else {
            1.0
        }
    }
    fn measure(
        &self,
        mesh: &FaceMesh,
        labels: Option<&BoundaryLabels>,
    ) -> Result<Measured, String> {
        let (surface, area) = surface_deviation(self.loaded, self.face, mesh)?;
        Ok(Measured {
            deviation: surface.max(self.boundary_deviation(mesh, labels)?),
            area_error: (area - self.area).abs() / self.area,
        })
    }
    fn boundary_deviation(
        &self,
        mesh: &FaceMesh,
        labels: Option<&BoundaryLabels>,
    ) -> Result<f64, String> {
        use haecceity::mesh::BoundaryPoint;
        if self.curves.is_empty() {
            return Ok(0.0);
        }
        let part = &self.loaded.part;
        let tolerance = self.tolerance;
        let mut incidence = BTreeMap::new();
        for &[a, b, c] in &mesh.triangles {
            for (i, j) in [(a, b), (b, c), (c, a)] {
                let entry = incidence
                    .entry((i.min(j), i.max(j)))
                    .or_insert((0_usize, (i, j)));
                entry.0 += 1;
            }
        }
        let fits = |vertex: usize, trim: &Trim| match labels.and_then(|l| l[vertex]) {
            Some(BoundaryPoint::Edge { edge, .. }) => edge == trim.edge,
            Some(BoundaryPoint::Vertex(v)) => {
                let e = &part.edges[trim.edge];
                e.vertices.0 == v
                    || e.vertices.1 == v
                    || geom::dist(mesh.points[vertex], e.start)
                        .min(geom::dist(mesh.points[vertex], e.end))
                        <= tolerance
            }
            None => false,
        };
        // Each entry is an emitted index-boundary, not a proxy polyline.
        let mut segments = Vec::new();
        for (_, (count, (a, b))) in incidence {
            if count != 1 {
                continue;
            }
            let (pa, pb) = (point(mesh.points[a]), point(mesh.points[b]));
            if pa == pb {
                continue;
            }
            let samples = [pa, geom::scale(geom::add(pa, pb), 0.5), pb];
            let owners: Vec<_> = self
                .curves
                .iter()
                .enumerate()
                .filter_map(|(i, t)| (fits(a, t) && fits(b, t)).then_some(i))
                .collect();
            let (mut best, mut owner, mut physical) = (f64::INFINITY, 0, !owners.is_empty());
            for (i, trim) in self.curves.iter().enumerate() {
                if !owners.is_empty() && !owners.contains(&i) {
                    continue;
                }
                let curve = &part.edges[trim.edge].curve;
                let distances =
                    samples.map(|p| geom::dist(p, trim_projection(curve, trim.interval, p)));
                if distances.iter().any(|d| !d.is_finite()) {
                    return Err("boundary projection returned a non-finite distance".into());
                }
                physical |= [mesh.points[a], mesh.points[b]]
                    .into_iter()
                    .all(|p| geom::dist(p, trim_projection(curve, trim.interval, p)) <= tolerance);
                let off = distances.into_iter().fold(0.0_f64, f64::max);
                if off < best {
                    best = off;
                    owner = i;
                }
            }
            segments.push(BoundarySegment {
                points: [pa, pb],
                uv: [mesh.uv[a], mesh.uv[b]],
                deviation: best,
                owner,
                physical,
                cut: false,
            });
        }
        // An interior periodic cut can have different subdivisions on its two
        // sides. Require opposite directed coverage on BOTH sides, not proximity
        // to a guessed cut line. Physical trims never qualify as interior pairs.
        for (axis, period) in self.periods.into_iter().enumerate() {
            let Some(period) = period else { continue };
            let component = |uv: (f64, f64), axis| if axis == 0 { uv.0 } else { uv.1 };
            let eps = 1e-6 * period;
            for i in 0..segments.len() {
                let (physical, uv) = (segments[i].physical, segments[i].uv);
                let fixed = uv.map(|p| component(p, axis));
                if physical || (fixed[1] - fixed[0]).abs() > eps {
                    continue;
                }
                let ends = uv.map(|p| component(p, 1 - axis));
                let (lo, hi) = (ends[0].min(ends[1]), ends[0].max(ends[1]));
                if hi <= lo {
                    continue;
                }
                let mut intervals: Vec<_> = segments
                    .iter()
                    .filter_map(|s| {
                        let side = s.uv.map(|p| component(p, axis));
                        let other = s.uv.map(|p| component(p, 1 - axis));
                        (!s.physical
                            && (side[1] - side[0]).abs() <= eps
                            && ((side[0] - fixed[0]).abs() - period).abs() <= eps
                            && (other[1] - other[0]) * (ends[1] - ends[0]) < 0.0)
                            .then_some((other[0].min(other[1]), other[0].max(other[1])))
                    })
                    .collect();
                intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut covered = lo;
                for (a, b) in intervals {
                    if a <= covered + 1e-8 * (1.0 + hi.abs()) {
                        covered = covered.max(b);
                    }
                }
                if covered >= hi {
                    segments[i].cut = true;
                }
            }
        }
        let mut covered = vec![false; self.curves.len()];
        let mut worst = 0.0_f64;
        for segment in &segments {
            if !segment.cut {
                worst = worst.max(segment.deviation);
                covered[segment.owner] = true;
            }
        }
        for (i, trim) in self.curves.iter().enumerate() {
            if covered[i] {
                continue;
            }
            // Reverse coverage: actual trim samples to actual emitted boundaries.
            // An unrelated target-thinned polyline says nothing about this mesh.
            for &sample in &part.edges[trim.edge].samples {
                let p = trim_projection(&part.edges[trim.edge].curve, trim.interval, sample);
                let distance = segments
                    .iter()
                    .filter(|s| !s.cut)
                    .map(|s| segment_distance(p, s.points[0], s.points[1]))
                    .fold(f64::INFINITY, f64::min);
                if !distance.is_finite() {
                    return Err(format!(
                        "boundary edge {} has no emitted boundary coverage",
                        trim.edge
                    ));
                }
                worst = worst.max(distance);
            }
        }
        Ok(worst)
    }
}

fn segment_distance(p: V3, a: V3, b: V3) -> f64 {
    let ab = geom::sub(b, a);
    let n = geom::dot(ab, ab);
    let t = if n > 0.0 {
        (geom::dot(geom::sub(p, a), ab) / n).clamp(0.0, 1.0)
    } else {
        0.0
    };
    geom::dist(p, geom::add(a, geom::scale(ab, t)))
}

// The circumcentre projected into the closed triangle: an obtuse triangle uses
// the closest boundary point, never a sample outside the emitted triangle.
fn projected_circumcentre(p: [V3; 3]) -> Option<[f64; 3]> {
    let (ab, ac) = (geom::sub(p[1], p[0]), geom::sub(p[2], p[0]));
    let (aa, cc, bc) = (geom::dot(ab, ab), geom::dot(ac, ac), geom::dot(ab, ac));
    let denominator = 2.0 * (aa * cc - bc * bc);
    if denominator <= 1e-24 * aa * cc || !denominator.is_finite() {
        return None;
    }
    let (v, w) = (cc * (aa - bc) / denominator, aa * (cc - bc) / denominator);
    let weights = [1.0 - v - w, v, w];
    if weights.iter().all(|&x| x >= 0.0) {
        return Some(weights);
    }
    let centre = geom::add(p[0], geom::add(geom::scale(ab, v), geom::scale(ac, w)));
    let (mut best, mut result) = (f64::INFINITY, [1.0, 0.0, 0.0]);
    for (i, j) in [(0, 1), (1, 2), (2, 0)] {
        let e = geom::sub(p[j], p[i]);
        let n = geom::dot(e, e);
        let t = if n > 0.0 {
            (geom::dot(geom::sub(centre, p[i]), e) / n).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = geom::dist(centre, geom::add(p[i], geom::scale(e, t)));
        if d < best {
            best = d;
            result = [0.0; 3];
            result[i] = 1.0 - t;
            result[j] = t;
        }
    }
    Some(result)
}

fn surface_deviation(
    loaded: &LoadedPart,
    face: usize,
    mesh: &FaceMesh,
) -> Result<(f64, f64), String> {
    if mesh.triangles.is_empty() {
        return Err("no triangles".into());
    }
    let surface = &loaded.part.faces[face].surface;
    let (mut worst, mut area) = (0.0_f64, 0.0_f64);
    for triangle in &mesh.triangles {
        let corners = triangle.map(|i| point(mesh.points[i]));
        area += 0.5
            * geom::norm(geom::cross(
                geom::sub(corners[1], corners[0]),
                geom::sub(corners[2], corners[0]),
            ));
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
        ]
        .into_iter()
        .chain(projected_circumcentre(corners))
        {
            let (mut sample, mut hint) = ([0.0; 3], (0.0, 0.0));
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
    Ok((worst, area))
}

fn trim_projection(curve: &geom::Curve, interval: (f64, f64), p: V3) -> V3 {
    let mut t = curve.parameter(p);
    let period = curve.period();
    if let Some(period) = period {
        t += period * (((interval.0 + interval.1) * 0.5 - t) / period).round();
    }
    t = t.clamp(interval.0, interval.1);
    let mut best = curve.value(t);
    let mut distance = geom::dist(p, best);
    let span = interval.1 - interval.0;
    // At a closed NURBS seam, the kernel's nearest sampled seed can be the
    // wrong endpoint. Its clamped Newton step then cannot reach the other side.
    // Try both one-sided seeds, retaining only closer *exact* curve evaluations.
    if matches!(curve, geom::Curve::Nurbs(_))
        && period.is_some()
        && span > 0.0
        && (t - interval.0).min(interval.1 - t) <= span / 512.0
    {
        for mut t in [interval.0 + span / 512.0, interval.1 - span / 512.0] {
            for _ in 0..20 {
                let q = curve.value(t);
                let off = geom::dist(p, q);
                if off < distance {
                    best = q;
                    distance = off;
                }
                let (lo, hi) = (
                    (t - span * 1e-7).max(interval.0),
                    (t + span * 1e-7).min(interval.1),
                );
                let derivative =
                    geom::scale(geom::sub(curve.value(hi), curve.value(lo)), 1.0 / (hi - lo));
                let speed = geom::dot(derivative, derivative);
                if !speed.is_finite() || speed <= 1e-300 {
                    break;
                }
                let step = geom::dot(geom::sub(p, q), derivative) / speed;
                let mut next = (t + step).clamp(interval.0, interval.1);
                for _ in 0..8 {
                    if geom::dist(p, curve.value(next)) <= off {
                        break;
                    }
                    next = (t + next) * 0.5;
                }
                if (next - t).abs() <= 1e-15 * (1.0 + t.abs()) {
                    break;
                }
                t = next;
            }
        }
    }
    best
}

/// [`mesh`] with the face triangulation supplied, so a test can make a face fail.
pub fn mesh_with(
    loaded: &LoadedPart,
    triangulate: impl FnMut(usize, f64) -> Result<FaceMesh, String>,
) -> Value {
    mesh_with_deflection(loaded, deflection(loaded), triangulate)
}

fn mesh_with_deflection(
    loaded: &LoadedPart,
    deflection: f64,
    mut triangulate: impl FnMut(usize, f64) -> Result<FaceMesh, String>,
) -> Value {
    let bounds = loaded.part.bounds();

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

    fn boundary_deviation(
        loaded: &LoadedPart,
        face: usize,
        mesh: &FaceMesh,
        _target: f64,
    ) -> Result<f64, String> {
        FaceCheck::new(loaded, face, diagonal(loaded))?.boundary_deviation(mesh, None)
    }
    fn verified_face(
        loaded: &LoadedPart,
        face: usize,
        initial: f64,
        limit: f64,
        mut triangulate: impl FnMut(f64) -> Result<FaceMesh, String>,
    ) -> Result<(FaceMesh, f64), String> {
        let check = FaceCheck::new(loaded, face, diagonal(loaded))?;
        refine_measured(initial, limit, check.margin(), |target| {
            let mesh = triangulate(target)?;
            let measured = check.measure(&mesh, None)?;
            Ok((mesh, measured))
        })
    }

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
    fn adding_corner_curve_candidates_does_not_inflate_the_boundary_error() {
        let loaded = crate::part::load_bytes(
            "plate.step",
            include_bytes!("../tests/fixtures/parts/plate.step").to_vec(),
        )
        .unwrap();
        let face = loaded
            .part
            .faces
            .iter()
            .enumerate()
            .find(|(_, f)| matches!(f.surface, geom::Surface::Plane { .. }) && f.loops.len() > 1)
            .unwrap()
            .0;
        let target = deflection(&loaded);
        let mesh = face_mesh(&loaded, face, target).unwrap();
        let nearby = boundary_deviation(&loaded, face, &mesh, target).unwrap();
        let all = boundary_deviation(&loaded, face, &mesh, 100.0).unwrap();
        assert!(nearby > 0.0 && nearby <= target);
        assert!(
            (all - nearby).abs() < 1e-10,
            "nearest trim {nearby}, broad candidate set {all}"
        );
    }

    #[test]
    fn closed_nurbs_projection_checks_both_sides_of_its_seam() {
        let r = 50.0;
        let points = vec![
            [r, 0.0, 0.0],
            [r, r, 0.0],
            [0.0, r, 0.0],
            [-r, r, 0.0],
            [-r, 0.0, 0.0],
            [-r, -r, 0.0],
            [0.0, -r, 0.0],
            [r, -r, 0.0],
            [r, 0.0, 0.0],
        ];
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let n = haecceity::nurbs::NurbsCurve::new(
            2,
            points,
            vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0],
            vec![
                0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
            ],
        )
        .unwrap();
        let curve = geom::Curve::Nurbs(n);
        for t in [0.0002, 0.9998] {
            let p = curve.value(t);
            let projected = trim_projection(&curve, (0.0, 1.0), p);
            assert!(
                geom::dist(p, projected) < 1e-8,
                "seam {t}: {:?} -> {:?}",
                p,
                projected
            );
        }
    }

    #[test]
    fn doubled_real_boundary_and_unlabelled_endpoints_are_measured() {
        let loaded = crate::part::load_bytes(
            "cylinder.step",
            include_bytes!("../tests/fixtures/parts/mesh_cylinder.step").to_vec(),
        )
        .unwrap();
        let face = loaded
            .part
            .faces
            .iter()
            .position(|f| matches!(f.surface, geom::Surface::Plane { .. }))
            .unwrap();
        let check = FaceCheck::new(&loaded, face, diagonal(&loaded)).unwrap();
        let mut mesh = face_mesh(&loaded, face, deflection(&loaded)).unwrap();
        let expected = check.boundary_deviation(&mesh, None).unwrap();
        assert!(expected > 0.01);
        let mut labels = vec![
            Some(haecceity::mesh::BoundaryPoint::Edge {
                edge: check.curves[0].edge,
                index: 0
            });
            mesh.points.len()
        ];
        labels[0] = None;
        assert!((check.boundary_deviation(&mesh, Some(&labels)).unwrap() - expected).abs() < 1e-10);
        let offset = mesh.points.len();
        mesh.points.extend(mesh.points.clone());
        mesh.uv.extend(mesh.uv.clone());
        let reverse: Vec<_> = mesh
            .triangles
            .iter()
            .map(|t| [t[0] + offset, t[2] + offset, t[1] + offset])
            .collect();
        mesh.triangles.extend(reverse);
        assert!(
            (check.boundary_deviation(&mesh, None).unwrap() - expected).abs() < 1e-10,
            "opposite coordinate copies of a physical trim must not be classified as a cut"
        );
        // A fully unlabelled triangle deep inside the cap must measure its real
        // distance to the rim, rather than skipping all three boundary segments.
        let surface = &loaded.part.faces[face].surface;
        let interior = FaceMesh {
            points: vec![
                surface.value(0.0, 0.0),
                surface.value(1.0, 0.0),
                surface.value(0.0, 1.0),
            ],
            uv: vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)],
            triangles: vec![[0, 1, 2]],
            converged: true,
            folded_dropped: (0, 0.0),
        };
        assert!(
            check
                .boundary_deviation(&interior, Some(&[None; 3]))
                .unwrap()
                > 49.0
        );
        // Simulate an omitted trim by adding the opposite cap's rim: its source
        // samples must be measured to emitted segments, not to its own polyline.
        let opposite = loaded
            .part
            .faces
            .iter()
            .enumerate()
            .find(|(i, f)| *i != face && matches!(f.surface, geom::Surface::Plane { .. }))
            .unwrap()
            .0;
        let mut check = check;
        check.curves.extend(
            FaceCheck::new(&loaded, opposite, diagonal(&loaded))
                .unwrap()
                .curves,
        );
        assert!(check.boundary_deviation(&mesh, None).unwrap() >= 39.999);
    }

    #[test]
    fn coverage_rejects_a_surface_correct_half_sphere() {
        let loaded = sphere();
        let mut mesh =
            triangulate::triangulate_for_deviation(&loaded.part, 0, 0.1, ANGULAR_DEFLECTION)
                .unwrap();
        mesh.triangles.truncate(mesh.triangles.len() / 2);
        let measured = FaceCheck::new(&loaded, 0, diagonal(&loaded))
            .unwrap()
            .measure(&mesh, None)
            .unwrap();
        assert!(measured.deviation < 0.1);
        assert!(measured.area_error > 0.4);
        let reason = verified_face(&loaded, 0, 0.1, 0.1, |_| Ok(mesh.clone())).unwrap_err();
        assert!(reason.contains("area error"), "{reason}");
    }

    #[test]
    fn circumcentre_samples_the_interior_and_projects_obtuse_triangles() {
        let acute = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
        assert_eq!(projected_circumcentre(acute).unwrap(), [0.5, 0.5, 0.0]);
        let obtuse = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.5, 0.1, 0.0]];
        let weights = projected_circumcentre(obtuse).unwrap();
        assert_eq!(weights, [0.5, 0.5, 0.0]);
        assert!(projected_circumcentre([[0.0; 3]; 3]).is_none());
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
