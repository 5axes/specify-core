//! Answers that write no PMI (ADR P9, maintainer's decision 2026-11-19): the file cannot state
//! them, so they are kept with the stored answers and resumed on reopening, not asked again,
//! where the file has nothing to contradict them. A file stating PMI for such a question
//! contradicts the answer: it follows the P9 conflict rules (docs/answers.md, "Reopening a
//! file Specify wrote").

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::{LoadedPart, load_bytes};
use specify_core::session::{self, Opened, Session};

/// The plate's answers that write no PMI, with the defaults it is offered.
const SILENT: [&str; 8] = [
    "part.coating",
    "part.heat_treatment",
    "part.notes",
    "part.locating",
    "hole.function:holes:9",
    "datum.A.flatness",
    "datum.B.control",
    "datum.C.control",
];

/// Of the plate's defaults, some that write PMI.
const WRITING: [&str; 6] = [
    "datum.A",
    "datum.B",
    "datum.C",
    "hole.position:holes:12",
    "hole.function:hole_patterns.grid:6.7.8.10.11.13",
    "part.general_tolerance",
];

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("answers_without_pmi")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn material() -> Map<String, Value> {
    map(json!({"part.material": "6082-T6"}))
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    load_bytes("plate.step", bytes).unwrap()
}

fn asked(analysis: &Value) -> (Vec<String>, Vec<String>) {
    let answers = analysis["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let review = api::review(analysis, &answers).unwrap();
    let ids = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|q| q.as_str().or_else(|| q["id"].as_str()).map(str::to_owned))
            .collect()
    };
    (ids(&review["questions"]), ids(&review["open"]))
}

/// The plate written with `answers` (defaults taken) and `stored` stored.
fn written(answers: &Map<String, Value>, stored: &Map<String, Value>) -> Vec<u8> {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, answers, true).unwrap();
    api::write_bytes(
        std::fs::read(plate()).unwrap(),
        &[(intent, Some(stored.clone()))],
    )
    .unwrap()
    .0
}

fn session_of(path: &Path) -> session::Shared {
    Arc::new(Mutex::new(
        Session::open(Opened::read(path).unwrap(), false).unwrap(),
    ))
}

/// The open questions of `part` with a default, with it: what the page offers to approve.
fn offered(analysis: &Value, answers: &Map<String, Value>) -> Map<String, Value> {
    let qs = api::questions(analysis, answers).unwrap();
    specify_core::rules::open_questions(&qs, answers)
        .iter()
        .filter_map(|q| q.default.clone().map(|d| (q.id.clone(), d)))
        .collect()
}

/// The page's Save with every default offered approved, round by round (taking one can ask
/// another): the file last saved.
fn approve_all(path: &Path, given: &Map<String, Value>) -> PathBuf {
    let shared = session_of(path);
    session::lock(&shared).set(0, given.clone(), "page");
    let analysis = api::analyse_file(path, None).unwrap();
    let mut saved = None;
    loop {
        let answers = session::lock(&shared)
            .answers(0)
            .cloned()
            .unwrap_or_default();
        let defaults = offered(&analysis, &answers);
        if defaults.is_empty() {
            break;
        }
        let approved: session::Approved = BTreeMap::from([(0, defaults)]);
        saved =
            Some(session::save(&shared, None, &approved, "page").unwrap_or_else(|e| panic!("{e}")));
    }
    PathBuf::from(saved.expect("defaults were offered").path)
}

/// Approved on the page, the defaults that write no PMI are stored and resumed: reopening
/// does not ask them again.
#[test]
fn approved_defaults_that_write_no_pmi_resume_and_are_not_asked() {
    let dir = scratch("approved");
    let input = dir.join("plate.step");
    std::fs::copy(plate(), &input).unwrap();
    let saved = approve_all(&input, &material());
    let again = api::analyse_file(&saved, None).unwrap();
    let resume = &again["resume"];
    assert!(resume.get("conflicts").is_none(), "{resume}");
    let answers = resume["answers"].as_object().unwrap();
    for id in SILENT.iter().chain(&WRITING) {
        assert!(answers.contains_key(*id), "{id} is not resumed: {resume}");
    }
    let (_, open) = asked(&again);
    assert!(open.is_empty(), "asked again: {open:?}");
}

