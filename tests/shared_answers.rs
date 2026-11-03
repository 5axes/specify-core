//! Answers held outside the STEP (#35), the application payload the writer carries, the
//! original part, and schema 1 files saved by the pre-alpha browser writer.
mod common;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use regex::bytes::{NoExpand, Regex};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::{api, carried, features::Feature, part, resume, writer};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn features(analysis: &Value) -> Vec<Feature> {
    serde_json::from_value(analysis["features"].clone()).unwrap()
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

// --- answers held outside the STEP ------------------------------------------------------------

/// FTC-10 as alpha.6 wrote it: its 29 answers (eight now move as #100 adds tip cones
/// and schema 6 completes a counterbore pattern)
/// and the face evidence stored with them.
fn ftc10() -> (part::LoadedPart, Value, Map<String, Value>, Value) {
    let base: Value =
        serde_json::from_slice(&std::fs::read(fixture("resume-alpha6/ftc10-reid.json")).unwrap())
            .unwrap();
    let loaded = part::load(&fixture("resume-alpha6/ftc10-reid.step.gz"), Some(0)).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let stored = resume::stored(&loaded.bytes).remove(&0).unwrap();
    let evidence = json!({"schema": stored["schema"], "face_count": stored["face_count"], "faces": stored["faces"]});
    (loaded, analysis, object(base["answers"].clone()), evidence)
}

#[test]
fn answers_with_their_evidence_migrate_as_answers_stored_in_the_step_do() {
    let (loaded, analysis, answers, evidence) = ftc10();
    // What a caller keeps now is what alpha.6 stored: the schema 2 fingerprint is the same.
    assert_eq!(resume::evidence(&loaded).unwrap(), evidence);
    let out = resume::rematch_answers(
        &loaded,
        &features(&analysis),
        &answers,
        Some(&evidence),
        None,
    )
    .unwrap();
    let value = serde_json::to_value(&out).unwrap();
    // Exactly the report the file's own stored answers give on opening.
    assert_eq!(value, analysis["resume"]);
    // #100 tip cones add six moves to the two schema-6 pattern moves.
    let base = json!({"answers": answers, "features": serde_json::from_slice::<Value>(
        &std::fs::read(fixture("resume-alpha6/ftc10-reid.json")).unwrap()).unwrap()["features"]});
    common::upgrade::check(&loaded, &base, &analysis, common::upgrade::FTC10);
    assert_eq!(out.migrated.len(), 8);
    assert!(out.unresolved.is_empty() && out.unverified.is_none());
    let review = api::review(&analysis, &out.answers).unwrap();
    assert_eq!(review["open"].as_array().unwrap().len(), 2);
}

/// Whether `id` is an answer on a feature or a datum (and the datum's settings).
fn geometric(id: &str) -> bool {
    id.starts_with("hole.") || id.starts_with("diameter.") || id.starts_with("datum.")
}

/// `out` left every answer on a feature or a datum open, with its value and a reason saying
/// `why`, and kept the rest of `answers` as they were, in order: nothing dropped or applied.
fn left_open(out: &resume::Resumed, answers: &Map<String, Value>, why: &str) {
    assert!(out.unverified.as_deref().unwrap().contains(why), "{out:?}");
    assert!(out.migrated.is_empty());
    let open: Vec<_> = out.unresolved.iter().map(|u| &u.id).collect();
    let geometric_ids: Vec<_> = answers.keys().filter(|id| geometric(id)).collect();
    assert_eq!(open, geometric_ids);
    for unresolved in &out.unresolved {
        assert_eq!(unresolved.answer, answers[&unresolved.id]);
        assert!(unresolved.reason.contains(why), "{}", unresolved.reason);
        assert!(!unresolved.automatic);
    }
    let kept: Vec<_> = answers.keys().filter(|id| !geometric(id)).collect();
    assert!(!kept.is_empty());
    assert_eq!(out.answers.keys().collect::<Vec<_>>(), kept);
}

// A legacy project, saved without evidence, is trusted by feature ID (the maintainer's
// decision, #35): answers on IDs this part has are kept. #100 cone claims change six more
// IDs in addition to the two schema-6 pattern moves: all eight stay unresolved without evidence.
#[test]
fn without_evidence_answers_are_matched_by_feature_id() {
    let (loaded, analysis, answers, _) = ftc10();
    let out = resume::rematch_answers(&loaded, &features(&analysis), &answers, None, None).unwrap();
    assert!(out.migrated.is_empty());
    let why = out.unverified.as_deref().unwrap();
    assert!(why.contains("no saved face fingerprint"), "{why}");
    assert!(why.contains("matched by feature ID"), "{why}");
    assert_eq!(out.unresolved.len(), 8);
    let moved: Vec<_> = analysis["resume"]["migrated"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| pair[0].as_str().unwrap())
        .collect();
    assert_eq!(
        out.unresolved
            .iter()
            .map(|u| u.id.as_str())
            .collect::<Vec<_>>(),
        moved
    );
    for unresolved in &out.unresolved {
        assert_eq!(unresolved.answer, answers[&unresolved.id]);
        assert!(unresolved.reason.contains("no feature with the saved ID"));
        assert!(!unresolved.automatic);
    }
    // Every other answer is kept as it was, in order, feature answers included; nothing is
    // dropped.
    let kept: Vec<_> = answers
        .keys()
        .filter(|id| !out.unresolved.iter().any(|u| &u.id == *id))
        .collect();
    assert_eq!(out.answers.keys().collect::<Vec<_>>(), kept);
    assert!(kept.iter().any(|id| id.starts_with("hole.")));
    // The kept answers are valid for this part: a review takes them.
    api::review(&analysis, &out.answers).unwrap();
}

// Evidence that contradicts the part is not a legacy project: every answer on a feature or a
// datum is left open with its value, even where its ID is unchanged (IDs renumber between
// kernels); only the part's own answers are kept.
#[test]
fn with_evidence_that_does_not_match_answers_on_features_and_datums_are_left_open() {
    let (loaded, analysis, answers, _) = ftc10();
    let other =
        json!({"schema": 2, "face_count": loaded.binding.face_count, "faces": "0".repeat(64)});
    let mismatched =
        resume::rematch_answers(&loaded, &features(&analysis), &answers, Some(&other), None)
            .unwrap();
    left_open(&mismatched, &answers, "does not match");
}

// Another part's answers with that part's evidence: the mounting plate's (alpha.6) on a part
// whose hole and datum faces happen to carry the same IDs attach nothing to its features or faces.
#[test]
fn another_parts_answers_and_evidence_attach_to_no_feature_or_datum() {
    let base: Value =
        serde_json::from_slice(&std::fs::read(fixture("resume-alpha6/plate.json")).unwrap())
            .unwrap();
    let answers = object(base["answers"].clone());
    let plate = part::load(&fixture("resume-alpha6/plate.step.gz"), None).unwrap();
    let evidence = resume::evidence(&plate).unwrap();
    let loaded = part::load(
        &fixture("parts/four_corner_holes_are_quiddity_rectangle_with_members.step"),
        None,
    )
    .unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let out = resume::rematch_answers(
        &loaded,
        &features(&analysis),
        &answers,
        Some(&evidence),
        None,
    )
    .unwrap();
    left_open(&out, &answers, "does not match");
}

#[test]
fn automatic_answers_are_marked_and_their_ids_follow_migration() {
    let (loaded, analysis, answers, evidence) = ftc10();
    let moved: Vec<String> = answers
        .keys()
        .filter(|id| id.contains("hole_patterns.linear:138.140"))
        .cloned()
        .collect();
    let material = "part.material".to_string();
    let automatic = [moved.clone(), vec![material.clone(), "datum.Z".into()]].concat();
    let migrated = resume::rematch_answers(
        &loaded,
        &features(&analysis),
        &answers,
        Some(&evidence),
        Some(&automatic),
    )
    .unwrap();
    let renamed: Vec<String> = moved
        .iter()
        .map(|old| {
            migrated
                .migrated
                .iter()
                .find(|(o, _)| o == old)
                .unwrap()
                .1
                .clone()
        })
        .collect();
    let mut expected = renamed;
    if answers.contains_key(&material) {
        expected.push(material.clone());
    }
    assert_eq!(migrated.automatic.as_ref().unwrap(), &expected);

    let unmatched = resume::rematch_answers(
        &loaded,
        &features(&analysis),
        &answers,
        None,
        Some(&automatic),
    )
    .unwrap();
    // #100 leaves six further answers unresolved by ID. Only the original two pattern
    // answers were marked automatic; the newly moved bore/row answers remain manual.
    assert_eq!(unmatched.unresolved.len(), 8);
    assert_eq!(
        unmatched.unresolved.iter().filter(|u| u.automatic).count(),
        2
    );
    for u in &unmatched.unresolved {
        assert_eq!(u.automatic, moved.contains(&u.id));
    }
    let value = serde_json::to_value(&unmatched).unwrap();
    for u in value["unresolved"].as_array().unwrap() {
        assert_eq!(
            u.get("automatic"),
            moved
                .iter()
                .any(|id| u["id"] == *id)
                .then_some(&json!(true))
        );
    }
    // The automatic answers kept by ID stay automatic; the unresolved ones are left out.
    let kept: Vec<String> = automatic
        .iter()
        .filter(|id| unmatched.answers.contains_key(*id))
        .cloned()
        .collect();
    assert!(kept.iter().all(|id| !moved.contains(id)));
    assert_eq!(unmatched.automatic.unwrap(), kept);
}

// Without evidence a datum is kept when its faces exist on the part; one on a face the part
// lacks is unresolved with its value.
#[test]
fn without_evidence_a_datum_on_a_face_the_part_lacks_is_unresolved() {
    let loaded = part::load(&fixture("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = object(json!({
        "datum.A": [0], "datum.B": [999], "datum.A.flatness": "0.05",
        "datum.B.control": "flatness", "part.material": "steel"
    }));
    let out = resume::rematch_answers(&loaded, &features(&analysis), &answers, None, None).unwrap();
    assert_eq!(
        Value::Object(out.answers),
        json!({"datum.A": [0], "datum.A.flatness": "0.05", "part.material": "steel"})
    );
    assert_eq!(out.unresolved.len(), 2);
    assert_eq!(out.unresolved[0].id, "datum.B");
    assert_eq!(out.unresolved[0].answer, json!([999]));
    assert!(
        out.unresolved[0]
            .reason
            .contains("a face this part does not have")
    );
    // Its setting follows the open datum rather than being kept on its own.
    assert_eq!(out.unresolved[1].id, "datum.B.control");
    assert_eq!(out.unresolved[1].answer, json!("flatness"));
    assert!(out.unresolved[1].reason.contains("datum.B is unresolved"));

