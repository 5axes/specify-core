//! Faces and features (`src/features.rs`, port of `features.py`), checked against specify-core.
//!
//! Ports `tests/test_features.py` and the reading parts of `tests/test_load.py` (face indices
//! are stable: the same bytes always describe the same faces), then sweeps every oracle case:
//! `analysis.faces` and `analysis.features`, each difference matched by an entry of
//! `tests/fixtures/known_differences/features.json`.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{Case, Difference, Tolerances, cases, check_known, compare, corpus_dir, is_error};
use serde_json::{Value, json};
use specify_core::features::{self, Feature};
use specify_core::part::{self, LoadedPart};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(name)
}

/// The spool of specify-core's `conftest.py`, if the corpus is here.
fn spool() -> Option<PathBuf> {
    let path = corpus_dir()?.join("cadgenbench/flanged_spool_132.step");
    assert!(
        path.is_file() || std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
        "QUIDDITY_CORPUS_REQUIRED: missing corpus input {}",
        path.display()
    );
    path.is_file().then_some(path)
}

fn analyse(path: &Path) -> (Vec<Value>, Vec<Feature>) {
    let loaded = part::load(path, None).unwrap();
    let faces = features::describe_faces(&loaded).unwrap();
    let found = features::recognise(&loaded).unwrap();
    (
        faces
            .iter()
            .map(|f| serde_json::to_value(f).unwrap())
            .collect(),
        found,
    )
}

fn patterns(found: &[Feature]) -> Vec<&Feature> {
    found
        .iter()
        .filter(|f| f.family == "hole_patterns")
        .collect()
}

// --- test_features.py ------------------------------------------------------------------------

/// test_xcaf_load_recognises_what_quiddity_load_does: the loaded part is recognised as
/// quiddity recognises the file read directly.
#[test]
fn the_loaded_part_is_recognised_as_quiddity_recognises_the_file() {
    for path in [fixture("plate.step"), fixture("pin.step")]
        .into_iter()
        .chain(spool())
    {
        let loaded = part::load(&path, None).unwrap();
        let ours = quiddity::correspondence::recognise(&loaded.part);
        let theirs =
            quiddity::correspondence::recognise(&haecceity::read_step_file(&path).unwrap());
        assert_eq!(
            serde_json::to_value(&ours.features).unwrap(),
            serde_json::to_value(&theirs.features).unwrap(),
            "{}",
            path.display()
        );
    }
}

/// test_analysis_is_json_and_ids_are_unique.
#[test]
fn features_are_json_and_ids_are_unique() {
    for path in [fixture("plate.step"), fixture("pin.step")]
        .into_iter()
        .chain(spool())
    {
        let (_, found) = analyse(&path);
        // Python's `json.dumps(allow_nan=False)`: the text is strict JSON (no NaN or Infinity
        // tokens, which serde_json would refuse to read back) and reads back to the same value.
        let value = serde_json::to_value(&found).unwrap();
        let text = serde_json::to_string(&found).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), value);
        let mut ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "{}: duplicate ids", path.display());
    }
}

/// test_hole_patterns_link_every_member: the spool's three bolt circles, every member linked.
#[test]
fn hole_patterns_link_every_member() {
    let Some(spool) = spool() else {
        eprintln!("skipped: no corpus");
        return;
    };
    let (_, found) = analyse(&spool);
    let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
    let patterns = patterns(&found);
    assert_eq!(patterns.len(), 3, "{ids:?}");
    for p in patterns {
        assert!(p.notes.is_empty(), "{:?}", p.notes);
        assert_eq!(p.members.len(), p.record["holes"].as_array().unwrap().len());
        assert!(p.members.iter().all(|m| ids.contains(&m.as_str())));
    }
}

/// test_grid_is_linked.
#[test]
fn the_plates_grid_is_linked() {
    let (_, found) = analyse(&fixture("plate.step"));
    let grids = patterns(&found);
    let kinds: Vec<&Value> = grids.iter().map(|g| &g.record["kind"]).collect();
    assert_eq!(kinds, [&json!("grid")]);
    assert_eq!(grids[0].members.len(), 6);
    // The pattern's faces are its members' faces.
    let mut faces: Vec<usize> = found
        .iter()
        .filter(|f| grids[0].members.contains(&f.id))
        .flat_map(|f| f.faces.clone())
        .collect();
    faces.sort_unstable();
    assert_eq!(grids[0].faces, faces);
}

