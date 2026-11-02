//! Shared text-to-cad PMI plate; pinned cadgen exports, never synthetic face names.
//! See fixtures/text-to-cad/pmi_plate/PROVENANCE.md and docs/face-names.md.
use haecceity::{p21::Document, pmi};
use serde_json::{Map, Value, json};
use specify_core::{api, characteristics, features, part, resume, revision, rules};
use std::path::Path;

const TOP: &str = "f:pmi_plate.body_make_box1.6;M:pmi_plate.body";
const BOTTOM: &str = "f:pmi_plate.body_make_box1.5;M:pmi_plate.body";
const BORE: &str = "f:pmi_plate.body_make_cylinder1.1;M:pmi_plate.body";
const NAMES: [&str; 7] = [
    "f:pmi_plate.body_make_box1.1", // left, -X
    "f:pmi_plate.body_make_box1.3", // front, -Y
    TOP,
    "f:pmi_plate.body_make_box1.4", // back, +Y
    BOTTOM,
    "f:pmi_plate.body_make_box1.2", // right, +X
    BORE,
];
const REV3_NAMES: [&str; 8] = [
    NAMES[0],
    NAMES[1],
    "f:pmi_plate.body~3969c36f3543",
    NAMES[3],
    "f:pmi_plate.body~620ac07d5aac",
    NAMES[5],
    "f:pmi_plate.body_cut1~7202e23a5382",
    "f:pmi_plate.body_make_cylinder2.1;M:pmi_plate.body",
];

fn fixture(revision: &str) -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/text-to-cad/pmi_plate")
            .join(format!("{revision}.step.gz")),
        None,
    )
    .unwrap()
}

fn specified(p: &part::LoadedPart, answers: &Map<String, Value>, name: &str) -> part::LoadedPart {
    // Retain the actual output for inspection; never write into the fixture directory.
    let output = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/text-to-cad/pmi_plate")
        .join(format!("{name}.specified.step"));
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    api::write(
        p,
        &api::apply(&api::analyse(p).unwrap(), answers, false).unwrap(),
        &output,
        Some(answers),
    )
    .unwrap();
    part::load(&output, None).unwrap()
}

fn named_faces<'a>(p: &'a part::LoadedPart, indices: &[usize]) -> Vec<&'a str> {
    indices
        .iter()
        .map(|&i| p.face_names[i].as_deref().unwrap())
        .collect()
}

/// Include the datum's supports when checking its dependent settings, too.
fn answer_supports<'a>(
    p: &'a part::LoadedPart,
    answers: &Map<String, Value>,
    id: &str,
) -> Vec<&'a str> {
    if let Some((_, feature)) = id.split_once(':') {
        let recognised = features::recognise(p).unwrap();
        let feature = recognised.iter().find(|f| f.id == feature).unwrap();
        named_faces(p, &feature.faces)
    } else if id.starts_with("datum.") {
        let datum = id.split('.').take(2).collect::<Vec<_>>().join(".");
        let faces: Vec<_> = answers[&datum]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_u64().unwrap() as usize)
            .collect();
        named_faces(p, &faces)
    } else {
        assert!(id.starts_with("part."), "unhandled answer support: {id}");
        Vec::new()
    }
}

fn pmi_supports<'a>(
    p: &'a part::LoadedPart,
    model: &pmi::PartPmi,
    feature: pmi::FeatureId,
) -> Vec<&'a str> {
    let pmi::Feature::Items(items) = &model.features[feature.0] else {
        panic!("the plate's PMI should reference face items")
    };
    let faces: Vec<_> = items
        .iter()
        .map(|anchor| match anchor {
            pmi::Anchor::Face(face) => face.0,
            other => panic!("expected face anchor, got {other:?}"),
        })
        .collect();
    named_faces(p, &faces)
}

