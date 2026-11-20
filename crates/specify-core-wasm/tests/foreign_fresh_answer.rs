//! ADR P9 through the editor's session: another tool re-authored Specify's hole position 0.5
//! as its own 1.5. A save keeps 1.5 and drops the stored answer; after reopening (no
//! conflict), answering 0.5 afresh and writing (`acceptDefaults`, then `writePmi`, as the
//! editor's worker does) must keep the other tool's 1.5 alone and say why 0.5 wasn't written.
use serde_json::{Map, Value, json};
use specify_core::{api, existing::read_existing, part::load_bytes};
use specify_core_wasm::session::Session;

const HOLE: &str = "hole.position:holes:12";
const PLATE: &[u8] = include_bytes!("../../../tests/fixtures/parts/plate.step");

fn re_authored() -> Vec<u8> {
    let given = json!({
        "part.material": "6082-T6",
        "part.locating": "position tolerances",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.03",
        "datum.B.control": "0.1",
        (HOLE): "0.5",
    })
    .as_object()
    .unwrap()
    .clone();
    let analysis = api::analyse(&load_bytes("plate.step", PLATE.to_vec()).unwrap()).unwrap();
    let intent = api::apply(&analysis, &given, true).unwrap();
    let bytes = api::write_bytes(PLATE.to_vec(), &[(intent, Some(given))])
        .unwrap()
        .0;
    let text = String::from_utf8(bytes).unwrap();
    let line = text
        .lines()
        .find(|l| l.contains("POSITION_TOLERANCE()") && !l.contains("WITH_MODIFIERS"))
        .unwrap()
        .to_string();
    let measure = line
        .split("GEOMETRIC_TOLERANCE(")
        .nth(1)
        .unwrap()
        .split(',')
        .nth(2)
        .unwrap()
        .trim()
        .to_string();
    let value = text
        .lines()
        .find(|l| l.starts_with(&format!("{measure}=")))
        .unwrap()
        .to_string();
    assert!(line.contains("'specify-core:written:v1'") && value.contains("(0.5)"));
    text.replace(&line, &line.replace("'specify-core:written:v1'", "''"))
        .replace(&value, &value.replace("(0.5)", "(1.5)"))
        .into_bytes()
}

fn positions(bytes: &[u8]) -> Vec<(f64, bool)> {
    read_existing(&load_bytes("plate.step", bytes.to_vec()).unwrap())
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == "Position" && e.faces == [12])
        .map(|e| (e.value.unwrap(), e.provenance.is_some()))
        .collect()
}

/// The editor's answers on opening (`reconcileStepEdits`): the resumed answers, conflicts
/// and file-answered questions left out, with `keep` carried over.
fn opened(session: &Session, keep: &[&str]) -> Map<String, Value> {
    let resume = session.snapshot()["analysis"]["resume"].clone();
    let mut answers = resume["answers"].as_object().unwrap().clone();
    if !keep.is_empty() {
        answers.insert("_specify".into(), json!({"keep": keep}));
    }
    answers
}

/// The editor's Save: `writePmi(acceptDefaults(answers))`.
fn save(session: &Session, answers: &Map<String, Value>) -> (Vec<u8>, Value) {
    let accepted = session.accept_defaults(answers).unwrap();
    let written = session.write_pmi(&accepted, None, None).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    (bytes, written["report"].clone())
}

fn scenario(keep: bool) {
    let edited = re_authored();
    assert_eq!(positions(&edited), [(1.5, false)]);
    let session = Session::open("plate.step", &edited).unwrap();
    let analysis = session.snapshot()["analysis"].clone();
    assert!(
        analysis["resume"]["conflicts"]
            .as_array()
            .is_some_and(|c| c.iter().any(|c| c["id"] == HOLE && c["foreign"] == true)),
        "{}",
        analysis["resume"]
    );
    let keep: &[&str] = if keep { &[HOLE] } else { &[] };
    let (saved, _) = save(&session, &opened(&session, keep));
    assert_eq!(positions(&saved), [(1.5, false)]);
    let session = Session::open("plate.step", &saved).unwrap();
    let resume = session.snapshot()["analysis"]["resume"].clone();
    assert!(resume.get("conflicts").is_none(), "{resume}");
    assert!(resume["answers"].get(HOLE).is_none(), "{resume}");
    let mut answers = opened(&session, keep);
    answers.insert(HOLE.into(), json!("0.5"));
    let (answered, report) = save(&session, &answers);
    assert_eq!(
        positions(&answered),
        [(1.5, false)],
        "keep {keep:?}: {report}"
    );
    assert!(
        report["not_written"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .any(|l| l.starts_with(HOLE)
                && l.contains("another tool's requirement holds this question")
                && l.contains("keep it, or change it in that tool")),
        "{report}"
    );
    // Reopened once more: still the other tool's 1.5, and no answer stored for it.
    let again = Session::open("plate.step", &answered).unwrap();
    let resume = again.snapshot()["analysis"]["resume"].clone();
    assert!(resume["answers"].get(HOLE).is_none(), "{resume}");
}

#[test]
fn the_editor_does_not_replace_another_tools_position_with_a_fresh_answer() {
    scenario(false);
}

#[test]
fn the_editor_does_not_replace_a_kept_foreign_position_with_a_fresh_answer() {
    scenario(true);
}