/// `write --accept-defaults` (every default, the person's "accept all"): the defaults that
/// write no PMI are stored with the answers and resumed; those that write PMI are not stored
/// and read back from the file as present (#204), and nothing is asked again.
#[test]
fn accept_defaults_on_the_command_line_stores_the_defaults_that_write_no_pmi() {
    let dir = scratch("cli");
    let analysis = dir.join("analysis.json");
    let answers = dir.join("answers.json");
    let out = dir.join("out.step");
    let run = |args: &[&std::ffi::OsStr]| {
        let got = std::process::Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            got.status.success(),
            "{}",
            String::from_utf8_lossy(&got.stderr)
        );
    };
    run(&[
        "analyse".as_ref(),
        plate().as_os_str(),
        "-o".as_ref(),
        analysis.as_os_str(),
    ]);
    std::fs::write(&answers, Value::Object(material()).to_string()).unwrap();
    run(&[
        "write".as_ref(),
        plate().as_os_str(),
        analysis.as_os_str(),
        answers.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
        "--accept-defaults".as_ref(),
    ]);
    let again = api::analyse_file(&out, None).unwrap();
    let resume = &again["resume"];
    assert!(resume.get("conflicts").is_none(), "{resume}");
    let answers = resume["answers"].as_object().unwrap();
    let present = resume["present"].as_object().unwrap();
    assert!(answers.contains_key("part.material"), "{resume}");
    for id in SILENT {
        assert!(answers.contains_key(id), "{id} is not resumed: {resume}");
    }
    for id in WRITING {
        assert!(!answers.contains_key(id), "{id} is stored: {resume}");
        assert!(present.contains_key(id), "{id} is not present: {resume}");
    }
    let (_, open) = asked(&again);
    assert!(open.is_empty(), "asked again: {open:?}");
}

/// `bytes` with a flatness of 0.05 on datum A's face added by another tool: haecceity's own
/// writer, so it carries no Specify provenance marker.
fn with_foreign_flatness(bytes: Vec<u8>) -> Vec<u8> {
    use haecceity::p21::Document;
    use haecceity::pmi;
    let mut flat = material();
    flat.insert("datum.A.flatness".into(), json!("0.05"));
    let source = written(&flat, &flat);
    let defs = haecceity::step::read_part_definitions(&source).unwrap();
    let read = pmi::read(&Document::parse(source.clone()).unwrap(), &defs).unwrap();
    let flatness: Vec<_> = read.parts[0]
        .tolerances
        .iter()
        .filter(|t| t.kind == pmi::ToleranceKind::Flatness)
        .map(|t| pmi::GeometricTolerance {
            // Specify's provenance marker, which another tool does not write.
            description: None,
            ..t.clone()
        })
        .collect();
    assert_eq!(flatness.len(), 1);
    let added = pmi::PartPmi {
        features: read.parts[0].features.clone(),
        tolerances: flatness,
        ..Default::default()
    };
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), added)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

fn flatness_on_a(existing: &Value) -> Vec<&Value> {
    existing
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "Flatness" && e["faces"] == json!([2]))
        .collect()
}

