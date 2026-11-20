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
    // The faces stay positional: one entry per face, the unmeasured one marked and without area
    // or centroid, every other with both.
    let faces = analysis["faces"].as_array().unwrap();
    assert_eq!(faces.len(), loaded.binding.face_count);
    for (position, f) in faces.iter().enumerate() {
        assert_eq!(f["id"], json!(position));
        if position == face {
            assert_eq!(f["unmeasured"], json!(true));
            assert!(
                f.get("area").is_none() && f.get("centroid").is_none(),
                "{f}"
            );
        } else {
            assert!(f.get("unmeasured").is_none());
            assert!(f["area"].is_number() && f["centroid"].is_array(), "{f}");
        }
    }
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

    // Anchors are made for groups of faces; none is keyed on the skipped one.
    let groups: Vec<Vec<usize>> = (0..faces.len()).map(|f| vec![f]).collect();
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
    // The write says why no dimensions were planned.
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("dimensions not planned") && w.contains(&face.to_string())),
        "{:?}",
        report.warnings
    );
    let review = api::review(&analysis, &answers).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "dimensions_not_planned")
    );

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
    assert_eq!(analysis["faces"][face]["unmeasured"], json!(true));
}

/// The closed fixture with the unmeasured wall moved to the front of the shell's face list, so
/// that every other face's index is one more than in the fixture. Returns the file and the
/// wall's new index (0).
fn wall_first(name: &str) -> std::path::PathBuf {
    let path = fixture("unmeasured_face_closed.step");
    let loaded = part::load(&path, Some(0)).unwrap();
    let wall = format!("#{}", loaded.face_entity(skipped(&loaded)));
    let text = std::fs::read_to_string(&path).unwrap();
    let shell = text.lines().find(|l| l.contains("CLOSED_SHELL")).unwrap();
    let list = shell
        .split_once("(#")
        .unwrap()
        .1
        .split_once("));")
        .unwrap()
        .0;
    let mut faces: Vec<String> = list
        .split(",#")
        .map(|f| format!("#{}", f.trim_start_matches('#')))
        .collect();
    let at = faces.iter().position(|f| *f == wall).unwrap();
    let moved = faces.remove(at);
    faces.insert(0, moved);
    let edited = shell.replace(list, &faces.join(",").replacen('#', "", 1));
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("unmeasured-face-first-{name}.step"));
    std::fs::write(&out, text.replace(shell, &edited)).unwrap();
    let first = part::load(&out, Some(0)).unwrap();
    assert_eq!(skipped(&first), 0, "the wall is the first face");
    out
}

#[test]
fn a_diff_reads_the_faces_after_the_unmeasured_one_by_their_own_index() {
    // The same part with its faces numbered differently (the wall last, then first): every
    // other face is carried, so nothing is reported changed or lost.
    let old = part::load(&wall_first("diff"), Some(0)).unwrap();
    let new = part::load(&fixture("unmeasured_face_closed.step"), Some(0)).unwrap();
    let revised = specify_core::revision::diff(&old, &new).unwrap();
    assert!(revised.other_faces.is_empty(), "{:?}", revised.other_faces);
    assert!(revised.unchanged_faces >= 6, "{}", revised.unchanged_faces);

    // And a change to a face after the wall in the old numbering is reported on that face, with
    // its own surface: the box is 2 mm longer, so the +x plane moved by 2.
    let text = std::fs::read_to_string(fixture("unmeasured_face_closed.step")).unwrap();
    assert!(text.contains("CARTESIAN_POINT('',(30.,"));
    let longer =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("unmeasured-face-longer.step");
    std::fs::write(
        &longer,
        text.replace("CARTESIAN_POINT('',(30.,", "CARTESIAN_POINT('',(32.,"),
    )
    .unwrap();
    let new = part::load(&longer, Some(0)).unwrap();
    let revised = specify_core::revision::diff(&old, &new).unwrap();
    let moved: Vec<_> = revised
        .other_faces
        .iter()
        .filter(|c| c.kind == "plane" && c.offset.is_some_and(|o| (o.abs() - 2.0).abs() < 1e-3))
        .collect();
    assert_eq!(moved.len(), 1, "{:?}", revised.other_faces);
    let described = api::analyse(&old).unwrap();
    let face = moved[0].old.unwrap();
    assert_eq!(described["faces"][face]["kind"], json!("plane"));
    assert_eq!(described["faces"][face]["centroid"][0], json!(30.0));
}

#[test]
fn location_axes_follow_the_reference_face_after_the_unmeasured_one() {
    // Written with the wall first and last, the same rows name the same measurement axes.
    let parameters = |path: &std::path::Path| {
        let loaded = part::load(path, Some(0)).unwrap();
        let analysis = api::analyse(&loaded).unwrap();
        let answers = specify_core::rules::with_all_defaults(
            &analysis,
            json!({"part.material": "steel"}).as_object().unwrap(),
        )
        .unwrap();
        let intent = api::apply(&analysis, &answers, true).unwrap();
        let (bytes, _) =
            specify_core::writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
        let written = part::load_bytes("origins.step", bytes).unwrap();
        let read = haecceity::pmi::read(
            &haecceity::p21::Document::parse(written.bytes.to_vec()).unwrap(),
            std::slice::from_ref(&written.definition),
        )
        .unwrap();
        let p = &read.parts[0];
        let mut out: Vec<String> = (0..p.dimensions.len())
            .filter_map(|i| {
                specify_core::pmi_origin::read(
                    p,
                    haecceity::pmi::NoteOwner::Dimension(haecceity::pmi::DimensionId(i)),
                )
                .source
                .map(|s| s.parameter)
            })
            .collect();
        out.sort();
        out
    };
    let last = parameters(&fixture("unmeasured_face_closed.step"));
    assert!(
        last.iter().any(|p| p.starts_with("location.length.")),
        "{last:?}"
    );
    let first = parameters(&wall_first("axes"));
    assert_eq!(first, last);
}

#[test]
fn a_reopened_file_resumes_its_answers_with_the_unmeasured_face_first() {
    // With the wall first every other face is one place later than in the fixture; the answers
    // still come back whole (resume's drift kinds are unit-tested in src/resume/truth.rs).
    let path = wall_first("resume");
    let loaded = part::load(&path, Some(0)).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = specify_core::rules::with_all_defaults(
        &analysis,
        json!({"part.material": "steel"}).as_object().unwrap(),
    )
    .unwrap();
    let intent = api::apply(&analysis, &answers, true).unwrap();
    let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let written = part::load_bytes("resume.step", bytes).unwrap();
    let again = api::analyse(&written).unwrap();
    assert_eq!(again["resume"]["answers"], Value::Object(answers));
    assert!(
        again["resume"].get("conflicts").is_none(),
        "{}",
        again["resume"]
    );
    assert!(
        again["resume"].get("unresolved").is_none(),
        "{}",
        again["resume"]
    );
    assert_eq!(again["unmeasured_faces"][0]["face"], json!(0));
}