/// test_four_corner_holes_are_quiddity_rectangle_with_members.
#[test]
fn four_corner_holes_are_a_rectangle_with_members() {
    let (_, found) = analyse(&fixture(
        "four_corner_holes_are_quiddity_rectangle_with_members.step",
    ));
    let rects = patterns(&found);
    assert_eq!(rects.len(), 1);
    let rect = rects[0];
    assert_eq!(rect.record["kind"], "rectangle");
    assert!(rect.id.starts_with("hole_patterns.rectangle:"));
    assert!(rect.members.len() == 4 && rect.notes.is_empty());
}

/// test_identical_holes_no_pattern_claims_form_a_set.
#[test]
fn identical_holes_no_pattern_claims_form_a_set() {
    let (_, found) = analyse(&fixture(
        "identical_holes_no_pattern_claims_form_a_set.step",
    ));
    let pairs = patterns(&found);
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        (pairs[0].record["kind"].as_str(), pairs[0].members.len()),
        (Some("set"), 2)
    );
}

// --- test_load.py (reading) ------------------------------------------------------------------

/// test_face_indices_are_stable_across_processes, and determinism: two loads of the same bytes
/// describe the same faces and recognise the same features, to the byte.
#[test]
fn two_runs_give_identical_json() {
    for path in [
        fixture("plate.step"),
        fixture("pin.step"),
        fixture("stack.step"),
    ]
    .into_iter()
    .chain(spool())
    {
        let run = || {
            part::load_all(&path)
                .unwrap()
                .iter()
                .map(|lp| {
                    serde_json::to_string(&(
                        features::describe_faces(lp).unwrap(),
                        features::recognise(lp).unwrap(),
                    ))
                    .unwrap()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run(), "{}", path.display());
    }
}

/// A part of an assembly with an unresolved face: haecceity's own `Part` of it records the face
/// (upstream need U1, met), so the part is not degraded, but its faces are not described, as
/// haecceity cannot integrate the unresolved face (no area or centroid is made up; the refusal
/// names the face). A part a caller rebuilt itself without that record (`rebuilt`) is still
/// refused by recognition, with the reason.
#[test]
fn recognition_refuses_a_degraded_part() {
    let text = std::fs::read_to_string(fixture("assembly.step")).unwrap();
    let plane = "#1181 = PLANE('',#1182);";
    assert!(text.contains(plane));
    let text = text.replace(
        plane,
        "#1181 = OFFSET_SURFACE('',#99999,0.,.F.);\n#99999 = PLANE('',#1182);",
    );
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("features-degraded.step");
    std::fs::write(&path, text).unwrap();
    let mut pin = part::load(&path, Some(1)).unwrap();
    assert_eq!(pin.degraded(), None);
    assert_eq!(pin.part.unresolved_faces(), [5]);
    assert_eq!(
        features::describe_faces(&pin).unwrap_err(),
        features::FeatureError::NoMoments { part: 1, face: 5 }
    );
    pin.rebuilt = true;
    let why = features::recognise(&pin).unwrap_err().to_string();
    assert!(why.contains("U1") && why.contains("[5]"), "{why}");
    assert!(features::recognise(&part::load(&path, Some(0)).unwrap()).is_ok());
}

/// quiddity #119: a failed discovery has source faces and a reason, but claims no feature.
#[test]
fn discovery_refusals_are_not_question_targets() {
    use quiddity::features::{Features, discovery::DiscoveryRefusal};
    let loaded = part::load(&fixture("plate.step"), None).unwrap();
    let mut recognition = quiddity::correspondence::recognise(&loaded.part);
    let families = [
        "pad_refusals",
        "prismatic_pocket_refusals",
        "section_passage_refusals",
        "oriented_slot_refusals",
        "polygonal_boss_refusals",
        "reconciliation_refusals",
    ];
    recognition.features = Features {
        discovery_refusals: families
            .into_iter()
            .map(|family| {
                (
                    family,
                    vec![DiscoveryRefusal {
                        faces: vec![0, 1],
                        reason: "unproved source roles".into(),
                    }],
                )
            })
            .collect(),
        ..Features::default()
    }
    .into();
    let document = quiddity::recognition::document(&recognition, loaded.part.faces.len());
    assert_eq!(document.records.len(), families.len());
    assert!(document.records.iter().all(|r| r.is_refusal()));
    let features = features::recognise_from(&loaded, &recognition).unwrap();
    assert!(features.is_empty());
    let analysis =
        json!({"faces": features::describe_faces(&loaded).unwrap(), "features": features});
    let questions = specify_core::rules::questions(&analysis, &Default::default()).unwrap();
    assert!(questions.iter().all(|q| q.feature.is_none()));
}

// --- the oracle sweeps -----------------------------------------------------------------------

/// The Rust part a case's analysis is of, or why there is none to compare.
enum Subject {
    Part(Box<LoadedPart>),
    /// A corpus file this run does not have, or a case with no analysis.
    Skip,
    /// The case is a part Rust does not have (NIST files XCAF splits: question Q7).
    Absent(String),
}

/// The case's analysis (Python's) and the Rust part it describes.
fn subject(case: &Case) -> (Option<Value>, Subject) {
    let Some(analysis) = case.artifact("analysis") else {
        return (None, Subject::Skip);
    };
    if is_error(&analysis) {
        return (None, Subject::Skip);
    }
    let Some(input) = case.input() else {
        return (None, Subject::Skip);
    };
    let k = case.part();
    // Python's parts of the file (the file-level case's listing) against Rust's.
    let python_parts = case.meta["parts"].as_u64().unwrap_or(1) as usize;
    let rust_parts = part::parts(&input).map_or(0, |p| p.len());
    let part = match k {
        Some(k) if rust_parts == 1 && python_parts > 1 => {
            if k > 0 {
                return (
                    Some(analysis),
                    Subject::Absent(format!(
                        "Python's part {k} of {python_parts}; one part in Rust (Q7)"
                    )),
                );
            }
            part::load(&input, Some(0))
        }
        k => part::load(&input, k),
    };
    match part {
        Ok(p) => (Some(analysis), Subject::Part(Box::new(p))),
        Err(e) => (Some(analysis), Subject::Absent(e.to_string())),
    }
}

fn absent(case: &Case, why: &str) -> Difference {
    Difference {
        case: case.name.clone(),
        path: "part".into(),
        python: Some(json!("analysed")),
        rust: Some(json!(why)),
    }
}

/// The features area's full sweep (one `check_known`): every case's faces and features.
///
/// Faces: kind and id exactly, area and centroid within 1e-6 relative (the centroid also within
/// its 6-decimal rounding), direction and origin within their rounding. Features: by id where
/// ids match (records within 1e-9), each feature only one side has paired where it can be
/// (`paired`), plus the order of the features both have; a refusal (a degraded part) is one
/// difference for the case.
#[test]
fn faces_and_features_match_the_oracle() {
    let mut differences = Vec::new();
    let mut compared = 0;
    let mut refused = Vec::new();
    for case in cases() {
        let (analysis, subject) = subject(&case);
        match subject {
            Subject::Skip => continue,
            Subject::Absent(why) => differences.push(absent(&case, &why)),
            Subject::Part(loaded) => {
                compared += 1;
                let analysis = analysis.unwrap();
                differences.extend(face_differences(&case, &analysis, &loaded));
                match feature_differences(&case, &analysis, &loaded) {
                    Ok(found) => differences.extend(found),
                    Err(d) => {
                        refused.push(case.name.clone());
                        differences.push(*d);
                    }
                }
            }
        }
    }
    println!(
        "{compared} parts compared; refused as degraded: {} {refused:?}",
        refused.len()
    );
    let mut families: BTreeMap<String, usize> = BTreeMap::new();
    for d in &differences {
        if let Some(rest) = d.path.strip_prefix("features[id=") {
            let family = rest.split([':', '.']).next().unwrap_or_default();
            *families.entry(family.to_string()).or_default() += 1;
        }
    }
    println!("feature differences by family: {families:?}");
    dump("features", &differences);
    check_area("features", &differences);
}

fn face_differences(case: &Case, analysis: &Value, loaded: &LoadedPart) -> Vec<Difference> {
    let tolerances = Tolerances::default()
        .with("faces[*].area", 1e-6, 0.0)
        // One unit of the centroid's 6-decimal rounding, and a little for the float error of
        // the difference: a value either side of a rounding boundary is not a difference.
        .with("faces[*].centroid[*]", 1e-6, 1.000_001e-6)
        .with("faces[*].direction[*]", 1e-9, 1e-9)
        .with("faces[*].origin[*]", 1e-6, 1e-6)
        .with("faces[*].radius", 1e-9, 1e-9);
    // `contact` is the mates' (api.analyse adds it for a part of an assembly; the mates stream
    // compares it), not describe_faces'.
    let mut faces = analysis["faces"].clone();
    for face in faces.as_array_mut().into_iter().flatten() {
        face.as_object_mut().map(|f| f.remove("contact"));
    }
    let python = json!({ "faces": faces });
    let rust = match features::describe_faces(loaded) {
        Ok(faces) => json!({ "faces": faces }),
        Err(e) => json!({"faces": e.to_string()}),
    };
    compare(&case.name, &python, &rust, &tolerances)
}

/// The case's feature differences, or the one difference of a refusal.
fn feature_differences(
    case: &Case,
    analysis: &Value,
    loaded: &LoadedPart,
) -> Result<Vec<Difference>, Box<Difference>> {
    let tolerances = Tolerances::default()
        .align("features", "id")
        .with("features*", 1e-9, 1e-9);
    let python = &analysis["features"];
    let found = features::recognise(loaded).map_err(|e| {
        Box::new(Difference {
            case: case.name.clone(),
            path: "features".into(),
            python: Some(json!(python.as_array().map_or(0, Vec::len))),
            rust: Some(json!(e.to_string())),
        })
    })?;
    let rust = serde_json::to_value(&found).unwrap();
    let mut out = paired(
        &case.name,
        compare(
            &case.name,
            &json!({ "features": python }),
            &json!({ "features": rust }),
            &tolerances,
        ),
        &analysis["faces"],
    );
    out.extend(order(&case.name, python, &rust));
    Ok(out)
}

/// The differences of one case with each feature only Python reports paired, where it can be,
/// with one only Rust reports of the same family and an equal record (first come, first
/// paired): the same feature with other faces, so other ids. Such a pair is one difference,
/// at `features[id=<Python's id>].faces<how>`, where `<how>` says what the faces differ by:
/// `+plane` when Python's faces are Rust's and planes besides (a blind hole's floor, a
/// counterbore's seat: constituent faces, U2), `-cylinder` when Rust has cylinders Python has
/// not, `±` when each has faces the other lacks. Kinds are those of Python's faces.
fn paired(case: &str, differences: Vec<Difference>, faces: &Value) -> Vec<Difference> {
    let whole = |d: &Difference| {
        d.path
            .strip_prefix("features[id=")
            .and_then(|rest| rest.strip_suffix(']'))
            .filter(|id| !id.contains(']'))
            .map(str::to_string)
    };
    let family = |v: &Value| v["family"].as_str().unwrap_or_default().to_string();
    // The same feature: records equal to a micron (recognisers measure from faces both
    // engines read slightly differently); the record's own differences are then compared as
    // any matched feature's are, under Python's id.
    let same = Tolerances::default().with("*", 1e-6, 1e-3);
    let exact = Tolerances::default().with("*", 1e-9, 1e-9);
    let mut rust_only: Vec<(usize, Value)> = differences
        .iter()
        .enumerate()
        .filter(|(_, d)| d.python.is_none() && whole(d).is_some())
        .map(|(i, d)| (i, d.rust.clone().unwrap()))
        .collect();
    let mut dropped = vec![false; differences.len()];
    let mut out = Vec::new();
    for (i, d) in differences.iter().enumerate() {
        let (Some(id), Some(py), None) = (whole(d), &d.python, &d.rust) else {
            continue;
        };
        // quiddity #217 marks a groove's duplicate hole/boss with `superseded_by`, which the
        // frozen Python record lacks. The marker alone must not unpair the same feature; it is
        // still compared exactly below.
        let unmarked = |r: &Value| {
            let mut r = r["record"].clone();
            if py["record"].get("superseded_by").is_none()
                && let Some(r) = r.as_object_mut()
            {
                r.remove("superseded_by");
            }
            r
        };
        let Some(k) = rust_only.iter().position(|(_, r)| {
            family(r) == family(py) && compare(case, &py["record"], &unmarked(r), &same).is_empty()
        }) else {
            continue;
        };
        let (j, rust) = rust_only.remove(k);
        dropped[i] = true;
        dropped[j] = true;
        let set = |v: &Value| -> Vec<u64> {
            v["faces"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .collect()
        };
        let (p, r) = (set(py), set(&rust));
        let kinds = |ids: Vec<u64>| -> String {
            let mut k: Vec<&str> = ids
                .iter()
                .map(|&i| faces[i as usize]["kind"].as_str().unwrap_or("?"))
                .collect();
            k.sort_unstable();
            k.dedup();
            k.join("+")
        };
        let extra: Vec<u64> = p.iter().copied().filter(|f| !r.contains(f)).collect();
        let lacking: Vec<u64> = r.iter().copied().filter(|f| !p.contains(f)).collect();
        let how = match (extra.is_empty(), lacking.is_empty()) {
            (false, true) => format!("+{}", kinds(extra)),
            (true, false) => format!("-{}", kinds(lacking)),
            _ => "±".to_string(),
        };
        for mut d in compare(case, &py["record"], &rust["record"], &exact) {
            d.path = format!("features[id={id}].record.{}", d.path);
            out.push(d);
        }
        out.push(Difference {
            case: case.to_string(),
            path: format!("features[id={id}].faces{how}"),
            python: Some(py["faces"].clone()),
            rust: Some(json!({"id": rust["id"], "faces": rust["faces"]})),
        });
    }
    differences
        .into_iter()
        .zip(dropped)
        .filter(|(_, gone)| !gone)
        .map(|(d, _)| d)
        .chain(out)
        .collect()
}

/// With `SPECIFY_DUMP` set to a directory, the sweep's differences in full, as JSON (for
/// writing and checking known-difference entries; the test output truncates values).
pub fn dump(name: &str, differences: &[Difference]) {
    let Some(dir) = std::env::var_os("SPECIFY_DUMP") else {
        return;
    };
    let rows: Vec<Value> = differences
        .iter()
        .map(|d| json!({"case": d.case, "path": d.path, "python": d.python, "rust": d.rust}))
        .collect();
    let path = PathBuf::from(dir).join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&rows).unwrap()).unwrap();
}

/// A difference when the features both engines report are not in the same relative order.
fn order(case: &str, python: &Value, rust: &Value) -> Option<Difference> {
    let ids = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f["id"].as_str().map(str::to_string))
            .collect()
    };
    let (p, r) = (ids(python), ids(rust));
    let p: Vec<&String> = p.iter().filter(|id| r.contains(id)).collect();
    let r: Vec<&String> = r.iter().filter(|id| p.contains(id)).collect();
    (p != r).then(|| Difference {
        case: case.to_string(),
        path: "features(order)".into(),
        python: Some(json!(p)),
        rust: Some(json!(r)),
    })
}

/// `check_known(area)`, but without the corpus the entries for corpus cases, and the
/// cross-case (`*`) entries that then match nothing, are left out: their cases were skipped, so
/// they cannot be stale. With the corpus, every entry must match.
fn check_area(area: &str, differences: &[Difference]) {
    if corpus_dir().is_some() {
        return check_known(area, differences);
    }
    let path = common::known_differences_file(area);
    let entries: Vec<_> = common::load_known(&path)
        .into_iter()
        .filter(|e| !e.case.starts_with("corpus/"))
        .filter(|e| e.case != "*" || differences.iter().any(|d| e.matches(d)))
        .collect();
    println!("no corpus: corpus entries of {area} not checked");
    common::check_known_with(&path.display().to_string(), &entries, differences);
}
