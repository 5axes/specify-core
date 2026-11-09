//! The mesh for the picker (`src/mesh.rs`, `src/triangulate.rs`, port of `mesh.py`), checked
//! by the behaviour specify-core's `tests/test_mesh.py` tests and by properties on every oracle
//! case.
//!
//! Triangles are not compared with Python's (the triangulation is a stand-in for OpenCascade's,
//! upstream need U4): the oracle's mesh summary is (face count, missing faces, bbox, each face's
//! mesh area, edge count and length), and each face's triangles are checked against the exact
//! geometry: area within 1% of haecceity's `face_moments` area, every triangle's centroid within
//! the deflection of the surface, every triangle wound outward.
//!
//! Property failures are differences too, so that each gets a verdict in
//! `known_differences/mesh.json`. Their paths start `check.`, and their `python` side holds the
//! reference the Rust value is checked against (haecceity's exact area, the deflection, zero),
//! not a Python value.

mod common;

use std::path::{Path, PathBuf};

use common::{
    Difference, Tolerances, cases, check_known_with, compare, corpus_dir, glob_match, is_error,
    known_differences_file, load_known,
};
use haecceity::geom::{self, V3};
use serde_json::{Value, json};
use specify_core::mesh::{self, ANGULAR_DEFLECTION};
use specify_core::part::{self, LoadedPart};
use specify_core::triangulate::FaceMesh;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(name)
}

fn load(name: &str) -> LoadedPart {
    part::load(&fixture(name), None).unwrap()
}

/// The oracle's analysis of a synthetic part (for face kinds, directions and areas: the
/// features stream ports `describe_faces`).
fn analysis(case: &str) -> Value {
    common::case(case).artifact("analysis").unwrap()
}

fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

fn usizes(value: &Value) -> Vec<usize> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as usize)
        .collect()
}

/// A mesh in the JSON format, its arrays read once.
struct Mesh {
    positions: Vec<f64>,
    indices: Vec<usize>,
    faces: Vec<(usize, usize)>,
}

impl Mesh {
    fn new(m: &Value) -> Self {
        Mesh {
            positions: numbers(&m["positions"]),
            indices: usizes(&m["indices"]),
            faces: m["faces"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    let r = usizes(r);
                    (r[0], r[1])
                })
                .collect(),
        }
    }

    /// Triangle `t`'s corners.
    fn triangle(&self, t: usize) -> [V3; 3] {
        let p = &self.positions;
        [0, 1, 2].map(|k| {
            let v = self.indices[3 * t + k];
            [p[3 * v], p[3 * v + 1], p[3 * v + 2]]
        })
    }

    /// The mesh's area on `face` (Python's `_area`).
    fn area(&self, face: usize) -> f64 {
        let (start, count) = self.faces[face];
        (start..start + count)
            .map(|t| geom::norm(normal(self.triangle(t))) / 2.0)
            .sum()
    }

    /// Signed volume of the mesh: positive and near the solid's only if every triangle, on
    /// curved and reversed faces too, winds outward (Python's `_volume`).
    fn volume(&self) -> f64 {
        (0..self.indices.len() / 3)
            .map(|t| {
                let [a, b, c] = self.triangle(t);
                geom::dot(a, geom::cross(b, c))
            })
            .sum::<f64>()
            / 6.0
    }
}

fn normal(tri: [V3; 3]) -> V3 {
    let [a, b, c] = tri;
    geom::cross(geom::sub(b, a), geom::sub(c, a))
}

/// The oracle's summary of a mesh (`tools/capture/cases.py`'s `mesh_summary`), without the
/// triangle counts (a property of the triangulation, not of the part).
fn summary(m: &Value) -> Value {
    let parsed = Mesh::new(m);
    let faces = parsed.faces.len();
    let mut length = 0.0;
    for line in m["edges"].as_array().unwrap() {
        let p = numbers(line);
        for j in (3..p.len()).step_by(3) {
            length += geom::dist([p[j], p[j + 1], p[j + 2]], [p[j - 3], p[j - 2], p[j - 1]]);
        }
    }
    json!({
        "format": m["format"],
        "version": m["version"],
        "binding": m["binding"],
        "bbox": m["bbox"],
        "face_count": faces,
        "missing": m["missing"],
        "areas": (0..faces).map(|f| parsed.area(f)).collect::<Vec<_>>(),
        "edges": {"count": m["edges"].as_array().unwrap().len(), "length": length},
    })
}

#[test]
fn the_mesh_is_bound_to_the_file() {
    let loaded = load("plate.step");
    let m = mesh::mesh(&loaded);
    assert_eq!(m["format"], "pmi-assist-mesh");
    assert_eq!(m["version"], 2);
    assert_eq!(m["binding"], serde_json::to_value(&loaded.binding).unwrap());
}

