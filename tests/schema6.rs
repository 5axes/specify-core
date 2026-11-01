//! Schema-6 published coordinates, checked against the actual slot walls and written PMI.
mod common;

use haecceity::brep_write::{WriteOptions, write_step};
use haecceity::geom::{V3, dot};
use haecceity::step::{IDENTITY, Placement};
use serde_json::{Map, Value, json};
use specify_core::{api, existing, part, rules};

fn vector(value: &Value) -> V3 {
    serde_json::from_value(value.clone()).unwrap()
}
fn direction(at: &Placement, p: V3) -> V3 {
    at.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2])
}
fn point(at: &Placement, p: V3) -> V3 {
    let p = direction(at, p);
    std::array::from_fn(|i| p[i] + at[i][3])
}
fn near(a: V3, b: V3, tolerance: f64) {
    assert!(
        a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < tolerance),
        "{a:?} != {b:?}"
    );
}
fn compare_numbers(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-5,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                compare_numbers(a, b)
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, a) in a {
                compare_numbers(a, &b[k])
            }
        }
        _ => assert_eq!(a, b),
    }
}

#[test]
fn golden_oriented_slots_end_planes_questions_defaults_and_written_pmi_move_rigidly() {
    // Geometry of quiddity's golden_oriented_slots.step, preserved in the pre-upgrade fixture.
    let source = part::load(
        &common::fixtures().join("resume_alpha7/golden_oriented_slots.step.gz"),
        None,
    )
    .unwrap();
    let published: std::collections::BTreeMap<_, _> =
        quiddity::correspondence::recognise(&source.part)
            .features
            .families()
            .into_iter()
            .collect();
    assert!(published.keys().all(|k| !k.starts_with("oriented_")));
    assert_eq!(published["slot_patterns"].len(), 1);
    assert_eq!(
        published["slot_patterns"][0]["direction"],
        json!([-1., 0., 0.])
    );
    let (s, c) = std::f64::consts::FRAC_PI_6.sin_cos();
    let z30 = [
        [c, -s, 0.0, 37.0],
        [s, c, 0.0, -19.0],
        [0.0, 0.0, 1.0, 11.0],
    ];
    // A proper generic rotation, Rz(30°) Ry(asin(0.6)), plus translation.
    let generic = [
        [0.8 * c, -s, 0.6 * c, 37.0],
        [0.8 * s, c, 0.6 * s, -19.0],
        [-0.6, 0.0, 0.8, 11.0],
    ];
    // A partially aligned converted frame: long_axis is a letter, transverse axes vectors.
    let (b, a) = 37_f64.to_radians().sin_cos();
    let mixed = [
        [-c, -s, 0.0, 37.0],
        [a * s, -a * c, -b, -19.0],
        [b * s, -b * c, a, 11.0],
    ];
    let scratch = std::env::temp_dir().join(format!("specify-schema6-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let mut reference: Option<(Value, Value, Map<String, Value>, Value)> = None;
    for (name, at) in [
        ("base", IDENTITY),
        ("z30", z30),
        ("generic", generic),
        ("mixed", mixed),
    ] {
        let moved = source.part.transformed(&at).unwrap();
        let (bytes, report) = write_step(
            &moved,
            &WriteOptions {
                name: "oriented slots",
                pmi: None,
            },
        )
        .unwrap();
        assert!(report.face_order_preserved);
        let loaded = part::load_bytes("slots.step", bytes).unwrap();
        let a = api::analyse(&loaded).unwrap();
        let slots: Vec<_> = a["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["family"] == "slots")
            .collect();
        assert_eq!(slots.len(), 3, "{name}: {slots:?}");
        for f in &slots {
            let r = &f["record"];
            assert_eq!(r["width"], 6.0);
            assert_eq!(r["length"], 24.0);
            assert!(r.get("_local").is_none() && r.get("_frame").is_none());
            let d = vector(&r["end_direction"]);
            // Each end plane agrees with an actual planar end wall (not a box or provenance).
            for end in [vector(&r["end_lo"]), vector(&r["end_hi"])] {
                assert!(
                    a["faces"].as_array().unwrap().iter().any(|p| {
                        p["kind"] == "plane"
                            && dot(vector(&p["direction"]), d).abs() > 0.999999
                            && dot(
                                std::array::from_fn(|i| vector(&p["centroid"])[i] - end[i]),
                                d,
                            )
                            .abs()
                                < 3e-6
                    }),
                    "{name}: end {end:?} of {} has no wall",
                    f["id"]
                );
            }
        }
        let qs = serde_json::to_value(
            api::questions(&a, &Map::new())
                .unwrap()
                .iter()
                .map(|q| q.to_dict())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let answers =
            rules::with_all_defaults(&a, json!({"part.material":"steel"}).as_object().unwrap())
                .unwrap();
        let intent = api::apply(&a, &answers, false).unwrap();
        let written = scratch.join(format!("{name}.step"));
        specify_core::writer::write(&loaded, &intent, &written, Some(&answers)).unwrap();
        let read = part::load(&written, None).unwrap();
        let pmi = serde_json::to_value(existing::read_existing(&read).unwrap()).unwrap();
        if let Some((base, base_qs, base_answers, base_pmi)) = &reference {
            assert_eq!(&qs, base_qs, "{name}: questions/defaults");
            assert_eq!(&answers, base_answers, "{name}: accepted defaults");
            compare_numbers(&pmi, base_pmi);
            for f in slots {
                let old = base["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|b| b["id"] == f["id"])
                    .unwrap();
                let d = vector(&f["record"]["end_direction"]);
                let expected_d = direction(&at, vector(&old["record"]["end_direction"]));
                assert!(dot(d, expected_d).abs() > 0.999999);
                let ends = [
                    vector(&f["record"]["end_lo"]),
                    vector(&f["record"]["end_hi"]),
                ];
                for old_end in [
                    vector(&old["record"]["end_lo"]),
                    vector(&old["record"]["end_hi"]),
                ] {
                    let expected = point(&at, old_end);
                    assert!(ends.iter().any(|end| {
                        dot(std::array::from_fn(|i| end[i] - expected[i]), d).abs() < 3e-6
                    }));
                }
            }
            for (old, new) in base["faces"]
                .as_array()
                .unwrap()
                .iter()
                .zip(api::analyse(&read).unwrap()["faces"].as_array().unwrap())
            {
                near(
                    point(&at, vector(&old["centroid"])),
                    vector(&new["centroid"]),
                    3e-6,
                );
                near(
                    direction(&at, vector(&old["direction"])),
                    vector(&new["direction"]),
                    3e-8,
                );
            }
        } else {
            reference = Some((a, qs, answers, pmi));
        }
        eprintln!("{name}: 3 slots, 6 end planes, unchanged questions/defaults and PMI");
    }
    std::fs::remove_dir_all(scratch).unwrap();
}

/// Independent material-side evidence for the #81 recoveries and #89 corrections used
/// in the oracle ledger: labels must agree with the actual cylinder's outward sense.
#[test]
fn corrected_round_inventory_agrees_with_material_side() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    for (name, convex, concave, convex_count, inside_count, turned_inside) in [
        (
            "cgb202",
            vec![
                1248, 1250, 1252, 1254, 1256, 1258, 1450, 1453, 1464, 1473, 1475, 1477, 1623, 1631,
                2056, 2057,
            ],
            vec![2, 3, 11, 44, 46, 692, 697, 1215, 1219],
            // quiddity-rust#134 moves 194 turned root fillets from convex to concave.
            113,
            595, // Includes #127's 401 prismatic inside rounds and sphere caps.
            194,
        ),
        (
            "cgb217",
            vec![177, 182, 205, 210, 233, 238, 261, 266],
            vec![],
            // quiddity-rust#134 moves five turned root fillets to the inside inventory.
            17,
            68, // Includes #127's 63 prismatic inside rounds.
            5,
        ),
        ("cgb241", vec![39, 47], vec![27], 2, 0, 0),
    ] {
        let path = corpus.join(format!("cadgenbench_inputs/{name}.step"));
        let path = if path.is_file() {
            path
        } else {
            path.with_extension("step.gz")
        };
        let loaded = part::load(&path, None).unwrap();
        let features = specify_core::features::recognise(&loaded).unwrap();
        let fillets: Vec<_> = features.iter().filter(|f| f.family == "fillets").collect();
        // v0.4.2 (#167/#159) groups interrupted torus patches into one fillet.
        // Keep the historical geometric coverage thresholds: count each native torus
        // patch, while a cylindrical round with spherical caps still counts once.
        let patches = |f: &specify_core::features::Feature| {
            f.faces
                .iter()
                .filter(|&&face| {
                    matches!(
                        loaded.part.faces[face].surface,
                        quiddity::kernel::geom::Surface::Torus { .. }
                    )
                })
                .count()
                .max(1)
        };
        // quiddity #120 recognises tangent convex cylinders at any wall angle/run
        // direction. #91's roll no longer changes these fillets into generic blends.
        assert_eq!(
            fillets.iter().map(|f| patches(f)).sum::<usize>(),
            convex_count + inside_count,
            "{name}"
        );
        assert_eq!(
            fillets
                .iter()
                .filter(|f| f.record["side"] == "convex")
                .map(|f| patches(f))
                .sum::<usize>(),
            convex_count,
            "{name}"
        );
        assert_eq!(
            fillets
                .iter()
                .filter(|f| f.record["side"] == "concave")
                .map(|f| patches(f))
                .sum::<usize>(),
            inside_count,
            "{name}"
        );
        assert_eq!(
            fillets
                .iter()
                .filter(|f| f.record["side"] == "concave" && f.record["turned"] == true)
                .map(|f| patches(f))
                .sum::<usize>(),
            turned_inside,
            "{name}: quiddity-rust#134 turned inside fillets"
        );
        for face in convex {
            assert_eq!(
                loaded.part.frame_points_outward(face),
                Some(true),
                "{name}:{face}"
            );
            assert!(
                features
                    .iter()
                    .any(|f| f.family == "fillets" && f.faces.contains(&face)),
                "{name}:{face}"
            );
        }
        if name == "cgb241" {
            assert_eq!(
                features
                    .iter()
                    .find(|f| f.id == "fillets:47")
                    .unwrap()
                    .record["radius"],
                23.0
            );
        }
        if name == "cgb217" {
            // quiddity #120: all sixteen R1 rounds are now fillets, on the same faces.
            for face in [
                177, 182, 191, 196, 205, 210, 219, 224, 233, 238, 247, 252, 261, 266, 296, 301,
            ] {
                let fillet = features
                    .iter()
                    .find(|f| f.family == "fillets" && f.faces.contains(&face))
                    .unwrap();
                assert_eq!(fillet.record["radius"], 1.0);
                assert_eq!(loaded.part.frame_points_outward(face), Some(true));
                assert!(
                    !features
                        .iter()
                        .any(|f| f.family == "blends" && f.faces.contains(&face))
                );
            }
        }
        for face in concave {
            assert_eq!(
                loaded.part.frame_points_outward(face),
                Some(false),
                "{name}:{face}"
            );
            // #127 restores proved inside rounds with their correct material side;
            // cgb241's large non-round face remains outside the fillet inventory.
            let found: Vec<_> = fillets.iter().filter(|f| f.faces.contains(&face)).collect();
            if name == "cgb202" {
                assert_eq!(found.len(), 1, "{name}:{face}");
                assert_eq!(found[0].record["side"], "concave", "{name}:{face}");
            } else {
                assert!(found.is_empty(), "{name}:{face}");
            }
        }
    }
    // Exact Python convex claims removed by #89 in this capture, checked against material.
    for (case, faces) in [
        ("corpus/mfcadpp/1000", vec![22]),
        ("corpus/mfcadpp/10033", vec![7]),
        ("corpus/mfcadpp/10103", vec![24]),
        ("corpus/mfcadpp/10127", vec![18]),
        ("corpus/mfcadpp/10156", vec![2]),
        ("corpus/mfcadpp/10163", vec![9]),
        ("corpus/mfcadpp_holdout/18016", vec![4]),
        ("corpus/mfcadpp_holdout/275", vec![15]),
        ("corpus/mfcadpp_holdout/363", vec![14, 25]),
        (
            "corpus/nist/nist_ctc_03_asme1_rc",
            vec![15, 21, 25, 31, 35, 38],
        ),
        ("nist/nist_ctc_03_asme1_ap242", vec![4, 59, 63, 67, 69, 8]),
    ] {
        let path = common::case(case)
            .input()
            .expect("the configured corpus/fixture");
        let loaded = part::load(&path, None).unwrap();
        let features = specify_core::features::recognise(&loaded).unwrap();
        for face in faces {
            assert_eq!(
                loaded.part.frame_points_outward(face),
                Some(false),
                "{case}:{face}"
            );
            assert!(
                !features
                    .iter()
                    .any(|f| f.family == "fillets" && f.faces.contains(&face)),
                "{case}:{face}"
            );
        }
    }
}

/// The formerly oriented chamfer is one merged feature, with its actual planar support.
#[test]
fn merged_chamfer_10252_has_the_same_support_and_run() {
    let case = common::case("corpus/mfcadpp/10252");
    let Some(path) = case.input() else { return };
    let loaded = part::load(&path, None).unwrap();
    let found = specify_core::features::recognise(&loaded).unwrap();
    assert!(!found.iter().any(|f| f.family.starts_with("oriented_")));
    let f = found.iter().find(|f| f.id == "chamfers:2").unwrap();
    assert_eq!(f.faces, [2]);
    let r = &f.record;
    assert_eq!(r["angle"], 45.0);
    assert_eq!(r["leg1"], 0.54);
    assert_eq!(r["leg2"], 0.54);
    assert_eq!(r["length"], 18.694);
    assert_eq!(r["turned"], false);
    let axis = vector(&r["axis"]);
    let leg1 = vector(&r["leg1_direction"]);
    let leg2 = vector(&r["leg2_direction"]);
    let surface = &loaded.part.faces[2].surface;
    let haecceity::geom::Surface::Plane { frame } = surface else {
        panic!("not a plane")
    };
    assert!(dot(axis, frame.z).abs() < 1e-6);
    assert!(dot(axis, leg1).abs() < 1e-6);
    assert!(dot(axis, leg2).abs() < 1e-6);
    // Length is the overlap of the two support edges, not the whole bevel's envelope.
    let station = dot(vector(&r["corner"]), axis);
    let mut runs: Vec<[f64; 2]> = loaded
        .part
        .face_edges(2)
        .into_iter()
        .filter_map(|edge| {
            let points = loaded.part.edges[edge].vertex_points();
            if points.len() != 2 {
                return None;
            }
            let delta = std::array::from_fn(|i| points[1][i] - points[0][i]);
            let length = dot(delta, delta).sqrt();
            if length < 10. || dot(delta, axis).abs() / length < 0.999999 {
                return None;
            }
            let mut span = points
                .iter()
                .map(|p| dot(*p, axis) - station)
                .collect::<Vec<_>>();
            span.sort_by(f64::total_cmp);
            Some([span[0], span[1]])
        })
        .collect();
    assert_eq!(runs.len(), 2);
    runs.sort_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])));
    let mut spans: Vec<[f64; 2]> = serde_json::from_value(r["support_spans"].clone()).unwrap();
    spans.sort_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])));
    for (actual, published) in runs.iter().zip(&spans) {
        for (a, b) in actual.iter().zip(published) {
            assert!((a - b).abs() < 0.001501);
        }
    }
    let overlap = runs[0][1].min(runs[1][1]) - runs[0][0].max(runs[1][0]);
    assert!((overlap - r["length"].as_f64().unwrap()).abs() < 0.000501);
}

