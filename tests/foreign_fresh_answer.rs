//! ADR P9: another tool's (unmarked) requirement is keep-only. A file Specify wrote with a hole
//! position of 0.5 is re-authored by another tool as 1.5: reopening lists a foreign conflict,
//! a save keeps 1.5 and drops the contradicted answer, and the next reopening has no
//! conflict. Answering 0.5 afresh then must not replace the other tool's 1.5, on the CLI's
//! path, the editor's (WASM session `writePmi`, tested in specify-core-wasm) and MCP's.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::existing::read_existing;
use specify_core::part::{LoadedPart, load_bytes};

const HOLE: &str = "hole.position:holes:12";

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    load_bytes("plate.step", bytes).unwrap()
}

fn answers(position: &str) -> Map<String, Value> {
    json!({
        "part.material": "6082-T6",
        "part.locating": "position tolerances",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.03",
        "datum.B.control": "0.1",
        (HOLE): position,
    })
    .as_object()
    .unwrap()
    .clone()
}

/// The plate as Specify wrote it with position 0.5 on hole 12, then another tool re-authored
/// that position as its own: provenance marker removed, value 1.5.
fn re_authored() -> LoadedPart {
    let given = answers("0.5");
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, &given, true).unwrap();
    let bytes = api::write_bytes(std::fs::read(plate()).unwrap(), &[(intent, Some(given))])
        .unwrap()
        .0;
    let text = String::from_utf8(bytes).unwrap();
    // The position tolerance without modifiers (the pattern's carries MMC).
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
    let text = text
        .replace(&line, &line.replace("'specify-core:written:v1'", "''"))
        .replace(&value, &value.replace("(0.5)", "(1.5)"));
    let loaded = load(text.into_bytes());
    assert_eq!(positions(&loaded), [(1.5, false)]);
    loaded
}

/// The hole's position tolerances: value and whether Specify's marker is on it.
pub fn positions(l: &LoadedPart) -> Vec<(f64, bool)> {
    read_existing(l)
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == "Position" && e.faces == [12])
        .map(|e| (e.value.unwrap(), e.provenance.is_some()))
        .collect()
}

fn assert_foreign_conflict(analysis: &Value) {
    assert!(
        analysis["resume"]["conflicts"]
            .as_array()
            .is_some_and(|c| c.iter().any(|c| c["id"] == HOLE && c["foreign"] == true)),
        "{}",
        analysis["resume"]
    );
}

fn assert_no_conflict(analysis: &Value) {
    assert!(
        analysis["resume"].get("conflicts").is_none(),
        "{}",
        analysis["resume"]
    );
    assert!(
        analysis["resume"]["answers"].get(HOLE).is_none(),
        "{}",
        analysis["resume"]
    );
}

fn assert_held(report: &Value) {
    let lines: Vec<&str> = report["not_written"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        lines.iter().any(|l| l.starts_with(HOLE)
            && l.contains("another tool's requirement holds this question")
            && l.contains("keep it, or change it in that tool")),
        "{report}"
    );
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-foreign-fresh-answer")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli(args: &[&Path]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or(Value::Null)
}

/// `analyse`, then `write --accept-defaults` of `answers` (the resumed ones when `None`).
fn cli_write(
    bytes: &[u8],
    answers: Option<&Map<String, Value>>,
    name: &str,
) -> (LoadedPart, Value) {
    let dir = dir(name);
    let input = dir.join("in.step");
    std::fs::write(&input, bytes).unwrap();
    let analysis = dir.join("analysis.json");
    cli(&[Path::new("analyse"), Path::new("-o"), &analysis, &input]);
    let analysed: Value = serde_json::from_slice(&std::fs::read(&analysis).unwrap()).unwrap();
    let given = answers.map_or(analysed["resume"]["answers"].clone(), |a| {
        Value::Object(a.clone())
    });
    let file = dir.join("answers.json");
    std::fs::write(&file, given.to_string()).unwrap();
    let output = dir.join("out.step");
    let report = cli(&[
        Path::new("write"),
        Path::new("-o"),
        &output,
        Path::new("--accept-defaults"),
        &input,
        &analysis,
        &file,
    ]);
    (load(std::fs::read(output).unwrap()), report)
}