#[test]
fn every_face_owns_a_contiguous_run_of_triangles() {
    let loaded = load("plate.step");
    let m = Mesh::new(&mesh::mesh(&loaded));
    assert_eq!(m.faces.len(), loaded.binding.face_count);
    let mut expected = 0;
    for (face, &(start, count)) in m.faces.iter().enumerate() {
        assert!(
            start == expected && count > 0,
            "face {face}: {start} {count}"
        );
        expected += count;
    }
    assert_eq!(expected * 3, m.indices.len());
    assert!(*m.indices.iter().max().unwrap() < m.positions.len() / 3);
}

/// A face's triangles wind with its outward side, reversed faces included.
#[test]
fn triangles_face_outward() {
    let loaded = load("plate.step");
    let m = Mesh::new(&mesh::mesh(&loaded));
    for face in analysis("parts/plate")["faces"].as_array().unwrap() {
        if face["kind"] != "plane" {
            continue;
        }
        let id = face["id"].as_u64().unwrap() as usize;
        let direction = numbers(&face["direction"]);
        let (start, count) = m.faces[id];
        for t in start..start + count {
            let n = normal(m.triangle(t));
            assert!(geom::dot(n, [direction[0], direction[1], direction[2]]) > 0.0);
        }
    }
}

#[test]
fn a_part_inside_an_assembly_is_meshed_in_its_own_numbering() {
    let loaded = load("wrapped.step");
    let m = mesh::mesh(&loaded);
    assert_eq!(m["faces"].as_array().unwrap().len(), 7);
    assert_eq!(loaded.binding.face_count, 7);
    let edges = m["edges"].as_array().unwrap();
    assert!(!edges.is_empty());
    assert!(edges.iter().all(|l| l.as_array().unwrap().len() % 3 == 0));
}

fn solid_volume(loaded: &LoadedPart) -> f64 {
    (0..loaded.part.solids.len())
        .map(|s| loaded.part.solid_mass(s).unwrap().0)
        .sum()
}

#[test]
fn every_triangle_winds_outward() {
    let mut parts = vec![("plate".to_owned(), load("plate.step"))];
    let spool = corpus_dir().map(|d| d.join("cadgenbench/flanged_spool_132.step"));
    if let Some(path) = &spool {
        assert!(
            path.is_file() || std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED: missing corpus input {}",
            path.display()
        );
    }
    match spool {
        Some(spool) if spool.is_file() => {
            parts.push(("spool".to_owned(), part::load(&spool, None).unwrap()))
        }
        _ => println!("skipped spool: no corpus (set QUIDDITY_CORPUS)"),
    }
    for (name, loaded) in parts {
        let (meshed, solid) = (
            Mesh::new(&mesh::mesh(&loaded)).volume(),
            solid_volume(&loaded),
        );
        assert!(
            (meshed - solid).abs() <= 0.01 * solid,
            "{name}: mesh volume {meshed}, solid {solid}"
        );
    }
}

/// The faces meshed are numbered as the analysis numbers them: each range's triangles have the
/// area of the face the analysis gives that index.
#[test]
fn each_face_range_covers_that_face() {
    let loaded = load("plate.step");
    let m = Mesh::new(&mesh::mesh(&loaded));
    for face in analysis("parts/plate")["faces"].as_array().unwrap() {
        let id = face["id"].as_u64().unwrap() as usize;
        let expected = face["area"].as_f64().unwrap();
        let got = m.area(id);
        assert!(
            (got - expected).abs() <= 0.02 * expected,
            "face {id}: {got} against {expected}"
        );
    }
}

/// Meshing reads the part and changes nothing: the same mesh before and after anything else
/// (anchors, which triangulate faces too; meshing again).
#[test]
fn meshing_is_independent_of_what_ran_before() {
    let fresh = mesh::mesh(&load("plate.step"));
    let used = load("plate.step");
    let groups: Vec<Vec<usize>> = (0..used.binding.face_count).map(|f| vec![f]).collect();
    specify_core::anchors::anchors(&used, &groups, mesh::diagonal(&used));
    assert_eq!(mesh::mesh(&used), fresh);
    assert_eq!(mesh::mesh(&used), fresh);
}

#[test]
fn meshing_twice_gives_the_same_text() {
    for name in ["plate.step", "pin.step", "assembly.step"] {
        let path = fixture(name);
        for loaded in part::load_all(&path).unwrap() {
            let a = serde_json::to_string(&mesh::mesh(&loaded)).unwrap();
            let b = serde_json::to_string(&mesh::mesh(
                &part::load(&path, Some(loaded.binding.part)).unwrap(),
            ))
            .unwrap();
            assert_eq!(a, b, "{name}#{}", loaded.binding.part);
        }
    }
}

