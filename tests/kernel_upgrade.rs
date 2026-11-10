//! Independent compatibility evidence for the v0.1.1 kernel upgrade. These are Rust-written
//! alpha.6 inputs, not outputs recaptured from Python or regenerated after the pin changed.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use specify_core::{
    api,
    features::{self, Feature},
    part, resume, rules, writer,
};
mod common;
use std::{collections::BTreeSet, path::Path};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn five_real_alpha6_files_resume_all_87_answers_including_a_rotated_part() {
    let mut total = 0;
    let mut changed_parts = 0;
    for name in ["plate", "plate-moved", "shaft", "corners", "pin"] {
        let base: Value = serde_json::from_slice(
            &std::fs::read(fixture(&format!("resume-alpha6/{name}.json"))).unwrap(),
        )
        .unwrap();
        let loaded = part::load(&fixture(&format!("resume-alpha6/{name}.step.gz")), None).unwrap();
        let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
        assert!(warnings.is_empty(), "{name}: {warnings:?}");
        assert_eq!(analysis["resume"]["answers"], base["answers"], "{name}");
        assert!(analysis["resume"]["unresolved"].is_null());
        let ids = |value: &Value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["id"].as_str().unwrap().to_string())
                .collect::<BTreeSet<_>>()
        };
        if ids(&base["features"]) != ids(&analysis["features"]) {
            changed_parts += 1;
        }
        let answers = analysis["resume"]["answers"].as_object().unwrap();
        total += answers.len();
        assert!(
            api::review(&analysis, answers).unwrap()["open"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{name}"
        );
        let intent = rules::apply(&analysis, answers).unwrap();
        let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(answers)).unwrap();
        let back = part::File::of_bytes(Path::new(name), bytes)
            .unwrap()
            .load(None)
            .unwrap();
        assert_eq!(resume::answers_for(&back).unwrap().as_ref(), Some(answers));
        // Stored base64 bytes and literal Unicode/quote/backslash text survive step-io 0.3.
        let before = resume::stored(&loaded.bytes);
        let after = resume::stored(&back.bytes);
        assert_eq!(
            resume::encode(&json!(before[&0])),
            resume::encode(&json!(after[&0]))
        );
        let settings = specify_core::existing::part_settings(&back).unwrap();
        assert_eq!(settings["notes"], base["answers"]["part.notes"]);
    }
    assert_eq!(total, 87);
    assert_eq!(
        changed_parts, 2,
        "shaft and pin feature IDs changed; saved answers must still resume"
    );
}

fn feature(id: &str, family: &str, faces: Vec<usize>) -> Feature {
    Feature {
        id: id.into(),
        family: family.into(),
        faces,
        record: Map::new(),
        members: vec![],
        notes: vec![],
    }
}

