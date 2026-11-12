//! Exercise the native implementation of IntentEngine, including verified PMI readback.
use serde_json::{Map, Value, json};
use specify_core_wasm::session::Session;

#[test]
fn alpha7_answered_tap_keeps_computed_depth_in_write_pmi_and_resave() {
    use haecceity::{p21::Document, pmi, step::read_part_definitions};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let baseline =
        specify_core::part::load(&root.join("thread-depth-alpha7.step.gz"), None).unwrap();
    let saved = Session::open("shaft.step", &baseline.bytes).unwrap();
    let given = saved.snapshot()["analysis"]["resume"]["answers"]
        .as_object()
        .unwrap()
        .clone();
    for bytes in [
        std::fs::read(root.join("parts/GRM-02_shaft.step")).unwrap(),
        baseline.bytes.to_vec(),
    ] {
        let session = Session::open("shaft.step", &bytes).unwrap();
        let review = session.review(&given).unwrap();
        let q = review["questions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["id"].as_str().unwrap().starts_with("hole.full_thread:"))
            .unwrap();
        assert_eq!(q["attention"], "refine");
        assert!(!review["open"].as_array().unwrap().contains(&q["id"]));
        assert!(!given.contains_key(q["id"].as_str().unwrap()));
        assert_eq!(
            review["intent"]["requirements"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["kind"] == "thread")
                .unwrap()["full_thread"],
            2.0
        );
        let out = session.write_pmi(&given, None, None).unwrap();
        let bytes: Vec<u8> = serde_json::from_value(out["bytes"].clone()).unwrap();
        let defs = read_part_definitions(&bytes).unwrap();
        let doc = Document::parse(bytes).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        assert_eq!(read.parts[0].threads.len(), 1);
        assert!(read.parts[0].attributes.iter().flat_map(|a| &a.items).any(|(n,v)| n == "minimum full thread" && matches!(v, pmi::AttributeValue::Measure(m) if matches!(&m.quantity, pmi::Quantity::Length(l) if l.mm() == 2.0))));
    }
}

#[test]
fn full_thread_edit_is_optional_and_survives_a_session_write() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/GRM-02_shaft.step");
    let session = Session::open("shaft.step", &std::fs::read(path).unwrap()).unwrap();
    let a = session.snapshot()["analysis"].clone();
    let review = session.review(&Map::new()).unwrap();
    let q = review["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"].as_str().unwrap().starts_with("hole.full_thread:"))
        .unwrap();
    let id = q["id"].as_str().unwrap();
    assert_eq!(q["kind"], "text");
    assert_eq!(q["default"], "2");
    assert_eq!(q["attention"], "refine");
    assert!(!review["open"].as_array().unwrap().contains(&json!(id)));
    let function = id.replace("hole.full_thread:", "hole.function:");
    let given = json!({"part.material":"steel", function.as_str():"tapped:M2", id:"1.8"})
        .as_object()
        .unwrap()
        .clone();
    let accepted = session.accept_defaults(&given).unwrap();
    assert_eq!(accepted[id], "1.8");
    assert_eq!(
        accepted,
        specify_core_wasm::accept_defaults(&a, &given).unwrap()
    );
    let review = session.review(&given).unwrap();
    assert_eq!(
        review["intent"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap()["full_thread"],
        1.8
    );
    let written = session.write_pmi(&given, None, None).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    let reopened = Session::open("edited.step", &bytes).unwrap();
    assert_eq!(
        reopened.snapshot()["analysis"]["resume"]["answers"][id],
        "1.8"
    );
    let bad = json!({function:"tapped:M2", id:"3"})
        .as_object()
        .unwrap()
        .clone();
    assert!(session.review(&bad).is_err());
    assert!(session.accept_defaults(&bad).is_err());
}

