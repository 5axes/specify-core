//! The blind thread depth is discoverable before an error, and visible when accepting defaults.
mod common;

use serde_json::{Map, Value, json};
use specify_core::{api, choices, rules, serve::Server};

fn string_post() -> Value {
    common::case("corpus/gramel/string_post")
        .artifact("analysis")
        .unwrap()
}

fn hole(analysis: &Value) -> rules::Question {
    rules::questions(analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id == "hole.function:holes:17.19")
        .unwrap()
}

#[test]
fn blind_tap_help_gives_valid_syntax_and_the_summary_shows_the_computed_depth() {
    let a = string_post();
    let q = hole(&a);
    assert!(q.why.contains("\"M2 tapped, 2.5 mm full thread\""));
    assert!(q.why.contains("drill depth minus 3 pitches"));
    assert!(q.basis.contains("Default minimum full thread: 2.5 mm."));
    assert_eq!(q.default, Some(json!("tapped:M2")));
    let choice = q
        .choices
        .iter()
        .find(|c| c["value"] == "tapped:M2")
        .unwrap();
    assert_eq!(choice["code"], "M2 tapped, 2.5 mm full thread");
    assert_eq!(
        choice["callout"],
        "M2×0.4-6H, 2.5 mm min full thread, ⌀1.6 tap drill"
    );
    let typed = choices::interpret(
        &q.id,
        "M2 tapped, 2.5 mm full thread",
        &q.options,
        q.diameter,
        q.depth,
    )
    .unwrap();
    assert_eq!(typed, "tapped:M2@2.5");
    let review = api::review(&a, &Map::new()).unwrap();
    let presented = review["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == q.id)
        .unwrap();
    assert_eq!(presented["why"], q.why);
    assert_eq!(presented["choices"], json!(q.choices));
    // #20: help remains visible while the proposed tap is unanswered, but its thread PMI
    // (and the depth being checked) appears only after that suggestion is accepted.
    assert!(
        rules::preview(&a, &Map::new())
            .unwrap()
            .requirements
            .is_empty()
    );
    let given = Map::from_iter([(q.id.clone(), q.default.clone().unwrap())]);
    // An answered tap keeps alpha.7's derived manufacturing depth without a depth answer.
    let intent = rules::preview(&a, &given).unwrap();
    assert_eq!(
        intent
            .requirements
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap()["full_thread"],
        2.5
    );
}

#[test]
fn full_thread_is_a_text_edit_and_defaults_stay_suggestions() {
    let a = string_post();
    let id = "hole.full_thread:holes:17.19";
    let q = rules::questions(&a, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id == id)
        .unwrap();
    assert_eq!(q.kind, "text");
    assert_eq!(q.default, Some(json!("2.5")));
    assert_eq!(q.attention, "refine");
    assert!(rules::open_questions(&[q], &Map::new()).is_empty());
    for text in [
        json!("0"),
        json!("-1"),
        json!("3"),
        json!("NaN"),
        json!("inf"),
        json!("6 inches"),
        json!(2),
    ] {
        let given = Map::from_iter([(id.into(), text)]);
        assert!(rules::questions(&a, &given).is_err(), "{given:?}");
    }
    let given = Map::from_iter([
        (id.into(), json!("2.3 mm")),
        ("hole.function:holes:17.19".into(), json!("tapped:M2@2.5")),
    ]);
    let accepted = rules::with_all_defaults(&a, &given).unwrap();
    assert_eq!(accepted[id], "2.3 mm");
    let intent = rules::preview(&a, &accepted).unwrap();
    assert_eq!(
        intent
            .requirements
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap()["full_thread"],
        2.3
    );
    for function in ["general", "fit:H7"] {
        let given = Map::from_iter([("hole.function:holes:17.19".into(), json!(function))]);
        assert!(
            rules::questions(&a, &given)
                .unwrap()
                .iter()
                .all(|q| q.id != id)
        );
    }
}

/// Frozen by cx/quiddity-010 (9c16b407), using its native parity_reference example:
/// GRM-02_shaft.step, name shaft.step, material steel and the M2 tap answered, no depth
/// answer or accept-defaults. quiddity-rust#137 adds callouts and separate location/composite
/// aspects: preserve the old semantic readback and pin every new byte (including #65 ids).
#[test]
fn answered_m2_without_depth_preserves_alpha7_semantics_and_pins_presentation_bytes() {
    use haecceity::{p21::Document, pmi, step::read_part_definitions};
    use specify_core::{part, writer};
    let loaded = part::load_bytes(
        "shaft.step",
        std::fs::read(common::fixtures().join("parts/GRM-02_shaft.step")).unwrap(),
    )
    .unwrap();
    let a = api::analyse(&loaded).unwrap();
    let q = rules::questions(&a, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with("hole.full_thread:"))
        .unwrap();
    let function = q.id.replace("hole.full_thread:", "hole.function:");
    let given = Map::from_iter([
        ("part.material".into(), json!("steel")),
        (function, json!("tapped:M2")),
    ]);
    assert!(!given.contains_key(&q.id));
    let intent = api::preview(&a, &given).unwrap();
    assert_eq!(
        intent
            .requirements
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap()["full_thread"],
        2.0
    );
    let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    let baseline = part::load(
        &common::fixtures().join("thread-depth-alpha7.step.gz"),
        None,
    )
    .unwrap();
    let current = part::load_bytes("shaft.step", bytes.clone()).unwrap();
    assert_eq!(
        common::numbering::legacy_existing(
            specify_core::existing::read_existing(&baseline).unwrap()
        ),
        common::numbering::legacy_existing(
            specify_core::existing::read_existing(&current).unwrap()
        ),
        "quiddity-rust#137 must preserve every legacy semantic PMI field"
    );
    assert_eq!(
        specify_core::existing::part_settings(&baseline).unwrap(),
        specify_core::existing::part_settings(&current).unwrap()
    );
    let numbered = part::load(
        &common::fixtures().join("thread-depth-numbered.step.gz"),
        None,
    )
    .unwrap();
    // This is also the WASM byte pin: #137 callouts, aspects and #65 numbering.
    // v0.4.1 regenerates GENERAL NOTES and plane views; recapture only this output.
    // Keep the historical alpha.7 input and every semantic readback check unchanged.
    assert_eq!(bytes.as_slice(), numbered.bytes.as_slice());
    for (source, answers) in [(loaded, given.clone()), (baseline, given)] {
        let analysis = api::analyse(&source).unwrap();
        let mut complete = rules::with_all_defaults(&analysis, &answers).unwrap();
        complete.remove(&q.id);
        let review = api::review(&analysis, &complete).unwrap();
        assert!(review["open"].as_array().unwrap().is_empty());
        assert_eq!(
            review["problems"],
            api::review(
                &analysis,
                &rules::with_all_defaults(&analysis, &complete).unwrap()
            )
            .unwrap()["problems"]
        );
        assert!(!complete.contains_key(&q.id));
        // Strict apply also accepts the old complete answer set without a new answer.
        api::apply(&analysis, &complete, false).unwrap();
        let intent = api::preview(&analysis, &answers).unwrap();
        let (written, _) = writer::write_bytes(&source, &intent, Some(&answers)).unwrap();
        let defs = read_part_definitions(&written).unwrap();
        let doc = Document::parse(written).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        assert_eq!(read.parts[0].threads.len(), 1);
        let (_, attr) = read.parts[0]
            .attributes
            .iter()
            .flat_map(|a| &a.items)
            .find(|(n, _)| n == "minimum full thread")
            .unwrap();
        let pmi::AttributeValue::Measure(m) = attr else {
            panic!("a measure")
        };
        let pmi::Quantity::Length(l) = &m.quantity else {
            panic!("a length")
        };
        assert_eq!(l.mm(), 2.0);
    }
}

#[test]
fn gramel_defaults_edit_step_pmi_and_resume() {
    use haecceity::{p21::Document, pmi, step::read_part_definitions};
    for (path, expected, edited) in [
        (
            Some(common::fixtures().join("parts/GRM-02_shaft.step")),
            2.0,
            "1.8",
        ),
        (
            common::corpus_dir().map(|p| p.join("gramel/GRM-03_thumbwheel_drive_screw.step")),
            // This corpus revision has a 3.8 mm flat-bottom drill, not the 8 mm
            // drill in #21. Exercise the reported 8 mm case separately below.
            2.5,
            "2.3",
        ),
    ] {
        let Some(path) = path else { continue };
        let loaded = specify_core::part::load(&path, None).unwrap();
        let a = api::analyse(&loaded).unwrap();
        let qs = rules::questions(&a, &Map::new()).unwrap();
        let q = qs
            .iter()
            .find(|q| q.id.starts_with("hole.full_thread:"))
            .unwrap();
        assert_eq!(
            q.default
                .as_ref()
                .unwrap()
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap(),
            expected,
            "{}",
            path.display()
        );
        let function = q.id.replace("hole.full_thread:", "hole.function:");
        let given = Map::from_iter([
            ("part.material".into(), json!("steel")),
            (function, json!("tapped:M2")),
            (q.id.clone(), json!(edited)),
        ]);
        let accepted = rules::with_all_defaults(&a, &given).unwrap();
        let intent = api::apply(&a, &accepted, false).unwrap();
        let full = edited.parse::<f64>().unwrap();
        assert_eq!(
            intent
                .requirements
                .iter()
                .find(|r| r["kind"] == "thread" && r["side"] == "internal")
                .unwrap()["full_thread"],
            full
        );
        let (bytes, _) =
            specify_core::writer::write_bytes(&loaded, &intent, Some(&accepted)).unwrap();
        let defs = read_part_definitions(&bytes).unwrap();
        let doc = Document::parse(bytes.clone()).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        let attr = read
            .parts
            .iter()
            .flat_map(|p| &p.attributes)
            .flat_map(|a| &a.items)
            .find(|(name, _)| name == "minimum full thread")
            .unwrap();
        let pmi::AttributeValue::Measure(measure) = &attr.1 else {
            panic!("a length measure")
        };
        let pmi::Quantity::Length(length) = &measure.quantity else {
            panic!("a length")
        };
        assert_eq!(length.value.to_string().parse::<f64>().unwrap(), full);
        let reopened = specify_core::part::load_bytes("edited.step", bytes).unwrap();
        assert_eq!(
            specify_core::resume::answers_for(&reopened)
                .unwrap()
                .unwrap()[&q.id],
            edited
        );
        let a = api::analyse(&reopened).unwrap();
        assert_eq!(a["resume"]["answers"][&q.id], edited);
        let resumed = a["resume"]["answers"].as_object().unwrap();
        assert_eq!(
            api::preview(&a, resumed)
                .unwrap()
                .requirements
                .iter()
                .find(|r| r["kind"] == "thread" && r["side"] == "internal")
                .unwrap()["full_thread"],
            full
        );
    }
}

#[test]
fn issue_21_eight_mm_m2_drill_suggests_six_point_five_and_accepts_six() {
    // Rules input for the geometry reported in #21. The committed GRM-03 capture and
    // available corpus both describe a 3.8 mm drill; do not change their evidence.
    let mut a = common::case("corpus/gramel/GRM-03_thumbwheel_drive_screw")
        .artifact("analysis")
        .unwrap();
    let hole = a["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["family"] == "holes" && f["record"]["diameter"] == 1.6)
        .unwrap();
    let fid = hole["id"].as_str().unwrap().to_string();
    hole["record"]["depth"] = json!(8.0);
    let id = format!("hole.full_thread:{fid}");
    let q = rules::questions(&a, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id == id)
        .unwrap();
    assert_eq!(q.default, Some(json!("6.5")));
    let given = Map::from_iter([
        (format!("hole.function:{fid}"), json!("tapped:M2")),
        (id, json!("6")),
    ]);
    let intent = rules::preview(&a, &given).unwrap();
    let thread = intent
        .requirements
        .iter()
        .find(|r| r["kind"] == "thread")
        .unwrap();
    assert_eq!(thread["drill_depth"], 8.0);
    assert_eq!(thread["full_thread"], 6.0);
}

#[test]
fn interpretation_reports_implicit_and_explicit_depths_without_changing_the_stored_value() {
    let a = string_post();
    let q = hole(&a);
    let mut server = Server::default();
    for (text, value, code) in [
        ("M2", "tapped:M2", "M2 tapped, 2.5 mm full thread"),
        (
            "M2 tapped, 2.3 mm full thread",
            "tapped:M2@2.3",
            "M2 tapped, 2.3 mm full thread",
        ),
    ] {
        let reply = server.reply(&json!({"id": 1, "op": "interpret", "analysis": a, "answers": {}, "question": q.id, "text": text}));
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["value"], value);
        assert_eq!(reply["result"]["code"], code);
    }
}

#[test]
fn through_and_unknown_depth_holes_do_not_promise_a_blind_thread_depth() {
    for bottom in ["through", "unknown_depth"] {
        let mut a = string_post();
        for f in a["features"].as_array_mut().unwrap() {
            if f["id"] == "holes:17.19" {
                if bottom == "through" {
                    f["record"]["bottom"] = json!("through");
                } else {
                    f["record"]["depth"] = Value::Null;
                }
            }
        }
        let q = hole(&a);
        assert!(q.depth.is_none());
        assert!(!q.why.contains("full-thread depth"));
        let choice = q
            .choices
            .iter()
            .find(|c| c["value"] == "tapped:M2")
            .unwrap();
        assert_eq!(choice["code"], "M2 tapped");
        assert!(
            !choice["callout"]
                .as_str()
                .unwrap()
                .contains("min full thread")
        );
    }
}

#[test]
fn the_requested_five_mm_syntax_and_depth_limits_are_preserved() {
    let a = string_post();
    let q = hole(&a);
    assert_eq!(
        choices::interpret(
            &q.id,
            "M2 tapped, 5 mm full thread",
            &q.options,
            q.diameter,
            Some(6.5)
        )
        .unwrap(),
        "tapped:M2@5"
    );
    assert!(
        choices::interpret(
            &q.id,
            "M2 tapped, 5 mm full thread",
            &q.options,
            q.diameter,
            q.depth
        )
        .is_err()
    );
    assert_eq!(
        choices::with_default_full_thread("tapped:M2", Some(3.8)),
        "tapped:M2@2.5"
    );
    assert_eq!(
        choices::with_default_full_thread("tapped:M2@2.3", Some(4.0)),
        "tapped:M2@2.3"
    );
}