#[test]
fn rematching_requires_unique_same_family_support_and_reports_every_refusal() {
    let answers = json!({"hole.function:holes:6": "plain", "part.notes": ["do not edit holes:6"]});
    let current = vec![feature("holes:6.7", "holes", vec![6, 7])];
    let migrated = resume::rematch(answers.as_object().unwrap(), &current);
    assert_eq!(migrated.answers["hole.function:holes:6.7"], "plain");
    assert_eq!(
        migrated.migrated,
        [(
            "hole.function:holes:6".into(),
            "hole.function:holes:6.7".into()
        )]
    );
    assert_eq!(migrated.answers["part.notes"], answers["part.notes"]);
    for (features, reason) in [
        (vec![], "no same-family"),
        (
            vec![feature("bosses:6", "bosses", vec![6])],
            "no same-family",
        ),
        (
            vec![
                feature("holes:6.7", "holes", vec![6, 7]),
                feature("holes:6.8", "holes", vec![6, 8]),
            ],
            "ambiguous",
        ),
    ] {
        let result = resume::rematch(answers.as_object().unwrap(), &features);
        assert_eq!(result.unresolved.len(), 1);
        assert_eq!(result.unresolved[0].id, "hole.function:holes:6");
        assert!(result.unresolved[0].reason.contains(reason));
        assert!(
            !result
                .answers
                .keys()
                .any(|id| id.starts_with("hole.function:"))
        );
    }
    for id in [
        "hole.function:holes:",
        "hole.function:holes:not-a-face",
        "hole.function:missing-family-and-faces",
    ] {
        let malformed = serde_json::Map::from_iter([(id.to_string(), json!("plain"))]);
        let result = resume::rematch(&malformed, &current);
        assert_eq!(result.unresolved.len(), 1, "{id}");
        assert!(result.answers.is_empty(), "{id}");
    }
    let collision = json!({"hole.function:holes:6":"plain", "hole.function:holes:7":"fit:H7"});
    let result = resume::rematch(collision.as_object().unwrap(), &current);
    assert_eq!(result.unresolved.len(), 2);
    assert!(result.answers.is_empty());
    let cross_question_collision =
        json!({"hole.function:holes:6":"plain", "hole.position:holes:7":"0.05"});
    let result = resume::rematch(cross_question_collision.as_object().unwrap(), &current);
    assert_eq!(result.unresolved.len(), 2);
    assert!(
        result.answers.is_empty(),
        "different question types cannot hide merged-feature ambiguity"
    );
}

fn moved_bytes(bytes: Vec<u8>, motion: &quiddity::framed_records::Rigid) -> Vec<u8> {
    let doc = Document::parse(bytes).unwrap();
    let mut edit = Edit::new();
    for id in doc.ids() {
        let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
        else {
            continue;
        };
        if !matches!(name.as_str(), "CARTESIAN_POINT" | "DIRECTION") {
            continue;
        }
        let Some(Attribute::List(items)) = attributes.get(1) else {
            continue;
        };
        let values: Option<Vec<f64>> = items
            .iter()
            .map(|a| match a {
                Attribute::Real(v) => Some(*v),
                _ => None,
            })
            .collect();
        let Some(values) = values else { continue };
        let Ok(v) = <[f64; 3]>::try_from(values) else {
            continue;
        };
        let v = if name == "CARTESIAN_POINT" {
            motion.point(v)
        } else {
            motion.direction(v)
        };
        let mut attributes = attributes.clone();
        attributes[1] = Attribute::List(v.into_iter().map(Attribute::Real).collect());
        edit.replace(id, haecceity::p21::simple(name, attributes));
    }
    doc.apply(&edit).unwrap().bytes
}

fn close(a: &[f64; 3], b: &[f64; 3], tolerance: f64) {
    assert!(
        (0..3).all(|i| (a[i] - b[i]).abs() < tolerance),
        "{a:?} != {b:?}"
    );
}

