//! ADR P9 data-loss regressions: a file Specify wrote and something else then changed, or
//! wrote with defaults it never stored, keeps that PMI when it is reopened and written again.
//! Both paths dropped PMI before P9: the editor's preview write (WASM `writePmi`) and the
//! CLI's `write` of the resumed answers.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use haecceity::p21::Document;
use haecceity::pmi::{self, PartId, PartPmi};
use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::existing::read_existing;
use specify_core::part::{LoadedPart, load_bytes};

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn answers(flatness: &str) -> Map<String, Value> {
    map(json!({
        "part.material": "6082-T6",
        "part.locating": "position tolerances",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": flatness,
        "datum.B.control": "0.1",
        "hole.position:holes:12": "0.1",
    }))
}

/// The plate written by Specify with `given` (defaults taken), storing only `given`.
fn written(given: &Map<String, Value>) -> Vec<u8> {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, given, true).unwrap();
    api::write_bytes(
        std::fs::read(plate()).unwrap(),
        &[(intent, Some(given.clone()))],
    )
    .unwrap()
    .0
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    load_bytes("plate.step", bytes).unwrap()
}

/// `bytes` with a flatness of 0.03 on face 2 added by another tool: haecceity's own writer,
/// so the tolerance carries no Specify provenance marker.
fn with_foreign_flatness(bytes: Vec<u8>) -> Vec<u8> {
    let source = written(&answers("0.03"));
    let defs = haecceity::step::read_part_definitions(&source).unwrap();
    let read = pmi::read(&Document::parse(source.clone()).unwrap(), &defs).unwrap();
    let mut flatness = read.parts[0]
        .tolerances
        .iter()
        .find(|t| t.kind == pmi::ToleranceKind::Flatness)
        .unwrap()
        .clone();
    flatness.description = None;
    let added = PartPmi {
        features: read.parts[0].features.clone(),
        tolerances: vec![flatness],
        ..Default::default()
    };
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(PartId(0), added)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

fn flatness(loaded: &LoadedPart) -> Vec<Value> {
    read_existing(loaded)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "geometric_tolerance" && e.r#type == "Flatness")
        .map(|e| json!({"faces": e.faces, "value": e.value, "marked": e.provenance.is_some()}))
        .collect()
}

fn count(loaded: &LoadedPart, kind: &str) -> usize {
    read_existing(loaded)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .count()
}

/// The editor's write (WASM `writePmi`): the preview of the answers plus the unasked rows.
fn preview_write(
    loaded: &LoadedPart,
    analysis: &Value,
    answers: &Map<String, Value>,
) -> LoadedPart {
    let mut intent = api::preview(analysis, answers).unwrap();
    intent
        .requirements
        .extend(specify_core::rules::unasked_rows(analysis, answers).unwrap());
    let bytes = specify_core::writer::write_bytes(loaded, &intent, Some(answers))
        .unwrap()
        .0;
    load(bytes)
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-p9-regressions")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli(args: &[&Path]) {
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The CLI's rewrite of a reopened file: `analyse`, then `write --accept-defaults` with the
/// resumed answers.
fn cli_rewrite(bytes: &[u8], name: &str) -> LoadedPart {
    let dir = dir(name);
    let input = dir.join("in.step");
    std::fs::write(&input, bytes).unwrap();
    let analysis = dir.join("analysis.json");
    cli(&[Path::new("analyse"), Path::new("-o"), &analysis, &input]);
    let analysed: Value = serde_json::from_slice(&std::fs::read(&analysis).unwrap()).unwrap();
    let answers = dir.join("answers.json");
    std::fs::write(&answers, analysed["resume"]["answers"].to_string()).unwrap();
    let output = dir.join("out.step");
    cli(&[
        Path::new("write"),
        Path::new("-o"),
        &output,
        Path::new("--accept-defaults"),
        &input,
        &analysis,
        &answers,
    ]);
    load(std::fs::read(output).unwrap())
}

/// Case 1: a bare part Specify wrote (a replace payload, no `pmi_scope`) to which another
/// tool added PMI. Reopened, the added PMI is the file's own (`existing`), and writing again,
/// by the editor or the CLI, keeps it: only Specify's marked requirements are replaced.
#[test]
fn pmi_another_tool_added_to_a_replace_written_part_is_kept() {
    let original = written(&answers("none"));
    let loaded = load(with_foreign_flatness(original));
    let added = flatness(&loaded);
    assert_eq!(
        added,
        [json!({"faces": [2], "value": 0.03, "marked": false})]
    );
    let analysis = api::analyse(&loaded).unwrap();
    assert!(analysis["resume"]["answers"].is_object());
    let resumed = analysis["resume"]["answers"].as_object().unwrap().clone();
    let editor = preview_write(&loaded, &analysis, &resumed);
    assert_eq!(flatness(&editor), added, "editor write");
    let cli = cli_rewrite(&loaded.bytes, "foreign");
    assert_eq!(flatness(&cli), added, "CLI write");
    // Shown on reopening as the file's own PMI.
    assert!(
        analysis["existing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "Flatness" && e["faces"] == json!([2]) && e["value"] == 0.03),
        "{}",
        analysis["existing"]
    );
    // Specify's own requirements are replaced, not stated twice.
    assert_eq!(count(&cli, "datum"), count(&loaded, "datum"));
    assert_eq!(
        count(&cli, "geometric_tolerance"),
        count(&loaded, "geometric_tolerance")
    );
}

/// Case 1, a file written before Specify marked its requirements (#22): none is marked, so
/// all its PMI is its answers' and a rewrite replaces it as before, stating none twice.
#[test]
fn an_unmarked_replace_written_part_is_rewritten_as_before() {
    let path = common::fixtures().join("legacy-schema1/legacy_specify.step");
    let loaded = load(std::fs::read(path).unwrap());
    assert!(
        read_existing(&loaded)
            .unwrap()
            .iter()
            .all(|e| e.provenance.is_none())
    );
    // Opened as before: its PMI is its answers', none of it the file's own.
    let analysis = api::analyse(&loaded).unwrap();
    assert_eq!(analysis["existing"], json!([]));
    assert!(analysis["resume"].get("conflicts").is_none());
    // Rewritten by replace from its answers: nothing of the old PMI is kept beside the new,
    // so nothing is stated twice (each datum letter once).
    let rewritten = cli_rewrite(&loaded.bytes, "unmarked");
    let letters: Vec<String> = read_existing(&rewritten)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "datum")
        .map(|e| e.r#type)
        .collect();
    let mut unique = letters.clone();
    unique.dedup();
    assert_eq!(letters, unique);
    assert!(
        read_existing(&rewritten)
            .unwrap()
            .iter()
            .all(|e| e.provenance.is_some()),
        "every requirement written again is Specify's own"
    );
}

/// Case 2: a file `write --accept-defaults` wrote, storing only the given answers. Reopened,
/// its defaulted requirements are present (not open questions, no conflict), and the editor's
/// write of the resumed answers keeps them.
#[test]
fn defaults_written_without_their_answers_are_kept() {
    let dir = dir("defaults");
    let given = dir.join("given.json");
    std::fs::write(&given, Value::Object(answers("0.05")).to_string()).unwrap();
    let analysis = dir.join("analysis.json");
    cli(&[Path::new("analyse"), Path::new("-o"), &analysis, &plate()]);
    let output = dir.join("out.step");
    cli(&[
        Path::new("write"),
        Path::new("-o"),
        &output,
        Path::new("--accept-defaults"),
        &plate(),
        &analysis,
        &given,
    ]);
    let loaded = load(std::fs::read(&output).unwrap());
    // Only the given answers are stored.
    assert_eq!(
        specify_core::resume::answers_for(&loaded).unwrap(),
        Some(answers("0.05"))
    );
    let pattern = "hole.position:hole_patterns.grid:6.7.8.10.11.13";
    let positions = |l: &LoadedPart| {
        read_existing(l)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces.contains(&6))
            .count()
    };
    assert_eq!(positions(&loaded), 1);
    let reopened = api::analyse(&loaded).unwrap();
    assert!(reopened["resume"].get("conflicts").is_none());
    let resumed = reopened["resume"]["answers"].as_object().unwrap().clone();
    let editor = preview_write(&loaded, &reopened, &resumed);
    assert_eq!(
        positions(&editor),
        1,
        "editor write keeps the defaulted position"
    );
    for kind in ["datum", "geometric_tolerance", "dimension", "note"] {
        assert_eq!(count(&editor, kind), count(&loaded, kind), "{kind}");
    }
    let review = api::review(&reopened, &resumed).unwrap();
    assert!(
        !review["open"].as_array().unwrap().contains(&json!(pattern)),
        "the defaulted position reads back as present: {}",
        review["open"]
    );
}

/// Case 1 through a session (the MCP `save`, as the page's Save writes): the NX-case file
/// reopened, one question answered, saved, and the other tool's flatness is still there.
#[test]
fn a_session_save_keeps_pmi_another_tool_added() {
    let dir = dir("session");
    let input = dir.join("nx.step");
    let loaded = load(with_foreign_flatness(written(&answers("none"))));
    let added = flatness(&loaded);
    std::fs::write(&input, loaded.bytes.as_slice()).unwrap();
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
    let doc = opened["doc"].clone();
    assert!(opened.get("conflicts").is_none(), "{opened}");
    call(
        "answer",
        json!({"doc": doc, "set": {"part.coating": "Anodise, clear"}}),
    );
    let saved = call("save", json!({"doc": doc, "accept_defaults": {}}));
    let back = load(std::fs::read(saved["path"].as_str().unwrap()).unwrap());
    assert_eq!(
        flatness(&back),
        added,
        "session save keeps the added flatness"
    );
    let reopened = api::analyse(&back).unwrap();
    assert_eq!(
        reopened["resume"]["answers"]["part.coating"],
        "Anodise, clear"
    );
}
