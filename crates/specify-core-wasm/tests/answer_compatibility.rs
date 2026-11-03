//! Pin the binding's compatibility to the independent Python-compatible core rules.
use serde_json::{Map, json};
use specify_core::{api, part, rules, tolerancing, writer};
use specify_core_wasm::session::Session;

#[test]
fn text_answers_keep_core_truthiness_and_json_values() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let loaded = part::load_bytes("plate.step", bytes.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let session = Session::open("plate.step", bytes).unwrap();
    for id in ["part.material", "part.coating", "part.heat_treatment"] {
        for value in [
            json!(null),
            json!(""),
            json!(42),
            json!(false),
            json!(0),
            json!([]),
            json!({}),
            json!(["legacy"]),
        ] {
            let answers = Map::from_iter([(id.to_string(), value)]);
            let expected = api::review(&analysis, &answers).unwrap();
            let mut actual = session.review(&answers).unwrap();
            actual["intent"]
                .as_object_mut()
                .unwrap()
                .shift_remove("anchors");
            assert_eq!(actual, expected, "{answers:?}");
            assert_eq!(
                session.accept_defaults(&answers).unwrap(),
                rules::with_all_defaults(&analysis, &answers).unwrap()
            );
            let intent = api::preview(&analysis, &answers).unwrap();
            let (bytes, report) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
            let written = session.write_pmi(&answers, None, None).unwrap();
            assert_eq!(written["bytes"], json!(bytes));
            assert_eq!(written["report"], report.to_dict());
        }
    }
}

#[test]
fn stale_and_repeated_auto_entries_keep_core_settling_semantics() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let loaded = part::load_bytes("plate.step", bytes.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let session = Session::open("plate.step", bytes).unwrap();
    for gdt in [None, Some(tolerancing::Mode::Geometric)] {
        let state = tolerancing::State {
            answers: Map::from_iter([("part.material".into(), json!("steel"))]),
            auto: Some(vec![
                "missing".into(),
                "missing".into(),
                "part.material".into(),
            ]),
            gdt,
        };
        let (expected_review, expected_state) = tolerancing::settle(&analysis, &state).unwrap();
        let mut actual = session.settle(&state).unwrap();
        assert_eq!(actual["state"], json!(expected_state));
        assert_eq!(
            actual["review"],
            session.review(&expected_state.answers).unwrap()
        );
        actual["review"]["intent"]
            .as_object_mut()
            .unwrap()
            .shift_remove("anchors");
        assert_eq!(actual["review"], expected_review);
    }
}

#[test]
fn carried_refusals_survive_wasm_defaults_review_and_write() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", bytes).unwrap();
    let loaded = part::load_bytes("plate.step", bytes.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let mut answers = json!({"part.material":"steel", "_specify":{
        "unresolved":{"datum.A":"face 4 has no match in the new part"}
    }}).as_object().unwrap().clone();
    assert!(session.accept_defaults(&answers).unwrap_err().contains("carried answer could not be applied"));
    assert!(specify_core_wasm::accept_defaults(&analysis, &answers).is_err());
    assert!(!session.review(&answers).unwrap()["open"].as_array().unwrap().is_empty());
    assert!(session.write_pmi(&answers, None, None).is_err());
    answers.insert("datum.A".into(), json!([4]));
    let accepted = session.accept_defaults(&answers).unwrap();
    assert_eq!(accepted["datum.A"], json!([4]));
    session.write_pmi(&accepted, None, None).unwrap();
    answers["_specify"]["evidence"] = session.evidence().unwrap();
    session.write_pmi(&answers, None, None).unwrap();
    answers["_specify"]["evidence"]["face_count"] = json!(999);
    assert!(session.write_pmi(&answers, None, None).unwrap_err().contains("stale answers"));
}