/// Conflict, save without deciding, reopen (no conflict), answer 0.5, save: 1.5 stays alone.
#[test]
fn a_fresh_answer_does_not_replace_another_tools_requirement_on_the_cli() {
    let edited = re_authored();
    assert_foreign_conflict(&api::analyse(&edited).unwrap());
    let (saved, _) = cli_write(&edited.bytes, None, "save");
    assert_eq!(positions(&saved), [(1.5, false)]);
    let reopened = api::analyse(&saved).unwrap();
    assert_no_conflict(&reopened);
    let mut given = reopened["resume"]["answers"].as_object().unwrap().clone();
    given.insert(HOLE.into(), json!("0.5"));
    let (answered, report) = cli_write(&saved.bytes, Some(&given), "answer");
    assert_eq!(positions(&answered), [(1.5, false)], "{report}");
    assert_held(&report);
}

/// The same after the person kept the file's 1.5 (the editor's only choice for a foreign
/// conflict): the Keep grants no permission to replace it later.
#[test]
fn a_fresh_answer_after_keeping_does_not_replace_another_tools_requirement() {
    let edited = re_authored();
    let analysis = api::analyse(&edited).unwrap();
    assert_foreign_conflict(&analysis);
    let mut kept = analysis["resume"]["answers"].as_object().unwrap().clone();
    kept.insert("_specify".into(), json!({"keep": [HOLE]}));
    let (saved, _) = cli_write(&edited.bytes, Some(&kept), "keep");
    assert_eq!(positions(&saved), [(1.5, false)]);
    let reopened = api::analyse(&saved).unwrap();
    assert_no_conflict(&reopened);
    let mut given = reopened["resume"]["answers"].as_object().unwrap().clone();
    given.insert(HOLE.into(), json!("0.5"));
    let (answered, report) = cli_write(&saved.bytes, Some(&given), "keep-answer");
    assert_eq!(positions(&answered), [(1.5, false)], "{report}");
    assert_held(&report);
}

/// The same over MCP: `open`, `save`, then reopen, `answer` 0.5 and `save`.
#[test]
fn a_fresh_answer_does_not_replace_another_tools_requirement_over_mcp() {
    let dir = dir("mcp");
    let input = dir.join("plate.step");
    std::fs::write(&input, re_authored().bytes.as_slice()).unwrap();
    let mut server = specify_core::mcp::Server::new(std::slice::from_ref(&dir)).unwrap();
    let mut call = |name: &str, arguments: Value| -> Value {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": name, "arguments": arguments,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": specify_core::mcp::MODERN,
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }});
        let result = server.handle(&request, None).body.unwrap()["result"].clone();
        assert_ne!(result["isError"], json!(true), "{name}: {result}");
        result["structuredContent"].clone()
    };
    let opened = call("open", json!({"path": input}));
    assert!(
        opened["conflicts"]
            .as_array()
            .is_some_and(|c| c.iter().any(|c| c["id"] == HOLE)),
        "{opened}"
    );
    let saved = call("save", json!({"doc": opened["doc"], "accept_defaults": {}}));
    let path = saved["path"].as_str().unwrap().to_string();
    let back = load(std::fs::read(&path).unwrap());
    assert_eq!(positions(&back), [(1.5, false)]);
    assert_no_conflict(&api::analyse(&back).unwrap());
    let reopened = call("open", json!({"path": path}));
    call(
        "answer",
        json!({"doc": reopened["doc"], "set": {(HOLE): "0.5"}}),
    );
    let saved = call(
        "save",
        json!({"doc": reopened["doc"], "accept_defaults": {}}),
    );
    let back = load(std::fs::read(saved["path"].as_str().unwrap()).unwrap());
    assert_eq!(positions(&back), [(1.5, false)], "{saved}");
    assert_held(&saved["report"]);
}