fn axis(value: &Value) -> V3 {
    match value.as_str() {
        Some("x") => [1., 0., 0.],
        Some("y") => [0., 1., 0.],
        Some("z") => [0., 0., 1.],
        _ => vector(value),
    }
}

/// Rejudge the selected cases of the 256 formerly-undetermined additive frame fields by reconstructing
/// their nominal levels and spans in file coordinates and checking the physical supports.
/// levels.rs admits near-axis planes at cosine > 0.99 and clusters origins within record.tol
/// (0.5 mm); the span check remains 0.001 mm, independently of that recognition policy.
#[test]
fn published_body_level_frames_reconstruct_the_physical_supports() {
    use std::collections::{BTreeMap, BTreeSet};
    let ledger: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/known_differences/features.json")).unwrap();
    let mut levels: BTreeMap<String, BTreeSet<(String, usize)>> = BTreeMap::new();
    for e in ledger {
        if e["pattern"] != "Schema-6 published measurement differs from frozen Python" {
            continue;
        }
        let path = e["path"].as_str().unwrap();
        let Some((feature, rest)) = path
            .strip_prefix("features[id=")
            .and_then(|p| p.split_once("].record.body_levels["))
        else {
            continue;
        };
        let (index, field) = rest.split_once("]").unwrap();
        if ![".origin", ".height_axis", ".x_axis", ".y_axis"].contains(&field) {
            continue;
        }
        levels
            .entry(e["case"].as_str().unwrap().to_owned())
            .or_default()
            .insert((feature.to_owned(), index.parse().unwrap()));
    }
    assert_eq!(levels.values().map(BTreeSet::len).sum::<usize>(), 64);
    let expected: usize = levels
        .iter()
        .filter(|(case, _)| common::case_selected(case))
        .map(|(_, supports)| supports.len())
        .sum();
    let mut checked = 0;
    // This is a corpus-wide ledger sweep, so use the same selection as the oracle sweeps.
    // Keep all physical assertions and the complete capture's 64-support inventory above.
    for (case, supports) in levels
        .into_iter()
        .filter(|(case, _)| common::case_selected(case))
    {
        let Some(path) = common::case(&case).input() else {
            continue;
        };
        let loaded = part::load(
            &path,
            common::case(&case).meta["part"]
                .as_u64()
                .map(|p| p as usize),
        )
        .unwrap();
        let found = specify_core::features::recognise(&loaded).unwrap();
        for (id, index) in supports {
            let f = found.iter().find(|f| f.id == id).unwrap();
            let level = &f.record["body_levels"][index];
            let origin = vector(&level["origin"]);
            let z = axis(&level["height_axis"]);
            let x = axis(&level["x_axis"]);
            let y = axis(&level["y_axis"]);
            for v in [x, y, z] {
                assert!((dot(v, v) - 1.).abs() < 1e-8);
            }
            assert!(dot(x, y).abs() < 1e-8 && dot(x, z).abs() < 1e-8 && dot(y, z).abs() < 1e-8);
            let height = dot(origin, z) + level["z"].as_f64().unwrap();
            let spans = [(x, "x_span"), (y, "y_span")].map(|(d, key)| {
                (
                    d,
                    [
                        dot(origin, d) + level[key][0].as_f64().unwrap(),
                        dot(origin, d) + level[key][1].as_f64().unwrap(),
                    ],
                )
            });
            let level_tolerance = f.record["tol"].as_f64().unwrap();
            let faces: Vec<_> = loaded
                .part
                .faces
                .iter()
                .enumerate()
                .filter_map(|(i, f)| {
                    let haecceity::geom::Surface::Plane { frame } = &f.surface else {
                        return None;
                    };
                    if dot(frame.z, z).abs() <= 0.99
                        || (dot(frame.origin, z) - height).abs() > level_tolerance + 1e-6
                    {
                        return None;
                    }
                    spans
                        .iter()
                        .all(|(d, span)| {
                            let (lo, hi) = loaded.part.extent_along(&[i], *d);
                            lo >= span[0] - 0.001001 && hi <= span[1] + 0.001001
                        })
                        .then_some(i)
                })
                .collect();
            assert!(
                !faces.is_empty(),
                "{case} {id} level {index}: no physical plane"
            );
            for (d, span) in spans {
                let (lo, hi) = loaded.part.extent_along(&faces, d);
                assert!(
                    (lo - span[0]).abs() < 0.001001 && (hi - span[1]).abs() < 0.001001,
                    "{case} {id} level {index}: {lo}..{hi} != {span:?}"
                );
            }
            checked += 1;
        }
    }
    if std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_some() {
        assert_eq!(
            checked, expected,
            "every selected body-level support checked"
        );
    }
    eprintln!(
        "checked {checked} body-level planes/spans ({} additive frame fields)",
        checked * 4
    );
}

