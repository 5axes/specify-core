//! Complete sidecars are revision-bound answers, not just a successful write invocation.
use serde_json::{Map, Value, json};
use specify_core::{api, existing, part, resume, revision, rules, serve::Server};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("answers-export-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(args)
        .output()
        .unwrap()
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn save(p: &Path, v: &Value) {
    std::fs::write(p, serde_json::to_vec(v).unwrap()).unwrap();
}
fn export(step: &Path, analysis: &Path, answers: &Path) -> Output {
    run(&[
        "questions",
        path(analysis),
        "--answers",
        path(answers),
        "--resolve-defaults",
        "--step",
        path(step),
    ])
}
fn succeeded(out: Output) -> Vec<u8> {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn refused(out: Output, message: &str) {
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "a failed export must not print a partial map"
    );
}
fn map(v: &Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn cli_complete_export_roundtrips_semantic_pmi_and_carries_to_changed_geometry() {
    let dir = scratch("roundtrip");
    let old = fixture("revisions/plate_split.old.step.gz");
    let new = fixture("revisions/plate_split.new.step.gz");
    let loaded = part::load(&old, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let a = dir.join("analysis.json");
    let answers = dir.join("given.json");
    let sidecar = dir.join("exported.json");
    let given = json!({"part.material":"6061-T6", "datum.A":[4], "datum.B":[1], "datum.C":[0]});
    save(&a, &analysis);
    save(&answers, &given);
    let bytes = succeeded(export(&old, &a, &answers));
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        exported["_specify"]["evidence"],
        resume::evidence(&loaded).unwrap()
    );
    let mut complete = map(&exported);
    complete.remove("_specify");
    assert_eq!(
        complete,
        rules::with_all_defaults(&analysis, &map(&given)).unwrap()
    );
    assert!(complete.len() > map(&given).len());
    assert!(
        api::review(&analysis, &map(&exported)).unwrap()["open"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(bytes, succeeded(export(&old, &a, &answers)));
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(
        bytes,
        format!("{}\n", serde_json::to_string_pretty(&exported).unwrap()).into_bytes()
    );
    std::fs::write(&sidecar, &bytes).unwrap();
    // Re-export is idempotent, including nested key ordering and metadata.
    assert_eq!(bytes, succeeded(export(&old, &a, &sidecar)));
    // The original default write and sidecar-only write must have the same read-back PMI.
    let expected = dir.join("defaults.step");
    let actual = dir.join("sidecar.step");
    succeeded(run(&[
        "write",
        path(&old),
        path(&a),
        path(&answers),
        "--accept-defaults",
        "-o",
        path(&expected),
    ]));
    succeeded(run(&[
        "write",
        path(&old),
        path(&a),
        path(&sidecar),
        "-o",
        path(&actual),
    ]));
    let expected = part::load(&expected, None).unwrap();
    let actual = part::load(&actual, None).unwrap();
    let pmi = existing::read_existing(&expected).unwrap();
    assert!(!pmi.is_empty());
    assert_eq!(pmi, existing::read_existing(&actual).unwrap());
    assert_eq!(
        existing::part_settings(&expected).unwrap(),
        existing::part_settings(&actual).unwrap()
    );
    assert_eq!(resume::answers_for(&actual).unwrap().unwrap(), complete);
    // All answer-taking review paths accept the sidecar.
    succeeded(run(&["questions", path(&a), "--answers", path(&sidecar)]));
    let new_analysis = dir.join("new.json");
    save(
        &new_analysis,
        &api::analyse(&part::load(&new, None).unwrap()).unwrap(),
    );
    let stale_output = dir.join("preserved.step");
    std::fs::write(&stale_output, b"keep").unwrap();
    refused(
        run(&[
            "write",
            path(&new),
            path(&new_analysis),
            path(&sidecar),
            "--accept-defaults",
            "-o",
            path(&stale_output),
        ]),
        "stale answers",
    );
    assert_eq!(std::fs::read(&stale_output).unwrap(), b"keep");
    let changes = dir.join("changes");
    succeeded(run(&[
        "diff",
        path(&old),
        path(&new),
        "--answers",
        path(&sidecar),
        "-o",
        path(&changes),
    ]));
    let carried: Value =
        serde_json::from_slice(&std::fs::read(changes.join("answers.json")).unwrap()).unwrap();
    assert_eq!(carried["part.material"], given["part.material"]);
    assert!(
        carried["_specify"]["unresolved"]["datum.A"]
            .as_str()
            .unwrap()
            .contains("no match")
    );
    refused(
        export(&new, &new_analysis, &changes.join("answers.json")),
        "datum.A (carried answer could not be applied:",
    );
}

#[test]
fn export_refuses_incomplete_stale_and_mismatched_inputs_without_restamping() {
    let dir = scratch("refusals");
    let old = fixture("revisions/plate_split.old.step.gz");
    let new = fixture("revisions/plate_split.new.step.gz");
    let loaded = part::load(&old, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let a = dir.join("analysis.json");
    let answers = dir.join("answers.json");
    save(&a, &analysis);
    save(&answers, &json!({}));
    refused(export(&old, &a, &answers), "part.material");
    let mut given =
        json!({"part.material":"steel", "_specify":{"unresolved":{"datum.A":"review the split"}}});
    save(&answers, &given);
    refused(
        export(&old, &a, &answers),
        "datum.A (carried answer could not be applied: review the split)",
    );
    given["datum.A"] = json!([4]);
    save(&answers, &given);
    let bytes = succeeded(export(&old, &a, &answers));
    let exported: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(exported["_specify"]["unresolved"], json!({}));
    save(&answers, &exported);
    let new_a = dir.join("new.json");
    save(
        &new_a,
        &api::analyse(&part::load(&new, None).unwrap()).unwrap(),
    );
    refused(export(&new, &new_a, &answers), "stale answers");
    save(&answers, &json!({"part.material":"steel"}));
    refused(export(&new, &a, &answers), "different file");
    let mut malformed = exported;
    malformed["_specify"]["evidence"] = json!({});
    save(&answers, &malformed);
    refused(export(&old, &a, &answers), "Invalid evidence");
}

#[test]
fn serve_exports_the_same_map_and_preserves_carry_refusals() {
    let old = part::load(&fixture("revisions/plate_split.old.step.gz"), None).unwrap();
    let new = part::load(&fixture("revisions/plate_split.new.step.gz"), None).unwrap();
    let mut server = Server::default();
    let analysis = api::analyse(&old).unwrap();
    let given = json!({"part.material":"steel", "datum.A":[4]});
    let reply = server.reply(
        &json!({"op":"export_answers", "step":old.path, "analysis":analysis, "answers":given}),
    );
    assert_eq!(reply["ok"], true, "{reply}");
    let saved = map(&reply["result"]["answers"]);
    assert_eq!(
        saved["_specify"]["evidence"],
        resume::evidence(&old).unwrap()
    );
    let mut plain = saved.clone();
    plain.remove("_specify");
    assert_eq!(
        plain,
        rules::with_all_defaults(&analysis, &map(&given)).unwrap()
    );
    let carried = revision::diff(&old, &new, &saved).unwrap();
    let refused = server.reply(&json!({"op":"export_answers", "step":new.path, "analysis":api::analyse(&new).unwrap(), "answers":carried.answers}));
    assert_eq!(refused["ok"], false);
    assert!(
        refused
            .to_string()
            .contains("carried answer could not be applied")
    );
}

#[test]
fn export_selects_the_assembly_part_and_sorts_supplied_answers() {
    let dir = scratch("assembly");
    let step = fixture("parts/assembly.step");
    let loaded = part::load(&step, Some(1)).unwrap();
    let a = dir.join("analysis.json");
    let given = dir.join("given.json");
    save(&a, &api::analyse(&loaded).unwrap());
    save(
        &given,
        &json!({"part.surface_finish":"Ra 1.6", "part.material":"steel"}),
    );
    let first = succeeded(export(&step, &a, &given));
    save(
        &given,
        &json!({"part.material":"steel", "part.surface_finish":"Ra 1.6"}),
    );
    assert_eq!(first, succeeded(export(&step, &a, &given)));
    let out: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(
        out["_specify"]["evidence"],
        resume::evidence(&loaded).unwrap()
    );
    let keys: Vec<_> = out.as_object().unwrap().keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}
