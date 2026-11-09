//! Face triangulation, the local stand-in until haecceity meshes faces (upstream need U4).
//!
//! specify-core meshes with OpenCascade's `BRepMesh_IncrementalMesh`; haecceity has no
//! triangulation. Each face is triangulated here in its surface's parameter space:
//!
//! 1. The face's boundary loops come from haecceity ([`Part::uv_loops`]: unwrapped across
//!    periodic parameters, with poles and apexes routed along their parameter lines), unwrapped
//!    again across the seam of a closed freeform surface (which haecceity marks non-periodic).
//!    Each loop is thinned to the chordal and angular deflection ([`thin`]): a chord stands for
//!    the samples it skips in space, its straight line in parameter space stands for them on the
//!    surface, and the normals along it turn by no more than the angular deflection. Every
//!    edge's first point and every point on a singular line is kept, so neighbouring faces thin
//!    a shared edge alike.
//! 2. The loops become closed polygons. A periodic face with no seam edge has two loops that
//!    each run once round (a seamless band, a cone up to its apex, a whole sphere between its
//!    poles): it is cut along a parameter line that crosses each of them once and misses every
//!    hole. Holes are moved by whole periods into the region they belong to. A polygon side along
//!    a parameter line (a cut, or haecceity's closing of a loop along a pole) is sampled like an
//!    edge; any other side whose image strays from the boundary is a jump across the surface and
//!    the face is refused.
//! 3. A constrained Delaunay triangulation (spade) of the polygons runs in parameter coordinates
//!    scaled to the spacing the surface needs in each direction (its normal's turning and its
//!    bending), so triangles lie along a cylinder's rulings rather than across them; boundary
//!    sides longer than that spacing are divided first. It is refined by inserting edge
//!    midpoints until the surface strays from no triangle by more than the chordal deflection,
//!    the normals at an edge's ends differ by at most the angular deflection, and no triangle
//!    leans from the surface by more than it. Boundary sides are never split afterwards. A plane
//!    is meshed from its boundary alone.
//! 4. The triangles inside the polygons (by crossing parity, flooding the triangulation) are
//!    mapped onto the exact surface and wound so their normal is the face's outward one:
//!    counter-clockwise in (u, v) is along the surface's natural normal `Su × Sv`, which a
//!    reversed face points against. Triangles with no extent to see (slivers whose plane is
//!    rounding, and specks or slivers folded against the surface next to a degenerate side) are
//!    left out.
//!
//! Honest failure: a face whose surface did not resolve, whose loops do not close or collapse,
//! whose boundary crosses itself in parameter space even unthinned, whose boundary jumps across
//! the surface, or whose periodic loops cannot be cut is not triangulated, and the caller
//! reports it (the mesh's `missing`). No patch is guessed.

// Sheet parameter arrays have length 2; axis arguments below are literals 0/1 or come from a bounded pair.
use std::collections::VecDeque;
use std::f64::consts::TAU;

use haecceity::Part;
use haecceity::geom::{self, Surface, V3};
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};

/// Refinement stops after this many rounds of insertions, however far from the tolerance.
const MAX_ROUNDS: usize = 40;
/// A face's triangulation never grows past this many vertices: it bounds the time and memory a
/// single face can take (a large freeform face at a fine deflection). A face stopped by it is
/// still meshed, coarser than asked, and says so ([`FaceMesh::converged`]).
const MAX_VERTICES: usize = 60_000;
/// Samples taken along a parameter line added to cut a seamless periodic face, before thinning.
const CUT_SAMPLES: usize = 128;

/// A face's surface with its periods: 2π in the periodic directions of the analytic kinds, and
/// the parameter range of a freeform surface in a direction where its two sides meet (a closed
/// B-spline surface, which haecceity marks non-periodic). A closed freeform direction is
/// evaluated wrapped back into the surface's range, so a loop unwrapped across its seam, and a
/// band cut open along it, can be triangulated like a cylinder's.
#[derive(Clone, Copy)]
struct Sheet<'a> {
    surface: &'a Surface,
    period: [Option<f64>; 2],
    /// Per direction, the start of the range a closed freeform direction wraps into.
    wrap: [Option<f64>; 2],
}

impl<'a> Sheet<'a> {
    fn new(surface: &'a Surface) -> Self {
        let (pu, pv) = surface.periodic();
        let mut sheet = Sheet {
            surface,
            period: [pu.then_some(TAU), pv.then_some(TAU)],
            wrap: [None, None],
        };
        if let Surface::Freeform { surface: nurbs, .. } = surface {
            let (u0, u1, v0, v1) = nurbs.domain();
            // The two sides of the range meet if they are the same points in space.
            let meets = |side: &dyn Fn(f64) -> (V3, V3)| {
                (0..=8).all(|i| {
                    let (a, b) = side(i as f64 / 8.0);
                    geom::dist(a, b) <= 1e-6 * (1.0 + geom::norm(a))
                })
            };
            if u1 > u0
                && meets(&|t| {
                    (
                        surface.value(u0, v0 + t * (v1 - v0)),
                        surface.value(u1, v0 + t * (v1 - v0)),
                    )
                })
            {
                sheet.period[0] = Some(u1 - u0);
                sheet.wrap[0] = Some(u0);
            }
            if v1 > v0
                && meets(&|t| {
                    (
                        surface.value(u0 + t * (u1 - u0), v0),
                        surface.value(u0 + t * (u1 - u0), v1),
                    )
                })
            {
                sheet.period[1] = Some(v1 - v0);
                sheet.wrap[1] = Some(v0);
            }
        }
        sheet
    }

    /// (u, v) wrapped into a closed freeform surface's range.
    fn wrapped(&self, u: f64, v: f64) -> (f64, f64) {
        // axis is passed only as 0 or 1 below; wrap and period are fixed arrays of length 2.
        let fold = |x: f64, axis: usize| match (self.wrap[axis], self.period[axis]) {
            (Some(start), Some(period)) => start + (x - start).rem_euclid(period),
            _ => x,
        };
        (fold(u, 0), fold(v, 1))
    }

    fn value(&self, u: f64, v: f64) -> V3 {
        let (u, v) = self.wrapped(u, v);
        self.surface.value(u, v)
    }

    fn normal(&self, u: f64, v: f64) -> Option<V3> {
        let (u, v) = self.wrapped(u, v);
        self.surface.normal(u, v)
    }

    /// `x` moved by whole periods of direction `axis` to lie nearest `reference`.
    fn nearest(&self, x: f64, reference: f64, axis: usize) -> f64 {
        // nearest() callers pass only the literal axes 0 and 1.
        match self.period[axis] {
            Some(period) => x + period * ((reference - x) / period).round(),
            None => x,
        }
    }
}

/// One face's triangles, wound outward.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceMesh {
    /// Vertices on the exact surface.
    pub points: Vec<V3>,
    /// Each vertex's surface parameters.
    pub uv: Vec<(f64, f64)>,
    /// Vertex indices, counter-clockwise seen from outside the solid.
    pub triangles: Vec<[usize; 3]>,
    /// Whether refinement met the deflections everywhere; false when [`MAX_VERTICES`] or
    /// [`MAX_ROUNDS`] stopped it first.
    pub converged: bool,
    /// Triangles left out because they faced against the surface (a sliver or speck folded at a
    /// degenerate side or corner, see `finish`), and their total area: kept count of so the gap
    /// they leave is reported, not hidden.
    pub folded_dropped: (usize, f64),
}

