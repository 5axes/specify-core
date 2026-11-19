//! ADR P9 data-loss regressions: a file Specify wrote and something else then changed, or
//! wrote with defaults it never stored, keeps that PMI when it is reopened and written again.
//! Both paths dropped PMI before P9: the editor's preview write (WASM `writePmi`) and the
//! CLI's `write` of the resumed answers.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use haecceity::pmi::{self, PartId, PartPmi};
use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::existing::read_existing;
use specify_core::part::{LoadedPart, load_bytes};
use specify_core::process;

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

/// Change the UDA item in place, optionally deleting the provenance on its property link.
fn edited_process(marked: bool) -> LoadedPart {
    let mut given = answers("0.05");
    given.insert(process::QUESTION.into(), json!(process::MACHINED));
    change_process(written(&given), marked)
}

fn change_process(bytes: Vec<u8>, marked: bool) -> LoadedPart {
    let original = String::from_utf8(bytes).unwrap();
    let item = |value| {
        format!(
            "DESCRIPTIVE_REPRESENTATION_ITEM('{}','{value}')",
            process::ITEM
        )
    };
    assert_eq!(original.matches(&item(process::MACHINED)).count(), 1);
    let bytes = original
        .replace(&item(process::MACHINED), &item(process::CAST))
        .into_bytes();
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let i = read.parts[0]
        .attributes
        .iter()
        .position(|set| set.name == process::PROPERTY)
        .unwrap();
    let ids = &read.provenance.parts[0].attributes[i];
    let markers: Vec<_> = ids.iter().flat_map(|&id| doc.referrers(id)).copied().filter(|&id| {
        matches!(doc.get(id), Some(RawEntity::Simple {name, attributes, ..})
            if name == "DESCRIPTION_ATTRIBUTE"
                && matches!(attributes.first(), Some(Attribute::String(s)) if s == "specify-core:written:v1"))
    }).collect();
    assert_eq!(markers.len(), 1);
    if marked {
        return load(doc.bytes().to_vec());
    }
    let mut edit = Edit::new();
    edit.remove(markers[0]);
    load(doc.apply(&edit).unwrap().bytes)
}

