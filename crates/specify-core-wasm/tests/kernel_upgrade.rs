//! The browser session exposes saved-answer recovery through its existing snapshot and review.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use specify_core::{part, resume};
use specify_core_wasm::session::Session;
use std::path::Path;

fn saved(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/resume-alpha6/{name}.step.gz"));
    part::load(&path, None).unwrap().bytes.to_vec()
}

/// Rename only the specified feature IDs, preserving every historical answer value.
fn rekey_answers(original: &Value, moves: &[(&str, &str)]) -> (Map<String, Value>, Vec<Value>) {
    let mut answers = Map::new();
    let mut migrated = Vec::new();
    for (id, value) in original.as_object().unwrap() {
        let new = id
            .split_once(':')
            .and_then(|(question, feature)| {
                moves
                    .iter()
                    .find(|(old, _)| *old == feature)
                    .map(|(_, new)| format!("{question}:{new}"))
            })
            .unwrap_or_else(|| id.clone());
        if new != *id {
            migrated.push(json!([id, new]));
        }
        answers.insert(new, value.clone());
    }
    (answers, migrated)
}

#[test]
fn browser_sessions_resume_all_87_alpha6_answers() {
    let mut count = 0;
    for name in ["plate", "plate-moved", "shaft", "corners", "pin"] {
        let bytes = saved(name);
        let original = &resume::stored(&bytes)[&0]["answers"];
        // #216 adds these planar floors to recognition IDs, not to the saved values.
        let moves: &[(&str, &str)] = match name {
            "shaft" => &[("holes:15", "holes:15.16")],
            "pin" => &[("holes:3", "holes:3.5")],
            _ => &[],
        };
        let (expected, migrated) = rekey_answers(original, moves);
        assert_eq!(migrated.len(), moves.len() * 2);
        let session = Session::open(name, &bytes).unwrap();
        let snapshot = session.snapshot();
        assert_eq!(snapshot["analysis"]["resume"]["answers"], json!(expected));
        assert_eq!(
            snapshot["analysis"]["resume"]
                .get("migrated")
                .cloned()
                .unwrap_or_else(|| json!([])),
            json!(migrated)
        );
        count += expected.len();
        // #21's optional depth override adds no review work to fully answered old files.
        let review = session.review(&expected).unwrap();
        assert!(review["open"].as_array().unwrap().is_empty());
    }
    assert_eq!(count, 87);
}