impl FaceMesh {
    /// The triangle's area in space.
    pub fn triangle_area(&self, t: usize) -> f64 {
        // WASM callers enumerate triangles; finish() creates all vertex indices from the same points list.
        let [a, b, c] = self.triangles[t].map(|i| self.points[i]);
        0.5 * geom::norm(geom::cross(geom::sub(b, a), geom::sub(c, a)))
    }

    /// The total area of the triangles.
    pub fn area(&self) -> f64 {
        (0..self.triangles.len())
            .map(|t| self.triangle_area(t))
            .sum()
    }
}

/// Triangulates `face` of `part` to the chordal `deflection` (mm) and `angular` deflection
/// (radians). `Err` says why the face cannot be triangulated; the caller reports it rather than
/// show a guessed patch.
pub fn triangulate(
    part: &Part,
    face: usize,
    deflection: f64,
    angular: f64,
) -> Result<FaceMesh, String> {
    let f = part
        .faces
        .get(face)
        .ok_or_else(|| format!("no face {face} in the part"))?;
    let surface = &f.surface;
    if let Surface::Other { kind } = surface {
        return Err(format!("its surface ({kind}) did not resolve"));
    }
    if !(deflection > 0.0 && deflection.is_finite()) {
        return Err(format!("deflection {deflection} is not a positive length"));
    }
    // OpenCascade's BRepMesh, given an angular deflection, divides a full circle into segments
    // spanning about half of it: 42 segments per circle at 0.3 rad, whatever the radius, on
    // every cylinder of specify-core's pin and plate (the oracle's 84 triangles per bore). The
    // same density is kept here, so small holes are as round as in Python's mesh.
    let angular = 0.5 * angular;
    // Thinning the boundary can make two close stretches of it cross (a narrow neck of the
    // face): thin less, down to the samples themselves, before refusing.
    for boundary in [1.0, 0.25, 1.0 / 16.0, 1e-6].map(|k| k * deflection) {
        match attempt(part, face, deflection, boundary, angular) {
            Err(why) if why == CROSSES => continue,
            result => return result,
        }
    }
    Err(CROSSES.into())
}

/// Opt-in tolerance path: STEP may represent a whole sphere with only pole vertex
/// loops. A latitude/longitude mesh avoids singular parameter-space triangulations.
/// Keep the published picker triangulation unchanged for compatibility.
pub(crate) fn triangulate_for_deviation(
    part: &Part,
    face: usize,
    deflection: f64,
    angular: f64,
) -> Result<FaceMesh, String> {
    let f = part
        .faces
        .get(face)
        .ok_or_else(|| format!("no face {face} in the part"))?;
    if let Surface::Sphere { radius, .. } = f.surface
        && !f.loops.is_empty()
        && f.loops.iter().all(|lp| lp.vertex.is_some())
    {
        let (_, _, lo, hi) = part
            .uv_bounds(face)
            .ok_or("sphere has no parameter bounds")?;
        // Conservative initial spacing; the caller still measures the actual rounded
        // triangles and retries at a smaller deflection if necessary.
        let angle = (3.0 * deflection / radius).sqrt().min(0.5 * angular);
        let rows = ((hi - lo) / angle).ceil().max(2.0) as usize;
        let columns = (TAU / angle).ceil().max(3.0) as usize;
        if rows.saturating_mul(columns.saturating_add(1)) > MAX_VERTICES {
            return Err("whole sphere exceeds the 60000-vertex refinement limit".into());
        }
        let mut mesh = FaceMesh {
            points: Vec::new(),
            uv: Vec::new(),
            triangles: Vec::new(),
            converged: true,
            folded_dropped: (0, 0.0),
        };
        mesh.uv.push((0.0, lo));
        for row in 1..rows {
            let v = lo + (hi - lo) * row as f64 / rows as f64;
            for column in 0..=columns {
                mesh.uv.push((TAU * column as f64 / columns as f64, v));
            }
        }
        let north = mesh.uv.len();
        mesh.uv.push((0.0, hi));
        mesh.points = mesh
            .uv
            .iter()
            .map(|&(u, v)| f.surface.value(u, v))
            .collect();
        for column in 0..columns {
            mesh.triangles.push([0, 2 + column, 1 + column]);
            let last = 1 + (rows - 2) * (columns + 1) + column;
            mesh.triangles.push([last, last + 1, north]);
        }
        for row in 0..rows - 2 {
            for column in 0..columns {
                let a = 1 + row * (columns + 1) + column;
                let b = a + 1;
                let d = a + columns + 1;
                mesh.triangles.extend([[a, b, d + 1], [a, d + 1, d]]);
            }
        }
        if f.reversed {
            for triangle in &mut mesh.triangles {
                triangle.swap(1, 2);
            }
        }
        return Ok(mesh);
    }
    triangulate(part, face, deflection, angular)
}

/// Why a face's boundary cannot be triangulated as it is thinned.
const CROSSES: &str = "its boundary crosses itself in parameter space";

