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
            let mut written = session.write_pmi(&answers, None, None).unwrap();
            assert_eq!(written["bytes"], json!(bytes));
            // The session's report is the writer's plus whether the answers were complete.
            let extra = written["report"].as_object_mut().unwrap();
            let complete = extra.shift_remove("complete").unwrap();
            let missing = extra.shift_remove("missing").unwrap();
            assert_eq!(written["report"], report.to_dict());
            assert_eq!(complete, json!(missing.as_array().unwrap().is_empty()));
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
    }})
    .as_object()
    .unwrap()
    .clone();
    assert!(
        session
            .accept_defaults(&answers)
            .unwrap_err()
            .contains("carried answer could not be applied")
    );
    assert!(specify_core_wasm::accept_defaults(&analysis, &answers).is_err());
    assert!(
        !session.review(&answers).unwrap()["open"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(session.write_pmi(&answers, None, None).is_err());
    answers.insert("datum.A".into(), json!([4]));
    let accepted = session.accept_defaults(&answers).unwrap();
    assert_eq!(accepted["datum.A"], json!([4]));
    session.write_pmi(&accepted, None, None).unwrap();
    answers["_specify"]["evidence"] = session.evidence().unwrap();
    session.write_pmi(&answers, None, None).unwrap();
    answers["_specify"]["evidence"]["face_count"] = json!(999);
    assert!(
        session
            .write_pmi(&answers, None, None)
            .unwrap_err()
            .contains("stale answers")
    );
}

/// ADR P9: the session's export takes exactly the defaults approved for its part, with the
/// values the person saw, as `api::export_answers` does; none without approvals.
#[test]
fn export_takes_only_the_defaults_approved_for_its_part() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let loaded = part::load_bytes("plate.step", bytes.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let session = Session::open("plate.step", bytes).unwrap();
    let given = Map::from_iter([("part.material".to_string(), json!("steel"))]);
    let default = api::questions(&analysis, &given)
        .unwrap()
        .into_iter()
        .find(|q| q.id == "part.general_tolerance")
        .and_then(|q| q.default)
        .unwrap();
    let approve = |part: usize, value: serde_json::Value| {
        api::Approved::from([(
            part,
            Map::from_iter([("part.general_tolerance".to_string(), value)]),
        )])
    };
    let without = |mut m: Map<String, serde_json::Value>| {
        m.remove("_specify");
        m
    };
    let none = session
        .export_answers(&given, &api::Approved::new())
        .unwrap();
    assert_eq!(without(none.clone()), given);
    let one = session
        .export_answers(&given, &approve(0, default.clone()))
        .unwrap();
    let mut expected = given.clone();
    expected.insert("part.general_tolerance".into(), default.clone());
    assert_eq!(without(one.clone()), expected);
    assert_eq!(
        one,
        api::export_answers(&loaded, &analysis, &given, &approve(0, default.clone()))
            .unwrap()
            .answers
    );
    let refused = session
        .export_answers(&given, &approve(0, json!("changed")))
        .unwrap_err();
    assert!(
        refused.contains("part 0: part.general_tolerance's default is now"),
        "{refused}"
    );
    assert_eq!(
        session
            .export_answers(&given, &approve(1, default))
            .unwrap(),
        none,
        "another part's approval does not apply"
    );
}