/// A face that does not triangulate has an empty range and is listed in `missing`. Built two
/// ways: a face whose surface is recorded as unresolved (the real path), and a triangulation
/// that fails on one face (Python's monkeypatched `_triangulation`).
#[test]
fn a_face_that_does_not_triangulate_is_reported() {
    let mut loaded = load("plate.step");
    loaded.unresolved_faces.push(2);
    let m = mesh::mesh(&loaded);
    assert_eq!(m["missing"], json!([2]));
    assert_eq!(Mesh::new(&m).faces[2].1, 0);
    assert!(Mesh::new(&m).faces[3].1 > 0);

    let loaded = load("plate.step");
    let m = mesh::mesh_with(&loaded, |face, deflection| {
        if face == 2 {
            Err("refused for the test".into())
        } else {
            mesh::face_mesh(&loaded, face, deflection)
        }
    });
    assert_eq!(m["missing"], json!([2]));
    assert_eq!(Mesh::new(&m).faces[2].1, 0);
}

/// The plate's 12 box edges and the bore's two circles; not the bore's seam.
#[test]
fn seams_are_not_drawn_as_edges() {
    assert_eq!(
        mesh::mesh(&load("wrapped.step"))["edges"]
            .as_array()
            .unwrap()
            .len(),
        14
    );
}

/// A solid placed inside an assembly: the mesh's planes are the analysis's.
#[test]
fn a_placed_part_is_meshed_in_the_frame_the_analysis_uses() {
    let name = "a_placed_part_is_meshed_in_the_frame_the_analysis_uses";
    let loaded = load(&format!("{name}.step"));
    let m = Mesh::new(&mesh::mesh(&loaded));
    let (p, ix) = (&m.positions, &m.indices);
    let mut planes = 0;
    for face in analysis(&format!("parts/{name}"))["faces"]
        .as_array()
        .unwrap()
    {
        if face["kind"] != "plane" {
            continue;
        }
        planes += 1;
        let id = face["id"].as_u64().unwrap() as usize;
        let (centroid, direction) = (numbers(&face["centroid"]), numbers(&face["direction"]));
        let (start, count) = m.faces[id];
        for &v in &ix[3 * start..3 * (start + count)] {
            let offset: f64 = (0..3)
                .map(|i| (p[3 * v + i] - centroid[i]) * direction[i])
                .sum();
            assert!(
                offset.abs() < 1e-3,
                "face {id}: a vertex {offset} off its plane"
            );
        }
    }
    assert!(planes > 0);
}

/// The length of a face's boundary (each loop's edges, a seam twice).
fn perimeter(loaded: &LoadedPart, face: usize) -> f64 {
    let part = &loaded.part;
    part.faces[face]
        .loops
        .iter()
        .flat_map(|l| &l.edges)
        .map(|&(e, _)| {
            part.edges[e]
                .samples
                .windows(2)
                .map(|w| geom::dist(w[0], w[1]))
                .sum::<f64>()
        })
        .sum()
}

/// The faces whose index does not name the face Python's analysis gives it (each a
/// `check.numbering` difference). Classifying an area difference against haecceity's exact area
/// of Rust's own face is circular when the two engines number that face differently: a
/// renumbered face would always look "nearer exact". So every index is first checked against the
/// oracle analysis, as tests/part.rs `numbering_mismatches` does: Rust's exact centroid
/// (`face_moments`) is a mismatch when it is more than 1e-4 of the diagonal from Python's
/// centroid at that index and another of Python's faces matches better, nearer by centroid and
/// by area both. Not the areas or a fixed distance alone: on small freeform faces haecceity's and OpenCascade's integrals of the same
/// face differ by a few percent (cgb203 faces 11-50: 0.3536 against 0.3580 mm2, centroids 0.005
/// mm apart; cgb243 face 237, 193.6 against 210.4 mm2 and 0.4 mm, where both meshes agree with
/// haecceity's area), which is a question for the exact areas, not the numbering. Faces haecceity cannot
/// integrate, and indices past Python's face count (question Q7), go unchecked.
fn renumbered_faces(loaded: &LoadedPart, analysis: &Value) -> Vec<(usize, Value, Value)> {
    let part = &loaded.part;
    let diagonal = mesh::diagonal(loaded);
    let python: Vec<(usize, f64, V3)> = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let c = numbers(&f["centroid"]);
            (
                f["id"].as_u64().unwrap() as usize,
                f["area"].as_f64().unwrap(),
                [c[0], c[1], c[2]],
            )
        })
        .collect();
    let mut out = Vec::new();
    for &(id, area, centroid) in &python {
        if id >= part.faces.len() {
            continue;
        }
        let Some(m) = part.face_moments(id) else {
            continue;
        };
        let own = geom::dist(m.centroid, centroid);
        let better = python.iter().any(|&(j, a, c)| {
            j != id && geom::dist(m.centroid, c) < own && (a - m.area).abs() < (area - m.area).abs()
        });
        if own > 1e-4 * diagonal && better {
            out.push((
                id,
                json!({"area": area, "centroid": centroid}),
                json!({"area": m.area, "centroid": m.centroid}),
            ));
        }
    }
    out
}