/// [`triangulate`] with the boundary thinned to `boundary` (mm) and the angular deflection
/// already as applied.
fn attempt(
    part: &Part,
    face: usize,
    deflection: f64,
    boundary: f64,
    angular: f64,
) -> Result<FaceMesh, String> {
    let f = part
        .faces
        .get(face)
        .ok_or_else(|| format!("no face {face} in the part"))?;
    let surface = &f.surface;
    let loops = part
        .uv_loops(face)
        .ok_or("its boundary has no parameter-space loops")?;
    let sheet = Sheet::new(surface);
    let period = sheet.period;

    // Thin each loop and sort it into closed loops and loops that run once round.
    let mut closed: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut winding_u: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut winding_v: Vec<Vec<(f64, f64)>> = Vec::new();
    for lp in loops {
        if lp.points.len() < 2 {
            // A vertex loop on a surface with no periodic direction: a single point, no
            // boundary to triangulate against.
            continue;
        }
        let points = thin_loop(&sheet, &lp.points, &lp.anchors, boundary, angular);
        // thin_loop keeps both endpoints of each loop whose input length was checked >= 2.
        let (first, last) = (points[0], points[points.len() - 1]);
        let (du, dv) = (last.0 - first.0, last.1 - first.1);
        // Runs once round a periodic direction: ends about a period from where it began.
        // Only the literal axes 0 and 1 are passed to this length-2 array.
        let round = |d: f64, axis: usize| period[axis].is_some_and(|p| d.abs() > 0.5 * p);
        if round(du, 0) && !round(dv, 1) {
            winding_u.push(points);
        } else if round(dv, 1) && !round(du, 0) {
            winding_v.push(points);
        } else {
            let gap = geom::dist(sheet.value(first.0, first.1), sheet.value(last.0, last.1));
            if gap > deflection || round(du, 0) || round(dv, 1) {
                return Err(format!(
                    "a boundary loop does not close in parameter space (gap {gap:.3e} mm)"
                ));
            }
            let mut points = points;
            points.pop();
            if points.len() < 3 {
                return Err("a boundary loop has fewer than three points".into());
            }
            closed.push(points);
        }
    }
    // A cone face bounded by one circle runs to its apex, which may have no vertex loop.
    if winding_u.len() == 1
        && winding_v.is_empty()
        && let Surface::Cone {
            radius, semi_angle, ..
        } = *surface
    {
        winding_u.push(periodic_line(-radius / semi_angle.sin()));
    }

    let mut polygons = match (winding_u.len(), winding_v.len()) {
        (0, 0) => outer_and_holes(closed, period)?,
        (2, 0) => strip(&sheet, &winding_u, closed, boundary, angular, false)?,
        (0, 2) => strip(&sheet, &winding_v, closed, boundary, angular, true)?,
        (u, v) => {
            return Err(format!(
                "{u} loop(s) run round u and {v} round v: no single cut makes it a polygon"
            ));
        }
    };
    for polygon in &mut polygons {
        dedup_consecutive(polygon);
        if polygon.len() < 3 {
            return Err("a boundary polygon collapses to fewer than three points".into());
        }
        // Each side, a straight line in parameter space, must lie along the boundary in space.
        // A side along a parameter line that strays from its chord is a line of the parameter
        // region itself (haecceity closes a loop round a sphere along its pole's line and the
        // meridian to it, with no samples between): it is sampled like an edge. Any other side
        // that strays jumps across the surface (a seam of a closed surface the loop's
        // parameters were not unwrapped across): no triangulation of that polygon is the face.
        let mut sampled = Vec::with_capacity(polygon.len());
        for i in 0..polygon.len() {
            // i ranges over polygon.len(); for an empty polygon the loop body is never entered.
            let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
            sampled.push(a);
            let mid = sheet.value(0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1));
            let off = segment_distance(mid, sheet.value(a.0, a.1), sheet.value(b.0, b.1));
            if off <= deflection {
                continue;
            }
            let scale = 1.0 + a.0.abs().max(a.1.abs());
            let iso = (a.0 - b.0).abs() <= 1e-9 * scale || (a.1 - b.1).abs() <= 1e-9 * scale;
            if iso {
                sampled.extend(straight_side(&sheet, a, b, boundary, angular));
            } else if off > 4.0 * deflection {
                return Err(format!(
                    "a boundary side jumps across the surface in parameter space ({a:?} to {b:?})"
                ));
            }
        }
        *polygon = sampled;
    }

    let scale = parameter_scale(&sheet, &polygons, deflection, angular)?;
    let refine = !matches!(surface, Surface::Plane { .. });
    if refine {
        // A boundary side longer than the spacing the surface needs would leave the triangles on
        // it long and thin across the curve (it is never split later): divide it evenly in
        // parameter space, each point on the surface.
        for polygon in &mut polygons {
            let mut dense = Vec::with_capacity(polygon.len());
            for i in 0..polygon.len() {
                // i ranges over polygon.len(); for an empty polygon the loop body is never entered.
                let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                let length = ((b.0 - a.0) * scale.0).hypot((b.1 - a.1) * scale.1);
                let pieces = length.round().clamp(1.0, 1e4) as usize;
                dense.extend((0..pieces).map(|k| {
                    let t = k as f64 / pieces as f64;
                    (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
                }));
            }
            *polygon = dense;
        }
    }
    let mut mesher = Mesher::new(sheet, scale, &polygons)?;
    let converged = if refine {
        // The triangles themselves are held to the angular deflection as given (OpenCascade's
        // between the normals at an edge's ends), not to the boundary's halved segments.
        mesher.refine(deflection, 2.0 * angular)
    } else {
        true
    };
    let mut mesh = mesher.finish(f.reversed, deflection);
    mesh.converged = converged;
    Ok(mesh)
}

/// A boundary polyline's place on a surface, for [`thin`]: each point's parameters and normal.
pub struct OnSurface<'a> {
    /// The surface point at parameters (u, v).
    pub value: &'a dyn Fn(f64, f64) -> V3,
    pub uv: &'a [(f64, f64)],
    pub normals: &'a [Option<V3>],
}