#[test]
fn named_parts_keep_questions_defaults_locations_and_written_pmi_under_rigid_motion() {
    let (s, c) = 37_f64.to_radians().sin_cos();
    let (t, d) = 23_f64.to_radians().sin_cos();
    let motion = quiddity::framed_records::Rigid {
        rotation: [[d, t * s, t * c], [0., c, -s], [-t, d * s, d * c]],
        translation: [31., -17., 43.],
    };
    let mut placements_checked = 0;
    let mut inputs: Vec<_> = [
        "plate",
        "pin",
        "GRM-02_shaft",
        "four_corner_holes_are_quiddity_rectangle_with_members",
    ]
    .into_iter()
    .map(|name| (name.to_string(), fixture(&format!("parts/{name}.step"))))
    .collect();
    // Fractional slot/channel ends exercise the same published precision in both paths.
    for case in ["corpus/mfcadpp/10047", "corpus/mfcadpp/10224"] {
        if let Some(path) = common::case(case).input() {
            inputs.push((case.into(), path));
        }
    }
    for (name, path) in inputs {
        let name = name.as_str();
        let bytes = std::fs::read(path).unwrap();
        let a = part::File::of_bytes(Path::new(name), bytes.clone())
            .unwrap()
            .load(None)
            .unwrap();
        let b = part::File::of_bytes(Path::new(name), moved_bytes(bytes, &motion))
            .unwrap()
            .load(None)
            .unwrap();
        // Independent Part::transformed route proves source face indices are preserved.
        let transformed = a
            .part
            .transformed(&[
                [d, t * s, t * c, 31.],
                [0., c, -s, -17.],
                [-t, d * s, d * c, 43.],
            ])
            .unwrap();
        for i in 0..a.part.faces.len() {
            close(
                &b.part.face_moments(i).unwrap().centroid,
                &transformed.face_moments(i).unwrap().centroid,
                1e-6,
            );
        }
        let aa = api::analyse(&a).unwrap();
        let ab = api::analyse(&b).unwrap();
        let qa = rules::questions(&aa, &Map::new()).unwrap();
        let qb = rules::questions(&ab, &Map::new()).unwrap();
        let differences = common::compare(
            name,
            &json!(qa.iter().map(|q| q.to_dict()).collect::<Vec<_>>()),
            &json!(qb.iter().map(|q| q.to_dict()).collect::<Vec<_>>()),
            &common::Tolerances::default().with("*", 1e-5, 1e-8),
        );
        assert!(differences.is_empty(), "{name}: {differences:?}");
        let mut seed = json!({"part.material":"steel"})
            .as_object()
            .unwrap()
            .clone();
        if let Some((question, option)) = qa.iter().find_map(|q| {
            q.id.starts_with("diameter.fit:")
                .then(|| {
                    q.options
                        .iter()
                        .find(|option| option.starts_with("thread:"))
                        .map(|option| (&q.id, option))
                })
                .flatten()
        }) {
            seed.insert(question.clone(), json!(option));
        }
        let answers = rules::with_all_defaults(&aa, &seed).unwrap();
        let mut ia = rules::apply(&aa, &answers).unwrap();
        let mut ib = rules::apply(&ab, &answers).unwrap();
        ia.binding = json!(null);
        ib.binding = json!(null);
        // Planner quantities/supports are in the part frame; its world placement
        // must follow the known motion rather than compare byte-identically (#75).
        let mut expected_intent = ia.to_dict();
        for collection in ["requirements", "general"] {
            for row in expected_intent[collection].as_array_mut().unwrap() {
                if let Some(frame) = row.get_mut("planner").and_then(|p| p.get_mut("frame")) {
                    let point: [f64; 3] = serde_json::from_value(frame["origin"].clone()).unwrap();
                    frame["origin"] = json!(motion.point(point));
                    for axis in ["x", "y", "z"] {
                        let direction: [f64; 3] =
                            serde_json::from_value(frame[axis].clone()).unwrap();
                        frame[axis] = json!(motion.direction(direction));
                    }
                }
            }
        }
        let differences = common::compare(
            name,
            &expected_intent,
            &ib.to_dict(),
            &common::Tolerances::default().with("*", 1e-5, 1e-8),
        );
        assert!(differences.is_empty(), "{name}: {differences:?}");
        // Use each source's binding for the real PMI writer and its independent readback check.
        let mut written_pmi = Vec::new();
        let mut written_parts = Vec::new();
        let mut readbacks = Vec::new();
        let mut reports = Vec::new();
        for loaded in [&a, &b] {
            let analysis = api::analyse(loaded).unwrap();
            let intent = rules::apply(&analysis, &answers).unwrap();
            let (written, report) = writer::write_bytes(loaded, &intent, Some(&answers)).unwrap();
            reports.push((report.written, report.not_written));
            let back = part::File::of_bytes(Path::new(name), written)
                .unwrap()
                .load(None)
                .unwrap();
            let existing = specify_core::existing::read_existing(&back).unwrap();
            if name.starts_with("corpus/") {
                // Some generic dimensions on 10224 have no semantic writer mapping.
                // The explicit write/refusal report must also be invariant under motion.
                assert!(!existing.is_empty(), "{name}");
            } else {
                assert_eq!(
                    existing.len(),
                    intent
                        .requirements
                        .iter()
                        .filter(|r| !r.contains_key("not_written_reason"))
                        .count()
                        + intent.datums.len(),
                    "{name}"
                );
            }
            readbacks.push(
                serde_json::to_value(specify_core::existing::read_existing(&back).unwrap())
                    .unwrap(),
            );
            let doc = Document::parse(back.bytes.to_vec()).unwrap();
            written_pmi.push(
                haecceity::pmi::read(&doc, std::slice::from_ref(&back.definition))
                    .unwrap()
                    .parts
                    .remove(0),
            );
            written_parts.push(back);
        }
        assert_eq!(reports[0], reports[1], "{name}: write and refusal reports");
        let mut expected_readback = readbacks[0].clone();
        // #77 sources are keyed in each stored pose's schema-2 face namespace. A rigid
        // move changes that fingerprint while leaving the face supports/intent intact.
        for (readback, loaded) in readbacks.iter().zip(&written_parts) {
            let fingerprint = resume::fingerprint(loaded).unwrap();
            for row in readback.as_array().unwrap() {
                if let Some(source) = row.get("source") {
                    assert_eq!(source["faces"], fingerprint);
                    let target = if row["type"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("Location"))
                    {
                        &row["faces2"]
                    } else {
                        &row["faces"]
                    };
                    assert_eq!(&source["support"], target);
                }
            }
        }
        let moved_fingerprint = resume::fingerprint(&written_parts[1]).unwrap();
        for row in expected_readback.as_array_mut().unwrap() {
            if let Some(source) = row.get_mut("source") {
                source["faces"] = json!(moved_fingerprint);
            }
        }
        if name == "GRM-02_shaft" {
            // For #65: these are independent FIRST writes of two numberless poses.
            // #75 omits the unsupported blind-depth row. The exact Ø2.5 drill
            // now defaults to a thread rather than an H11 size; positions precede
            // the two internal threads. Stored-frame view/position order can change under rotation. Pin both
            // allocations explicitly; every other PMI field still compares below.
            let numbers: Vec<_> = readbacks[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["number"].clone())
                .collect();
            assert_eq!(json!(numbers), json!([9, 10, 1, 2, 11, 12]));
            // Rotated Z coordinates separate the end-on holes and side flat that
            // coincide in the original right view. Datums A/B remain first.
            for (row, number) in expected_readback
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .zip([9, 12, 1, 2, 10, 13])
            {
                row["number"] = json!(number);
            }
        }
        if name == "corpus/mfcadpp/10224" {
            // Independent first writes: the recovered #75 hole dimensions enter
            // different stored-frame view positions after the move. Pin just
            // these four changed allocations; all other fields/numbers compare.
            for (index, before, after) in [(3, 19, 20), (5, 20, 21), (8, 18, 19), (10, 21, 22)] {
                assert_eq!(expected_readback[index]["number"], before);
                expected_readback[index]["number"] = json!(after);
            }
        }
        let differences = common::compare(
            name,
            &expected_readback,
            &readbacks[1],
            &common::Tolerances::default().with("*", 1e-5, 1e-8),
        );
        assert!(
            differences.is_empty(),
            "{name}: written PMI {differences:?}"
        );
        let (base_pmi, moved_pmi) = (&written_pmi[0], &written_pmi[1]);
        assert_eq!(
            base_pmi.features, moved_pmi.features,
            "{name}: PMI source-face attachments"
        );
        assert_eq!(
            base_pmi.datums, moved_pmi.datums,
            "{name}: written datum associations"
        );
        // This writer places semantic PMI by source-face associations, not supplemental
        // geometry. Inspect the actual geometry those written anchors address in both files.
        for feature in &base_pmi.features {
            if let haecceity::pmi::Feature::Items(anchors) = feature {
                for anchor in anchors {
                    if let haecceity::pmi::Anchor::Face(face) = anchor {
                        placements_checked += 1;
                        let a = &written_parts[0].part;
                        let b = &written_parts[1].part;
                        close(
                            &motion.point(a.face_moments(face.0).unwrap().centroid),
                            &b.face_moments(face.0).unwrap().centroid,
                            1e-5,
                        );
                        close(
                            &motion.direction(a.face_normal(face.0, 0.0, 0.0).unwrap()),
                            &b.face_normal(face.0, 0.0, 0.0).unwrap(),
                            1e-6,
                        );
                    }
                }
            }
        }
        let fa = features::recognise(&a).unwrap();
        let fb = features::recognise(&b).unwrap();
        for hole in fa.iter().filter(|f| f.family == "holes") {
            let other = fb.iter().find(|f| f.id == hole.id).unwrap();
            let vector = |f: &Feature, k: &str| -> [f64; 3] {
                serde_json::from_value(f.record[k].clone()).unwrap()
            };
            close(
                &motion.point(vector(hole, "location")),
                &vector(other, "location"),
                1e-5,
            );
            close(
                &motion.direction(vector(hole, "axis")),
                &vector(other, "axis"),
                1e-6,
            );
        }
    }
    assert!(
        placements_checked > 0,
        "written PMI geometry must be tested, not just item counts"
    );
}