/// "none" stored for datum A's flatness, then another tool adds a flatness on A's face: the
/// answer is not resumed as "none". It is a conflict with another tool's requirement, whose
/// only choice is keeping it; the file's flatness is shown and kept, and the question is not
/// asked. Saved, the contradicted answer is not stored: reopening reads the file's flatness.
#[test]
fn a_file_stating_pmi_for_a_silent_answer_contradicts_it() {
    let all = specify_core::rules::with_all_defaults(
        &api::analyse_file(&plate(), None).unwrap(),
        &material(),
    )
    .unwrap();
    assert_eq!(all["datum.A.flatness"], "none");
    let edited = load(with_foreign_flatness(written(&all, &all)));
    let analysis = api::analyse(&edited).unwrap();
    let resume = &analysis["resume"];
    let conflicts = resume["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 1, "{resume}");
    assert_eq!(conflicts[0]["id"], "datum.A.flatness");
    assert_eq!(conflicts[0]["stored"], "none");
    assert!(
        conflicts[0]["file"].as_str().unwrap().contains("0.05"),
        "{conflicts:?}"
    );
    assert_eq!(conflicts[0]["foreign"], true);
    assert!(
        !resume["answers"]
            .as_object()
            .unwrap()
            .contains_key("datum.A.flatness"),
        "{resume}"
    );
    // The other silent answers still resume.
    for id in SILENT.iter().filter(|id| **id != "datum.A.flatness") {
        assert!(
            resume["answers"].as_object().unwrap().contains_key(*id),
            "{id}: {resume}"
        );
    }
    let flatness = flatness_on_a(&analysis["existing"]);
    assert_eq!(flatness.len(), 1, "{}", analysis["existing"]);
    assert_eq!(flatness[0]["question"], "datum.A.flatness");
    let (questions, _) = asked(&analysis);
    assert!(!questions.contains(&"datum.A.flatness".to_string()));
    let answers = resume["answers"].as_object().unwrap().clone();
    let review = api::review(&analysis, &answers).unwrap();
    assert_eq!(review["conflicts"][0]["id"], "datum.A.flatness");
    assert_eq!(
        review["conflicts"][0]["choices"].as_array().unwrap().len(),
        1
    );

    // Saved: the flatness stays, and the contradicted "none" is not stored.
    let intent = api::apply(&analysis, &answers, false).unwrap();
    let back = load(
        api::write_bytes(edited.bytes.to_vec(), &[(intent, Some(answers.clone()))])
            .unwrap()
            .0,
    );
    let again = api::analyse(&back).unwrap();
    assert!(
        again["resume"].get("conflicts").is_none(),
        "{}",
        again["resume"]
    );
    assert!(
        !again["resume"]["answers"]
            .as_object()
            .unwrap()
            .contains_key("datum.A.flatness")
    );
    assert_eq!(flatness_on_a(&again["existing"]).len(), 1);
    let (questions, _) = asked(&again);
    assert!(!questions.contains(&"datum.A.flatness".to_string()));
}

/// An answer that wrote PMI whose PMI was then removed in the file is still a conflict,
/// "removed in the file": its answer is not mistaken for one that never wrote anything.
#[test]
fn an_answer_whose_pmi_was_removed_is_still_a_conflict() {
    let mut given = material();
    given.insert("datum.A.flatness".into(), json!("0.05"));
    let mut wrote = given.clone();
    wrote.insert("datum.A.flatness".into(), json!("none"));
    // Written without the flatness while storing it: the file removed it.
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, &wrote, true).unwrap();
    let mut stored = specify_core::rules::with_all_defaults(&analysis, &given).unwrap();
    stored.insert("datum.A.flatness".into(), json!("0.05"));
    let bytes = api::write_bytes(
        std::fs::read(plate()).unwrap(),
        &[(intent, Some(stored.clone()))],
    )
    .unwrap()
    .0;
    let again = api::analyse(&load(bytes)).unwrap();
    let conflicts = again["resume"]["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 1, "{}", again["resume"]);
    assert_eq!(conflicts[0]["id"], "datum.A.flatness");
    assert_eq!(conflicts[0]["stored"], "0.05");
    assert_eq!(conflicts[0]["file"], Value::Null);
    // The silent answers stored beside it resume.
    let answers = again["resume"]["answers"].as_object().unwrap();
    for id in SILENT.iter().filter(|id| **id != "datum.A.flatness") {
        assert!(answers.contains_key(*id), "{id}: {}", again["resume"]);
    }
}

/// A page's Save of an assembly approving defaults for a part with no answers of its own
/// (`acceptDefaults: {"1": {...}}`): the part is written with them, and they are stored.
#[test]
fn an_assembly_part_with_only_approved_defaults_is_written() {
    let dir = scratch("assembly");
    let input = dir.join("assembly.step");
    std::fs::copy(
        common::fixtures().join("parts").join("assembly.step"),
        &input,
    )
    .unwrap();
    let pin = api::analyse_file(&input, Some(1)).unwrap();
    let defaults = offered(&pin, &Map::new());
    assert!(!defaults.is_empty());
    let shared = session_of(&input);
    let approved: session::Approved = BTreeMap::from([(1, defaults.clone())]);
    let saved = session::save(&shared, None, &approved, "page").unwrap_or_else(|e| panic!("{e}"));
    let written = saved.report["written"].to_string();
    assert!(written != "[]", "{}", saved.report);
    let again = api::analyse_file(Path::new(&saved.path), Some(1)).unwrap();
    let answers = again["resume"]["answers"].as_object().unwrap();
    for (id, value) in &defaults {
        assert_eq!(answers.get(id), Some(value), "{id}: {}", again["resume"]);
    }
}