fn assert_process(loaded: &LoadedPart, expected: &str) {
    let doc = Document::parse(loaded.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let sets: Vec<_> = read.parts[0]
        .attributes
        .iter()
        .filter(|set| set.name == process::PROPERTY)
        .collect();
    assert_eq!(
        sets.len(),
        1,
        "expected exactly one process UDA with {expected}"
    );
    assert_eq!(process::read_pmi(&read.parts[0]).unwrap().value, expected);
}

fn process_conflict(analysis: &Value) -> Option<&Value> {
    analysis["resume"]["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|conflict| conflict["id"] == process::QUESTION)
}

#[test]
fn externally_edited_process_is_a_conflict_on_reopening() {
    for marked in [true, false] {
        let loaded = edited_process(marked);
        assert_process(&loaded, process::CAST);
        let analysis = api::analyse(&loaded).unwrap();
        let conflict =
            process_conflict(&analysis).expect("edited process conflicts with saved answer");
        assert_eq!(conflict["stored"], process::MACHINED);
        assert_eq!(conflict["file"], process::CAST);
        assert_eq!(conflict["foreign"].as_bool().unwrap_or(false), !marked);
        assert_eq!(analysis["part"]["process"], process::CAST);
        assert!(
            analysis["resume"]["answers"]
                .get(process::QUESTION)
                .is_none()
        );
    }
}

#[test]
fn cli_keeps_a_process_edited_with_provenance() {
    assert_process(
        &cli_rewrite(&edited_process(true).bytes, "marked-process"),
        process::CAST,
    );
}

#[test]
fn cli_keeps_a_process_edited_without_provenance() {
    assert_process(
        &cli_rewrite(&edited_process(false).bytes, "unmarked-process"),
        process::CAST,
    );
}

#[test]
fn editor_keeps_a_process_edited_with_provenance() {
    editor_keeps_edited_process(true);
}

#[test]
fn editor_keeps_a_process_edited_without_provenance() {
    editor_keeps_edited_process(false);
}

fn editor_keeps_edited_process(marked: bool) {
    let loaded = edited_process(marked);
    let analysis = api::analyse(&loaded).unwrap();
    let mut resumed = map(analysis["resume"]["answers"].clone());
    resumed.remove(process::QUESTION);
    let written = preview_write(&loaded, &analysis, &resumed);
    assert_process(&written, process::CAST);
    let reopened = api::analyse(&written).unwrap();
    assert!(process_conflict(&reopened).is_none());
    assert_eq!(reopened["part"]["process"], process::CAST);
    assert_eq!(
        reopened["resume"]["file"]
            .as_array()
            .is_some_and(|ids| ids.contains(&json!(process::QUESTION))),
        marked,
    );
    assert_eq!(
        reopened["process"]["source"],
        if marked { "answered" } else { "file" },
    );
}

#[test]
fn a_process_conflict_on_a_scoped_file_keeps_the_foreign_uda() {
    let loaded = load(with_foreign_flatness(std::fs::read(plate()).unwrap()));
    let analysis = api::analyse(&loaded).unwrap();
    let given = map(json!({"part.material": "6082-T6", "part.process": process::MACHINED}));
    let written = preview_write(&loaded, &analysis, &given);
    assert_eq!(
        specify_core::resume::stored(&written.bytes)[&0]["pmi_scope"],
        specify_core::writer::PMI_PROVENANCE,
    );
    let edited = change_process(written.bytes.to_vec(), false);
    let analysis = api::analyse(&edited).unwrap();
    assert_eq!(process_conflict(&analysis).unwrap()["foreign"], true);
    // A stale editor/answers file may still send the saved answer in full.
    let written = preview_write(&edited, &analysis, &given);
    assert_process(&written, process::CAST);
    let reopened = api::analyse(&written).unwrap();
    assert!(process_conflict(&reopened).is_none());
    assert!(
        reopened["resume"]["answers"]
            .get(process::QUESTION)
            .is_none()
    );
}

#[test]
fn a_marked_process_conflict_can_be_replaced_before_or_after_keeping() {
    let edited = edited_process(true);
    let analysis = api::analyse(&edited).unwrap();
    let mut resumed = map(analysis["resume"]["answers"].clone());
    resumed.insert(process::QUESTION.into(), json!(process::MOULDED));
    assert_process(
        &preview_write(&edited, &analysis, &resumed),
        process::MOULDED,
    );

    resumed.remove(process::QUESTION);
    resumed.insert("_specify".into(), json!({"keep": [process::QUESTION]}));
    let kept = preview_write(&edited, &analysis, &resumed);
    assert_process(&kept, process::CAST);
    let reopened = api::analyse(&kept).unwrap();
    assert_eq!(reopened["process"]["source"], "file");
    assert!(process_conflict(&reopened).is_none());
    let mut resumed = map(reopened["resume"]["answers"].clone());
    assert_eq!(resumed["_specify"]["keep"], json!([process::QUESTION]));
    resumed.insert(process::QUESTION.into(), json!(process::MOULDED));
    assert_process(&preview_write(&kept, &reopened, &resumed), process::MOULDED);
}

#[test]
fn an_external_edit_after_keeping_an_owned_process_has_its_own_provenance() {
    let edited = edited_process(true);
    let analysis = api::analyse(&edited).unwrap();
    let mut resumed = map(analysis["resume"]["answers"].clone());
    resumed.insert("_specify".into(), json!({"keep": [process::QUESTION]}));
    let kept = preview_write(&edited, &analysis, &resumed);
    assert_process(&kept, process::CAST);

    let text = String::from_utf8(kept.bytes.to_vec()).unwrap();
    let item = |value| {
        format!(
            "DESCRIPTIVE_REPRESENTATION_ITEM('{}','{value}')",
            process::ITEM
        )
    };
    assert_eq!(text.matches(&item(process::CAST)).count(), 1);
    let changed = load(
        text.replace(&item(process::CAST), &item(process::SHEET_METAL))
            .into_bytes(),
    );
    let analysis = api::analyse(&changed).unwrap();
    let mut resumed = map(analysis["resume"]["answers"].clone());
    resumed.insert(process::QUESTION.into(), json!(process::MOULDED));
    assert_process(
        &preview_write(&changed, &analysis, &resumed),
        process::SHEET_METAL,
    );
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
    // The given answers are stored, and of the defaults taken only those that write no PMI
    // (2026-11-19: "accept all" keeps answers the file cannot state).
    let mut stored = answers("0.05");
    for (id, value) in [
        ("part.coating", json!("None (as machined)")),
        ("part.heat_treatment", json!("None")),
        ("part.notes", json!([])),
        ("hole.function:holes:9", json!("general")),
        ("datum.C.control", json!("none")),
    ] {
        stored.insert(id.into(), value);
    }
    assert_eq!(
        specify_core::resume::answers_for(&loaded).unwrap(),
        Some(stored)
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
    // The imported flatness withholds "none" and follows ordinary file-PMI handling.
    let analysis = api::analyse(&loaded).unwrap();
    assert!(
        analysis["resume"]["answers"]
            .get("datum.A.flatness")
            .is_none()
    );
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

/// A case-2 file whose defaulted pattern position another tool changed (0.6 → 0.3), keeping
/// Specify's marker: the file answers that question (`resume.file`). It is not a conflict,
/// it is not asked, its default is not restated, and the editor's and the CLI's writes keep
/// 0.3, written again and again.
#[test]
fn a_default_another_tool_changed_is_the_files_answer() {
    let given = answers("0.05");
    let text = String::from_utf8(written(&given)).unwrap();
    // The pattern's position zone, written as Specify writes it.
    let zone = "LENGTH_MEASURE(0.6)";
    assert_eq!(text.matches(zone).count(), 1, "one 0.6 zone");
    let loaded = load(text.replace(zone, "LENGTH_MEASURE(0.3)").into_bytes());
    let pattern = "hole.position:hole_patterns.grid:6.7.8.10.11.13";
    let positions = |l: &LoadedPart| -> Vec<f64> {
        read_existing(l)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces.contains(&6))
            .map(|e| e.value.unwrap())
            .collect()
    };
    assert_eq!(positions(&loaded), [0.3]);
    let reopened = |l: &LoadedPart| {
        let analysis = api::analyse(l).unwrap();
        assert!(
            analysis["resume"].get("conflicts").is_none(),
            "{}",
            analysis["resume"]
        );
        assert!(
            analysis["resume"]["file"]
                .as_array()
                .unwrap()
                .contains(&json!(pattern)),
            "{}",
            analysis["resume"]
        );
        let resumed = analysis["resume"]["answers"].as_object().unwrap().clone();
        let review = api::review(&analysis, &resumed).unwrap();
        assert!(
            !review["questions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|q| q["id"] == pattern),
            "the file's position is not asked"
        );
        (analysis, resumed)
    };
    let (analysis, resumed) = reopened(&loaded);
    let editor = preview_write(&loaded, &analysis, &resumed);
    assert_eq!(positions(&editor), [0.3], "editor write");
    let (again, resumed) = reopened(&editor);
    let second = preview_write(&editor, &again, &resumed);
    assert_eq!(positions(&second), [0.3], "editor write, again");
    let cli = cli_rewrite(&second.bytes, "file-answer");
    assert_eq!(positions(&cli), [0.3], "CLI write");
    reopened(&cli);
    let cli_again = cli_rewrite(&cli.bytes, "file-answer-again");
    assert_eq!(positions(&cli_again), [0.3], "CLI write, again");
}

/// A conflict kept through the editor's write (WASM `writePmi`): the kept requirement is
/// written as the file's own, and survives a reopening and another editor write.
#[test]
fn a_conflict_kept_through_the_editor_lasts() {
    // A file Specify wrote whose hole position (stored 0.1) was changed to 0.2 elsewhere.
    let loaded = specify_core::part::load(
        &common::fixtures().join("resume-p9/edited-plate.step.gz"),
        None,
    )
    .unwrap();
    let hole = "hole.position:holes:12";
    let positions = |l: &LoadedPart| -> Vec<(f64, bool)> {
        read_existing(l)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces == [12])
            .map(|e| (e.value.unwrap(), e.provenance.is_some()))
            .collect()
    };
    let analysis = api::analyse(&loaded).unwrap();
    assert!(
        analysis["resume"]["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == hole),
        "{}",
        analysis["resume"]
    );
    let mut kept = analysis["resume"]["answers"].as_object().unwrap().clone();
    kept.insert(
        "_specify".into(),
        json!({"keep": [hole, "datum.A.flatness"]}),
    );
    let first = preview_write(&loaded, &analysis, &kept);
    assert_eq!(positions(&first), [(0.2, false)]);
    let again = api::analyse(&first).unwrap();
    assert!(
        again["resume"].get("conflicts").is_none(),
        "{}",
        again["resume"]
    );
    let resumed = again["resume"]["answers"].as_object().unwrap().clone();
    let second = preview_write(&first, &again, &resumed);
    assert_eq!(positions(&second), [(0.2, false)]);
}

/// The CLI's write of `answers` on a file: `analyse`, then `write --accept-defaults`.
fn cli_write(bytes: &[u8], answers: &Map<String, Value>, name: &str) -> LoadedPart {
    let dir = dir(name);
    let input = dir.join("in.step");
    std::fs::write(&input, bytes).unwrap();
    let analysis = dir.join("analysis.json");
    cli(&[Path::new("analyse"), Path::new("-o"), &analysis, &input]);
    let given = dir.join("answers.json");
    std::fs::write(&given, Value::Object(answers.clone()).to_string()).unwrap();
    let output = dir.join("out.step");
    cli(&[
        Path::new("write"),
        Path::new("-o"),
        &output,
        Path::new("--accept-defaults"),
        &input,
        &analysis,
        &given,
    ]);
    load(std::fs::read(output).unwrap())
}

fn hole_positions(l: &LoadedPart) -> Vec<f64> {
    read_existing(l)
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == "Position" && e.faces == [12])
        .map(|e| e.value.unwrap())
        .collect()
}

/// Keep, write, then change one's mind: answering the kept question writes the new value and
/// the kept requirement gives way, on the editor's path and the CLI's, so the file holds one
/// requirement with the new value and reopens with no conflict.
#[test]
fn answering_a_kept_question_replaces_the_kept_requirement() {
    let edited = specify_core::part::load(
        &common::fixtures().join("resume-p9/edited-plate.step.gz"),
        None,
    )
    .unwrap();
    let hole = "hole.position:holes:12";
    for editor in [true, false] {
        let analysis = api::analyse(&edited).unwrap();
        let mut answers = analysis["resume"]["answers"].as_object().unwrap().clone();
        answers.insert("_specify".into(), json!({"keep": [hole]}));
        let kept = if editor {
            preview_write(&edited, &analysis, &answers)
        } else {
            cli_write(&edited.bytes, &answers, "keep")
        };
        assert_eq!(hole_positions(&kept), [0.2], "kept (editor: {editor})");
        // Reopened and answered anew.
        let reopened = api::analyse(&kept).unwrap();
        assert!(reopened["resume"].get("conflicts").is_none());
        let mut answers = reopened["resume"]["answers"].as_object().unwrap().clone();
        answers.insert(hole.into(), json!("0.1"));
        let answered = if editor {
            preview_write(&kept, &reopened, &answers)
        } else {
            cli_write(&kept.bytes, &answers, "answer")
        };
        assert_eq!(
            hole_positions(&answered),
            [0.1],
            "answered (editor: {editor})"
        );
        let again = api::analyse(&answered).unwrap();
        assert!(
            again["resume"].get("conflicts").is_none(),
            "{}",
            again["resume"]
        );
        assert_eq!(again["resume"]["answers"][hole], "0.1");
    }
}

/// A file Specify wrote (flatness 0.03 on face 2, stored as answered) whose flatness another
/// tool re-authored as its own: unmarked, 0.05.
fn re_authored_flatness() -> LoadedPart {
    let text = String::from_utf8(written(&answers("0.03"))).unwrap();
    let line = text
        .lines()
        .find(|l| l.contains("FLATNESS_TOLERANCE("))
        .unwrap()
        .to_string();
    let measure = line.split(',').nth(2).unwrap().trim().to_string();
    let value = text
        .lines()
        .find(|l| l.starts_with(&format!("{measure}=")))
        .unwrap()
        .to_string();
    assert!(line.contains("'specify-core:written:v1'") && value.contains("(0.03)"));
    let text = text
        .replace(&line, &line.replace("'specify-core:written:v1'", "''"))
        .replace(&value, &value.replace("(0.03)", "(0.05)"));
    load(text.into_bytes())
}

/// The other tool's 0.05 stands against answers saved before its edit (which still hold
/// 0.03): written with them through the CLI, it is kept and 0.03 is not written over it.
#[test]
fn another_tools_requirement_stands_against_saved_answers_on_the_cli() {
    let loaded = re_authored_flatness();
    assert_eq!(
        flatness(&loaded),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
    // The conflict is listed, and marked as another tool's.
    let analysis = api::analyse(&loaded).unwrap();
    let conflicts = analysis["resume"]["conflicts"].as_array().unwrap();
    assert!(
        conflicts
            .iter()
            .any(|c| c["id"] == "datum.A.flatness" && c["foreign"] == true),
        "{conflicts:?}"
    );
    // Answers saved before the edit: they say 0.03.
    let saved = answers("0.03");
    let review = api::review(&analysis, &saved).unwrap();
    assert!(
        review["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "datum.A.flatness"),
        "still undecided: {review}"
    );
    let written = cli_write(&loaded.bytes, &saved, "saved-answers");
    assert_eq!(
        flatness(&written),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
    let editor = preview_write(&loaded, &analysis, &saved);
    assert_eq!(
        flatness(&editor),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
}

/// The same over MCP: `open` with an answers file saved before the edit lists the conflict,
/// and `save` keeps the other tool's 0.05.
#[test]
fn another_tools_requirement_stands_against_an_mcp_answers_file() {
    let dir = dir("mcp-foreign");
    let input = dir.join("plate.step");
    std::fs::write(&input, re_authored_flatness().bytes.as_slice()).unwrap();
    let answers_file = dir.join("answers.json");
    std::fs::write(&answers_file, Value::Object(answers("0.03")).to_string()).unwrap();
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
    let opened = call("open", json!({"path": input, "answers_file": answers_file}));
    assert!(
        opened["conflicts"]
            .as_array()
            .is_some_and(|c| c.iter().any(|c| c["id"] == "datum.A.flatness")),
        "{opened}"
    );
    let saved = call("save", json!({"doc": opened["doc"], "accept_defaults": {}}));
    let back = load(std::fs::read(saved["path"].as_str().unwrap()).unwrap());
    assert_eq!(
        flatness(&back),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
}

/// Keep, write, then restore the stored answer: the kept (unmarked) requirement gives way and
/// the file holds the restored value alone, with no conflict on reopening.
#[test]
fn restoring_after_keeping_replaces_the_kept_requirement() {
    let edited = specify_core::part::load(
        &common::fixtures().join("resume-p9/edited-plate.step.gz"),
        None,
    )
    .unwrap();
    let hole = "hole.position:holes:12";
    let analysis = api::analyse(&edited).unwrap();
    let mut answers = analysis["resume"]["answers"].as_object().unwrap().clone();
    answers.insert("_specify".into(), json!({"keep": [hole]}));
    let kept = preview_write(&edited, &analysis, &answers);
    assert_eq!(hole_positions(&kept), [0.2]);
    let reopened = api::analyse(&kept).unwrap();
    let mut answers = reopened["resume"]["answers"].as_object().unwrap().clone();
    answers.insert(hole.into(), json!("0.1"));
    for written in [
        preview_write(&kept, &reopened, &answers),
        cli_write(&kept.bytes, &answers, "restore"),
    ] {
        assert_eq!(hole_positions(&written), [0.1]);
        let again = api::analyse(&written).unwrap();
        assert!(
            again["resume"].get("conflicts").is_none(),
            "{}",
            again["resume"]
        );
    }
}

/// Saved with the answers from before another tool's edit, the contradicted answer is not
/// stored: reopening resumes no 0.03, shows the file's 0.05, and lists no conflict.
#[test]
fn a_save_does_not_store_an_answer_another_tools_requirement_holds() {
    let loaded = re_authored_flatness();
    let saved = cli_write(&loaded.bytes, &answers("0.03"), "saved-over");
    let again = api::analyse(&saved).unwrap();
    assert!(
        again["resume"].get("conflicts").is_none(),
        "{}",
        again["resume"]
    );
    assert!(
        again["resume"]["answers"].get("datum.A.flatness").is_none(),
        "{}",
        again["resume"]
    );
    assert!(
        again["existing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "Flatness" && e["value"] == 0.05),
        "{}",
        again["existing"]
    );
    assert_eq!(
        flatness(&saved),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
}