#[test]
fn unresolved_saved_answers_reach_review_and_leave_questions_open() {
    let loaded = part::load(&fixture("resume-alpha6/pin.step.gz"), None).unwrap();
    let mut payload = json!(resume::stored(&loaded.bytes)[&0]);
    let key = payload["answers"]
        .as_object()
        .unwrap()
        .keys()
        .find(|id| id.starts_with("hole.function:"))
        .unwrap()
        .clone();
    let value = payload["answers"]
        .as_object_mut()
        .unwrap()
        .remove(&key)
        .unwrap();
    payload["answers"]["hole.function:holes:999"] = value.clone();
    let rewrite = |payload: &Value| {
        let doc = Document::parse(loaded.bytes.as_ref().clone()).unwrap();
        let mut edit = Edit::new();
        for id in doc.ids() {
            if let Some(RawEntity::Simple {
                name, attributes, ..
            }) = doc.get(id)
                && name == "DESCRIPTIVE_REPRESENTATION_ITEM"
                && matches!(attributes.first(), Some(Attribute::String(s)) if s == resume::NAME)
            {
                let mut attributes = attributes.clone();
                attributes[1] = Attribute::String(resume::encode(payload));
                edit.replace(id, haecceity::p21::simple(name, attributes));
            }
        }
        doc.apply(&edit).unwrap().bytes
    };
    let bytes = rewrite(&payload);
    let part = part::File::of_bytes(Path::new("orphan.step"), bytes)
        .unwrap()
        .load(None)
        .unwrap();
    let (analysis, warnings) = api::analyse_reporting(&part).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("hole.function:holes:999"));
    assert_eq!(analysis["resume"]["unresolved"][0]["answer"], value);
    assert_eq!(analysis["resume"]["unresolved"][0]["feature"], "holes:999");
    // File-only consumers get applicable answers, while analysis keeps the
    // unsupported answer above for review instead of applying an unknown ID.
    assert!(
        !resume::answers_for(&part)
            .unwrap()
            .unwrap()
            .contains_key("hole.function:holes:999")
    );
    let review = api::review(
        &analysis,
        analysis["resume"]["answers"].as_object().unwrap(),
    )
    .unwrap();
    assert!(review["open"].as_array().unwrap().contains(&json!(key)));
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answer_unresolved"
                && p["message"].as_str().unwrap().contains("holes:999"))
    );
    // Edited geometry opens as a foreign file, retaining written PMI and part settings.
    // Migration is restricted to the original geometry, for both independent checks.
    let original = payload.clone();
    for field in ["faces", "face_count"] {
        payload = original.clone();
        payload[field] = if field == "faces" {
            json!("unproved")
        } else {
            json!(999)
        };
        let part = part::File::of_bytes(Path::new("changed.step"), rewrite(&payload))
            .unwrap()
            .load(None)
            .unwrap();
        let (analysis, warnings) = api::analyse_reporting(&part).unwrap();
        assert!(analysis.get("resume").is_none(), "{field}");
        assert!(warnings.is_empty(), "{field}: {warnings:?}");
        assert!(
            !analysis["existing"].as_array().unwrap().is_empty(),
            "{field}"
        );
        assert_eq!(analysis["part"]["material"], "steel", "{field}");
        assert_eq!(
            analysis["part"]["notes"], original["answers"]["part.notes"],
            "{field}"
        );
    }
}

