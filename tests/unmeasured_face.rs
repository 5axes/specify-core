//! A face whose area and centroid cannot be computed (a cylinder bounded by one closed circle,
//! as some converters write a degenerate face) is left out and reported; the rest of the part
//! opens, is specified and written (pzfreo/quiddity#303). The synthetic fixtures are described
//! in `tests/fixtures/unmeasured_face.README.md`.

mod common;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::features::{self, UNMEASURED_REASON};
use specify_core::part::{self, LoadedPart};

fn fixture(name: &str) -> std::path::PathBuf {
    common::fixtures().join(name)
}

fn skipped(loaded: &LoadedPart) -> usize {
    let faces = features::unmeasured_faces(loaded);
    assert_eq!(faces.len(), 1, "the one degenerate wall: {faces:?}");
    faces[0]
}

fn uses(value: &Value, face: usize) -> bool {
    value["faces"]
        .as_array()
        .is_some_and(|f| f.iter().any(|x| x.as_u64() == Some(face as u64)))
}

#[test]
fn the_closed_part_is_analysed_without_the_unmeasured_face() {
    let path = fixture("unmeasured_face_closed.step");
    let loaded = part::load(&path, Some(0)).unwrap();
    let face = skipped(&loaded);
    // The face is the radius-5 blind-hole wall.
    assert!(matches!(
        loaded.part.faces[face].surface,
        haecceity::geom::Surface::Cylinder { radius, .. } if (radius - 5.0).abs() < 1e-9
    ));

    let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
    // The finding names the face and the reason.
    let findings = analysis["unmeasured_faces"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["face"], json!(face));
    assert_eq!(findings[0]["reason"], json!(UNMEASURED_REASON));
    assert!(
        findings[0]["detail"]
            .as_str()
            .unwrap()
            .contains(&format!("face {face}"))
    );
    assert!(warnings.iter().any(|w| w.contains(&format!("face {face}"))));

    // No face, feature or question refers to it; the other faces are all there.
    let faces = analysis["faces"].as_array().unwrap();
    assert_eq!(faces.len() + 1, loaded.binding.face_count);
    assert!(faces.iter().all(|f| f["id"] != json!(face)));
    let described = analysis["features"].as_array().unwrap();
    assert!(described.iter().all(|f| !uses(f, face)), "{described:?}");
    // The through hole (radius 4) is described, with its depth the box's 20, not 0.
    let hole = described
        .iter()
        .find(|f| f["family"] == "holes" && f["record"]["diameter"] == json!(8.0))
        .unwrap_or_else(|| panic!("the through hole: {described:?}"));
    assert_eq!(hole["record"]["depth"], json!(20.0), "{hole}");
    assert!(
        !described
            .iter()
            .any(|f| f["family"] == "holes" && f["record"]["depth"] == json!(0.0)),
        "{described:?}"
    );

    let answers = Map::new();
    let questions = api::questions(&analysis, &answers).unwrap();
    assert!(!questions.is_empty());
    assert!(questions.iter().all(|q| !q.faces.contains(&(face as i64))));
    let review = api::review(&analysis, &answers).unwrap();
    let problems = review["problems"].as_array().unwrap();
    let problem = problems
        .iter()
        .find(|p| p["code"] == "unmeasured_face")
        .unwrap_or_else(|| panic!("a problem for the face: {problems:?}"));
    assert_eq!(problem["faces"], json!([face]));

    // Anchors are made for groups of the analysis's faces; none is keyed on the skipped one.
    let groups: Vec<Vec<usize>> = faces
        .iter()
        .map(|f| vec![f["id"].as_u64().unwrap() as usize])
        .collect();
    let anchors = api::anchors(&loaded, &groups);
    assert!(!anchors.contains_key(&face.to_string()), "{anchors:?}");
}

#[test]
fn a_feature_on_the_unmeasured_face_is_never_described() {
    // Whatever quiddity reports for the face (before its fix, a hole of depth 0), nothing
    // that uses it is a feature.
    let loaded = part::load(&fixture("unmeasured_face_closed.step"), Some(0)).unwrap();
    let face = skipped(&loaded);
    let features = features::recognise(&loaded).unwrap();
    assert!(features.iter().all(|f| !f.faces.contains(&face)));
    assert!(!features.is_empty());
}

#[test]
fn the_closed_part_is_written_without_targeting_the_unmeasured_face() {
    let path = fixture("unmeasured_face_closed.step");
    let loaded = part::load(&path, Some(0)).unwrap();
    let face = skipped(&loaded);
    let analysis = api::analyse(&loaded).unwrap();
    let answers = json!({"part.material": "steel"})
        .as_object()
        .unwrap()
        .clone();
    let intent = api::apply(&analysis, &answers, true).unwrap();
    assert!(
        intent
            .requirements
            .iter()
            .all(|r| !uses(&Value::Object(r.clone()), face))
    );
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("unmeasured-face-write");
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join("out.step");
    let report = api::write(&loaded, &intent, &output, Some(&answers)).unwrap();
    assert!(!report.written.is_empty(), "{report:?}");

    // The written file's PMI names no face but the original's own uses of the wall.
    let before = std::fs::read_to_string(&path).unwrap();
    let after = std::fs::read_to_string(&output).unwrap();
    let entity = loaded.face_entity(face);
    let references = |text: &str| {
        let mut n = 0;
        let needle = format!("#{entity}");
        for (at, _) in text.match_indices(&needle) {
            let next = text
                .as_bytes()
                .get(at + needle.len())
                .copied()
                .unwrap_or(b' ');
            if !next.is_ascii_digit() {
                n += 1;
            }
        }
        n
    };
    assert!(
        references(&after) <= references(&before),
        "{entity}: PMI references the wall"
    );

    // Reopened, nothing the file says is about the wall, and the face is still reported.
    let reread = part::load(&output, Some(0)).unwrap();
    let again = api::analyse(&reread).unwrap();
    for existing in again["existing"].as_array().unwrap() {
        assert!(!uses(existing, face), "{existing}");
    }
    assert_eq!(again["unmeasured_faces"][0]["face"], json!(face));
}

#[test]
fn the_open_shell_is_analysed_with_the_wall_reported() {
    // An open shell recognises no feature. Before, the unmeasured wall refused the whole part;
    // now it is reported by name beside the part's other faces.
    let loaded = part::load(&fixture("unmeasured_face_open.step"), Some(0)).unwrap();
    let face = skipped(&loaded);
    let analysis = api::analyse(&loaded).unwrap();
    assert_eq!(analysis["unmeasured_faces"][0]["face"], json!(face));
    assert!(analysis["features"].as_array().unwrap().is_empty());
    assert!(
        analysis["faces"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["id"] != json!(face))
    );
}