#[test]
fn answering_material_does_not_write_unanswered_defaults() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/GRM-02_shaft.step");
    let session = Session::open("shaft.step", &std::fs::read(path).unwrap()).unwrap();
    for answers in [
        Map::new(),
        json!({"part.material": "steel"})
            .as_object()
            .unwrap()
            .clone(),
    ] {
        let review = session.review(&answers).unwrap();
        assert!(review["intent"]["datums"].as_array().unwrap().is_empty());
        assert!(
            review["intent"]["requirements"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(review["intent"]["part"].get("general_tolerance").is_none());
        assert!(
            review["open"]
                .as_array()
                .unwrap()
                .contains(&json!("part.general_tolerance"))
        );

        let state = specify_core::tolerancing::State {
            answers: answers.clone(),
            auto: Some(vec![]),
            gdt: None,
        };
        let settled = session.settle(&state).unwrap();
        assert!(
            settled["review"]["intent"]["datums"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            settled["review"]["intent"]["part"]
                .get("general_tolerance")
                .is_none()
        );

        let written = session.write_pmi(&answers, None, None).unwrap();
        let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
        let reopened = Session::open("shaft-written.step", &bytes)
            .unwrap()
            .snapshot();
        assert_eq!(reopened["analysis"]["resume"]["answers"], json!(answers));
        // A resumed analysis hides its PMI in favour of stored answers; inspect the actual
        // semantic records independently to prove which defaults were (not) written.
        let loaded = specify_core::part::load_bytes("shaft-written.step", bytes).unwrap();
        let existing = specify_core::existing::read_existing(&loaded).unwrap();
        assert!(
            existing
                .iter()
                .all(|p| p.kind != "datum" && p.kind != "geometric_tolerance")
        );
        let settings = specify_core::existing::part_settings(&loaded).unwrap();
        assert!(settings.get("general_tolerance").is_none());
        assert_eq!(settings.get("material"), answers.get("part.material"));
    }
}

#[test]
fn accepting_one_default_writes_only_that_default() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", &std::fs::read(path).unwrap()).unwrap();
    let initial = session.review(&Map::new()).unwrap();
    let datum = initial["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "datum.A")
        .unwrap();
    let given = json!({"datum.A": datum["default"]})
        .as_object()
        .unwrap()
        .clone();
    let review = session.review(&given).unwrap();
    assert_eq!(review["intent"]["datums"].as_array().unwrap().len(), 1);
    assert!(
        review["intent"]["requirements"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        review["open"]
            .as_array()
            .unwrap()
            .contains(&json!("datum.B"))
    );
    let written = session.write_pmi(&given, None, None).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    let loaded = specify_core::part::load_bytes("plate-written.step", bytes).unwrap();
    let existing = specify_core::existing::read_existing(&loaded).unwrap();
    let datums: Vec<_> = existing.iter().filter(|p| p.kind == "datum").collect();
    assert_eq!(datums.len(), 1);
    assert_eq!(datums[0].r#type, "A");
}

#[test]
fn session_reports_an_answered_control_until_its_frame_is_accepted() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", &std::fs::read(path).unwrap()).unwrap();
    let initial = session.review(&Map::new()).unwrap();
    let questions = initial["questions"].as_array().unwrap();
    let position = questions
        .iter()
        .find(|q| {
            q["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("hole.position:"))
        })
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let mut given = json!({position: "0.2"}).as_object().unwrap().clone();
    let review = session.review(&given).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "awaits_datum_frame" && p["about"] == position)
    );
    let written = session.write_pmi(&given, None, None).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    let loaded = specify_core::part::load_bytes("pending.step", bytes).unwrap();
    assert!(
        specify_core::existing::read_existing(&loaded)
            .unwrap()
            .iter()
            .all(|p| p.kind != "geometric_tolerance")
    );
    for q in questions {
        if q["id"]
            .as_str()
            .is_some_and(|id| ["datum.A", "datum.B", "datum.C"].contains(&id))
        {
            given.insert(q["id"].as_str().unwrap().into(), q["default"].clone());
        }
    }
    let review = session.review(&given).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["code"] != "awaits_datum_frame")
    );
    let written = session.write_pmi(&given, None, None).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    let loaded = specify_core::part::load_bytes("accepted.step", bytes).unwrap();
    assert!(
        specify_core::existing::read_existing(&loaded)
            .unwrap()
            .iter()
            .any(|p| p.kind == "geometric_tolerance" && p.r#type == "Position")
    );
}

fn check_bulk_acceptance(path: &std::path::Path, given: Map<String, Value>) {
    let bytes = std::fs::read(path).unwrap();
    let session = Session::open("accepted.step", &bytes).unwrap();
    let analysis = session.snapshot()["analysis"].clone();
    let accepted = session.accept_defaults(&given).unwrap();
    assert_eq!(
        accepted,
        specify_core_wasm::accept_defaults(&analysis, &given).unwrap()
    );
    assert_eq!(accepted, session.accept_defaults(&accepted).unwrap());
    for (id, value) in &given {
        assert_eq!(accepted.get(id), Some(value));
    }
    let review = session.review(&accepted).unwrap();
    assert!(review["open"].as_array().unwrap().is_empty());
    assert_eq!(review["intent"]["part"]["general_tolerance"], "ISO 2768-m");
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["code"] != "awaits_datum_frame")
    );
    let state = specify_core::tolerancing::State {
        answers: accepted.clone(),
        auto: Some(vec![]),
        gdt: None,
    };
    let settled = session.settle(&state).unwrap();
    assert_eq!(settled["state"]["answers"], json!(accepted));
    assert_eq!(settled["review"]["intent"], review["intent"]);

    let loaded = specify_core::part::load_bytes("accepted.step", bytes).unwrap();
    let expected = specify_core::api::apply(&analysis, &given, true).unwrap();
    let mut actual = review["intent"].clone();
    actual.as_object_mut().unwrap().remove("anchors");
    for key in ["datums", "requirements"] {
        for item in actual[key].as_array_mut().unwrap() {
            if item.get("not_written_reason").is_some() {
                // Rejected planner proposals have no interview/default status.
                assert!(item.get("question").is_none());
                assert!(item.get("default").is_none());
            } else {
                assert_eq!(
                    item.as_object_mut().unwrap().remove("default"),
                    Some(json!(false))
                );
            }
        }
    }
    assert_eq!(actual, expected.to_dict());
    let written = session.write_pmi(&accepted, None, None).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    let (expected_bytes, _) =
        specify_core::writer::write_bytes(&loaded, &expected, Some(&accepted)).unwrap();
    assert_eq!(bytes, expected_bytes);
    let reopened = Session::open("accepted.step", &bytes).unwrap();
    assert_eq!(
        reopened.snapshot()["analysis"]["resume"]["answers"],
        json!(accepted)
    );
    let semantic = specify_core::part::load_bytes("accepted.step", bytes).unwrap();
    let pmi = specify_core::existing::read_existing(&semantic).unwrap();
    assert_eq!(
        pmi.iter().filter(|p| p.kind == "datum").count(),
        expected.datums.len()
    );
    assert!(
        pmi.iter()
            .any(|p| p.kind == "geometric_tolerance" && p.r#type == "Position")
    );
    assert_eq!(
        specify_core::existing::part_settings(&semantic).unwrap()["general_tolerance"],
        "ISO 2768-m"
    );

    // Accepted defaults are ordinary editable answers, not locked or automatic state.
    let mut edited = accepted;
    edited.insert("part.general_tolerance".into(), json!("ISO 2768-f"));
    assert_eq!(
        session.accept_defaults(&edited).unwrap()["part.general_tolerance"],
        "ISO 2768-f"
    );
    assert_eq!(
        session.review(&edited).unwrap()["intent"]["part"]["general_tolerance"],
        "ISO 2768-f"
    );
}