fn assert_round_trip(
    original: &part::LoadedPart,
    written: &part::LoadedPart,
    answers: &Map<String, Value>,
) {
    assert_eq!(written.face_names, original.face_names);
    assert_eq!(written.definition.faces, original.definition.faces);
    assert_eq!(
        resume::fingerprint(written).unwrap(),
        resume::fingerprint(original).unwrap()
    );
    let before = Document::parse(original.bytes.to_vec()).unwrap();
    let after = Document::parse(written.bytes.to_vec()).unwrap();
    for &entity in &original.definition.faces {
        // The original ADVANCED_FACE records, including their authored names and
        // topology references, survive byte-for-byte on the same STEP entities.
        assert_eq!(
            &before.bytes()[before.span(entity).unwrap()],
            &after.bytes()[after.span(entity).unwrap()]
        );
    }
    let stored = resume::stored(&written.bytes).remove(&0).unwrap();
    assert_eq!(stored["schema"], 3);
    assert_eq!(stored["face_names"], json!(original.face_names));
    assert_eq!(stored["answers"], json!(answers));
    assert_eq!(
        resume::answers_for(written).unwrap().as_ref(),
        Some(answers)
    );

    // Read the actual AP242 PMI, independently of the stored-answer payload.
    // xcaf-pmi can follow these anchors back to the original named STEP faces.
    let read = pmi::read(&after, std::slice::from_ref(&written.definition)).unwrap();
    let model = &read.parts[0];
    assert_eq!(model.material.as_ref().unwrap().id, "aluminium");
    for (label, face) in [("A", TOP), ("B", NAMES[1]), ("C", NAMES[0])] {
        let datum = model
            .datums
            .iter()
            .find(|d| d.label().to_string() == label)
            .unwrap();
        assert_eq!(
            pmi_supports(written, model, datum.feature().unwrap()),
            [face]
        );
    }
    let diameter = model
        .dimensions
        .iter()
        .find_map(|dimension| match dimension.kind {
            pmi::DimensionKind::Size {
                feature,
                kind: pmi::SizeKind::Diameter,
                ..
            } => Some((dimension, feature)),
            _ => None,
        })
        .unwrap();
    assert_eq!(diameter.0.nominal.as_ref().unwrap().si(), 10.0);
    assert_eq!(pmi_supports(written, model, diameter.1), [BORE]);
    assert!(model.dimensions.iter().any(|dimension| {
        let pmi::DimensionKind::Location { from, to, .. } = dimension.kind else {
            return false;
        };
        pmi_supports(written, model, from) == [BOTTOM]
            && pmi_supports(written, model, to) == [TOP]
            && dimension.nominal.as_ref().unwrap().si() == 10.0
    }));
    let position = model
        .tolerances
        .iter()
        .find(|t| t.kind == pmi::ToleranceKind::Position)
        .unwrap();
    let pmi::ToleranceTarget::Feature(feature) = position.target else {
        panic!("position should reference the bore")
    };
    assert_eq!(pmi_supports(written, model, feature), [BORE]);
}