#[test]
fn real_alpha6_ctc02_answers_exercise_changed_ids_and_explicit_refusals() {
    let base: Value =
        serde_json::from_slice(&std::fs::read(fixture("resume-alpha6/ctc02-reid.json")).unwrap())
            .unwrap();
    assert_eq!(base["answers"].as_object().unwrap().len(), 34);
    let loaded = part::load(&fixture("resume-alpha6/ctc02-reid.step.gz"), Some(0)).unwrap();
    let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
    let resumed = &analysis["resume"];

    assert_eq!(resumed["answers"].as_object().unwrap().len(), 32);
    // #100 adds the same holes' tip cones: 13 exact answer moves, two #82 refusals remain.
    common::upgrade::check(&loaded, &base, &analysis, common::upgrade::CTC02);
    assert_eq!(resumed["unresolved"].as_array().unwrap().len(), 2);
    assert_eq!(warnings.len(), 2);
    for unresolved in resumed["unresolved"].as_array().unwrap() {
        assert!(
            unresolved["id"]
                .as_str()
                .unwrap()
                .contains("hole_patterns.linear:462.463.466.467.502.503")
        );
        assert_eq!(
            unresolved["answer"],
            base["answers"][unresolved["id"].as_str().unwrap()]
        );
        assert!(
            unresolved["reason"]
                .as_str()
                .unwrap()
                .contains("no same-family")
        );
    }
}