/// A per-face area difference from Python's mesh, classified by the exact area: Rust's mesh
/// nearer the exact area than Python's (`areas_rust_nearer_exact`), within the boundary's
/// deflection of it (`areas_within_boundary_deflection`; of Python's mesh area where haecceity
/// has no exact area), or neither (`areas`, unexplained). A face in `renumbered` (see
/// `renumbered_faces`) is not classified: its exact area is another face's.
fn classify_area(loaded: &LoadedPart, renumbered: &[usize], difference: &mut Difference) {
    let Some(face) = difference
        .path
        .strip_prefix("areas[")
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|i| i.parse::<usize>().ok())
    else {
        return;
    };
    if renumbered.contains(&face) {
        return;
    }
    let (Some(python), Some(rust)) = (
        difference.python.as_ref().and_then(Value::as_f64),
        difference.rust.as_ref().and_then(Value::as_f64),
    ) else {
        return;
    };
    let bound = perimeter(loaded, face) * mesh::deflection(loaded);
    // Where haecceity cannot integrate the face, the two meshes are judged against each other.
    let exact = loaded.part.face_moments(face).map(|m| m.area);
    let kind = match exact {
        Some(exact) if (rust - exact).abs() <= (python - exact).abs() => "areas_rust_nearer_exact",
        Some(exact) if (rust - exact).abs() <= bound => "areas_within_boundary_deflection",
        None if (rust - python).abs() <= bound => "areas_within_boundary_deflection",
        _ => return,
    };
    difference.path = format!("{kind}[{face}]");
}

/// A box corner difference where Rust's exact box lies inside Python's (OpenCascade's
/// `BRepBndLib` box, padded and bounding B-splines by their control points): `bbox_python_looser`.
/// A Rust corner outside Python's stays `bbox` (unexplained).
fn classify_bbox(difference: &mut Difference) {
    let Some(rest) = difference.path.strip_prefix("bbox[") else {
        return;
    };
    let (Some(python), Some(rust)) = (
        difference.python.as_ref().and_then(Value::as_f64),
        difference.rust.as_ref().and_then(Value::as_f64),
    ) else {
        return;
    };
    // bbox[0] is the low corner, bbox[1] the high one.
    let inside = if rest.starts_with('0') {
        rust >= python
    } else {
        rust <= python
    };
    if inside {
        difference.path = format!("bbox_python_looser[{rest}");
    }
}

