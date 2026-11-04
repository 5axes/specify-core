//! #126: an answer valid in form whose question is not asked, given the other answers, is
//! never dropped silently: every write report lists it in `not_written`, saying why. An ID no
//! question can have is refused by every entry point with "unknown answer ID".

mod common;

use std::path::PathBuf;
use std::process::Command;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::serve::Server;

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-unasked-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

/// A caller revising answers across a change of `part.locating`: it keeps both modes' answers.
fn revised(locating: &str) -> Map<String, Value> {
    map(json!({
        "part.material": "6082-T6",
        "part.locating": locating,
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.05",
        "datum.B.control": "0.1",
        "hole.position:holes:12": "0.1",
        "hole.location:holes:12": "±0.1",
    }))
}

fn not_written(id: &str, why: &str) -> String {
    format!("{id} is not asked {why}, so its answer was not written")
}

fn write(answers: &Map<String, Value>, name: &str) -> specify_core::writer::WriteReport {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, answers, true).unwrap();
    let out = temp(name).join("out.step");
    api::write_file(&plate(), &intent, &out, Some(answers)).unwrap()
}

#[test]
fn flatness_is_written_by_position_and_listed_by_dimensions() {
    let report = write(&revised("position tolerances"), "position");
    assert!(
        report.written.contains(&"flatness 0.05 on [2]".to_string()),
        "{:?}",
        report.written
    );
    assert_eq!(
        report.not_written,
        [not_written(
            "hole.location:holes:12",
            "when part.locating is 'position tolerances'"
        )]
    );

    let report = write(&revised("± dimensions"), "dimensions");
    assert!(
        !report.written.iter().any(|w| w.contains("flatness")),
        "{:?}",
        report.written
    );
    let when = "when part.locating is '± dimensions'";
    assert_eq!(
        report.not_written,
        [
            not_written("datum.A.flatness", when),
            not_written("datum.B.control", when),
            not_written("hole.position:holes:12", when),
        ]
    );
}

/// The plate's M5 hole is through, so no full-thread depth is asked of it under either mode.
#[test]
fn a_full_thread_depth_of_a_through_hole_is_listed() {
    let mut answers = revised("position tolerances");
    answers.remove("hole.location:holes:12");
    answers.insert("hole.full_thread:holes:12".into(), json!("4"));
    let report = write(&answers, "full-thread");
    assert_eq!(
        report.not_written,
        [not_written(
            "hole.full_thread:holes:12",
            "of this part given the other answers"
        )]
    );
}

/// The editor writes its preview intent plus `rules::unasked_rows` (WASM `writePmi`); the row
/// reaches that report too, and the answer stays stored so the question takes it again when it
/// returns. Preview itself leaves the rows out, so reviews do not pay for them.
#[test]
fn a_preview_write_lists_it_and_keeps_the_answer() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let mut answers =
        specify_core::rules::with_all_defaults(&analysis, &revised("± dimensions")).unwrap();
    let mut intent = api::preview(&analysis, &answers).unwrap();
    assert!(!intent.requirements.iter().any(|r| r["kind"] == "unasked"));
    intent
        .requirements
        .extend(specify_core::rules::unasked_rows(&analysis, &answers).unwrap());
    let bytes = std::fs::read(plate()).unwrap();
    let (written, report) = api::write_bytes(bytes, &[(intent, Some(answers.clone()))]).unwrap();
    assert!(
        report.not_written.contains(&not_written(
            "datum.A.flatness",
            "when part.locating is '± dimensions'"
        )),
        "{:?}",
        report.not_written
    );
    let loaded = specify_core::part::load_bytes("out.step", written).unwrap();
    let resumed = api::analyse(&loaded).unwrap();
    assert_eq!(resumed["resume"]["answers"]["datum.A.flatness"], "0.05");
    // Switching back asks it again, with the kept answer.
    answers.insert("part.locating".into(), json!("position tolerances"));
    let intent = api::apply(&analysis, &answers, true).unwrap();
    assert!(intent.requirements.iter().any(|r| r["kind"] == "flatness"));
}