#[test]
fn cadgen_exports_have_real_names_geometry_and_no_source_pmi() {
    for (revision, thickness, radius, expected) in [
        ("rev1", 10.0, 5.0, NAMES.as_slice()),
        ("rev2", 12.0, 6.0, NAMES.as_slice()),
        ("rev3", 10.0, 5.0, REV3_NAMES.as_slice()),
    ] {
        let p = fixture(revision);
        assert_eq!(p.binding.face_count, expected.len());
        assert_eq!(
            p.face_names
                .iter()
                .map(|n| n.as_deref().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        let doc = Document::parse(p.bytes.to_vec()).unwrap();
        assert_eq!(
            haecceity::json::Anchors::read(&doc, &p.definition).names,
            p.face_names
        );
        let analysis = api::analyse(&p).unwrap();
        for (i, name) in expected.iter().enumerate() {
            assert_eq!(analysis["faces"][i]["name"], *name, "{revision}: face {i}");
        }
        let evidence = resume::evidence(&p).unwrap();
        assert_eq!(evidence["schema"], 3);
        assert_eq!(evidence["face_names"], json!(expected));
        assert!(
            evidence.get("geometry").is_none(),
            "all supports have names"
        );
        assert!(resume::stored(&p.bytes).is_empty());
        let read = pmi::read(&doc, std::slice::from_ref(&p.definition)).unwrap();
        let model = &read.parts[0];
        assert!(
            model.datums.is_empty() && model.dimensions.is_empty() && model.tolerances.is_empty()
        );
        assert!(model.material.is_none() && model.notes.is_empty());

        let faces = features::describe_faces(&p).unwrap();
        for (i, direction) in [
            (0, [-1., 0., 0.]),
            (1, [0., -1., 0.]),
            (2, [0., 0., 1.]),
            (3, [0., 1., 0.]),
            (4, [0., 0., -1.]),
            (5, [1., 0., 0.]),
        ] {
            assert_eq!(faces[i].kind, "plane");
            assert_eq!(faces[i].direction, Some(direction));
        }
        assert_eq!(faces[0].centroid[0], -50.0);
        assert_eq!(faces[5].centroid[0], 50.0);
        assert_eq!(faces[1].centroid[1], -30.0);
        assert_eq!(faces[3].centroid[1], 30.0);
        assert_eq!(faces[2].centroid[2], thickness / 2.0);
        assert_eq!(faces[4].centroid[2], -thickness / 2.0);
        assert_eq!(faces[6].kind, "cylinder");
        assert_eq!(faces[6].radius, Some(radius));
        assert_eq!(faces[6].origin, Some([0., 0., -thickness / 2.0]));
        let added_area = if revision == "rev3" {
            assert_eq!(faces[7].kind, "cylinder");
            assert_eq!(faces[7].radius, Some(3.0));
            assert_eq!(faces[7].origin, Some([30., 15., -5.]));
            std::f64::consts::PI * 9.0
        } else {
            0.0
        };
        let area = 6000.0 - std::f64::consts::PI * radius * radius - added_area;
        for i in [2, 4] {
            assert!((faces[i].area - area).abs() < 1e-6);
        }
    }
    let changed: Vec<_> = NAMES
        .iter()
        .zip(REV3_NAMES)
        .enumerate()
        .filter_map(|(i, (&old, new))| (old != new).then_some((i, old, new)))
        .collect();
    assert_eq!(
        changed,
        [
            (2, TOP, REV3_NAMES[2]),
            (4, BOTTOM, REV3_NAMES[4]),
            (6, BORE, REV3_NAMES[6])
        ]
    );
}

#[test]
fn specified_answers_resume_per_name_and_refuse_cadgens_added_hole_renames() {
    let rev1 = fixture("rev1");
    let rev2 = fixture("rev2");
    let rev3 = fixture("rev3");
    let answers = rules::with_all_defaults(
        &api::analyse(&rev1).unwrap(),
        json!({"part.material": "aluminium"}).as_object().unwrap(),
    )
    .unwrap();
    assert_eq!(answers.len(), 16);
    assert_eq!(answers["datum.A"], json!([2])); // defaults choose the top, not the source PMI's bottom
    assert_eq!(answers["datum.B"], json!([1]));
    assert_eq!(answers["datum.C"], json!([0]));
    let written1 = specified(&rev1, &answers, "rev1");
    assert_round_trip(&rev1, &written1, &answers);
    let saved = Value::Object(resume::stored(&written1.bytes).remove(&0).unwrap());
    let resumed2 = resume::rematch_answers(
        &rev2,
        &features::recognise(&rev2).unwrap(),
        &answers,
        Some(&saved),
        None,
    )
    .unwrap();
    assert!(
        resumed2.unresolved.is_empty() && resumed2.unverified.is_none(),
        "{resumed2:?}"
    );
    assert!(resumed2.migrated.is_empty());
    assert_eq!(resumed2.answers.len(), answers.len());
    let diff2 = revision::diff(&written1, &rev2, &Map::new()).unwrap();
    assert!(diff2.unresolved.is_empty(), "{:?}", diff2.unresolved);
    for (id, value) in &answers {
        assert_eq!(resumed2.answers.get(id), Some(value), "rev2 resume: {id}");
        assert_eq!(diff2.answers.get(id), Some(value), "rev2 stored diff: {id}");
        assert_eq!(
            answer_supports(&rev1, &answers, id),
            answer_supports(&rev2, &resumed2.answers, id),
            "rev2 support: {id}"
        );
    }
    let written2 = specified(&rev2, &resumed2.answers, "rev2");
    // Exercise both the original saved answers and the sequential rev2 → rev3 path.
    let saved2 = Value::Object(resume::stored(&written2.bytes).remove(&0).unwrap());
    let expected_unresolved = [
        (
            "hole.function:holes:6",
            format!("face name {BORE:?} not in this revision"),
        ),
        (
            "hole.position:holes:6",
            format!("face name {BORE:?} not in this revision"),
        ),
        ("datum.A", format!("face name {TOP:?} not in this revision")),
        ("datum.A.flatness", "its datum datum.A is unresolved".into()),
    ];
    for (old, evidence) in [(&written1, &saved), (&written2, &saved2)] {
        let resumed = resume::rematch_answers(
            &rev3,
            &features::recognise(&rev3).unwrap(),
            &answers,
            Some(evidence),
            None,
        )
        .unwrap();
        let diff = revision::diff(old, &rev3, &Map::new()).unwrap();
        // Flip this to all-attached once cadgen keeps names stable (text-to-cad
        // PR #692). The exact expected refusal list must then change as well.
        assert_eq!(resumed.answers.len(), 12);
        assert_eq!(resumed.unresolved.len(), expected_unresolved.len());
        assert_eq!(diff.unresolved.len(), expected_unresolved.len());
        assert!(resumed.unverified.is_none() && resumed.migrated.is_empty());
        for (id, reason) in &expected_unresolved {
            let unresolved = resumed.unresolved.iter().find(|u| u.id == *id).unwrap();
            assert_eq!(&unresolved.reason, reason, "rev3 resume: {id}");
            assert_eq!(unresolved.answer, answers[*id]);
            let unresolved = diff.unresolved.iter().find(|u| u.id == *id).unwrap();
            assert_eq!(&unresolved.reason, reason, "rev3 diff: {id}");
            assert_eq!(unresolved.answer, answers[*id]);
            assert_eq!(unresolved.class, "unresolved");
            assert!(unresolved.candidates.is_empty());
            assert!(!resumed.answers.contains_key(*id) && !diff.answers.contains_key(*id));
        }
        for (id, value) in &answers {
            if expected_unresolved
                .iter()
                .any(|(missing, _)| *missing == id)
            {
                continue;
            }
            assert_eq!(resumed.answers.get(id), Some(value), "rev3 resume: {id}");
            assert_eq!(diff.answers.get(id), Some(value), "rev3 stored diff: {id}");
            assert_eq!(
                answer_supports(old, &answers, id),
                answer_supports(&rev3, &resumed.answers, id),
                "rev3 support: {id}"
            );
        }
        assert!(resumed.answers.keys().all(|id| answers.contains_key(id)));
        assert!(
            diff.answers
                .keys()
                .all(|id| answers.contains_key(id) || id == characteristics::CARRY)
        );
        // Both bores are still recognisable, and the old hole even has the same
        // face index; neither that fact nor geometry may bypass the missing name.
        assert_eq!(
            features::recognise(&rev3)
                .unwrap()
                .iter()
                .filter(|f| f.family == "holes")
                .count(),
            2
        );
        assert!(!resumed.answers.keys().any(|id| id.starts_with("hole.")));
    }
    eprintln!(
        "shared plate: rev2 16 attached/0 unresolved; rev3 12 attached/4 unresolved; rev1 STEP retained at {}",
        written1.path.display()
    );
}

fn gist_plate() -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/text-to-cad/pmi_plate_gist/pmi_plate.step.gz"),
        None,
    )
    .unwrap()
}