/// A polyline thinned to the chordal `deflection` and, if given, the angular deflection: the
/// indices of the points kept, always the first and last and every point `keep` marks. A chord
/// stands for the points it skips while none lies farther than `deflection` from it and no
/// skipped segment turns more than half the angular deflection from it (so the polyline turns by
/// at most the angular deflection at each kept point).
///
/// For a boundary on a surface (`on`), the chord is a straight line in parameter space, so it
/// must also stand for the points there: the surface point at each skipped point's place along
/// the parameter chord lies within `deflection` of it (a straight line in parameters is a curve
/// on most surfaces). And a chord longer than the deflection stands only for points whose
/// normals are within the angular deflection of its ends' normals, so the triangles along the
/// boundary meet the angular deflection too.
pub fn thin(
    points: &[V3],
    keep: &[bool],
    on: Option<&OnSurface<'_>>,
    deflection: f64,
    angular: Option<f64>,
) -> Vec<usize> {
    // V3 coordinates have length 3; parameter axes throughout this module are literals 0 or 1.
    let n = points.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let mut kept = vec![false; n];
    // n > 2 here; kept has n entries and every subsequent point index stays within 0..n.
    kept[0] = true;
    kept[n - 1] = true;
    for (k, &forced) in keep.iter().enumerate().take(n) {
        kept[k] |= forced;
    }
    // Whether the chord from point i to point j stands for the points between.
    let stands = |i: usize, j: usize| -> bool {
        // stands() is called only with 0 <= i < j < n.
        let (a, b) = (points[i], points[j]);
        let chord = geom::unit(geom::sub(b, a));
        for k in i + 1..j {
            let mut off = segment_distance(points[k], a, b);
            if let Some(on) = on {
                // WASM callers build uv and normals by mapping the same points list used for thin().
                let (ua, ub, uk) = (on.uv[i], on.uv[j], on.uv[k]);
                let d = (ub.0 - ua.0, ub.1 - ua.1);
                let len2 = d.0 * d.0 + d.1 * d.1;
                let t = if len2 > 0.0 {
                    (((uk.0 - ua.0) * d.0 + (uk.1 - ua.1) * d.1) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let image = (on.value)(ua.0 + t * d.0, ua.1 + t * d.1);
                off = off.max(geom::dist(image, points[k]));
            }
            if off > deflection {
                return false;
            }
            if let (Some(angular), Some(chord)) = (angular, chord) {
                // i < k < j < n ensures k-1 and k+1 are both valid point indices.
                for (p, q) in [(points[k - 1], points[k]), (points[k], points[k + 1])] {
                    // A sub-segment far shorter than the deflection has no direction to speak
                    // of (sample noise at an edge's ends).
                    if geom::dist(p, q) < 1e-6 * deflection {
                        continue;
                    }
                    if geom::unit(geom::sub(q, p)).is_some_and(|d| angle(d, chord) > 0.5 * angular)
                    {
                        return false;
                    }
                }
            }
            if let (Some(angular), Some(on)) = (angular, on)
                && let (Some(nk), Some(ni), Some(nj)) =
                    // WASM callers build normals with the same length as points; i < k < j < n.
                    (on.normals[k], on.normals[i], on.normals[j])
                && geom::dist(a, b) > deflection
                && angle(ni, nk).max(angle(nk, nj)) > angular
            {
                return false;
            }
        }
        true
    };
    // Greedily, each chord as long as it can be (found by doubling, then halving the step):
    // halving a failed chord instead could keep up to twice the points needed.
    let breaks: Vec<usize> = (0..n).filter(|&k| kept[k]).collect();
    for w in breaks.windows(2) {
        // windows(2) has exactly two entries, both indices collected from 0..n.
        let (mut i, end) = (w[0], w[1]);
        while i < end {
            let (mut good, mut step) = (i + 1, 1);
            while good < end && stands(i, (i + 2 * step).min(end)) {
                good = (i + 2 * step).min(end);
                step *= 2;
            }
            let mut bad = (i + 2 * step).min(end);
            if good < end {
                // stands(i, good) and not stands(i, bad): narrow the gap.
                while bad > good + 1 {
                    let mid = (good + bad) / 2;
                    if stands(i, mid) {
                        good = mid;
                    } else {
                        bad = mid;
                    }
                }
            }
            kept[good] = true;
            i = good;
        }
    }
    (0..n).filter(|&k| kept[k]).collect()
}

/// Whether three triangulation vertices lie on one line in parameter space, to rounding (a
/// point inserted on a straight boundary side that the triangulation took as just beside it):
/// such a triangle covers no part of the face, and its plane in space is noise.
fn parameter_sliver(a: &Node, b: &Node, c: &Node) -> bool {
    let (pa, pb, pc) = (a.position, b.position, c.position);
    let cross = (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x);
    let longest = [(pa, pb), (pb, pc), (pc, pa)]
        .iter()
        .map(|(p, q)| (q.x - p.x).hypot(q.y - p.y))
        .fold(0.0, f64::max);
    cross.abs() <= 1e-6 * longest * longest
}

/// The angle between two unit vectors.
fn angle(a: V3, b: V3) -> f64 {
    geom::dot(a, b).clamp(-1.0, 1.0).acos()
}

/// The distance from `p` to the segment `a`–`b`.
fn segment_distance(p: V3, a: V3, b: V3) -> f64 {
    let ab = geom::sub(b, a);
    let len2 = geom::dot(ab, ab);
    let t = if len2 > 0.0 {
        (geom::dot(geom::sub(p, a), ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    geom::dist(p, geom::add(a, geom::scale(ab, t)))
}

/// The distance from `p` to the triangle `a b c` (to the plane where `p` projects inside it,
/// else to the nearest side): how far the surface strays from the triangle near its centroid.
/// Not the plane's distance alone, which a sliver's unsteady plane would inflate.
fn triangle_distance(p: V3, a: V3, b: V3, c: V3) -> f64 {
    let sides = || {
        segment_distance(p, a, b)
            .min(segment_distance(p, b, c))
            .min(segment_distance(p, c, a))
    };
    let Some(n) = geom::unit(geom::cross(geom::sub(b, a), geom::sub(c, a))) else {
        return sides();
    };
    let height = geom::dot(geom::sub(p, a), n);
    let q = geom::sub(p, geom::scale(n, height));
    let inside = [(a, b), (b, c), (c, a)]
        .iter()
        .all(|&(x, y)| geom::dot(geom::cross(geom::sub(y, x), geom::sub(q, x)), n) >= 0.0);
    if inside { height.abs() } else { sides() }
}

/// One loop thinned on the exact surface. Each edge's first point (its `anchors`' run starts)
/// is kept, so the same edge thins alike in both faces it bounds, and so is every point on a
/// singular line (a pole or apex, where many parameter points are one point in space and only
/// the parameters say where the boundary runs).
///
/// The loop's parameters are unwrapped across a closed freeform surface's seam (each point a
/// whole period from where the surface's range puts it, if that is nearer the point before).
fn thin_loop(
    sheet: &Sheet<'_>,
    points: &[(f64, f64)],
    anchors: &[Vec<(f64, f64)>],
    deflection: f64,
    angular: f64,
) -> Vec<(f64, f64)> {
    let surface = sheet.surface;
    let mut keep: Vec<bool> = points
        .iter()
        .map(|&(_, v)| surface.singular_v(v).is_some_and(|s| (s - v).abs() < 1e-9))
        .collect();
    let mut from = 0;
    for start in anchors.iter().flatten() {
        // from starts at 0 and advances only to positions found within the remaining suffix.
        if let Some(k) = points[from..].iter().position(|p| p == start) {
            // keep maps points one-for-one, and k was found in the suffix beginning at from.
            keep[from + k] = true;
            from += k;
        }
    }
    let mut points = points.to_vec();
    for k in 1..points.len() {
        // k iterates over 1..points.len(), so both neighbouring indices exist.
        let (last, here) = (points[k - 1], points[k]);
        // k iterates over 1..points.len(), and no length changes occur.
        points[k] = (
            sheet.nearest(here.0, last.0, 0),
            sheet.nearest(here.1, last.1, 1),
        );
    }
    let space: Vec<V3> = points.iter().map(|&(u, v)| sheet.value(u, v)).collect();
    // Consecutive points the same to rounding (an edge's end and the next edge's start, read
    // back from the surface) would make a sliver side whose direction is noise: keep the first.
    let mut merged: Vec<usize> = Vec::with_capacity(points.len());
    for k in 0..points.len() {
        if let Some(&last) = merged.last() {
            // merged contains only earlier indices into points; k ranges over points.len().
            let (a, b): ((f64, f64), (f64, f64)) = (points[last], points[k]);
            let scale = 1.0 + a.0.abs().max(a.1.abs());
            if (a.0 - b.0).abs() + (a.1 - b.1).abs() <= 1e-12 * scale && k + 1 != points.len() {
                // keep is built one-for-one from points; last and k are indices into points.
                keep[last] |= keep[k];
                continue;
            }
        }
        merged.push(k);
    }
    // merged contains only point indices; space, keep and points have identical lengths.
    let space: Vec<V3> = merged.iter().map(|&k| space[k]).collect();
    // merged contains only point indices; keep was built one-for-one from points.
    let keep: Vec<bool> = merged.iter().map(|&k| keep[k]).collect();
    // merged contains only indices obtained by enumerating points.
    let uv: Vec<(f64, f64)> = merged.iter().map(|&k| points[k]).collect();
    let normals: Vec<Option<V3>> = uv.iter().map(|&(u, v)| sheet.normal(u, v)).collect();
    let on = OnSurface {
        value: &|u, v| sheet.value(u, v),
        uv: &uv,
        normals: &normals,
    };
    thin(&space, &keep, Some(&on), deflection, Some(angular))
        .into_iter()
        // thin returns indices into merged; merged contains only indices into points.
        .map(|k| points[merged[k]])
        .collect()
}

/// The whole u line at `v`, as haecceity gives a vertex loop on a periodic surface.
fn periodic_line(v: f64) -> Vec<(f64, f64)> {
    let n = 64;
    (0..=n).map(|i| (TAU * i as f64 / n as f64, v)).collect()
}

fn dedup_consecutive(polygon: &mut Vec<(f64, f64)>) {
    polygon.dedup();
    while polygon.len() > 1 && polygon.first() == polygon.last() {
        polygon.pop();
    }
}

/// Twice the signed area of a closed polygon.
fn signed_area(polygon: &[(f64, f64)]) -> f64 {
    let n = polygon.len();
    (0..n)
        .map(|i| {
            // i ranges over 0..n; when n is zero the loop body is never entered.
            let (a, b) = (polygon[i], polygon[(i + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum()
}

/// Whether `p` lies inside the closed polygon, by crossing parity.
fn inside(polygon: &[(f64, f64)], p: (f64, f64)) -> bool {
    let n = polygon.len();
    let mut odd = false;
    for i in 0..n {
        // i ranges over 0..n; when n is zero the loop body is never entered.
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        if (a.1 > p.1) != (b.1 > p.1) {
            let x = a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0);
            if x > p.0 {
                odd = !odd;
            }
        }
    }
    odd
}

/// Closed loops only: the largest is the outer boundary, the rest holes, each moved by whole
/// periods so it lies in the outer boundary.
fn outer_and_holes(
    mut closed: Vec<Vec<(f64, f64)>>,
    period: [Option<f64>; 2],
) -> Result<Vec<Vec<(f64, f64)>>, String> {
    if closed.is_empty() {
        return Err("it has no boundary loop".into());
    }
    // The largest by area in parameter space; the first of equals.
    let mut outer = 0;
    for (i, lp) in closed.iter().enumerate() {
        // closed was checked nonempty; outer is initially 0 and later an enumerate() index.
        if signed_area(lp).abs() > signed_area(&closed[outer]).abs() {
            outer = i;
        }
    }
    let outer_loop = closed.remove(outer);
    let mut polygons = vec![outer_loop];
    for hole in closed {
        // polygons is initialized with outer_loop and only appended to.
        polygons.push(place_hole(&polygons[0], hole, period, None)?);
    }
    Ok(polygons)
}

/// `hole` moved by whole periods (in the periodic directions) until its first point lies in
/// `region`. `fixed_u` pins u's shift (a seamless band has already placed its holes in u).
fn place_hole(
    region: &[(f64, f64)],
    mut hole: Vec<(f64, f64)>,
    period: [Option<f64>; 2],
    fixed_u: Option<f64>,
) -> Result<Vec<(f64, f64)>, String> {
    let turns = |p: bool| if p { -2..=2 } else { 0..=0 };
    let start = *hole.first().ok_or("an inner loop has no points")?;
    let mut found = None;
    // period is a fixed array of exactly two optional periods.
    let (pu, pv) = (period[0].unwrap_or(0.0), period[1].unwrap_or(0.0));
    'search: for ku in turns(period[0].is_some() && fixed_u.is_none()) {
        for kv in turns(period[1].is_some()) {
            let shift = (fixed_u.unwrap_or(ku as f64 * pu), kv as f64 * pv);
            if inside(region, (start.0 + shift.0, start.1 + shift.1)) {
                found = Some(shift);
                break 'search;
            }
        }
    }
    let (su, sv) = found.ok_or("an inner loop lies outside the face's outer loop")?;
    for p in &mut hole {
        p.0 += su;
        p.1 += sv;
    }
    Ok(hole)
}

/// A face bounded by two loops that each run once round a periodic direction (u, or v when
/// `swapped`): cut along a constant line that crosses each loop once and misses every hole,
/// giving one polygon (the band between the loops) plus its holes.
fn strip(
    sheet: &Sheet<'_>,
    winding: &[Vec<(f64, f64)>],
    closed: Vec<Vec<(f64, f64)>>,
    deflection: f64,
    angular: f64,
    swapped: bool,
) -> Result<Vec<Vec<(f64, f64)>>, String> {
    let (axis, other) = if swapped { (1, 0) } else { (0, 1) };
    // axis and other are chosen only from 0 and 1.
    let turn = sheet.period[axis].ok_or("a loop runs round a direction that is not periodic")?;
    // Work as if the winding direction were u.
    let swap = |p: (f64, f64)| if swapped { (p.1, p.0) } else { p };
    let winding: Vec<Vec<(f64, f64)>> = winding
        .iter()
        .map(|lp| {
            let mut lp: Vec<(f64, f64)> = lp.iter().map(|&p| swap(p)).collect();
            // attempt() passes only winding loops with both endpoints retained from an input of length >= 2.
            if lp[lp.len() - 1].0 < lp[0].0 {
                lp.reverse();
            }
            lp
        })
        .collect();
    let holes: Vec<Vec<(f64, f64)>> = closed
        .into_iter()
        .map(|lp| lp.into_iter().map(swap).collect())
        .collect();
    // other is chosen only from the literal axes 0 and 1.
    let other_period = sheet.period[other];

    // Candidate cuts, spread over the turn at an angle no modelled symmetry is likely to share.
    let cut = (0..37)
        .map(|j| 0.123_456_789 * turn / TAU + turn * j as f64 / 37.0)
        .find(|&c| {
            winding.iter().all(|lp| crossings(lp, c, turn) == 1)
                && holes.iter().all(|h| {
                    let (lo, hi) = h
                        .iter()
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                            (lo.min(p.0), hi.max(p.0))
                        });
                    let margin = 1e-6;
                    hi - lo < turn - 2.0 * margin
                        && turn_index(hi + margin, c, turn) == turn_index(lo - margin, c, turn)
                })
        })
        .ok_or("no parameter line crosses each of its periodic loops once and misses its holes")?;

    // attempt() calls strip only after matching exactly two winding loops.
    let a = one_turn(&winding[0], cut, turn);
    // attempt() calls strip only after matching exactly two winding loops.
    let b = one_turn(&winding[1], cut, turn);
    // The cut line from b's end to a's end, at u = cut + 2π, and back at u = cut. Both lie on
    // the same curve in space and are sampled alike.
    let va = a.first().ok_or("the first periodic loop cannot be cut")?.1;
    let vb = b.first().ok_or("the second periodic loop cannot be cut")?.1;
    let line: Vec<f64> =
        straight_side(sheet, swap((cut, va)), swap((cut, vb)), deflection, angular)
            .into_iter()
            .map(|p| swap(p).1)
            .collect();
    let interior = &line[..];
    let mut band: Vec<(f64, f64)> = a.clone();
    // a ends at (cut + 2π, va): go to b's end along the line, then back along b.
    band.extend(interior.iter().map(|&v| (cut + turn, v)));
    band.extend(b.iter().rev());
    // b starts at (cut, vb): back to a's start.
    band.extend(interior.iter().rev().map(|&v| (cut, v)));
    // The cut result a was checked nonempty above.
    band.pop_if(|p| *p == a[0]);

    let mut polygons = vec![band];
    for hole in holes {
        let (lo, _) = hole
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                (lo.min(p.0), hi.max(p.0))
            });
        // Into [cut, cut + 2π) in u, then by whole turns in v if v is periodic too.
        let shift_u = -turn * turn_index(lo, cut, turn) as f64;
        let periods = [None, other_period];
        // polygons starts with band and is only appended to.
        let placed = place_hole(&polygons[0], hole, periods, Some(shift_u))?;
        polygons.push(placed);
    }
    Ok(polygons
        .into_iter()
        .map(|p| p.into_iter().map(swap).collect())
        .collect())
}

/// The points strictly between `a` and `b` along the straight parameter line joining them (an
/// iso-parameter line: a cut, a seam or a pole's closing line), sampled and thinned on the
/// surface like a boundary edge.
fn straight_side(
    sheet: &Sheet<'_>,
    a: (f64, f64),
    b: (f64, f64),
    deflection: f64,
    angular: f64,
) -> Vec<(f64, f64)> {
    let uv: Vec<(f64, f64)> = (0..=CUT_SAMPLES)
        .map(|i| {
            let t = i as f64 / CUT_SAMPLES as f64;
            (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
        })
        .collect();
    let space: Vec<V3> = uv.iter().map(|&(u, v)| sheet.value(u, v)).collect();
    let normals: Vec<Option<V3>> = uv.iter().map(|&(u, v)| sheet.normal(u, v)).collect();
    let on = OnSurface {
        value: &|u, v| sheet.value(u, v),
        uv: &uv,
        normals: &normals,
    };
    let kept = thin(&space, &[], Some(&on), deflection, Some(angular));
    // The sampled side has CUT_SAMPLES+1 points; thin() retains its first and last indices.
    // kept contains indices into uv, which was mapped one-for-one to space.
    kept[1..kept.len() - 1].iter().map(|&k| uv[k]).collect()
}

/// Which turn `u` is on, counting turns of length `turn` from `cut`: `u` lies in
/// `[cut + k turn, cut + (k + 1) turn)`.
fn turn_index(u: f64, cut: f64, turn: f64) -> i64 {
    ((u - cut) / turn).floor() as i64
}

/// How many times the open polyline crosses the lines `u = cut + k turn`.
fn crossings(lp: &[(f64, f64)], cut: f64, turn: f64) -> usize {
    lp.windows(2)
        // windows(2) always contains exactly two points.
        .map(|w| turn_index(w[0].0, cut, turn).abs_diff(turn_index(w[1].0, cut, turn)) as usize)
        .sum()
}

/// The loop (rising in u by one turn, crossing the cut once) rotated to run from `u = cut` to
/// `u = cut + turn` exactly, the ends interpolated on the cut.
fn one_turn(lp: &[(f64, f64)], cut: f64, turn: f64) -> Vec<(f64, f64)> {
    // The loop followed by its next turn covers any window one turn long.
    let mut extended: Vec<(f64, f64)> = lp.to_vec();
    // one_turn is private and receives winding loops of length >= 2 from attempt().
    extended.extend(lp[1..].iter().map(|&(u, v)| (u + turn, v)));
    // one_turn is private and receives winding loops of length >= 2 from attempt().
    let k = turn_index(lp[0].0, cut, turn);
    let (from, to) = (cut + turn * (k + 1) as f64, cut + turn * (k + 2) as f64);
    let at = |a: (f64, f64), b: (f64, f64), u: f64| {
        let t = (u - a.0) / (b.0 - a.0);
        (u, a.1 + t * (b.1 - a.1))
    };
    let mut out = Vec::new();
    for w in extended.windows(2) {
        // windows(2) always contains exactly two points.
        let (a, b) = (w[0], w[1]);
        if out.is_empty() {
            if a.0 < from && b.0 >= from {
                out.push(at(a, b, from));
                if b.0 < to {
                    out.push(b);
                }
            }
        } else if b.0 >= to {
            out.push(at(a, b, to));
            break;
        } else {
            out.push(b);
        }
    }
    // Back to the window [cut, cut + turn].
    let shift = -turn * (k + 1) as f64;
    out.into_iter().map(|(u, v)| (u + shift, v)).collect()
}

/// Per-parameter scale factors for the triangulation's coordinates: the reciprocal of the
/// spacing each parameter needs to meet the deflections, so a needed triangle is about one unit
/// across either way and the Delaunay triangulation lays its edges along the directions the
/// surface does not bend in (a cylinder's rulings) instead of across them.
///
/// The spacing along a parameter is the least of what the angular deflection allows (the
/// normal's turning rate), what the chordal deflection allows (the second derivative), and the
/// polygons' extent in it; rates are root-mean-square over a grid on the polygons' box,
/// measured by finite differences. A plane is left unscaled.
fn parameter_scale(
    sheet: &Sheet<'_>,
    polygons: &[Vec<(f64, f64)>],
    deflection: f64,
    angular: f64,
) -> Result<(f64, f64), String> {
    if matches!(sheet.surface, Surface::Plane { .. }) {
        return Ok((1.0, 1.0));
    }
    let (mut u0, mut u1, mut v0, mut v1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for &(u, v) in polygons.iter().flatten() {
        (u0, u1, v0, v1) = (u0.min(u), u1.max(u), v0.min(v), v1.max(v));
    }
    let (eu, ev) = (u1 - u0, v1 - v0);
    if !(eu > 0.0 && ev > 0.0 && eu.is_finite() && ev.is_finite()) {
        return Err("its boundary spans no area in parameter space".into());
    }
    // Squared speed, turning rate of the normal and second derivative, per parameter.
    let n = 5;
    let (mut speed, mut turn, mut bend) = ([0.0_f64; 2], [0.0_f64; 2], [0.0_f64; 2]);
    let mut samples = 0;
    for i in 0..n {
        for j in 0..n {
            let u = u0 + eu * (i as f64 + 0.5) / n as f64;
            let v = v0 + ev * (j as f64 + 0.5) / n as f64;
            let centre = sheet.value(u, v);
            if sheet.normal(u, v).is_none() {
                continue;
            }
            samples += 1;
            for (axis, h) in [(0, 1e-4 * eu), (1, 1e-4 * ev)] {
                let at = |t: f64| if axis == 0 { (u + t, v) } else { (u, v + t) };
                let (a, b) = (at(-h), at(h));
                let (pa, pb) = (sheet.value(a.0, a.1), sheet.value(b.0, b.1));
                let first = geom::dist(pa, pb) / (2.0 * h);
                // axis is a literal 0 or 1 above; speed has exactly two entries.
                speed[axis] += first * first;
                let second = geom::scale(
                    geom::sub(geom::add(pa, pb), geom::scale(centre, 2.0)),
                    1.0 / (h * h),
                );
                // axis is a literal 0 or 1 above; bend has exactly two entries.
                bend[axis] += geom::dot(second, second);
                if let (Some(na), Some(nb)) = (sheet.normal(a.0, a.1), sheet.normal(b.0, b.1)) {
                    let rate = geom::norm(geom::sub(nb, na)) / (2.0 * h);
                    // axis is a literal 0 or 1 above; turn has exactly two entries.
                    turn[axis] += rate * rate;
                }
            }
        }
    }
    if samples == 0 {
        return Err("its surface has no normal over the face".into());
    }
    // rms is called only with the literal axes 0 and 1 on length-2 arrays.
    let rms = |x: [f64; 2], axis: usize| (x[axis] / samples as f64).sqrt();
    let spacing = |axis: usize, extent: f64| {
        let (turn, bend) = (rms(turn, axis), rms(bend, axis));
        let mut h = extent;
        if turn > 0.0 {
            h = h.min(angular / turn);
        }
        if bend > 0.0 {
            h = h.min((8.0 * deflection / bend).sqrt());
        }
        h
    };
    let (mut hu, mut hv) = (spacing(0, eu), spacing(1, ev));
    // A triangle long along a curved direction and short across it stands askew of the surface
    // (its long side's sag tilts it by about sag / height): the long side is held to a sag of
    // half the angular deflection times the short side.
    let (su, sv) = (rms(speed, 0), rms(speed, 1));
    if su > 0.0 && sv > 0.0 {
        let (lu, lv) = (hu * su, hv * sv);
        let (ku, kv) = (rms(turn, 0) / su, rms(turn, 1) / sv);
        if lu > lv && ku > 0.0 {
            hu = hu.min((4.0 * angular * lv / ku).sqrt() / su);
        } else if lv > lu && kv > 0.0 {
            hv = hv.min((4.0 * angular * lu / kv).sqrt() / sv);
        }
    }
    if hu > 0.0 && hv > 0.0 && hu.is_finite() && hv.is_finite() {
        Ok((1.0 / hu, 1.0 / hv))
    } else {
        Err("its surface has no usable parameter speed over the face".into())
    }
}

/// A triangulation vertex: its scaled position, parameters, point and normal on the surface.
#[derive(Clone, Copy, Debug)]
struct Node {
    position: Point2<f64>,
    uv: (f64, f64),
    point: V3,
    normal: Option<V3>,
}

impl HasPosition for Node {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.position
    }
}

struct Mesher<'a> {
    surface: Sheet<'a>,
    scale: (f64, f64),
    cdt: ConstrainedDelaunayTriangulation<Node>,
}

impl<'a> Mesher<'a> {
    /// The constrained triangulation of the polygons' points and sides; `Err` when a side
    /// crosses another (the boundary crosses itself in parameter space).
    fn new(
        surface: Sheet<'a>,
        scale: (f64, f64),
        polygons: &[Vec<(f64, f64)>],
    ) -> Result<Self, String> {
        let mut mesher = Mesher {
            surface,
            scale,
            cdt: ConstrainedDelaunayTriangulation::new(),
        };
        for polygon in polygons {
            let handles = polygon
                .iter()
                .map(|&uv| mesher.insert(uv))
                .collect::<Result<Vec<_>, _>>()?;
            for i in 0..handles.len() {
                // i ranges over handles.len(); an empty polygon never enters the body.
                let (a, b) = (handles[i], handles[(i + 1) % handles.len()]);
                if a == b {
                    continue;
                }
                if !mesher.cdt.can_add_constraint(a, b) {
                    return Err(CROSSES.into());
                }
                mesher.cdt.add_constraint(a, b);
            }
        }
        Ok(mesher)
    }

    fn node(&self, uv: (f64, f64)) -> Node {
        // spade refuses coordinates nearer zero than about 1e-38 (but not zero itself).
        let flush = |x: f64| if x.abs() < 1e-30 { 0.0 } else { x };
        Node {
            position: Point2::new(flush(uv.0 * self.scale.0), flush(uv.1 * self.scale.1)),
            uv,
            point: self.surface.value(uv.0, uv.1),
            normal: self.surface.normal(uv.0, uv.1),
        }
    }

    fn insert(&mut self, uv: (f64, f64)) -> Result<spade::handles::FixedVertexHandle, String> {
        let node = self.node(uv);
        self.cdt
            .insert(node)
            .map_err(|e| format!("a boundary point cannot be triangulated ({e:?})"))
    }

    /// Whether each face (by index) lies inside the polygons: flooded from the convex hull,
    /// crossing a constraint (a polygon side) toggles inside and outside.
    fn inside_faces(&self) -> Vec<bool> {
        let mut state: Vec<Option<bool>> = vec![None; self.cdt.num_all_faces()];
        let mut queue = VecDeque::new();
        for edge in self.cdt.convex_hull() {
            for side in [edge, edge.rev()] {
                if let Some(face) = side.face().as_inner() {
                    let index = face.fix().index();
                    // The live face handle index is below num_all_faces(); this flood does not mutate cdt.
                    if state[index].is_none() {
                        // The live face handle index is below num_all_faces(); this flood does not mutate cdt.
                        state[index] = Some(edge.is_constraint_edge());
                        queue.push_back(face.fix());
                    }
                }
            }
        }
        while let Some(fixed) = queue.pop_front() {
            let face = self.cdt.face(fixed);
            // Only faces whose state was set are queued; no faces are inserted or removed during this flood.
            // state has num_all_faces() entries; each queued index is a live handle of this unchanged cdt.
            let here = state[fixed.index()].expect("queued faces are set");
            for edge in face.adjacent_edges() {
                if let Some(next) = edge.rev().face().as_inner() {
                    let index = next.fix().index();
                    // The live face handle index is below num_all_faces(); this flood does not mutate cdt.
                    if state[index].is_none() {
                        // The live face handle index is below num_all_faces(); this flood does not mutate cdt.
                        state[index] = Some(here != edge.is_constraint_edge());
                        queue.push_back(next.fix());
                    }
                }
            }
        }
        state.into_iter().map(|s| s == Some(true)).collect()
    }

    /// Inserts edge midpoints and centroids until every inside triangle meets the deflections,
    /// or a cap stops it. Returns whether it converged.
    ///
    /// An edge is split at its parameter midpoint when the surface there strays from it by more
    /// than the chordal deflection, or when the normals at its ends differ by more than the
    /// angular deflection (only for an edge longer than the chordal deflection: near a singular
    /// point the normal turns fast however short the edge). A triangle none of whose edges is
    /// split is refined when the surface near its centroid strays from it by more than the
    /// chordal deflection, or when its own normal leans from the surface's at a corner by more
    /// than the angular deflection (a thin triangle across a curve can meet the chordal test
    /// and still stand askew, overstating the area: the Schwarz lantern); it is split along its
    /// longest inner edge, or at its centroid when every edge is the boundary. Boundary edges
    /// are never split.
    fn refine(&mut self, deflection: f64, angular: f64) -> bool {
        for _ in 0..MAX_ROUNDS {
            let inside = self.inside_faces();
            let mut seen = vec![false; self.cdt.num_undirected_edges()];
            let mut wanted: Vec<(f64, f64)> = Vec::new();
            let mut want_edge = |index: usize, a: &Node, b: &Node, wanted: &mut Vec<(f64, f64)>| {
                // seen has num_undirected_edges() entries; index is a live edge handle and insertion is deferred.
                if !seen[index] {
                    // index is a live edge handle below num_undirected_edges(), with insertion deferred.
                    seen[index] = true;
                    wanted.push(((a.uv.0 + b.uv.0) / 2.0, (a.uv.1 + b.uv.1) / 2.0));
                }
            };
            for face in self.cdt.inner_faces() {
                // inside_faces() returns num_all_faces() entries before any triangulation mutation.
                // inside_faces() returns num_all_faces() entries and finish() does not mutate cdt.
                if !inside[face.fix().index()] {
                    continue;
                }
                let mut split = false;
                // The longest inner edge, for a triangle refined as a whole.
                let mut longest: Option<(f64, usize, Node, Node)> = None;
                for edge in face.adjacent_edges() {
                    let undirected = edge.as_undirected();
                    if undirected.is_constraint_edge() {
                        continue;
                    }
                    let index = undirected.fix().index();
                    let [a, b] = edge.vertices().map(|v| *v.data());
                    let length = geom::dist(a.point, b.point);
                    if longest.is_none_or(|l| length > l.0) {
                        longest = Some((length, index, a, b));
                    }
                    let mid = ((a.uv.0 + b.uv.0) / 2.0, (a.uv.1 + b.uv.1) / 2.0);
                    let bent = match (a.normal, b.normal) {
                        (Some(na), Some(nb)) => angle(na, nb) > angular && length > deflection,
                        _ => false,
                    };
                    // The surface's distance from the edge (not from its midpoint: where the
                    // parameterisation is uneven the parameter midpoint lies well along it).
                    let off = segment_distance(self.surface.value(mid.0, mid.1), a.point, b.point);
                    if bent || off > deflection {
                        split = true;
                        want_edge(index, &a, &b, &mut wanted);
                    }
                }
                if split {
                    continue;
                }
                let [a, b, c] = face.vertices().map(|v| *v.data());
                if parameter_sliver(&a, &b, &c) {
                    continue;
                }
                let uv = (
                    (a.uv.0 + b.uv.0 + c.uv.0) / 3.0,
                    (a.uv.1 + b.uv.1 + c.uv.1) / 3.0,
                );
                let off =
                    triangle_distance(self.surface.value(uv.0, uv.1), a.point, b.point, c.point);
                let size = geom::dist(a.point, b.point)
                    .max(geom::dist(b.point, c.point))
                    .max(geom::dist(c.point, a.point));
                // A sliver (its height a thousandth of its length, three points nearly on one
                // line) has no steady plane to lean: it is not judged by its lean (one folded
                // against the surface is left out of the mesh, see `finish`).
                let doubled = geom::cross(geom::sub(b.point, a.point), geom::sub(c.point, a.point));
                let askew = size > 1e-3 * deflection
                    && geom::norm(doubled) > 1e-3 * size * size
                    && geom::unit(doubled).is_some_and(|n| {
                        [a.normal, b.normal, c.normal]
                            .into_iter()
                            .flatten()
                            .any(|m| angle(n, m) > angular)
                    });
                if off > deflection || askew {
                    match longest {
                        Some((_, index, a, b)) => want_edge(index, &a, &b, &mut wanted),
                        None => wanted.push(uv),
                    }
                }
            }
            if wanted.is_empty() {
                return true;
            }
            for uv in wanted {
                if self.cdt.num_vertices() >= MAX_VERTICES {
                    return false;
                }
                // A point the triangulation cannot take (it already has one there) is skipped;
                // the round after judges the result again.
                let _ = self.insert(uv);
            }
        }
        false
    }

    /// The inside triangles on the surface, wound outward, with degenerate ones (no area in
    /// space: two corners on one pole) left out.
    fn finish(self, reversed: bool, deflection: f64) -> FaceMesh {
        let inside = self.inside_faces();
        let mut index = vec![usize::MAX; self.cdt.num_vertices()];
        let mut points = Vec::new();
        let mut uv = Vec::new();
        let mut triangles = Vec::new();
        // A triangle less than a millionth of the deflection high covers nothing to see, and
        // its normal is noise (two corners on one pole, or three on one boundary line). So is a
        // sliver (a thousandth as high as it is long), or a speck (less than a millionth of a
        // deflection square), folded against the surface: next to a degenerate side or corner,
        // where a parameter barely moves the point, the triangle's plane is set by rounding and
        // curvature, not by the surface it stands for.
        let thin = 1e-6 * deflection;
        let mut folded_dropped = (0, 0.0);
        for face in self.cdt.inner_faces() {
            // inside_faces() returns num_all_faces() entries and finish() does not mutate cdt.
            if !inside[face.fix().index()] {
                continue;
            }
            let corners = face.vertices();
            let [a, b, c] = corners.map(|v| v.data().point);
            let longest = geom::dist(a, b).max(geom::dist(b, c)).max(geom::dist(c, a));
            let [na, nb, nc] = corners.map(|v| *v.data());
            let doubled = geom::cross(geom::sub(b, a), geom::sub(c, a));
            let normals = [na.normal, nb.normal, nc.normal];
            let folded = normals.iter().flatten().count() > 0
                && normals
                    .iter()
                    .flatten()
                    .all(|&m| geom::dot(doubled, m) < 0.0);
            let sliver = geom::norm(doubled) <= 1e-3 * longest * longest;
            let speck = geom::norm(doubled) <= 2.0 * (1e-3 * deflection).powi(2);
            if geom::norm(doubled) <= thin * longest || parameter_sliver(&na, &nb, &nc) {
                continue;
            }
            if (sliver && folded)
                || (speck && {
                    // A speck at a degenerate corner can have one corner's normal anything: it
                    // is judged by the surface's normal at its middle.
                    let mid = (
                        (na.uv.0 + nb.uv.0 + nc.uv.0) / 3.0,
                        (na.uv.1 + nb.uv.1 + nc.uv.1) / 3.0,
                    );
                    self.surface
                        .normal(mid.0, mid.1)
                        .is_some_and(|m| geom::dot(doubled, m) < 0.0)
                })
            {
                folded_dropped.0 += 1;
                folded_dropped.1 += 0.5 * geom::norm(doubled);
                continue;
            }
            let mut tri = corners.map(|v| {
                let k = v.fix().index();
                // index has num_vertices() entries; k is a live vertex handle, with no removals creating holes.
                if index[k] == usize::MAX {
                    // k is a live vertex handle below num_vertices(); cdt is immutable in finish().
                    // k is a live vertex handle below num_vertices(); the slot was assigned just above.
                    index[k] = points.len();
                    points.push(v.data().point);
                    uv.push(v.data().uv);
                }
                // k is a live vertex handle below num_vertices(); the slot was assigned just above.
                index[k]
            });
            // Counter-clockwise in (u, v) is along Su × Sv; a reversed face's outside is not.
            if reversed {
                tri.swap(1, 2);
            }
            triangles.push(tri);
        }
        FaceMesh {
            points,
            uv,
            triangles,
            converged: true,
            folded_dropped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinning_keeps_the_ends_and_drops_collinear_points() {
        let points: Vec<V3> = (0..=10).map(|i| [i as f64, 0.0, 0.0]).collect();
        assert_eq!(thin(&points, &[], None, 0.01, Some(0.3)), vec![0, 10]);
    }

    #[test]
    fn thinning_a_circle_meets_both_deflections() {
        let r = 10.0;
        let points: Vec<V3> = (0..=1000)
            .map(|i| {
                let t = TAU * i as f64 / 1000.0;
                [r * t.cos(), r * t.sin(), 0.0]
            })
            .collect();
        // Coarse chordal deflection: the angular one (0.3 rad per segment) decides.
        let kept = thin(&points, &[], None, 1.0, Some(0.3));
        assert!(kept.len() > (TAU / 0.3).ceil() as usize, "{}", kept.len());
        // The samples' directions stand for the tangent, so a segment may span the angular
        // deflection plus one sample step, and no more; greedy chords use most of it.
        let step = TAU / 1000.0;
        for w in kept.windows(2) {
            let turn = step * (w[1] - w[0]) as f64;
            assert!(turn <= 0.3 + step + 1e-9, "a segment spans {turn} rad");
        }
        assert!(
            kept.len() <= (TAU / 0.3).ceil() as usize + 3,
            "{}",
            kept.len()
        );
        // Chordal alone.
        let kept = thin(&points, &[], None, 0.01, None);
        for w in kept.windows(2) {
            let turn = TAU * (w[1] - w[0]) as f64 / 1000.0;
            assert!(r * (1.0 - (turn / 2.0).cos()) <= 0.01 + 1e-9);
        }
    }

    #[test]
    fn a_forced_point_is_kept() {
        let points: Vec<V3> = (0..=10).map(|i| [i as f64, 0.0, 0.0]).collect();
        let mut keep = vec![false; 11];
        keep[4] = true;
        assert_eq!(thin(&points, &keep, None, 0.01, None), vec![0, 4, 10]);
    }

    #[test]
    fn one_turn_runs_from_the_cut_round_once() {
        let lp: Vec<(f64, f64)> = (0..=8).map(|i| (1.0 + TAU * i as f64 / 8.0, 2.0)).collect();
        let cut = 0.5;
        assert_eq!(crossings(&lp, cut, TAU), 1);
        let turn = one_turn(&lp, cut, TAU);
        assert!((turn[0].0 - cut).abs() < 1e-12, "{turn:?}");
        assert!(
            (turn[turn.len() - 1].0 - (cut + TAU)).abs() < 1e-12,
            "{turn:?}"
        );
        assert!(turn.windows(2).all(|w| w[1].0 > w[0].0));
    }
}