/// Export keeps an unasked answer: dropping it would lose it silently, and refusing would break
/// callers that revise answers across a change of mode. A regression guard: main already kept it.
#[test]
fn export_keeps_an_unasked_answer() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let loaded = specify_core::part::load(&plate(), None).unwrap();
    let saved = api::export_answers(&loaded, &analysis, &revised("± dimensions")).unwrap();
    assert_eq!(saved["datum.A.flatness"], "0.05");
    assert_eq!(saved["hole.position:holes:12"], "0.1");
}

const UNKNOWN: &str = "Invalid answers: bogus.question: unknown answer ID";

fn bogus() -> Map<String, Value> {
    let mut answers = revised("position tolerances");
    answers.insert("bogus.question".into(), json!("x"));
    answers
}

#[test]
fn the_library_refuses_an_unknown_id() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let answers = bogus();
    let message = |e: api::Error| e.to_string();
    assert_eq!(
        message(api::questions(&analysis, &answers).unwrap_err()),
        UNKNOWN
    );
    assert_eq!(
        message(api::apply(&analysis, &answers, true).unwrap_err()),
        UNKNOWN
    );
    assert_eq!(
        message(api::apply(&analysis, &answers, false).unwrap_err()),
        UNKNOWN
    );
    assert_eq!(
        message(api::preview(&analysis, &answers).unwrap_err()),
        UNKNOWN
    );
    assert_eq!(
        message(api::review(&analysis, &answers).unwrap_err()),
        UNKNOWN
    );
    let loaded = specify_core::part::load(&plate(), None).unwrap();
    assert_eq!(
        message(api::export_answers(&loaded, &analysis, &answers).unwrap_err()),
        UNKNOWN
    );
    // Added requirements and reserved members remain answers.
    let mut answers = revised("position tolerances");
    answers.insert(
        "extra.note".into(),
        json!({"kind": "flatness", "faces": [2], "value": "0.1"}),
    );
    answers.insert("_specify".into(), json!({"unresolved": {}}));
    api::apply(&analysis, &answers, true).unwrap();
}

#[test]
fn the_command_line_refuses_an_unknown_id() {
    let dir = temp("cli");
    let analysis = dir.join("plate.json");
    let answers = dir.join("answers.json");
    std::fs::write(
        &analysis,
        api::analyse_file(&plate(), None).unwrap().to_string(),
    )
    .unwrap();
    std::fs::write(&answers, Value::Object(bogus()).to_string()).unwrap();
    let out = dir.join("out.step");
    let runs = [
        vec![
            "questions".as_ref(),
            analysis.as_os_str(),
            "--answers".as_ref(),
            answers.as_os_str(),
        ],
        vec![
            "write".as_ref(),
            plate().into_os_string().leak(),
            analysis.as_os_str(),
            answers.as_os_str(),
            "-o".as_ref(),
            out.as_os_str(),
            "--accept-defaults".as_ref(),
        ],
    ];
    for args in runs {
        let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(run.status.code(), Some(2), "{args:?}");
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(stderr.contains(UNKNOWN), "{stderr}");
    }
    assert!(!out.exists());
}

#[test]
fn serve_refuses_an_unknown_id() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let out = temp("serve").join("out.step");
    let mut server = Server::default();
    for request in [
        json!({"id": 1, "op": "questions", "analysis": analysis, "answers": bogus()}),
        json!({"id": 2, "op": "preview", "analysis": analysis, "answers": bogus()}),
        json!({"id": 3, "op": "write", "step": plate(), "analysis": analysis,
               "answers": bogus(), "output": out, "accept_defaults": true}),
    ] {
        let reply = server.reply(&request);
        assert_eq!(reply["ok"], false, "{reply}");
        assert!(
            reply["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains(UNKNOWN)),
            "{reply}"
        );
    }
    assert!(!out.exists());
    // And a serve write lists an unasked answer like every other write.
    let reply = server.reply(&json!({"id": 4, "op": "write", "step": plate(),
        "analysis": analysis, "answers": revised("± dimensions"), "output": out,
        "accept_defaults": true}));
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(
        reply["result"]["report"]["not_written"]
            .as_array()
            .unwrap()
            .contains(&json!(not_written(
                "datum.A.flatness",
                "when part.locating is '± dimensions'"
            ))),
        "{reply}"
    );
}