/// Each face's triangles against the exact geometry; a failure is a `check.` difference.
fn face_checks(
    case: &str,
    loaded: &LoadedPart,
    faces: &[Result<FaceMesh, String>],
) -> Vec<Difference> {
    let part = &loaded.part;
    let deflection = mesh::deflection(loaded);
    let mut out = Vec::new();
    let mut differ = |path: String, reference: Value, got: Value| {
        out.push(Difference {
            case: case.to_owned(),
            path,
            python: Some(reference),
            rust: Some(got),
        })
    };
    for (face, result) in faces.iter().enumerate() {
        let fm = match result {
            Ok(fm) => fm,
            Err(why) => {
                differ(
                    format!("check.triangulated[{face}]"),
                    json!(true),
                    json!(why),
                );
                continue;
            }
        };
        if !fm.converged {
            differ(
                format!("check.converged[{face}]"),
                json!(true),
                json!(false),
            );
        }
        // Triangles the triangulation left out as folded against the surface leave a gap in
        // the face; each face with any is reported (count and area), not passed silently. A gap
        // of less than one deflection square in all is below what the mesh resolves anyway
        // (`folded_dropped_sub_deflection`); a larger one is `folded_dropped`.
        if fm.folded_dropped.0 > 0 {
            let kind = if fm.folded_dropped.1 <= deflection * deflection {
                "folded_dropped_sub_deflection"
            } else {
                "folded_dropped"
            };
            differ(
                format!("check.{kind}[{face}]"),
                json!([0, 0.0]),
                json!([fm.folded_dropped.0, fm.folded_dropped.1]),
            );
        }
        if let Some(exact) = part.face_moments(face).map(|m| m.area) {
            let got = fm.area();
            if (got - exact).abs() > 0.01 * exact {
                // Within what the boundary's chords may cut off or add (its length times the
                // deflection), the mesh meets its tolerance on a slender face; beyond it, not.
                let kind = if (got - exact).abs() <= perimeter(loaded, face) * deflection {
                    "moments_area_within_boundary_deflection"
                } else {
                    "moments_area"
                };
                differ(format!("check.{kind}[{face}]"), json!(exact), json!(got));
            }
        }
        let surface = &part.faces[face].surface;
        let (mut worst, mut wrong) = (0.0_f64, 0);
        for (t, tri) in fm.triangles.iter().enumerate() {
            let corners = tri.map(|k| fm.points[k]);
            let centre = geom::scale(
                geom::add(geom::add(corners[0], corners[1]), corners[2]),
                1.0 / 3.0,
            );
            let hint = tri.map(|k| fm.uv[k]);
            let hint = (
                (hint[0].0 + hint[1].0 + hint[2].0) / 3.0,
                (hint[0].1 + hint[1].1 + hint[2].1) / 3.0,
            );
            if let Some((u, v)) = surface.parameters(centre, Some(hint)) {
                worst = worst.max(geom::dist(surface.value(u, v), centre));
            }
            if fm.triangle_area(t) > 0.0
                && let Some(n) = part.face_normal(face, hint.0, hint.1)
                && geom::dot(normal(corners), n) <= 0.0
            {
                wrong += 1;
            }
        }
        if worst > deflection * (1.0 + 1e-6) {
            differ(
                format!("check.centroid_deviation[{face}]"),
                json!(deflection),
                json!(worst),
            );
        }
        if wrong > 0 {
            differ(
                format!("check.inward_triangles[{face}]"),
                json!(0),
                json!(wrong),
            );
        }
    }
    out
}

/// The mesh of every oracle case against Python's summary and the exact geometry.
#[test]
fn mesh_matches_the_oracle() {
    let tolerances = Tolerances::default()
        // Both boxes are rounded to 4 decimals: a hair's difference can round a unit apart.
        .with("bbox*", 1e-6, 1.01e-4)
        .with("areas*", 0.01, 0.0)
        .with("edges.length", 0.01, 0.0);
    let mut differences = Vec::new();
    let mut compared = Vec::new();
    let (mut meshed, mut triangles, mut unconverged) = (0, 0, 0);
    for case in cases() {
        let Some(python) = case.artifact("mesh") else {
            continue;
        };
        let Some(input) = case.input() else { continue };
        compared.push(case.name.clone());
        let loaded = match part::load(&input, case.part()) {
            Ok(loaded) => loaded,
            Err(e) => {
                differences.push(Difference {
                    case: case.name.clone(),
                    path: String::new(),
                    python: Some(python),
                    rust: Some(json!({"error": e.to_string()})),
                });
                continue;
            }
        };
        if let Some(why) = loaded.degraded() {
            println!("{}: degraded: {why}", case.name);
        }
        let deflection = mesh::deflection(&loaded);
        let faces: Vec<Result<FaceMesh, String>> = (0..loaded.binding.face_count)
            .map(|f| mesh::face_mesh(&loaded, f, deflection))
            .collect();
        let rust = mesh::mesh_with(&loaded, |f, _| faces[f].clone());
        meshed += 1;
        triangles += rust["indices"].as_array().unwrap().len() / 3;
        unconverged += faces.iter().flatten().filter(|f| !f.converged).count();
        let mut python = python;
        assert!(!is_error(&python), "{}: Python failed: {python}", case.name);
        python.as_object_mut().unwrap().remove("triangles");
        let renumbered = match case.artifact("analysis") {
            Some(analysis) if !is_error(&analysis) => renumbered_faces(&loaded, &analysis),
            _ => Vec::new(),
        };
        let renumbered_ids: Vec<usize> = renumbered.iter().map(|r| r.0).collect();
        for (face, python, rust) in renumbered {
            differences.push(Difference {
                case: case.name.clone(),
                path: format!("check.numbering[{face}]"),
                python: Some(python),
                rust: Some(rust),
            });
        }
        for mut difference in compare(&case.name, &python, &summary(&rust), &tolerances) {
            classify_area(&loaded, &renumbered_ids, &mut difference);
            classify_bbox(&mut difference);
            differences.push(difference);
        }
        differences.extend(face_checks(&case.name, &loaded, &faces));
    }
    println!(
        "meshed {meshed} cases, {triangles} triangles, {unconverged} faces stopped short of the \
         deflection (angular {ANGULAR_DEFLECTION})"
    );
    check_known_compared("mesh", &differences, &compared);
}

