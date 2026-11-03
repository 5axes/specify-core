//! The child-process protocol a host such as a web app's server speaks: specify-core's
//! `tests/test_serve.py`, against the built binary over pipes (`specify-core serve`), plus the
//! protocol's promises: every stdout line is a reply, malformed lines are answered and the
//! child carries on, and the child exits 0 when stdin closes.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use specify_core::serve::Server;

fn part(stem: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{stem}.step"))
}

#[test]
fn full_thread_depth_is_exposed_and_edited_over_serve() {
    let analysis = specify_core::api::analyse_file(&part("GRM-02_shaft"), None).unwrap();
    let mut child = Session::start();
    let asked = child.ask(&json!({"id":1, "op":"questions", "analysis":analysis, "answers":{}}));
    assert_eq!(asked["ok"], true, "{asked}");
    let q = asked["result"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"].as_str().unwrap().starts_with("hole.full_thread:"))
        .unwrap();
    let id = q["id"].as_str().unwrap();
    assert_eq!(q["default"], "2");
    assert_eq!(q["kind"], "text");
    let function = id.replace("hole.full_thread:", "hole.function:");
    let reply = child.ask(&json!({"id":2, "op":"preview", "analysis":analysis, "answers":{function:"tapped:M2", id:"1.8"}}));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(
        reply["result"]["intent"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap()["full_thread"],
        1.8
    );
    child.finish();
}

/// A `specify-core serve` child, asked one line at a time.
struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("serve starts");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Session {
            child,
            stdin,
            stdout,
        }
    }

    /// Sends `line` and reads one reply line, which must be JSON (every stdout line is a reply).
    fn line(&mut self, line: &str) -> Value {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut reply = String::new();
        self.stdout.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap_or_else(|e| panic!("not a reply: {reply:?}: {e}"))
    }

    fn ask(&mut self, request: &Value) -> Value {
        self.line(&request.to_string())
    }

    /// Closes stdin; the child must exit 0 with nothing more on stdout.
    fn finish(mut self) {
        drop(self.stdin.take());
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut self.stdout, &mut rest).unwrap();
        assert_eq!(rest, "", "nothing but replies on stdout");
        assert!(self.child.wait().unwrap().success());
    }
}