/// A #121 carry block on `datum.A.flatness`, with the datum itself re-answered.
fn blocked(locating: &str) -> Map<String, Value> {
    let mut answers = revised(locating);
    answers.remove("datum.A.flatness");
    answers.insert(
        "_specify".into(),
        json!({"unresolved": {"datum.A.flatness": "its datum datum.A is unresolved"}}),
    );
    answers
}

/// A block applies only while its question is asked: under ± dimensions it does not stop
/// write (nor export, nor the editor's preview write) and is reported instead.
#[test]
fn a_carry_block_on_an_unasked_question_is_reported_not_refused() {
    let answers = blocked("± dimensions");
    let report = write(&answers, "blocked-dimensions");
    let line = "datum.A.flatness was carried (carried answer could not be applied: its datum \
                datum.A is unresolved) but is not asked when part.locating is '± dimensions', \
                so nothing was written for it";
    assert!(
        report.not_written.contains(&line.to_string()),
        "{:?}",
        report.not_written
    );
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let complete = specify_core::rules::with_all_defaults(&analysis, &answers).unwrap();
    let mut intent = api::preview(&analysis, &complete).unwrap();
    intent
        .requirements
        .extend(specify_core::rules::unasked_rows(&analysis, &complete).unwrap());
    let bytes = std::fs::read(plate()).unwrap();
    let (_, report) = api::write_bytes(bytes, &[(intent, Some(complete))]).unwrap();
    assert!(report.not_written.contains(&line.to_string()));
    // The export keeps the block, so it applies again if the question returns.
    let loaded = specify_core::part::load(&plate(), None).unwrap();
    let saved = api::export_answers(&loaded, &analysis, &answers).unwrap();
    assert_eq!(
        saved["_specify"]["unresolved"]["datum.A.flatness"],
        "its datum datum.A is unresolved"
    );
}

/// #128 with #126: an added requirement (`extra.*`) has no question, so its carry block is
/// never "unasked". Beside a block that is reported as unasked, it still refuses apply and
/// write, and names the requirement: a dropped requirement the person added is never lost.
#[test]
fn a_dropped_added_requirement_stays_refused_beside_an_unasked_block() {
    let mut answers = blocked("± dimensions");
    answers["_specify"]["unresolved"]
        .as_object_mut()
        .unwrap()
        .insert(
            "extra.gone.size".into(),
            json!("face 7 has no match in the new part"),
        );
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let refused = api::apply(&analysis, &answers, true)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("extra.gone.size"), "{refused}");
    assert!(!refused.contains("datum.A.flatness"), "{refused}");
    // The writer checks the answers itself, whatever intent it is given.
    let intent = api::preview(&analysis, &answers).unwrap();
    let bytes = std::fs::read(plate()).unwrap();
    let error = api::write_bytes(bytes, &[(intent, Some(answers.clone()))])
        .unwrap_err()
        .to_string();
    assert!(error.contains("extra.gone.size"), "{error}");
    // Answering it again on this part's faces clears it; the unasked block is then reported.
    answers.insert(
        "extra.gone.size".into(),
        json!({"faces": [2], "kind": "flatness", "value": "0.05"}),
    );
    let report = write(&answers, "added-cleared");
    assert!(
        report
            .not_written
            .iter()
            .any(|line| line.starts_with("datum.A.flatness was carried")),
        "{:?}",
        report.not_written
    );
}

/// Switching back to position tolerances asks the question again: the block applies again,
/// shows as required and stops bulk defaults and write, as #121 decided.
#[test]
fn a_carry_block_applies_again_when_its_question_returns() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    api::apply(&analysis, &blocked("± dimensions"), true).unwrap();
    let answers = blocked("position tolerances");
    let flatness = api::questions(&analysis, &answers)
        .unwrap()
        .into_iter()
        .find(|q| q.id == "datum.A.flatness")
        .unwrap();
    assert_eq!(flatness.attention, "required");
    assert_eq!(flatness.default, None);
    let refused = api::apply(&analysis, &answers, true)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("datum.A.flatness"), "{refused}");
    assert!(
        refused.contains("carried answer could not be applied"),
        "{refused}"
    );
    // Nor can a preview intent write past it.
    let intent = api::preview(&analysis, &answers).unwrap();
    let out = temp("blocked-position").join("out.step");
    let refused = api::write_file(&plate(), &intent, &out, Some(&answers)).unwrap_err();
    assert!(
        refused.to_string().contains("datum.A.flatness"),
        "{refused}"
    );
    assert!(!out.exists());
}