/// An oriented box is not a file AABB: check each published interval against the actual B-rep,
/// rather than comparing its scalar tuple with Python's tuple in another basis (#92).
#[test]
fn published_profile_bounds_measure_the_body_in_their_stated_basis() {
    for (case, count) in [
        ("corpus/cadgenbench/threaded_connector_109", 10),
        ("corpus/cadgenbench_inputs/cgb217", 4),
    ] {
        let Some(path) = common::case(case).input() else {
            continue;
        };
        let loaded = part::load(&path, None).unwrap();
        let found = specify_core::features::recognise(&loaded).unwrap();
        let faces: Vec<_> = (0..loaded.part.faces.len()).collect();
        let mut checked = 0;
        for f in found {
            let Some(profile) = f.record.get("profile") else {
                continue;
            };
            let bounds = &profile["body_bounds"];
            if !bounds.is_object() {
                continue;
            }
            let origin = vector(&bounds["origin"]);
            let axes: Vec<_> = bounds["axes"]
                .as_array()
                .unwrap()
                .iter()
                .map(axis)
                .collect();
            for (i, d) in axes.iter().enumerate() {
                assert!((dot(*d, *d) - 1.).abs() < 1e-8);
                for other in &axes[..i] {
                    assert!(dot(*d, *other).abs() < 1e-8);
                }
                let (lo, hi) = loaded.part.extent_along(&faces, *d);
                for (expected, actual) in [
                    (lo, bounds["bounds"][i][0].as_f64().unwrap()),
                    (hi, bounds["bounds"][i][1].as_f64().unwrap()),
                ] {
                    assert!(
                        (expected - dot(origin, *d) - actual).abs() < 1e-6,
                        "{case} {} axis {i}: {expected} relative to {origin:?} != {actual}",
                        f.id
                    );
                }
            }
            if case.contains("threaded_connector") {
                // The 10 newly different X origins are only 3.01e-9 mm off the physical Z
                // axis, far below the existing 1e-6 face-coordinate publication step.
                assert!(profile["axis_origin"][0].as_f64().unwrap().abs() < 1e-8);
            }
            checked += 1;
        }
        assert_eq!(checked, count, "{case}");
    }
}