/// `check_known(area, differences)`, with the entries for cases this run did not compare (a
/// corpus file absent) left out of the stale check. Every difference must still match
/// an entry, and every entry for a compared case must match a difference.
pub fn check_known_compared(area: &str, differences: &[Difference], compared: &[String]) {
    let path = known_differences_file(area);
    let entries: Vec<_> = load_known(&path)
        .into_iter()
        .filter(|e| compared.iter().any(|c| glob_match(&e.case, c)))
        .collect();
    check_known_with(&path.display().to_string(), &entries, differences);
}

/// Independently check 66 barycentric lattice points per curved output triangle against
/// analytic radial distances. This lattice does not reuse the production samples.
#[test]
fn requested_deviation_is_measured_on_curved_output() {
    for (name, kind, radius) in [
        ("mesh_sphere.step", "sphere", 60.0),
        ("mesh_cylinder.step", "cylinder", 50.0),
        ("mesh_torus.step", "torus", 3.0),
    ] {
        let loaded = load(name);
        for limit in [0.1, 0.01, 1.0] {
            let value = mesh::mesh_with_max_deviation(&loaded, limit).unwrap();
            assert_eq!(value["max_deviation_mm"], limit);
            assert!(value["missing"].as_array().unwrap().is_empty());
            let parsed = Mesh::new(&value);
            let mut worst = 0.0_f64;
            let mut area = 0.0;
            for (face, &(start, count)) in parsed.faces.iter().enumerate() {
                if matches!(
                    loaded.part.faces[face].surface,
                    haecceity::geom::Surface::Plane { .. }
                ) {
                    continue;
                }
                for triangle in parsed.indices[3 * start..3 * (start + count)]
                    .as_chunks::<3>()
                    .0
                {
                    let p: Vec<V3> = triangle
                        .iter()
                        .map(|&i| parsed.positions[3 * i..3 * i + 3].try_into().unwrap())
                        .collect();
                    let normal = geom::cross(geom::sub(p[1], p[0]), geom::sub(p[2], p[0]));
                    if kind == "sphere" {
                        assert!(geom::dot(normal, geom::add(geom::add(p[0], p[1]), p[2])) > 0.0);
                    } else if kind == "cylinder" {
                        let centre = geom::add(geom::add(p[0], p[1]), p[2]);
                        assert!(normal[0] * centre[0] + normal[1] * centre[1] > 0.0);
                    } else if kind == "torus" {
                        let centre = geom::scale(geom::add(geom::add(p[0], p[1]), p[2]), 1.0 / 3.0);
                        let rho = centre[0].hypot(centre[1]);
                        let outward = [
                            centre[0] * (1.0 - 40.0 / rho),
                            centre[1] * (1.0 - 40.0 / rho),
                            centre[2],
                        ];
                        assert!(geom::dot(normal, outward) > 0.0);
                    }
                    area += 0.5 * geom::norm(normal);
                    for i in 0..=10 {
                        for j in 0..=10 - i {
                            let w = [i as f64 / 10.0, j as f64 / 10.0, (10 - i - j) as f64 / 10.0];
                            let q = geom::add(
                                geom::add(geom::scale(p[0], w[0]), geom::scale(p[1], w[1])),
                                geom::scale(p[2], w[2]),
                            );
                            let radial = match kind {
                                "sphere" => geom::norm(q),
                                "torus" => (q[0].hypot(q[1]) - 40.0).hypot(q[2]),
                                _ => q[0].hypot(q[1]),
                            };
                            worst = worst.max((radius - radial).abs());
                        }
                    }
                }
            }
            if kind == "sphere" {
                let exact = 4.0 * std::f64::consts::PI * radius * radius;
                assert!(
                    (area - exact).abs() / exact < 0.01,
                    "whole sphere area: {area} vs {exact}"
                );
            }
            if kind == "cylinder" {
                let exact = 2.0 * std::f64::consts::PI * radius * 40.0;
                assert!(
                    (area - exact).abs() / exact < 0.01,
                    "complete cylinder area {area} vs {exact}"
                );
            }
            if kind == "torus" {
                let exact = 4.0 * std::f64::consts::PI.powi(2) * 40.0 * radius;
                assert!(
                    (area - exact).abs() / exact < 0.01,
                    "whole torus area {area} vs {exact}"
                );
            }
            let reported = value["achieved_deviation_mm"].as_f64().unwrap();
            assert!(worst > 0.0 && worst <= limit, "{name}: {worst} > {limit}");
            // The independent lattice and production quadrature sample different
            // points, and the reported maximum also includes planar cap trim sag.
            // These fixtures must not report less than the dense curved reference.
            assert!(
                reported >= worst - 1e-10,
                "{name}: reported {reported} < dense {worst}"
            );
            // Both maxima independently have to meet the requested bound.
            assert!(
                reported > 0.0 && reported <= limit,
                "{name}: reported {reported} > {limit}"
            );
            println!(
                "{name}: requested {limit}, reported {reported}, dense {worst}, {} bytes, {} triangles",
                value.to_string().len(),
                parsed.indices.len() / 3
            );
            assert_ne!(reported, limit, "must not echo the requested bound");
        }
        assert_eq!(
            specify_core::api::mesh_with_max_deviation(&loaded, None)
                .unwrap()
                .to_string(),
            mesh::mesh(&loaded).to_string(),
        );
    }
}