/// The review's sequence: write under ± dimensions with a block on `datum.A.flatness`, reopen
/// the STEP, switch to position tolerances, accept defaults. The block survives the STEP (beside
/// its stored answers, not among them) and refuses, rather than flatness taking "none".
#[test]
fn a_carry_block_survives_the_step_and_refuses_when_asked_again() {
    let report = write(&blocked("± dimensions"), "blocked-roundtrip");
    let out = report.output.clone();
    assert!(
        report
            .not_written
            .iter()
            .any(|l| l.starts_with("datum.A.flatness was carried"))
    );
    // The stored answers hold no `_specify`, which older builds refuse; the block is beside them.
    let stored = specify_core::resume::stored(&std::fs::read(&out).unwrap());
    let payload = Value::Object(stored[&0].clone());
    assert!(payload["answers"].get("_specify").is_none(), "{payload}");
    assert_eq!(
        payload["unresolved"],
        json!({"datum.A.flatness": "its datum datum.A is unresolved"})
    );
    let reopened = api::analyse_file(&out, None).unwrap();
    let mut answers = reopened["resume"]["answers"].as_object().unwrap().clone();
    assert_eq!(
        answers["_specify"]["unresolved"]["datum.A.flatness"],
        "its datum datum.A is unresolved"
    );
    answers.insert("part.locating".into(), json!("position tolerances"));
    let refused = api::apply(&reopened, &answers, true)
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("datum.A.flatness")
            && refused.contains("carried answer could not be applied"),
        "{refused}"
    );
    let flatness = api::questions(&reopened, &answers)
        .unwrap()
        .into_iter()
        .find(|q| q.id == "datum.A.flatness")
        .unwrap();
    assert_eq!(flatness.attention, "required");
}

/// A STEP written before unknown IDs were refused stores them verbatim. It still reopens and
/// diffs: the unknown ID is left open with a warning naming it, never resumed.
#[test]
fn a_stored_unknown_id_is_left_open_with_a_warning() {
    let dir = temp("stored-unknown");
    let answers = map(json!({"part.material": "6082-T6"}));
    let written = write(&answers, "stored-unknown-source").output;
    // As main wrote it: the stored answers carry the unknown ID verbatim. Inject it into this
    // write's stored payload (same faces, so the same evidence).
    let text = String::from_utf8(std::fs::read(&written).unwrap()).unwrap();
    let mut payload = Value::Object(specify_core::resume::stored(text.as_bytes())[&0].clone());
    let before = specify_core::resume::encode(&payload);
    payload["answers"]["bogus.question"] = json!("x");
    let after = specify_core::resume::encode(&payload);
    assert_eq!(text.matches(&before).count(), 1);
    let bytes = text.replace(&before, &after).into_bytes();
    let out = dir.join("main.step");
    std::fs::write(&out, &bytes).unwrap();
    assert_eq!(
        specify_core::resume::stored(&bytes)[&0]["answers"]["bogus.question"],
        "x"
    );

    let reopened = specify_core::part::load(&out, None).unwrap();
    let (analysis, warnings) = api::analyse_reporting(&reopened).unwrap();
    let resumed = analysis["resume"]["answers"].as_object().unwrap();
    assert!(!resumed.contains_key("bogus.question"), "{resumed:?}");
    assert_eq!(resumed["part.material"], "6082-T6");
    assert_eq!(analysis["resume"]["unresolved"][0]["id"], "bogus.question");
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("saved answer bogus.question: unknown answer ID")),
        "{warnings:?}"
    );
    api::questions(&analysis, resumed).unwrap();
    // diff OLD NEW without --answers reads the stored answers.
    let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .arg(&out)
        .arg(&out)
        .arg("-o")
        .arg(dir.join("changes"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