    // With evidence that does not match, datums and their settings are left open.
    let other =
        json!({"schema": 2, "face_count": loaded.binding.face_count, "faces": "0".repeat(64)});
    let mismatched =
        resume::rematch_answers(&loaded, &features(&analysis), &answers, Some(&other), None)
            .unwrap();
    left_open(&mismatched, &answers, "does not match");
}

#[test]
fn malformed_evidence_is_refused() {
    let loaded = part::load(&fixture("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    for bad in [
        json!([]),
        json!({"schema": 3, "face_count": 1, "faces": ""}),
        json!({"schema": 2, "faces": ""}),
    ] {
        let error =
            resume::rematch_answers(&loaded, &features(&analysis), &Map::new(), Some(&bad), None)
                .unwrap_err();
        assert!(error.to_string().starts_with("Invalid evidence"), "{error}");
    }
}

// --- schema 1 files of the pre-alpha browser writer ---------------------------------------------

// tests/fixtures/legacy-schema1/README.md: written by Specify 42ff22c0 under the kernel of the
// time, whose signs of zero differ from quiddity v0.1.1's, so its schema 1 hash cannot be
// reproduced. Its answers resume by feature ID (the maintainer's decision for legacy files,
// #35), with a warning and a review problem, and rewriting stores them as schema 2.
#[test]
fn a_pre_alpha_browser_export_resumes_its_answers() {
    let path = fixture("legacy-schema1/legacy_specify.step");
    let loaded = part::load(&path, None).unwrap();
    let stored = resume::stored(&loaded.bytes).remove(&0).unwrap();
    assert_eq!(stored["schema"], json!(1));
    assert_ne!(
        stored["faces"],
        json!(resume::fingerprint_v1(&loaded).unwrap())
    );
    let expected = json!({"part.material": "6082-T6", "hole.function:holes:9": "fit:H7"});
    assert_eq!(
        resume::answers_for(&loaded).unwrap().map(Value::Object),
        Some(expected.clone())
    );
    let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
    let resume = &analysis["resume"];
    assert_eq!(resume["answers"], expected);
    assert!(resume["unresolved"].is_null(), "{resume}");
    assert!(resume["unverified"].as_str().unwrap().contains("schema 1"));
    assert_eq!(analysis["existing"], json!([]));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let resumed = resume["answers"].as_object().unwrap();
    let review = api::review(&analysis, resumed).unwrap();
    let codes: Vec<_> = review["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].clone())
        .collect();
    assert!(
        codes.contains(&json!("saved_answers_unverified")),
        "{codes:?}"
    );
    assert!(
        !codes.contains(&json!("saved_answer_unresolved")),
        "{codes:?}"
    );

    // Rewriting replaces its PMI (it is the writer's own) and stores schema 2, which verifies.
    let intent = api::preview(&analysis, resumed).unwrap();
    let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(resumed)).unwrap();
    let back = part::load_bytes("again.step", bytes).unwrap();
    assert_eq!(resume::stored(&back.bytes)[&0]["schema"], json!(2));
    let again = &api::analyse(&back).unwrap()["resume"];
    assert_eq!(again["answers"], expected);
    assert!(again["unverified"].is_null() && again["unresolved"].is_null());
}

// --- the application payload and the original part ----------------------------------------------

const PROJECT: &str = "draftwright editor project";

/// The editor's `readProject` / draftwright-mcp's `project::read`: the last item of the project
/// name, base64 of JSON.
fn editor_read(step: &[u8]) -> Option<Value> {
    let found = Regex::new(&format!(
        r"DESCRIPTIVE_REPRESENTATION_ITEM\s*\(\s*'{PROJECT}'\s*,\s*'([A-Za-z0-9+/=]+)'\s*\)"
    ))
    .unwrap();
    let encoded = found
        .captures_iter(step)
        .last()?
        .get(1)?
        .as_bytes()
        .to_vec();
    serde_json::from_slice(&STANDARD.decode(encoded).ok()?).ok()
}

fn schema_statement() -> Regex {
    Regex::new(r"FILE_SCHEMA\s*\([\s\S]*?\)\s*;").unwrap()
}

/// draftwright-mcp's `project::next_instance`, the editor's `nextInstance`.
fn next_instance(step: &[u8]) -> u64 {
    let names = Regex::new(r"'(?:[^']|'')*'|/\*[\s\S]*?\*/|#(\d+)\s*=").unwrap();
    names
        .captures_iter(step)
        .filter_map(|c| c.get(1))
        .filter_map(|m| std::str::from_utf8(m.as_bytes()).ok()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

/// draftwright-mcp's `project::original`, the editor's `originalOf`.
fn editor_original(step: &[u8], project: &Value) -> Option<Vec<u8>> {
    let from = project["generatedFrom"].as_u64()?;
    let start = Regex::new(&format!(r"(?m)^#{from}\s*="))
        .unwrap()
        .find(step)?
        .start();
    let end = Regex::new(r"ENDSEC\s*;")
        .unwrap()
        .find_iter(step)
        .last()?
        .start();
    let joined = [&step[..start], &step[end..]].concat();
    let header = project["source"]["header"].as_str()?.as_bytes();
    Some(
        schema_statement()
            .replace(&joined, NoExpand(header))
            .into_owned(),
    )
}

/// The editor's stored project for `source`, as `storedProject` makes it.
fn editor_project(source: &[u8]) -> Value {
    let header =
        String::from_utf8_lossy(schema_statement().find(source).unwrap().as_bytes()).into_owned();
    json!({
        "schema": 1, "name": "Plate ‘α’",
        "source": {"filename": "plate.step", "sha256": sha256(source), "header": header},
        "generatedFrom": next_instance(source),
        "edits": {"spec": {"schema": "draftwright.spec/2", "sheet": {}}, "answers": {"part.material": "steel"}, "auto": []},
    })
}

fn write(
    loaded: &part::LoadedPart,
    answers: &Value,
    extra: &[haecceity::pmi::AttributeSet],
) -> Vec<u8> {
    let analysis = api::analyse(loaded).unwrap();
    let answers = object(answers.clone());
    let intent = api::preview(&analysis, &answers).unwrap();
    writer::write_bytes_with(loaded, &intent, Some(&answers), extra)
        .unwrap()
        .0
}

#[test]
fn the_carried_project_is_read_by_the_editors_reader_and_gives_back_the_original() {
    let source = std::fs::read(fixture("parts/plate.step")).unwrap();
    let loaded = part::load_bytes("plate.step", source.clone()).unwrap();
    let project = editor_project(&source);
    let extra = carried::attributes(&source, PROJECT, &project).unwrap();
    let written = write(&loaded, &json!({"part.material": "steel"}), &extra);

    // The editor and draftwright-mcp read what is written today, and recover the part.
    assert_eq!(editor_read(&written), Some(project.clone()));
    assert_eq!(
        sha256(&editor_original(&written, &project).unwrap()),
        sha256(&source)
    );
    // And so does the engine.
    assert_eq!(carried::payload(&written, PROJECT), Some(project.clone()));
    assert_eq!(carried::original(&written), Some(source.clone()));
    let back = part::load_bytes("plate.step", written.clone()).unwrap();
    assert_eq!(
        resume::answers_for(&back).unwrap().map(Value::Object),
        Some(json!({"part.material": "steel"}))
    );

    // A file the editor wrote (its base64, no original recorded) reads the same.
    let encoded = STANDARD.encode(project.to_string());
    let editor_only = vec![haecceity::pmi::AttributeSet {
        name: PROJECT.into(),
        on: None,
        items: vec![(
            PROJECT.into(),
            haecceity::pmi::AttributeValue::Text(encoded),
        )],
        description: None,
        general_property: None,
    }];
    let old = write(&loaded, &json!({"part.material": "steel"}), &editor_only);
    assert_eq!(carried::payload(&old, PROJECT), Some(project));
    assert_eq!(carried::original(&old), None);
}

#[test]
fn long_carried_payloads_recover_text_and_the_first_original() {
    let source = std::fs::read(fixture("parts/plate.step")).unwrap();
    let loaded = part::load_bytes("plate.step", source.clone()).unwrap();
    let value = json!({"text": "quoted ' Ω ".repeat(1500)});
    let extra = carried::attributes(&source, PROJECT, &value).unwrap();
    let first = write(&loaded, &json!({"part.material":"steel"}), &extra);
    assert!(String::from_utf8_lossy(&first).contains("haecceity text chunk/1"));
    assert_eq!(carried::payload(&first, PROJECT), Some(value.clone()));
    assert_eq!(
        carried::part_payload(&first, 0, PROJECT),
        Some(value.clone())
    );
    assert_eq!(carried::original(&first), Some(source.clone()));
    let back = part::load_bytes("plate.step", first.clone()).unwrap();
    let extra = carried::attributes(&first, PROJECT, &value).unwrap();
    let second = write(&back, &json!({"part.material":"brass"}), &extra);
    assert_eq!(carried::payload(&second, PROJECT), Some(value));
    assert_eq!(carried::original(&second), Some(source));
}

#[test]
fn rewriting_a_written_file_keeps_its_first_original() {
    let source = std::fs::read(fixture("parts/plate.step")).unwrap();
    let loaded = part::load_bytes("plate.step", source.clone()).unwrap();
    let value = json!({"revision": 1});
    let extra = carried::attributes(&source, "an app", &value).unwrap();
    let first = write(&loaded, &json!({"part.material": "steel"}), &extra);
    let reopened = part::load_bytes("plate.step", first.clone()).unwrap();
    let value = json!({"revision": 2, "text": "it's \\ ‘quoted’"});
    let extra = carried::attributes(&first, "an app", &value).unwrap();
    let second = write(&reopened, &json!({"part.material": "brass"}), &extra);
    assert_ne!(first, second);
    assert_eq!(carried::payload(&second, "an app"), Some(value));
    assert_eq!(carried::original(&second), Some(source));
    // A write without a payload records no original.
    assert_eq!(
        carried::original(&write(&loaded, &json!({"part.material": "steel"}), &[])),
        None
    );
}

// Only an item the writer stores, in a user defined attribute, is the payload: an item of the
// same name elsewhere in the file is not.
#[test]
fn a_payload_is_only_an_item_of_a_user_defined_attribute() {
    let source = std::fs::read_to_string(fixture("parts/plate.step")).unwrap();
    let encoded = STANDARD.encode(json!({"revision": 1}).to_string());
    let end = source.rfind("ENDSEC;").unwrap();
    let stray = format!(
        "{}#999999=DESCRIPTIVE_REPRESENTATION_ITEM('an app','{encoded}');\n{}",
        &source[..end],
        &source[end..]
    );
    assert!(haecceity::p21::Document::parse(stray.clone().into_bytes()).is_ok());
    assert_eq!(carried::payload(stray.as_bytes(), "an app"), None);
    let loaded = part::load_bytes("plate.step", stray.clone().into_bytes()).unwrap();
    let extra = carried::attributes(stray.as_bytes(), "an app", &json!({"revision": 2})).unwrap();
    let written = write(&loaded, &json!({"part.material": "steel"}), &extra);
    assert_eq!(
        carried::payload(&written, "an app"),
        Some(json!({"revision": 2}))
    );
}

#[test]
fn a_payload_must_have_a_name_of_its_own() {
    let source = std::fs::read(fixture("parts/plate.step")).unwrap();
    for name in ["", resume::NAME, carried::ORIGINAL] {
        assert!(
            carried::attributes(&source, name, &json!({})).is_err(),
            "{name:?}"
        );
    }
    assert!(carried::attributes_encoded(&source, PROJECT, "not base64!").is_err());
}

#[test]
fn a_dropped_carry_stays_blocked_across_default_paths_and_repeated_diffs() {
    let old = part::load(&fixture("revisions/plate_split.old.step.gz"), None).unwrap();
    let new = part::load(&fixture("revisions/plate_split.new.step.gz"), None).unwrap();
    let analysis = api::analyse(&new).unwrap();
    let given = object(json!({"part.material":"6061-T6", "datum.A":[4]}));
    let revised = specify_core::revision::diff(&old, &new, &given).unwrap();
    let reason = &revised
        .unresolved
        .iter()
        .find(|u| u.id == "datum.A")
        .unwrap()
        .reason;
    for defaults in [false, true] {
        let error = api::apply(&analysis, &revised.answers, defaults).unwrap_err();
        assert!(
            error.to_string().contains(&format!(
                "datum.A (carried answer could not be applied: {reason})"
            )),
            "{error}"
        );
    }
    let qs = api::questions(&analysis, &revised.answers).unwrap();
    assert!(!specify_core::rules::with_defaults(&qs, &revised.answers).contains_key("datum.A"));
    assert!(specify_core::rules::with_all_defaults(&analysis, &revised.answers).is_err());
    let again = specify_core::revision::diff(&new, &new, &revised.answers).unwrap();
    assert!(api::apply(&analysis, &again.answers, true).is_err());
    assert_eq!(again.answers["_specify"]["unresolved"]["datum.A"], *reason);
    let mut explicit = revised.answers.clone();
    explicit.insert("datum.A".into(), json!([4, 8]));
    api::apply(&analysis, &explicit, true).unwrap();
    let accepted = specify_core::rules::with_all_defaults(&analysis, &explicit).unwrap();
    assert_eq!(accepted["datum.A"], json!([4, 8]));
}

#[test]
fn dropped_answers_of_every_class_block_and_explicit_answers_clear_them() {
    let loaded = part::load(&fixture("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    for reason in [
        "face 4 has no match in the new part",
        "several faces match",
        "unmatched feature",
    ] {
        let mut answers =
            object(json!({"part.material":"steel", "_specify":{"unresolved":{"datum.A":reason}}}));
        let error = api::apply(&analysis, &answers, true).unwrap_err();
        assert!(error.to_string().contains(reason));
        answers.insert("datum.A".into(), json!([4]));
        api::apply(&analysis, &answers, true).unwrap();
    }
}

#[test]
fn writer_checks_answer_evidence_against_geometry_even_with_a_fresh_intent() {
    let old = part::load(&fixture("revisions/plate_split.old.step.gz"), None).unwrap();
    let new = part::load(&fixture("revisions/plate_split.new.step.gz"), None).unwrap();
    let given = object(json!({"part.material":"6061-T6", "datum.A":[4]}));
    let answers = specify_core::revision::diff(&old, &old, &given)
        .unwrap()
        .answers;
    let intent = api::apply(&api::analyse(&old).unwrap(), &answers, true).unwrap();
    writer::write_bytes(&old, &intent, Some(&answers)).unwrap();
    let fresh = api::apply(&api::analyse(&new).unwrap(), &given, true).unwrap();
    let error = writer::write_bytes(&new, &fresh, Some(&answers)).unwrap_err();
    assert!(matches!(error, writer::WriteError::Invalid(_)));
    assert!(error.to_string().contains("stale answers"), "{error}");
    assert!(error.to_string().contains("diff"), "{error}");
}

#[test]
fn same_face_count_still_requires_the_saved_fingerprint_and_metadata_must_be_valid() {
    let old = part::load(&fixture("revisions/plate_thickened.old.step.gz"), None).unwrap();
    let new = part::load(&fixture("revisions/plate_thickened.new.step.gz"), None).unwrap();
    assert_eq!(old.binding.face_count, new.binding.face_count);
    let given = object(json!({"part.material":"steel", "datum.A":[4]}));
    let saved = specify_core::revision::diff(&old, &old, &given)
        .unwrap()
        .answers;
    let intent = api::apply(&api::analyse(&new).unwrap(), &given, true).unwrap();
    let error = writer::write_bytes(&new, &intent, Some(&saved)).unwrap_err();
    assert!(error.to_string().contains("stale answers"), "{error}");
    for meta in [
        json!(null),
        json!({"unresolved":[]}),
        json!({"unresolved":{"datum.A":null}}),
        json!({"evidence":{}}),
        json!({"evidence":{"schema":99,"face_count":8,"faces":"bad"}}),
    ] {
        let mut malformed = given.clone();
        malformed.insert("_specify".into(), meta);
        assert!(api::apply(&api::analyse(&old).unwrap(), &malformed, true).is_err());
    }
}

#[test]
fn editor_rematching_keeps_embedded_evidence_and_carry_blocks() {
    let old = part::load(&fixture("revisions/plate_split.old.step.gz"), None).unwrap();
    let new = part::load(&fixture("revisions/plate_split.new.step.gz"), None).unwrap();
    let analysis = api::analyse(&new).unwrap();
    let given = object(json!({"part.material":"steel", "datum.A":[4]}));
    let saved = specify_core::revision::diff(&old, &old, &given)
        .unwrap()
        .answers;
    let resumed = resume::rematch_answers(&new, &features(&analysis), &saved, None, None).unwrap();
    assert!(!resumed.answers.contains_key("datum.A"));
    assert_eq!(
        resumed.answers["_specify"]["evidence"],
        resume::evidence(&new).unwrap()
    );
    assert!(api::apply(&analysis, &resumed.answers, true).is_err());
    let blocked =
        object(json!({"_specify":{"unresolved":{"part.locating":"its feature was unmatched"}}}));
    let (_, state) = specify_core::tolerancing::settle(
        &analysis,
        &specify_core::tolerancing::State {
            answers: blocked,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!state.answers.contains_key("part.locating"));
}