#[test]
fn requested_deviation_refuses_invalid_values_and_unresolved_faces() {
    let mut loaded = load("mesh_cylinder.step");
    for limit in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let reason = mesh::mesh_with_max_deviation(&loaded, limit).unwrap_err();
        assert!(reason.contains("positive finite"), "{reason}");
    }
    let reason = mesh::mesh_with_max_deviation(&load("mesh_sphere.step"), 1e-12).unwrap_err();
    assert!(
        reason.contains("face 0") && reason.contains("refinement limit"),
        "{reason}"
    );
    let cap = loaded
        .part
        .faces
        .iter()
        .find(|f| matches!(f.surface, haecceity::geom::Surface::Plane { .. }))
        .unwrap();
    loaded.unresolved_edges.push(cap.loops[0].edges[0].0);
    let reason = mesh::mesh_with_max_deviation(&loaded, 0.1).unwrap_err();
    assert!(
        reason.contains("boundary edge") && reason.contains("curve did not resolve"),
        "{reason}"
    );
    loaded.unresolved_edges.clear();
    loaded.unresolved_faces.push(0);
    let reason = mesh::mesh_with_max_deviation(&loaded, 0.1).unwrap_err();
    assert!(
        reason.contains("face 0") && reason.contains("surface did not resolve"),
        "{reason}"
    );
}

#[test]
fn reversed_whole_sphere_cavity_has_outward_winding_and_complete_area() {
    let loaded = load("mesh_spherical_cavity.step");
    let value = mesh::mesh_with_max_deviation(&loaded, 0.01).unwrap();
    let parsed = Mesh::new(&value);
    let mut spheres = 0;
    let mut area = 0.0;
    for (face, &(start, count)) in parsed.faces.iter().enumerate() {
        if !matches!(
            loaded.part.faces[face].surface,
            haecceity::geom::Surface::Sphere { .. }
        ) {
            continue;
        }
        spheres += 1;
        assert!(
            loaded.part.faces[face].reversed,
            "must exercise the reversed swap"
        );
        for triangle in parsed.indices[3 * start..3 * (start + count)]
            .as_chunks::<3>()
            .0
        {
            let p = triangle.map(|i| parsed.positions[3 * i..3 * i + 3].try_into().unwrap());
            let normal = geom::cross(geom::sub(p[1], p[0]), geom::sub(p[2], p[0]));
            assert!(
                geom::dot(normal, geom::add(geom::add(p[0], p[1]), p[2])) < 0.0,
                "cavity normals must point into the spherical void"
            );
            area += 0.5 * geom::norm(normal);
        }
    }
    assert_eq!(spheres, 1);
    let exact = 4.0 * std::f64::consts::PI * 60.0 * 60.0;
    assert!(
        (area - exact).abs() / exact < 0.01,
        "cavity area {area} vs {exact}"
    );
}

#[test]
fn planar_trim_chords_contribute_to_achieved_deviation() {
    let loaded = load("mesh_cylinder.step");
    let caps = mesh::mesh_with(&loaded, |face, deflection| {
        if matches!(
            loaded.part.faces[face].surface,
            haecceity::geom::Surface::Plane { .. }
        ) {
            mesh::face_mesh(&loaded, face, deflection)
        } else {
            Err("caps only".into())
        }
    });
    // The caps' surface distance is exactly zero. Their boundary polygon still sags
    // inside the true R50 rim, and must contribute to the achieved global value.
    let parsed = Mesh::new(&caps);
    let mut cap_sag = 0.0_f64;
    for &(start, count) in &parsed.faces {
        let mut edges = std::collections::BTreeMap::new();
        for &[a, b, c] in parsed.indices[3 * start..3 * (start + count)]
            .as_chunks::<3>()
            .0
        {
            for (i, j) in [(a, b), (b, c), (c, a)] {
                *edges.entry((i.min(j), i.max(j))).or_insert(0) += 1;
            }
        }
        for ((i, j), count) in edges {
            if count != 1 {
                continue;
            }
            let x = (parsed.positions[3 * i] + parsed.positions[3 * j]) * 0.5;
            let y = (parsed.positions[3 * i + 1] + parsed.positions[3 * j + 1]) * 0.5;
            cap_sag = cap_sag.max(50.0 - x.hypot(y));
        }
    }
    let value = mesh::mesh_with_max_deviation(&loaded, 1.0).unwrap();
    assert!(cap_sag > 0.01);
    assert!(value["achieved_deviation_mm"].as_f64().unwrap() >= cap_sag - 1e-10);
}