#[test]
fn browser_resumes_v024_answers_on_bosses_with_corrected_end_heights() {
    // quiddity-rust#134 changes the measurement, while the saved face evidence and
    // diameter fit still denote precisely the same physical cylinder.
    for (name, height) in [("chamfer", 37.6), ("round", 20.)] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/quiddity025");
        let base: Value =
            serde_json::from_slice(&std::fs::read(dir.join(format!("{name}.json"))).unwrap())
                .unwrap();
        let loaded = part::load(&dir.join(format!("{name}.step.gz")), None).unwrap();
        let session = Session::open(name, &loaded.bytes).unwrap();
        let snapshot = session.snapshot();
        let boss = snapshot["analysis"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == base["boss"]["id"])
            .unwrap();
        assert_eq!(boss["record"]["height"], height);
        // #216 adds floor 16 to the chamfered shaft's hole. Exactly three keys
        // migrate; the corrected boss heights and every answer value stay pinned.
        let moves: &[(&str, &str)] = if name == "chamfer" {
            &[("holes:15", "holes:15.16")]
        } else {
            &[]
        };
        let (answers, migrated) = rekey_answers(&base["answers"], moves);
        assert_eq!(migrated.len(), moves.len() * 3);
        assert_eq!(snapshot["analysis"]["resume"]["answers"], json!(answers));
        assert_eq!(
            snapshot["analysis"]["resume"]
                .get("migrated")
                .cloned()
                .unwrap_or_else(|| json!([])),
            json!(migrated)
        );
        assert!(snapshot["analysis"]["resume"]["unresolved"].is_null());
        assert!(
            session.review(&answers).unwrap()["open"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        for evidence in [Some(&base["evidence"]), None] {
            let out = session
                .rematch_answers(base["answers"].as_object().unwrap(), evidence, None)
                .unwrap();
            assert_eq!(out["answers"], json!(answers));
            assert_eq!(
                out.get("migrated").cloned().unwrap_or_else(|| json!([])),
                json!(migrated)
            );
            assert!(out["unresolved"].is_null());
            assert_eq!(out["unverified"].is_string(), evidence.is_none());
        }
    }
}

#[test]
fn browser_reports_unmatched_values_and_leaves_current_question_open() {
    let bytes = saved("pin");
    let mut payload = json!(resume::stored(&bytes)[&0]);
    let answers = payload["answers"].as_object_mut().unwrap();
    let key = answers
        .keys()
        .find(|key| key.starts_with("hole.function:"))
        .unwrap()
        .clone();
    let value = answers.shift_remove(&key).unwrap();
    answers.insert("hole.function:holes:999".into(), value.clone());
    let doc = Document::parse(bytes).unwrap();
    let mut edit = Edit::new();
    for id in doc.ids() {
        if let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
            && name == "DESCRIPTIVE_REPRESENTATION_ITEM"
            && matches!(attributes.first(), Some(Attribute::String(s)) if s == resume::NAME)
        {
            let mut attributes = attributes.clone();
            attributes[1] = Attribute::String(resume::encode(&payload));
            edit.replace(id, haecceity::p21::simple(name, attributes));
        }
    }
    let session = Session::open("orphan.step", &doc.apply(&edit).unwrap().bytes).unwrap();
    let snapshot = session.snapshot();
    let resumed = &snapshot["analysis"]["resume"];
    let mut applicable = payload["answers"].clone();
    applicable
        .as_object_mut()
        .unwrap()
        .shift_remove("hole.function:holes:999")
        .unwrap();
    let (expected, migrated) = rekey_answers(&applicable, &[("holes:3", "holes:3.5")]);
    assert_eq!(resumed["answers"], json!(expected));
    assert_eq!(
        migrated,
        vec![json!(["hole.position:holes:3", "hole.position:holes:3.5"])]
    );
    assert_eq!(resumed["migrated"], json!(migrated));
    assert_eq!(resumed["unresolved"].as_array().unwrap().len(), 1);
    assert_eq!(resumed["unresolved"][0]["answer"], value);
    assert_eq!(resumed["unresolved"][0]["id"], "hole.function:holes:999");
    let review: Value = session
        .review(resumed["answers"].as_object().unwrap())
        .unwrap();
    // The orphan retains its saved key/value; the current hole question owns floor 5.
    assert_eq!(key, "hole.function:holes:3");
    assert!(
        review["open"]
            .as_array()
            .unwrap()
            .contains(&json!("hole.function:holes:3.5"))
    );
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answer_unresolved")
    );
}

#[test]
fn browser_exposes_real_alpha6_pattern_id_migration_and_new_questions() {
    let bytes = saved("ftc10-reid");
    let original = &resume::stored(&bytes)[&0]["answers"];
    let session = Session::open("ftc10.step", &bytes).unwrap();
    let snapshot = session.snapshot();
    let resumed = &snapshot["analysis"]["resume"];
    assert_eq!(resumed["answers"].as_object().unwrap().len(), 29);
    // #100 claims drill-point cones: six bore/row answers now move in addition to
    // the two schema-6 counterbore-pattern answers. Native kernel_upgrade's exact
    // support/adjacency checks prove these are the same physical features.
    assert_eq!(resumed["migrated"].as_array().unwrap().len(), 8);
    let moves = [
        ("holes:175", "holes:174.175"),
        ("holes:177", "holes:176.177"),
        (
            "hole_patterns.linear:138.140.142.144.146.147.148.149",
            "hole_patterns.linear:130.132.134.136.138.140.142.144.146.147.148.149",
        ),
        (
            "hole_patterns.linear:189.191.193",
            "hole_patterns.linear:188.189.190.191.192.193",
        ),
    ];
    let expected: Vec<_> = original
        .as_object()
        .unwrap()
        .keys()
        .filter_map(|id| {
            let (question, old) = id.split_once(':')?;
            let (_, new) = moves.iter().find(|(from, _)| *from == old)?;
            Some(json!([id, format!("{question}:{new}")]))
        })
        .collect();
    assert_eq!(resumed["migrated"], json!(expected));
    assert!(resumed["unresolved"].is_null());
    for pair in resumed["migrated"].as_array().unwrap() {
        assert_ne!(pair[0], pair[1]);
        assert_eq!(
            resumed["answers"][pair[1].as_str().unwrap()],
            original[pair[0].as_str().unwrap()]
        );
    }
    let review = session
        .review(resumed["answers"].as_object().unwrap())
        .unwrap();
    assert_eq!(
        review["open"],
        json!([
            "hole.function:hole_patterns.linear:194.195.196.197.198.199",
            "hole.position:hole_patterns.linear:194.195.196.197.198.199"
        ])
    );
}

