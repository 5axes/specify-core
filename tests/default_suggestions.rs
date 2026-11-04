//! Issue #20: answering one question must not accept the other questions' defaults.
mod common;

use serde_json::{Map, Value, json};
use specify_core::{api, part, serve::Server, tolerancing};

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn check_suggestions(analysis: &Value, given: &Map<String, Value>) {
    let review = api::review(analysis, given).unwrap();
    assert!(review["intent"]["datums"].as_array().unwrap().is_empty());
    assert!(review["intent"]["part"].get("general_tolerance").is_none());
    assert!(review["intent"]["general"].as_array().unwrap().is_empty());
    assert!(
        review["intent"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| {
                !matches!(
                    r["kind"].as_str(),
                    Some("position" | "runout" | "flatness" | "perpendicularity")
                )
            })
    );
    for id in ["datum.A", "datum.B", "datum.C", "part.general_tolerance"] {
        assert!(
            review["open"].as_array().unwrap().contains(&json!(id)),
            "{id}"
        );
        let q = review["questions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["id"] == id)
            .unwrap();
        assert!(!q["default"].is_null(), "{q}");
        let reason = q.get("basis").unwrap_or(&q["why"]);
        assert!(!reason.as_str().unwrap().is_empty(), "{q}");
    }
    let served = Server::default()
        .reply(&json!({"id": 1, "op": "preview", "analysis": analysis, "answers": given}));
    assert_eq!(served["result"]["intent"], review["intent"]);
    assert!(api::apply(analysis, given, false).is_err());
    if given.contains_key("part.material") {
        let accepted = api::apply(analysis, given, true).unwrap();
        assert_eq!(accepted.part["general_tolerance"], "ISO 2768-m");
        assert!(!accepted.datums.is_empty());
    }

    // #16's automatic ±/plain answers must not accept datum or tolerance suggestions either.
    let state = tolerancing::State {
        answers: given.clone(),
        auto: Some(vec![]),
        gdt: None,
    };
    let (settled, state) = tolerancing::settle(analysis, &state).unwrap();
    assert!(settled["intent"]["datums"].as_array().unwrap().is_empty());
    assert!(settled["intent"]["part"].get("general_tolerance").is_none());
    assert!(!state.answers.contains_key("part.general_tolerance"));
    assert!(!state.answers.contains_key("datum.A"));
}

#[test]
fn grm02_material_finish_and_holes_leave_the_general_tolerance_unanswered() {
    // GRM-02 is absent from the supplied corpus; the repository carries the original STEP.
    let path = common::fixtures().join("parts/GRM-02_shaft.step");
    let analysis = api::analyse_file(&path, None).unwrap();
    let mut given = answers(json!({"part.material": "steel", "part.surface_finish": "Ra 1.6"}));
    for q in api::questions(&analysis, &given).unwrap() {
        if q.id.starts_with("hole.function:") {
            given.insert(q.id, json!("general"));
        }
    }
    check_suggestions(&analysis, &Map::new());
    check_suggestions(&analysis, &given);
}

#[test]
fn grm03_tap_thread_and_knurl_do_not_add_datums_or_position() {
    let Some(root) = common::corpus_dir() else {
        return;
    };
    let path = root.join("gramel/GRM-03_thumbwheel_drive_screw.step");
    let loaded = part::load(&path, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let given = answers(json!({
        "hole.function:holes:14": "tapped:M2",
        "diameter.fit:turned_steps:6": "thread:M3",
        "diameter.fit:turned_steps:3": "knurl:straight",
    }));
    check_suggestions(&analysis, &Map::new());
    check_suggestions(&analysis, &given);
    let verbatim = api::preview(&analysis, &given).unwrap();
    // #126: the stale hole id is not asked, so its answer is reported as not written (one
    // `unasked` row) rather than ignored; it still contributes no PMI.
    let (unasked, made): (Vec<_>, Vec<_>) = verbatim
        .requirements
        .iter()
        .partition(|r| r["kind"] == specify_core::rules::UNASKED);
    assert_eq!(
        made.len(),
        2,
        "the issue's stale hole id must not accept the tap suggestion"
    );
    assert_eq!(unasked.len(), 1);
    assert_eq!(unasked[0]["answer"], "hole.function:holes:14");
    let hole = api::questions(&analysis, &given)
        .unwrap()
        .into_iter()
        .find(|q| {
            q.id.starts_with("hole.function:") && q.diameter.is_some_and(|d| (d - 1.6).abs() < 1e-6)
        })
        .unwrap();
    assert_eq!(
        hole.id, "hole.function:holes:12",
        "alpha.8 Ø1.6 hole identity"
    );
    let mut given = given;
    given.insert(hole.id, json!("tapped:M2"));
    check_suggestions(&analysis, &given);
    let intent = api::preview(&analysis, &given).unwrap();
    // The stale holes:14 answer is still given, so its `unasked` row (#126) follows the PMI.
    assert_eq!(intent.requirements.len(), 4, "{intent:?}");
    assert_eq!(
        intent
            .requirements
            .iter()
            .map(|r| r["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["thread", "knurl", "thread", specify_core::rules::UNASKED]
    );
    let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    let reopened = part::load_bytes("grm03.step", bytes).unwrap();
    let reread = api::analyse(&reopened).unwrap();
    // A deliberately stale key stays recoverable in the resume report, rather than being
    // silently carried as if it still addressed a recognised feature.
    let mut current = given.clone();
    let stale = current.shift_remove("hole.function:holes:14").unwrap();
    assert_eq!(reread["resume"]["answers"], json!(current));
    assert_eq!(
        reread["resume"]["unresolved"][0]["id"],
        "hole.function:holes:14"
    );
    assert_eq!(reread["resume"]["unresolved"][0]["answer"], stale);
    assert!(
        reread["resume"]["unresolved"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("no same-family")
    );
    // Analyse intentionally hides stored PMI when it resumes answers. Read semantic PMI
    // directly as well, so these absence assertions cannot pass just because it resumed.
    assert!(
        specify_core::existing::part_settings(&reopened)
            .unwrap()
            .get("general_tolerance")
            .is_none()
    );
    assert!(
        specify_core::existing::read_existing(&reopened)
            .unwrap()
            .iter()
            .all(|p| { p.kind != "datum" && p.kind != "geometric_tolerance" })
    );
}

#[test]
fn grm03_answered_position_reports_its_pending_frame_then_is_written() {
    let Some(root) = common::corpus_dir() else {
        return;
    };
    let loaded = part::load(
        &root.join("gramel/GRM-03_thumbwheel_drive_screw.step"),
        None,
    )
    .unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let qs = api::questions(&analysis, &Map::new()).unwrap();
    let position = qs
        .iter()
        .find(|q| q.id == "hole.position:holes:12")
        .cloned()
        .unwrap();
    let mut given = answers(json!({position.id.clone(): "0.2"}));
    let review = api::review(&analysis, &given).unwrap();
    let pending = review["problems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["code"] == "awaits_datum_frame" && p["about"] == position.id)
        .unwrap();
    assert_eq!(pending["awaiting"], json!(["A"]));
    assert!(
        review["intent"]["requirements"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !review["open"]
            .as_array()
            .unwrap()
            .contains(&json!(position.id))
    );
    let served = Server::default()
        .reply(&json!({"id": 1, "op": "questions", "analysis": analysis, "answers": given}));
    assert_eq!(served["result"]["problems"], review["problems"]);
    for q in qs {
        if ["datum.A", "datum.B", "datum.C"].contains(&q.id.as_str()) {
            given.insert(q.id, q.default.unwrap());
        }
    }
    let review = api::review(&analysis, &given).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["code"] != "awaits_datum_frame")
    );
    let intent = api::preview(&analysis, &given).unwrap();
    assert!(
        intent
            .requirements
            .iter()
            .any(|r| r["question"] == position.id && r["kind"] == "position")
    );
    let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    let reopened = part::load_bytes("grm03-position.step", bytes).unwrap();
    assert!(
        specify_core::existing::read_existing(&reopened)
            .unwrap()
            .iter()
            .any(|p| p.kind == "geometric_tolerance" && p.r#type.starts_with("Position"))
    );
}

fn check_accepted_defaults(path: &std::path::Path, given: Map<String, Value>) {
    let loaded = part::load(path, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let accepted = specify_core::rules::with_all_defaults(&analysis, &given).unwrap();
    assert_eq!(
        specify_core::rules::with_all_defaults(&analysis, &accepted).unwrap(),
        accepted
    );
    for (id, value) in &given {
        assert_eq!(accepted.get(id), Some(value));
    }
    let review = api::review(&analysis, &accepted).unwrap();
    assert!(review["open"].as_array().unwrap().is_empty());
    assert_eq!(review["intent"]["part"]["general_tolerance"], "ISO 2768-m");
    let expected = api::apply(&analysis, &given, true).unwrap();
    let mut preview = api::preview(&analysis, &accepted).unwrap();
    for item in preview
        .datums
        .iter_mut()
        .chain(preview.requirements.iter_mut())
    {
        if item.contains_key("not_written_reason") {
            // Planner diagnostics are reported proposals, not accepted interview
            // requirements. They have no question/default status (#75).
            assert!(!item.contains_key("question"));
            assert!(!item.contains_key("default"));
        } else {
            assert_eq!(item.remove("default"), Some(json!(false)));
        }
    }
    assert_eq!(preview.to_dict(), expected.to_dict());
    assert!(!expected.datums.is_empty());
    assert!(
        expected
            .requirements
            .iter()
            .any(|r| r["kind"] == "position")
    );
    let (actual_bytes, _) =
        specify_core::writer::write_bytes(&loaded, &preview, Some(&accepted)).unwrap();
    let (expected_bytes, _) =
        specify_core::writer::write_bytes(&loaded, &expected, Some(&accepted)).unwrap();
    assert_eq!(actual_bytes, expected_bytes);
    let reopened = part::load_bytes("accepted.step", actual_bytes).unwrap();
    assert_eq!(
        api::analyse(&reopened).unwrap()["resume"]["answers"],
        json!(accepted)
    );
    assert_eq!(
        specify_core::existing::part_settings(&reopened).unwrap()["general_tolerance"],
        "ISO 2768-m"
    );
    assert!(
        specify_core::existing::read_existing(&reopened)
            .unwrap()
            .iter()
            .any(|p| p.kind == "geometric_tolerance" && p.r#type == "Position")
    );
}

#[test]
fn grm02_explicit_bulk_acceptance_matches_apply_with_defaults() {
    check_accepted_defaults(
        &common::fixtures().join("parts/GRM-02_shaft.step"),
        answers(json!({"part.material": "steel", "part.surface_finish": "Ra 1.6"})),
    );
}

#[test]
fn grm03_explicit_bulk_acceptance_matches_apply_with_defaults() {
    let Some(root) = common::corpus_dir() else {
        return;
    };
    check_accepted_defaults(
        &root.join("gramel/GRM-03_thumbwheel_drive_screw.step"),
        answers(json!({
            "part.material": "steel",
            "hole.function:holes:12": "tapped:M2",
            "diameter.fit:turned_steps:6": "thread:M3",
            "diameter.fit:turned_steps:3": "knurl:straight",
        })),
    );
}