#[test]
fn trim_measurement_distinguishes_cap_and_cone_parameter_cuts() {
    let cap = load("mesh_spherical_cap.step");
    let ctc = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz"),
        None,
    )
    .unwrap();
    for loaded in [&cap, &ctc] {
        for limit in [0.1, 0.05] {
            let value = mesh::mesh_with_max_deviation(loaded, limit).unwrap();
            assert!(value["missing"].as_array().unwrap().is_empty());
            assert!(value["achieved_deviation_mm"].as_f64().unwrap() <= limit);
        }
    }
}

/// Full real-part tolerance audit. Explicit opt-in keeps the normal mesh binary
/// quick; CI/maintainers can require the corpus with QUIDDITY_CORPUS_REQUIRED=1.
#[test]
#[ignore = "full corpus survey: cargo test --release --test mesh corpus_requested_deviation -- --ignored --nocapture"]
fn corpus_requested_deviation() {
    let root =
        common::corpus_dir().expect("set QUIDDITY_CORPUS to run the full mesh tolerance survey");
    fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(&path, out);
            } else {
                let name = path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_ascii_lowercase();
                if [".step", ".stp", ".step.gz", ".stp.gz"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
                {
                    out.push(path);
                }
            }
        }
    }
    let mut paths = Vec::new();
    visit(&root, &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "the corpus has no STEP files");
    // Only independently investigated blockers that cannot be fixed in scope may be added here:
    // (relative path, part, face, exact diagnostic reason). Entries must be seen.
    const REMAINING: &[(&str, usize, usize, &str)] = &[];
    let mut seen = std::collections::BTreeSet::new();
    let (mut parts, mut passed, mut missing, mut exceptions) = (0, 0, 0, 0);
    let mut unexpected = Vec::new();
    for path in &paths {
        let relative = path.strip_prefix(&root).unwrap().to_string_lossy();
        for loaded in part::load_all(path).unwrap() {
            parts += 1;
            println!("checking {relative} part {}", loaded.binding.part);
            let default = mesh::mesh(&loaded);
            let absent = usizes(&default["missing"]);
            match mesh::mesh_with_max_deviation(&loaded, 0.1) {
                Ok(value) => {
                    let achieved = value["achieved_deviation_mm"].as_f64().unwrap();
                    assert!(
                        achieved.is_finite() && achieved <= 0.1,
                        "{relative}: {achieved}"
                    );
                    assert!(value["missing"].as_array().unwrap().is_empty());
                    passed += 1;
                    println!(
                        "{relative} part {}: achieved {achieved} mm",
                        loaded.binding.part
                    );
                }
                Err(reason) => {
                    let face = reason
                        .strip_prefix("face ")
                        .and_then(|s| s.split(':').next())
                        .and_then(|s| s.parse::<usize>().ok());
                    println!(
                        "{relative} part {}: REFUSED default missing={absent:?}: {reason}",
                        loaded.binding.part
                    );
                    if face.is_some_and(|f| absent.contains(&f)) {
                        assert!(
                            reason.contains("does not triangulate at any supported tolerance"),
                            "{reason}"
                        );
                        missing += 1;
                    } else if let Some(&(file, part, face, why)) =
                        REMAINING.iter().find(|&&(file, part, f, _)| {
                            file == relative && part == loaded.binding.part && Some(f) == face
                        })
                    {
                        assert!(reason.contains(why), "{relative}: {reason}");
                        seen.insert((file, part, face));
                        exceptions += 1;
                    } else {
                        unexpected.push(format!(
                            "{relative} part {} default missing={absent:?}: {reason}",
                            loaded.binding.part
                        ));
                    }
                }
            }
        }
    }
    for &(file, part, face, _) in REMAINING {
        assert!(
            seen.contains(&(file, part, face)),
            "stale tolerance refusal: {file} part {part} face {face}"
        );
    }
    println!(
        "survey: {} files, {parts} parts, {passed} met 0.1 mm, {missing} default-missing refusals, {exceptions} documented exceptions, {} unexpected refusals",
        paths.len(),
        unexpected.len()
    );
    assert!(
        unexpected.is_empty(),
        "unexpected tolerance refusals:\n{}",
        unexpected.join("\n")
    );
}
