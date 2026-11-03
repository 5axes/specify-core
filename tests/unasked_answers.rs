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

/// The editor writes its preview intent (WASM `writePmi`); the row reaches that report too, and
/// the answer stays stored so the question takes it again when it returns.
#[test]
fn a_preview_write_lists_it_and_keeps_the_answer() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let mut answers =
        specify_core::rules::with_all_defaults(&analysis, &revised("± dimensions")).unwrap();
    let intent = api::preview(&analysis, &answers).unwrap();
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
/// callers that revise answers across a change of mode.
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