#[test]
fn bulk_acceptance_round_trips_the_grm02_shaft() {
    check_bulk_acceptance(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/parts/GRM-02_shaft.step"),
        json!({"part.material": "steel", "part.surface_finish": "Ra 1.6"})
            .as_object()
            .unwrap()
            .clone(),
    );
}

#[test]
fn bulk_acceptance_round_trips_grm03() {
    let Some(root) = ["QUIDDITY_CORPUS", "SPECIFY_CORPUS"]
        .iter()
        .filter_map(std::env::var_os)
        .map(std::path::PathBuf::from)
        .find(|p| p.is_dir())
    else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED: set QUIDDITY_CORPUS or SPECIFY_CORPUS to the corpus directory"
        );
        return;
    };
    // #216 adds planar floor 13 to this hole ID. Keep the same M2 answer and
    // every complete-intent, accepted-default and exact written-byte comparison.
    check_bulk_acceptance(
        &root.join("gramel/GRM-03_thumbwheel_drive_screw.step"),
        json!({"part.material": "steel", "hole.function:holes:12.13": "tapped:M2", "diameter.fit:turned_steps:6": "thread:M3", "diameter.fit:turned_steps:3": "knurl:straight"}).as_object().unwrap().clone(),
    );
}

#[test]
fn accepting_defaults_leaves_questions_without_defaults_open_and_does_not_mutate_the_session() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", &std::fs::read(path).unwrap()).unwrap();
    let before = session.review(&Map::new()).unwrap();
    let accepted = session.accept_defaults(&Map::new()).unwrap();
    assert!(!accepted.contains_key("part.material"));
    let after = session.review(&accepted).unwrap();
    assert_eq!(after["open"], json!(["part.material"]));
    assert_eq!(session.review(&Map::new()).unwrap(), before);
}

#[test]
fn accepting_defaults_preserves_explicit_rejections_and_empty_answers() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", &std::fs::read(path).unwrap()).unwrap();
    let given = json!({"datum.A": [], "part.notes": [], "part.material": "steel", "part.general_tolerance": "ISO 2768-f"})
        .as_object().unwrap().clone();
    let accepted = session.accept_defaults(&given).unwrap();
    for (id, value) in &given {
        assert_eq!(accepted.get(id), Some(value));
    }
    assert_eq!(session.accept_defaults(&accepted).unwrap(), accepted);
    let review = session.review(&accepted).unwrap();
    assert!(
        review["intent"]["datums"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["letter"] != "A")
    );
    assert_eq!(review["intent"]["part"]["general_tolerance"], "ISO 2768-f");
}
