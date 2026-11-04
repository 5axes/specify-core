//! #126 through the editor's session: its preview-based write lists answers and carry blocks
//! whose question is not asked, and a reopened write restores the block.
use serde_json::{Map, Value, json};
use specify_core_wasm::session::Session;

fn answers(extra: Value) -> Map<String, Value> {
    let mut answers = json!({
        "part.material": "6082-T6",
        "part.locating": "± dimensions",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
    })
    .as_object()
    .unwrap()
    .clone();
    answers.extend(extra.as_object().unwrap().clone());
    answers
}

fn not_written(written: &Value) -> Vec<String> {
    serde_json::from_value(written["report"]["not_written"].clone()).unwrap()
}

#[test]
fn the_session_write_lists_unasked_answers_and_keeps_blocks() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", bytes).unwrap();
    let written = session
        .write_pmi(&answers(json!({"datum.A.flatness": "0.05"})), None, None)
        .unwrap();
    assert!(
        not_written(&written).contains(
            &"datum.A.flatness is not asked when part.locating is '± dimensions', so its \
              answer was not written"
                .to_string()
        ),
        "{written}"
    );
    let block = json!({"_specify": {"unresolved":
        {"datum.A.flatness": "its datum datum.A is unresolved"}}});
    let written = session.write_pmi(&answers(block), None, None).unwrap();
    assert!(
        not_written(&written)
            .iter()
            .any(|l| l.starts_with("datum.A.flatness was carried")),
        "{written}"
    );
    let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    let reopened = Session::open("written.step", &bytes).unwrap();
    assert_eq!(
        reopened.snapshot()["analysis"]["resume"]["answers"]["_specify"]["unresolved"]["datum.A.flatness"],
        "its datum datum.A is unresolved"
    );
}