/// test_serve.py::test_a_session_over_the_pipe
#[test]
fn a_session_over_the_pipe() {
    let dir = std::env::temp_dir().join(format!("specify-core-serve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Session::start();
    assert_eq!(
        child.ask(&json!({"id": 1, "op": "hello"})),
        json!({"id": 1, "ok": true, "result": {"protocol": 1}})
    );
    let analysed = child.ask(&json!({"id": 2, "op": "analyse", "step": part("plate")}));
    assert_eq!(analysed["ok"], json!(true), "{analysed}");
    let analysis = analysed["result"]["analysis"].clone();
    assert_eq!(analysed["result"]["mesh"]["binding"], analysis["binding"]);

    let questions =
        child.ask(&json!({"id": 3, "op": "questions", "analysis": analysis, "answers": {}}));
    assert!(
        questions["result"]["open"]
            .as_array()
            .unwrap()
            .contains(&json!("part.material"))
    );
    assert!(questions["result"]["problems"].is_array());

    let answers = json!({"part.material": "6082-T6"});
    let preview =
        child.ask(&json!({"id": 4, "op": "preview", "analysis": analysis, "answers": answers}));
    assert_eq!(
        preview["result"]["intent"]["part"]["material"],
        json!("6082-T6")
    );

    let out = dir.join("plate-pmi.step");
    let written = child.ask(&json!({
        "id": 5, "op": "write", "step": part("plate"), "analysis": analysis,
        "answers": answers, "output": out, "accept_defaults": true,
    }));
    assert_eq!(written["ok"], json!(true), "{written}");
    assert!(
        written["result"]["report"]["written"]
            .as_array()
            .unwrap()
            .contains(&json!("material 6082-T6"))
    );
    assert_eq!(
        written["result"]["intent"]["part"]["material"],
        json!("6082-T6")
    );
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .starts_with("ISO-10303-21")
    );
    child.finish();
    let _ = std::fs::remove_dir_all(&dir);
}

/// test_serve.py::test_errors_are_structured_and_the_child_survives
#[test]
fn errors_are_structured_and_the_child_survives() {
    let mut child = Session::start();
    let bad = child.ask(&json!({
        "id": 6, "op": "questions", "analysis": {"faces": [], "features": []},
        "answers": {"datum.A": [5]},
    }));
    assert_eq!(bad["ok"], json!(false));
    assert_eq!(bad["error"]["code"], json!("invalid"));
    assert_eq!(bad["id"], json!(6));
    assert_eq!(
        child.ask(&json!({"id": 7, "op": "hello"}))["ok"],
        json!(true)
    );
    child.finish();
}

/// test_serve.py::test_unanswered_questions_are_listed
#[test]
fn unanswered_questions_are_listed() {
    let out = Server::default().reply(&json!({
        "id": 8, "op": "write", "step": "x",
        "analysis": {"binding": {}, "faces": [], "features": [], "existing": []},
        "answers": {}, "output": "y",
    }));
    assert_eq!(out["error"]["code"], json!("incomplete"));
    assert!(
        out["error"]["missing"]
            .as_array()
            .unwrap()
            .contains(&json!("part.material"))
    );
    // A single part's refusal names no part.
    assert!(out["error"].get("part").is_none());
}

/// test_serve.py::test_a_crash_inside_one_request_is_reported
#[test]
fn a_crash_inside_one_request_is_reported() {
    let out =
        Server::default().reply(&json!({"id": 9, "op": "analyse", "step": "/no/such/file.step"}));
    assert_eq!(out["ok"], json!(false));
    assert!(["failed", "invalid"].contains(&out["error"]["code"].as_str().unwrap()));
}

/// test_serve.py::test_a_line_that_is_not_an_object_is_refused_and_the_child_survives, and a
/// line that is not JSON at all, and blank lines (no reply).
#[test]
fn a_line_that_is_not_an_object_is_refused_and_the_child_survives() {
    let mut child = Session::start();
    for line in ["[]", "null", "3", "\"hello\"", "{", "not json"] {
        let out = child.line(line);
        assert_eq!(out["ok"], json!(false), "{line}: {out}");
        assert_eq!(out["error"]["code"], json!("invalid"), "{line}: {out}");
        assert_eq!(out["id"], json!(null));
    }
    // A blank line gets no reply: the next reply is the hello's.
    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "   ").unwrap();
    assert_eq!(
        child.ask(&json!({"id": 9, "op": "hello"}))["result"]["protocol"],
        json!(1)
    );
    child.finish();
}

/// A missing field is answered as Python's KeyError reads, and an unknown op is named.
#[test]
fn missing_fields_and_unknown_ops_are_invalid() {
    let mut server = Server::default();
    let out = server.reply(&json!({"id": 1, "op": "parts"}));
    assert_eq!(
        out["error"],
        json!({"code": "invalid", "message": "missing field 'step'"})
    );
    let out = server.reply(&json!({"id": 2}));
    assert_eq!(
        out["error"],
        json!({"code": "invalid", "message": "missing field 'analysis'"})
    );
    let out = server.reply(&json!({"id": 3, "op": "bogus", "analysis": {}}));
    assert_eq!(
        out["error"],
        json!({"code": "invalid", "message": "unknown op 'bogus'"})
    );
}