#[test]
fn real_alpha6_ftc10_answers_migrate_to_larger_geometric_supports() {
    let base: Value =
        serde_json::from_slice(&std::fs::read(fixture("resume-alpha6/ftc10-reid.json")).unwrap())
            .unwrap();
    assert_eq!(base["answers"].as_object().unwrap().len(), 29);
    let loaded = part::load(&fixture("resume-alpha6/ftc10-reid.step.gz"), Some(0)).unwrap();
    let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
    let resumed = &analysis["resume"];
    assert!(warnings.is_empty());
    assert_eq!(resumed["answers"].as_object().unwrap().len(), 29);
    // #100 adds tip cones to two bores and one row: six new moves, plus the two
    // schema-6 counterbore-pattern moves. Exact IDs and cone adjacency are checked.
    common::upgrade::check(&loaded, &base, &analysis, common::upgrade::FTC10);
    assert!(resumed["unresolved"].is_null());
    let review = api::review(&analysis, resumed["answers"].as_object().unwrap()).unwrap();
    assert_eq!(
        review["open"],
        json!([
            "hole.function:hole_patterns.linear:194.195.196.197.198.199",
            "hole.position:hole_patterns.linear:194.195.196.197.198.199"
        ])
    );
    for pair in resumed["migrated"].as_array().unwrap() {
        assert!(!review["open"].as_array().unwrap().contains(&pair[1]));
    }
}