/// Compare the geometry across the namespace change, preserving separate uncertainty about
/// old body volume/area integrals: those are not proved by matching a bevel's support.
#[test]
fn merged_families_keep_the_frozen_oriented_feature_geometry() {
    let mut checked = 0;
    for name in [
        "corpus/mfcadpp/1007",
        "corpus/mfcadpp/10252",
        "corpus/mfcadpp_holdout/408",
        "corpus/mfcadpp_holdout/467",
        "corpus/nist/nist_ctc_01_asme1_rd",
        "corpus/nist/nist_ctc_04_asme1_rd#0",
        "nist/nist_ctc_01_asme1_ap242",
        "nist/nist_ctc_04_asme1_ap242#0",
    ] {
        let case = common::case(name);
        let Some(path) = case.input() else { continue };
        let loaded = part::load(&path, case.meta["part"].as_u64().map(|p| p as usize)).unwrap();
        let found = specify_core::features::recognise(&loaded).unwrap();
        let old = case.artifact("analysis").unwrap();
        for old in old["features"].as_array().unwrap().iter().filter(|f| {
            ["oriented_chamfers", "oriented_slots"].contains(&f["family"].as_str().unwrap())
        }) {
            let id = old["id"]
                .as_str()
                .unwrap()
                .strip_prefix("oriented_")
                .unwrap();
            let f = found.iter().find(|f| f.id == id).unwrap();
            assert_eq!(serde_json::to_value(&f.faces).unwrap(), old["faces"]);
            let before = &old["record"];
            let after = &f.record;
            if f.family == "chamfers" {
                for field in ["length", "angle"] {
                    assert_eq!(after[field], before[field]);
                }
                near(vector(&after["at"]), vector(&before["at"]), 1e-6);
                near(vector(&after["corner"]), vector(&before["corner"]), 1e-6);
                let run = vector(&after["axis"]);
                let old_run = vector(&before["run"]);
                let sense = if dot(run, old_run) > 0. { 1. } else { -1. };
                let norm = dot(old_run, old_run).sqrt();
                near(run, old_run.map(|x| sense * x / norm), 1e-6);
                for (n, other) in [(1, 2), (2, 1)] {
                    // Equal legs can be swapped, carrying their direction and span together.
                    let direction_key = format!("leg{n}_direction");
                    let leg = format!("leg{n}");
                    let candidates = [n, other];
                    assert!(
                        candidates.iter().any(|m| {
                            if before[format!("leg{m}")].as_f64() != after[&leg].as_f64() {
                                return false;
                            }
                            let a = vector(&before[format!("leg{m}_direction")]);
                            let b = vector(&after[&direction_key]);
                            if a.iter().zip(b).any(|(a, b)| (a - b).abs() > 1e-6) {
                                return false;
                            }
                            let old_span = &before["support_spans"][m - 1];
                            let mut span = [
                                sense * old_span[0].as_f64().unwrap(),
                                sense * old_span[1].as_f64().unwrap(),
                            ];
                            span.sort_by(f64::total_cmp);
                            (span[0] - after["support_spans"][n - 1][0].as_f64().unwrap()).abs()
                                < 1e-6
                                && (span[1] - after["support_spans"][n - 1][1].as_f64().unwrap())
                                    .abs()
                                    < 1e-6
                        }),
                        "{name} {id}: leg {n}"
                    );
                }
            } else {
                for field in ["width", "length"] {
                    assert_eq!(after[field], before[field]);
                }
                let d = vector(&after["end_direction"]);
                let old_d = vector(&before["long_direction"]);
                assert!(dot(d, old_d).abs() > 0.999999);
                let center = vector(&before["center"]);
                let half = before["length"].as_f64().unwrap() / 2.;
                for sign in [-1., 1.] {
                    let expected = dot(center, d) + sign * half;
                    assert!(
                        ["end_lo", "end_hi"]
                            .iter()
                            .any(|key| (dot(vector(&after[*key]), d) - expected).abs() < 0.001001)
                    );
                }
                assert!(
                    dot(
                        vector(&after["width_axis"]),
                        vector(&before["width_direction"])
                    )
                    .abs()
                        > 0.999999
                );
            }
            checked += 1;
        }
    }
    if common::corpus_dir().is_some() {
        assert_eq!(checked, 20);
    }
}