/// cadgen's own semantic PMI on the gist plate reads as the file's, with nothing doubled.
#[test]
fn cadgen_semantic_pmi_reads_once_with_its_basics() {
    let p = gist_plate();
    let a = api::analyse(&p).unwrap();
    assert_eq!(a["part"]["general_tolerance"], "ISO 2768-mK");
    assert_eq!(a["part"]["material"], "Alu_G6061_T6");
    let existing = a["existing"].as_array().unwrap();
    let of = |kind: &'static str| existing.iter().filter(move |e| e["kind"] == kind);
    let datums: Vec<_> = of("datum")
        .map(|d| (d["type"].as_str().unwrap(), d["faces"].clone()))
        .collect();
    assert_eq!(
        datums,
        [("A", json!([4])), ("B", json!([1])), ("C", json!([0]))]
    );
    let dims: Vec<_> = of("dimension").collect();
    assert_eq!(dims.len(), 4);
    let thickness = dims.iter().find(|d| d["faces"] == json!([4])).unwrap();
    assert_eq!(
        (
            &thickness["value"],
            &thickness["upper"],
            &thickness["lower"]
        ),
        (&json!(10.0), &json!(0.2), &json!(-0.2))
    );
    let bore = dims.iter().find(|d| d["type"] == "Size_Diameter").unwrap();
    assert_eq!(
        (&bore["faces"], &bore["upper"], &bore["lower"]),
        (&json!([6]), &json!(0.1), &json!(0.0))
    );
    // The hole is located by basic 30 from B and basic 50 from C (#114).
    for (from, value) in [(1, 30.0), (0, 50.0)] {
        let d = dims
            .iter()
            .find(|d| d["faces"] == json!([from]) && d["faces2"] == json!([6]))
            .unwrap();
        assert_eq!(d["value"], json!(value));
        assert_eq!(d["basic"], json!(true), "basic {value}");
        assert!(d.get("upper").is_none() && d.get("lower").is_none());
    }
    assert!(
        dims.iter()
            .filter(|d| d["value"] != json!(30.0) && d["value"] != json!(50.0))
            .all(|d| d.get("basic").is_none())
    );
    let tolerances: Vec<_> = of("geometric_tolerance")
        .map(|t| (t["type"].as_str().unwrap(), t["value"].as_f64().unwrap()))
        .collect();
    assert_eq!(
        tolerances,
        [
            ("Flatness", 0.05),
            ("Position", 0.1),
            ("ProfileOfSurface", 0.1),
            ("ProfileOfSurface", 0.1),
            ("ProfileOfSurface", 0.5)
        ]
    );

    // The general tolerance is stated once, as the file's.
    let scene = specify_core::pmi_scene::pmi_scene(&p.bytes, None).unwrap();
    let meta = scene["specify"].as_object().unwrap();
    let general: Vec<_> = meta
        .values()
        .filter(|m| m["notation"].as_str().is_some_and(|n| n.contains("2768")))
        .collect();
    assert_eq!(general.len(), 1);
    assert_eq!(general[0]["notation"], "GENERAL TOLERANCES ISO 2768-mK");
    assert_eq!(
        meta.values().filter(|m| m["basic"] == json!(true)).count(),
        2
    );

    // No Specify size or location requirement repeats the file's dimensions (#113).
    let intent = api::apply(&a, &Map::new(), true).unwrap();
    for r in &intent.requirements {
        let kind = r.get("kind").and_then(Value::as_str);
        assert!(
            !matches!(kind, Some("size" | "location")) || r.contains_key("not_written_reason"),
            "an extra requirement beside the file's own dimensions: {r:?}"
        );
    }
}