#[test]
fn browser_sessions_resume_all_84_alpha7_answers_after_schema6_upgrade() {
    let mut total = 0;
    for (name, count, moved) in [
        ("golden_oriented_slots", 13, 0),
        ("cgb217", 37, 5),
        ("ctc02_ap242", 34, 15),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/fixtures/resume_alpha7/{name}.step.gz"));
        let loaded = part::load(&path, None).unwrap();
        let expected = &resume::stored(&loaded.bytes)[&0]["answers"];
        let session = Session::open(name, &loaded.bytes).unwrap();
        let snapshot = session.snapshot();
        let resumed = &snapshot["analysis"]["resume"];
        // #100 adds terminal cones to cgb217/CTC-02's existing bores and derived
        // patterns. #91 restores the golden slot walls, but its saved answers are
        // all part/datum settings. Keep every value, explicitly checking changed IDs
        // against the saved physical supports rather than expecting unchanged keys.
        let metadata: Value = serde_json::from_slice(
            &std::fs::read(path.with_file_name(format!("{name}.json"))).unwrap(),
        )
        .unwrap();
        // #216 adds two moves to cgb217: the M8 clearance and position answers
        // on the three-hole set gain floors 105/107/109. The prior three rectangle
        // moves and all fifteen CTC-02 moves still claim only #100 tip cones.
        let moves: &[(&str, &str)] = match name {
            "golden_oriented_slots" => &[],
            "cgb217" => &[
                (
                    "hole_patterns.rectangle:10.13.16.19",
                    "hole_patterns.rectangle:10.11.13.14.16.17.19.20",
                ),
                (
                    "hole_patterns.set:106.108.110",
                    "hole_patterns.set:105.106.107.108.109.110",
                ),
            ],
            "ctc02_ap242" => &[
                (
                    "hole_patterns.bolt_circle:167.168.173.174.179.180",
                    "hole_patterns.bolt_circle:167.168.173.174.179.180.187.188.193.194.199.200",
                ),
                (
                    "hole_patterns.linear:317.318.319.320.321.322.323.324.347.348",
                    "hole_patterns.linear:208.220.221.222.223.260.261.262.263.275.317.318.319.320.321.322.323.324.347.348",
                ),
                (
                    "hole_patterns.linear:309.310.315.316.325.326.327.328",
                    "hole_patterns.linear:218.219.224.227.256.259.264.265.309.310.315.316.325.326.327.328",
                ),
                (
                    "hole_patterns.linear:597.598.599.600.601.602.603.604.605.606",
                    "hole_patterns.linear:109.110.111.112.113.435.436.437.438.439.597.598.599.600.601.602.603.604.605.606",
                ),
                (
                    "hole_patterns.linear:561.562.563.564.565.566",
                    "hole_patterns.linear:129.130.131.417.418.419.561.562.563.564.565.566",
                ),
                (
                    "hole_patterns.set:169.170.175.176.177.178.183.184",
                    "hole_patterns.set:169.170.175.176.177.178.183.184.189.190.195.196.197.198.203.204",
                ),
                (
                    "hole_patterns.set:305.306.307.308.311.312.313.314.339.340.341.342.343.344.345.346",
                    "hole_patterns.set:209.210.211.212.225.226.228.229.254.255.257.258.271.272.273.274.305.306.307.308.311.312.313.314.339.340.341.342.343.344.345.346",
                ),
                (
                    "hole_patterns.set:553.554.555.556.557.558.559.560.583.584.585.586.587.588.589.590",
                    "hole_patterns.set:117.118.119.120.132.133.134.135.413.414.415.416.428.429.430.431.553.554.555.556.557.558.559.560.583.584.585.586.587.588.589.590",
                ),
                (
                    "hole_patterns.set:620.621.632.633",
                    "hole_patterns.set:0.97.448.620.621.632.633.634",
                ),
            ],
            _ => unreachable!(),
        };
        let (expected_answers, expected_migrations) = rekey_answers(expected, moves);
        let migrations = resumed["migrated"].as_array().cloned().unwrap_or_default();
        assert_eq!(migrations.len(), moved, "{name}");
        assert_eq!(migrations, expected_migrations, "{name}");
        assert_eq!(resumed["answers"], json!(expected_answers), "{name}");
        if name == "cgb217" {
            assert_eq!(
                expected_answers["hole.function:hole_patterns.set:105.106.107.108.109.110"],
                "clearance:M8"
            );
            assert_eq!(
                expected_answers["hole.position:hole_patterns.set:105.106.107.108.109.110"],
                "1"
            );
        }
        assert_eq!(
            resumed["answers"].as_object().unwrap().len(),
            count,
            "{name}"
        );
        for (old, value) in expected.as_object().unwrap() {
            let new = migrations
                .iter()
                .find(|pair| pair[0] == *old)
                .and_then(|pair| pair[1].as_str())
                .unwrap_or(old);
            assert_eq!(&resumed["answers"][new], value, "{name}: {old} -> {new}");
        }
        for pair in migrations {
            let old = pair[0].as_str().unwrap().split_once(':').unwrap().1;
            let new = pair[1].as_str().unwrap().split_once(':').unwrap().1;
            let before = metadata["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["id"] == old)
                .unwrap();
            let after = snapshot["analysis"]["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["id"] == new)
                .unwrap();
            assert_eq!(before["family"], after["family"]);
            let old_faces = before["faces"].as_array().unwrap();
            let new_faces = after["faces"].as_array().unwrap();
            assert!(old_faces.iter().all(|f| new_faces.contains(f)));
            let extra: Vec<_> = new_faces
                .iter()
                .filter(|f| !old_faces.contains(f))
                .collect();
            assert!(!extra.is_empty());
            if name == "cgb217" && old == "hole_patterns.set:106.108.110" {
                // Independently prove the two additional answer moves are the
                // same three bores gaining their own adjacent planar floors.
                assert_eq!(before["faces"], json!([106, 108, 110]));
                assert_eq!(after["faces"], json!([105, 106, 107, 108, 109, 110]));
                assert_eq!(json!(extra), json!([105, 107, 109]));
                for (floor, bore) in [(105, 106), (107, 108), (109, 110)] {
                    assert!(matches!(
                        loaded.part.faces[floor].surface,
                        haecceity::geom::Surface::Plane { .. }
                    ));
                    assert!(matches!(
                        loaded.part.faces[bore].surface,
                        haecceity::geom::Surface::Cylinder { .. }
                    ));
                    assert!(loaded.part.neighbours(floor).contains(&bore));
                }
            } else {
                for face in extra {
                    let i = face.as_u64().unwrap() as usize;
                    assert!(matches!(
                        loaded.part.faces[i].surface,
                        haecceity::geom::Surface::Cone { .. }
                    ));
                    assert!(
                        loaded
                            .part
                            .neighbours(i)
                            .iter()
                            .any(|j| old_faces.contains(&json!(j)))
                    );
                }
            }
        }
        assert_eq!(expected.as_object().unwrap().len(), count);
        assert!(resumed["unresolved"].is_null(), "{name}: {resumed}");
        assert!(resumed["unverified"].is_null(), "{name}: {resumed}");
        let review = session
            .review(resumed["answers"].as_object().unwrap())
            .unwrap();
        assert!(
            !review["problems"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["code"] == "saved_answer_unresolved"
                    || p["code"] == "saved_answers_unverified")
        );
        total += count;
    }
    assert_eq!(total, 84);
}

#[test]
fn browser_refuses_suppressed_od_boss_answers_in_saved_and_shared_projects() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/quiddity023/turned_body_disc.step");
    let loaded = part::load(&path, None).unwrap();
    // #123: face 1 is still the Ø22 OD, co-owned by turned_bodies, but no boss.
    let key = "diameter.fit:bosses:1";
    let answers = json!({key:"g6", "part.material":"steel"})
        .as_object()
        .unwrap()
        .clone();
    let session = Session::open("disc.step", &loaded.bytes).unwrap();
    let evidence = session.evidence().unwrap();
    for e in [Some(&evidence), None] {
        let out = session.rematch_answers(&answers, e, None).unwrap();
        assert_eq!(out["unresolved"].as_array().unwrap().len(), 1);
        assert_eq!(out["unresolved"][0]["id"], key);
        assert_eq!(out["unresolved"][0]["answer"], "g6");
        assert!(out["answers"].get(key).is_none());
        assert!(out["migrated"].is_null());
    }
    let a = specify_core::api::analyse(&loaded).unwrap();
    let intent = specify_core::rules::apply_with_defaults(
        &a,
        json!({"part.material":"steel"}).as_object().unwrap(),
        true,
    )
    .unwrap();
    let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let session = Session::open("saved.step", &bytes).unwrap();
    let snapshot = session.snapshot();
    let out = &snapshot["analysis"]["resume"];
    assert_eq!(out["unresolved"][0]["id"], key);
    assert_eq!(out["unresolved"][0]["answer"], "g6");
    assert!(out["answers"].get(key).is_none());
    let review = session.review(out["answers"].as_object().unwrap()).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answer_unresolved")
    );
}
