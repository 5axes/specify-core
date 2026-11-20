//! Complete sidecars are revision-bound answers, not just a successful write invocation.
use serde_json::{Map, Value, json};
use specify_core::{api, existing, part, resume, rules, serve::Server};
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
fn export_approving(step: &Path, analysis: &Path, answers: &Path, approved: &Path) -> Output {
    run(&[
        "questions",
        path(analysis),
        "--answers",
        path(answers),
        "--resolve-defaults",
        "--step",
        path(step),
        "--accept-defaults",
        path(approved),
    ])
}
/// Every default `with_all_defaults` takes for `given`, approved for part 0 with its value:
/// what a person who saw and confirmed the whole list would send.
fn every_default(analysis: &Value, given: &Value) -> Value {
    let mut all = rules::with_all_defaults(analysis, &map(given)).unwrap();
    all.retain(|id, _| !map(given).contains_key(id));
    json!({"0": all})
}
fn plain(exported: &Map<String, Value>) -> Map<String, Value> {
    let mut plain = exported.clone();
    plain.remove("_specify");
    plain
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
fn cli_complete_export_roundtrips_semantic_pmi_and_keeps_a_carry_block() {
    let dir = scratch("roundtrip");
    let old = fixture("revisions/plate_split.old.step.gz");
    let new = fixture("revisions/plate_split.new.step.gz");
    let loaded = part::load(&old, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let a = dir.join("analysis.json");
    let answers = dir.join("given.json");
    let sidecar = dir.join("exported.json");
    let approvals = dir.join("approved.json");
    let given = json!({"part.material":"6061-T6", "datum.A":[4], "datum.B":[1], "datum.C":[0]});
    save(&a, &analysis);
    save(&answers, &given);
    // Every default, each approved (ADR P9): the same answers `write --accept-defaults` takes.
    save(&approvals, &every_default(&analysis, &given));
    let bytes = succeeded(export_approving(&old, &a, &answers, &approvals));
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
    assert_eq!(
        bytes,
        succeeded(export_approving(&old, &a, &answers, &approvals))
    );
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
    // An answers file carried to the new geometry with its datum unresolved, as an earlier
    // `diff` wrote one (diff no longer writes answers: ADR P11).
    let carried = dir.join("carried.json");
    let mut block = complete.clone();
    for datum in ["datum.A", "datum.B", "datum.C"] {
        block.remove(datum);
    }
    block.insert(
        "_specify".into(),
        json!({"evidence": resume::evidence(&part::load(&new, None).unwrap()).unwrap(),
               "unresolved": {"datum.A": "face 4 has no match in the new part"}}),
    );
    save(&carried, &Value::Object(block));
    // Incomplete is normal: the carry block is kept for the person, its question open.
    let out = export(&new, &new_analysis, &carried);
    assert!(String::from_utf8_lossy(&out.stderr).contains("datum.A"));
    let kept: Value = serde_json::from_slice(&succeeded(out)).unwrap();
    assert!(
        kept["_specify"]["unresolved"]["datum.A"]
            .as_str()
            .unwrap()
            .contains("no match")
    );
    assert!(kept.get("datum.A").is_none());
}

#[test]
fn export_keeps_incomplete_answers_and_refuses_stale_and_mismatched_inputs() {
    let dir = scratch("refusals");
    let old = fixture("revisions/plate_split.old.step.gz");
    let new = fixture("revisions/plate_split.new.step.gz");
    let loaded = part::load(&old, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let a = dir.join("analysis.json");
    let answers = dir.join("answers.json");
    save(&a, &analysis);
    save(&answers, &json!({}));
    // Nothing given, nothing approved: no defaults, and what is open said on stderr.
    let out = export(&old, &a, &answers);
    assert!(String::from_utf8_lossy(&out.stderr).contains("part.material"));
    let exported: Value = serde_json::from_slice(&succeeded(out)).unwrap();
    assert_eq!(plain(&map(&exported)), Map::new());
    let mut given =
        json!({"part.material":"steel", "_specify":{"unresolved":{"datum.A":"review the split"}}});
    save(&answers, &given);
    let exported: Value = serde_json::from_slice(&succeeded(export(&old, &a, &answers))).unwrap();
    assert_eq!(
        exported["_specify"]["unresolved"],
        json!({"datum.A": "review the split"})
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
fn serve_exports_approved_defaults_only_and_keeps_carry_blocks() {
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
    // No approvals, no defaults (ADR P9); what is open is said.
    assert_eq!(plain(&saved), map(&given));
    assert_eq!(reply["result"]["complete"], false);
    assert!(
        reply["result"]["missing"]
            .as_array()
            .unwrap()
            .contains(&json!("datum.B"))
    );
    // Every default approved, by part: what `write` with accept_defaults takes.
    let approved = server.reply(&json!({"op":"export_answers", "step":old.path,
        "analysis":analysis, "answers":given, "accept_defaults": every_default(&analysis, &given)}));
    assert_eq!(approved["ok"], true, "{approved}");
    assert_eq!(approved["result"]["complete"], true);
    assert_eq!(approved["result"]["missing"], json!([]));
    let saved = map(&approved["result"]["answers"]);
    assert_eq!(
        plain(&saved),
        rules::with_all_defaults(&analysis, &map(&given)).unwrap()
    );
    // A default approved with a value it no longer has is refused, naming both.
    let questions = api::questions(&analysis, &map(&given)).unwrap();
    let tolerance = questions
        .iter()
        .find(|q| q.id == "part.general_tolerance")
        .unwrap();
    let stale = server.reply(&json!({"op":"export_answers", "step":old.path,
        "analysis":analysis, "answers":given,
        "accept_defaults": {"0": {"part.general_tolerance": "not what was offered"}}}));
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["error"]["code"], "invalid");
    assert_eq!(stale["error"]["part"], 0);
    assert_eq!(stale["error"]["question"], "part.general_tolerance");
    assert_eq!(stale["error"]["expected"], "not what was offered");
    assert_eq!(
        stale["error"]["current"],
        *tolerance.default.as_ref().unwrap()
    );
    let malformed = server.reply(&json!({"op":"export_answers", "step":old.path,
        "analysis":analysis, "answers":given, "accept_defaults": true}));
    assert_eq!(malformed["error"]["code"], "invalid");
    // A carry block no longer refuses the export: it is kept, its question open.
    // An answers file carrying a datum that could not be applied to the new geometry, as an
    // earlier `diff` wrote one (diff no longer writes answers: ADR P11).
    let mut carried = plain(&saved);
    carried.remove("datum.A");
    carried.insert(
        "_specify".into(),
        json!({"evidence": resume::evidence(&new).unwrap(),
               "unresolved": {"datum.A": "face 4 has no match in the new part"}}),
    );
    let kept = server.reply(&json!({"op":"export_answers", "step":new.path, "analysis":api::analyse(&new).unwrap(), "answers":carried}));
    assert_eq!(kept["ok"], true, "{kept}");
    assert!(
        kept["result"]["missing"]
            .as_array()
            .unwrap()
            .contains(&json!("datum.A"))
    );
    assert!(
        kept["result"]["answers"]["_specify"]["unresolved"]["datum.A"]
            .as_str()
            .unwrap()
            .contains("no match")
    );
}

/// ADR P9: an export takes exactly the defaults approved for its part, with the values the
/// person saw. None approved, none taken; a value that changed, refused; another part's
/// approval does not apply.
#[test]
fn export_takes_only_the_defaults_approved_for_its_part() {
    let loaded = part::load(&fixture("revisions/plate_split.old.step.gz"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let given = map(&json!({"part.material":"steel"}));
    let offered = api::questions(&analysis, &given).unwrap();
    let default = |id: &str| {
        offered
            .iter()
            .find(|q| q.id == id)
            .and_then(|q| q.default.clone())
            .unwrap()
    };
    let tolerance = default("part.general_tolerance");
    let approve = |part: usize, value: Value| {
        api::Approved::from([(part, map(&json!({"part.general_tolerance": value})))])
    };

    let none = api::export_answers(&loaded, &analysis, &given, &api::Approved::new()).unwrap();
    assert_eq!(plain(&none.answers), given);
    assert!(!none.complete());
    assert!(none.missing.contains(&"part.general_tolerance".to_string()));

    let one =
        api::export_answers(&loaded, &analysis, &given, &approve(0, tolerance.clone())).unwrap();
    let mut expected = given.clone();
    expected.insert("part.general_tolerance".into(), tolerance.clone());
    assert_eq!(
        plain(&one.answers),
        expected,
        "the approved default, and no other"
    );
    assert!(!one.missing.contains(&"part.general_tolerance".to_string()));
    assert_eq!(
        one.answers["_specify"]["sources"]["part.general_tolerance"]["rule"],
        "part.general_tolerance.machined"
    );

    match api::export_answers(&loaded, &analysis, &given, &approve(0, json!("ISO 2768-c"))) {
        Err(api::Error::Unapproved(u)) => {
            assert_eq!((u.part, u.question.as_str()), (0, "part.general_tolerance"));
            assert_eq!(*u.values.unwrap(), (json!("ISO 2768-c"), tolerance.clone()));
        }
        other => panic!("a changed default must be refused: {other:?}"),
    }
    match api::export_answers(
        &loaded,
        &analysis,
        &given,
        &api::Approved::from([(0, map(&json!({"part.material": "steel"})))]),
    ) {
        Err(api::Error::Unapproved(u)) => assert!(u.values.is_none()),
        other => panic!("an approval of no open default must be refused: {other:?}"),
    }

    let elsewhere =
        api::export_answers(&loaded, &analysis, &given, &approve(1, tolerance)).unwrap();
    assert_eq!(
        elsewhere, none,
        "part 1's approval does not apply to part 0"
    );
}

#[test]
fn cli_refuses_a_changed_default_and_malformed_approvals() {
    let dir = scratch("cli-approvals");
    let old = fixture("revisions/plate_split.old.step.gz");
    let a = dir.join("analysis.json");
    let answers = dir.join("answers.json");
    let approvals = dir.join("approved.json");
    save(&a, &api::analyse(&part::load(&old, None).unwrap()).unwrap());
    save(&answers, &json!({"part.material":"steel"}));
    save(
        &approvals,
        &json!({"0": {"part.general_tolerance": "ISO 2768-c"}}),
    );
    refused(
        export_approving(&old, &a, &answers, &approvals),
        "part 0: part.general_tolerance's default is now",
    );
    save(&approvals, &json!({"part.general_tolerance": "ISO 2768-m"}));
    refused(
        export_approving(&old, &a, &answers, &approvals),
        "--accept-defaults: {\"<part>\"",
    );
    let out = run(&["questions", path(&a), "--accept-defaults", path(&approvals)]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("requires --resolve-defaults"));
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