/// `interpret` reads what was typed for a choice question as the answer it means, with the
/// call-out for a hole.
#[test]
fn what_was_typed_is_interpreted() {
    let analysis = specify_core::api::analyse_file(&part("plate"), None).unwrap();
    let qs = specify_core::api::questions(&analysis, &Default::default()).unwrap();
    let q = qs
        .iter()
        .find(|q| q.id.starts_with("hole.function:") && q.default == Some(json!("tapped:M5")))
        .expect("the plate's tapped hole");
    let out = Server::default().reply(&json!({
        "id": 1, "op": "interpret", "analysis": analysis, "answers": {},
        "question": q.id, "text": "M5",
    }));
    assert_eq!(
        out["result"],
        json!({"value": "tapped:M5", "code": "M5 tapped", "callout": "M5×0.8-6H, ⌀4.2 tap drill"}),
        "{out}"
    );
}

#[test]
fn accept_defaults_cannot_replace_a_dropped_carried_datum() {
    let old = common::fixtures().join("revisions/plate_split.old.step.gz");
    let new = common::fixtures().join("revisions/plate_split.new.step.gz");
    let before = specify_core::part::load(&old, None).unwrap();
    let after = specify_core::part::load(&new, None).unwrap();
    let given = json!({"part.material":"6061-T6", "datum.A":[4]});
    let revised =
        specify_core::revision::diff(&before, &after, given.as_object().unwrap()).unwrap();
    let analysis = specify_core::api::analyse(&after).unwrap();
    let output =
        std::env::temp_dir().join(format!("serve-dropped-datum-{}.step", std::process::id()));
    let _ = std::fs::remove_file(&output);
    let mut server = Server::default();
    let mut request = json!({"id":1, "op":"write", "step":new, "output":output,
        "analysis":analysis, "answers":revised.answers, "accept_defaults":true});
    let asked = server
        .reply(&json!({"id":2,"op":"questions","analysis":analysis,"answers":request["answers"]}));
    assert_eq!(asked["ok"], true, "{asked}");
    let q = asked["result"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "datum.A")
        .unwrap();
    assert_eq!(q["default"], json!(null));
    assert_eq!(q["attention"], "required");
    assert!(
        q["reason"]
            .as_str()
            .unwrap()
            .contains("face 4 has no match in the new part")
    );
    for multiple in [false, true] {
        let mut req = request.clone();
        if multiple {
            req["parts"] = json!([{"analysis":req["analysis"], "answers":req["answers"]}]);
        }
        let reply = server.reply(&req);
        assert_eq!(reply["ok"], false, "{reply}");
        assert_eq!(reply["error"]["code"], "incomplete", "{reply}");
        assert!(
            reply["error"]["message"]
                .as_str()
                .unwrap()
                .contains("datum.A (carried answer could not be applied:")
        );
        assert!(!output.exists());
    }
    request["answers"]["datum.A"] = json!([4, 8]);
    let reply = server.reply(&request);
    assert_eq!(reply["ok"], true, "{reply}");
    std::fs::remove_file(output).unwrap();
}

#[test]
fn serve_write_refuses_saved_answers_on_changed_geometry() {
    let old = specify_core::part::load(
        &common::fixtures().join("revisions/plate_split.old.step.gz"),
        None,
    )
    .unwrap();
    let new_path = common::fixtures().join("revisions/plate_split.new.step.gz");
    let new = specify_core::part::load(&new_path, None).unwrap();
    let given = json!({"part.material":"6061-T6", "datum.A":[4]});
    let saved = specify_core::revision::diff(&old, &old, given.as_object().unwrap())
        .unwrap()
        .answers;
    let analysis = specify_core::api::analyse(&new).unwrap();
    let output =
        std::env::temp_dir().join(format!("serve-stale-evidence-{}.step", std::process::id()));
    let _ = std::fs::remove_file(&output);
    let mut server = Server::default();
    let reply = server.reply(&json!({"id":1,"op":"write","step":new_path,
        "output":output,"analysis":analysis,"answers":saved,"accept_defaults":true}));
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"]["code"], "invalid", "{reply}");
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("stale answers")
    );
    assert!(!output.exists());
}
