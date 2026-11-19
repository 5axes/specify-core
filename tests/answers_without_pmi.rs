//! Reopening skips stored answers where the file already has PMI in their scope. Ordinary
//! file reconciliation handles the question, and the file's PMI survives saving.

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

#[test]
fn foreign_flatness_withholds_stored_none() {
    let (all, bytes) = accepted_all();
    assert_eq!(all["datum.A.flatness"], "none");
    let mut part = load(with_foreign_flatness(bytes));
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        // Existing foreign flatness already answers the question under #204.
        assert_withheld(&analysis, "datum.A.flatness", false);
        // The other silent answers, which nothing in the file contradicts, still resume.
        let answers = analysis["resume"]["answers"].as_object().unwrap();
        for id in SILENT.iter().filter(|id| **id != "datum.A.flatness") {
            assert!(answers.contains_key(*id), "{id}: {}", analysis["resume"]);
        }
        let flatness = flatness_on_a(&analysis["existing"]);
        assert_eq!(flatness.len(), 1, "{}", analysis["existing"]);
        assert_eq!(flatness[0]["value"], 0.05);
        assert!(flatness[0].get("question").is_none());
        (part, _) = saved_again(&part, &analysis);
        assert!(
            specify_core::existing::read_existing(&part)
                .unwrap()
                .iter()
                .any(|e| { e.r#type == "Flatness" && e.faces == [2] && e.value == Some(0.05) })
        );
    }
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

/// `bytes` with `added` written by haecceity's own writer (Add): another tool's PMI, without
/// Specify's provenance marker.
fn with_foreign(bytes: Vec<u8>, added: haecceity::pmi::PartPmi) -> Vec<u8> {
    use haecceity::p21::Document;
    use haecceity::pmi;
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

/// The plate written with every default approved: `part.notes: []` and the other silent
/// answers stored.
fn accepted_all() -> (Map<String, Value>, Vec<u8>) {
    let all = specify_core::rules::with_all_defaults(
        &api::analyse_file(&plate(), None).unwrap(),
        &material(),
    )
    .unwrap();
    let bytes = written(&all, &all);
    (all, bytes)
}

#[test]
fn silent_answers_resume_on_clean_plate_and_enclosure_faces() {
    for path in [
        plate(),
        common::fixtures().join("parts/electronics-enclosure-base.step.gz"),
    ] {
        let part = specify_core::part::load(&path, None).unwrap();
        let analysis = api::analyse(&part).unwrap();
        let mut answers = material();
        let hole = api::questions(&analysis, &answers)
            .unwrap()
            .into_iter()
            .find(|q| q.id.starts_with("hole.function:"))
            .expect("the clean part has a hole function question");
        answers.insert(hole.id.clone(), json!("general"));
        answers.insert("part.coating".into(), json!("None (as machined)"));
        answers.insert("part.heat_treatment".into(), json!("None"));
        answers.insert("part.notes".into(), json!([]));
        let intent = api::preview(&analysis, &answers).unwrap();
        let (bytes, _) =
            api::write_bytes(part.bytes.to_vec(), &[(intent, Some(answers.clone()))]).unwrap();
        let back = load_bytes("clean.step", bytes).unwrap();
        assert!(
            specify_core::existing::read_existing(&back)
                .unwrap()
                .is_empty()
        );
        let again = api::analyse(&back).unwrap();
        assert_eq!(again["resume"]["answers"], json!(answers));
        assert!(again["resume"].get("conflicts").is_none());
        for id in answers.keys() {
            assert!(!asked(&again).1.contains(id), "{id}");
        }
    }
}

fn assert_withheld(analysis: &Value, id: &str, open: bool) {
    let resume = &analysis["resume"];
    assert!(resume["answers"].get(id).is_none(), "{id}: {resume}");
    assert!(
        !resume["conflicts"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|c| c["id"] == id),
        "{id}: {resume}"
    );
    assert_eq!(
        asked(analysis).1.iter().any(|q| q == id),
        open,
        "{id}: {resume}"
    );
}

/// Reopen and save as the editor does, leaving newly open questions unanswered.
fn saved_again(loaded: &LoadedPart, analysis: &Value) -> (LoadedPart, Value) {
    let answers = analysis["resume"]["answers"].as_object().unwrap().clone();
    let intent =
        api::preview(analysis, &answers).unwrap_or_else(|e| panic!("{e}\n{}", analysis["resume"]));
    let back = load(
        api::write_bytes(loaded.bytes.to_vec(), &[(intent, Some(answers))])
            .unwrap()
            .0,
    );
    let again = api::analyse(&back).unwrap();
    (back, again)
}

/// Locate the nominal measure through the dimension's own STEP provenance.
fn location_measure(part: &LoadedPart, mm: f64, basic: bool) -> Option<std::ops::Range<usize>> {
    use haecceity::p21::Document;
    use haecceity::pmi::{self, DimTolerance, DimensionKind, Quantity};
    let doc = Document::parse(part.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&part.bytes).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let (i, _) = read.parts[0].dimensions.iter().enumerate().find(|(_, d)| {
        matches!(d.kind, DimensionKind::Location { .. })
            && d.tolerance == if basic { DimTolerance::Basic } else { DimTolerance::None }
            && matches!(d.nominal.as_ref().map(|v| &v.quantity), Some(Quantity::Length(n)) if n.mm() == mm)
    })?;
    read.provenance.parts[0].dimensions[i]
        .iter()
        .find_map(|&id| {
            let span = doc.span(id)?;
            let record = std::str::from_utf8(&part.bytes[span.clone()]).unwrap();
            (record.contains("MEASURE_WITH_UNIT") && record.contains("'nominal value'"))
                .then_some(span)
        })
}

#[test]
fn edited_marked_basic_location_survives_two_saves() {
    edited_location_survives_two_saves(95, 97, true, "hole.position:holes:12");
}

#[test]
fn edited_marked_nominal_location_survives_two_saves() {
    edited_location_survives_two_saves(50, 52, false, "hole.function:holes:9");
}

fn edited_location_survives_two_saves(old: i32, new: i32, basic: bool, question: &str) {
    let (mut stored, _) = accepted_all();
    let answers = stored.clone();
    if basic {
        stored.insert(question.into(), json!("none"));
    }
    let source = load(written(&answers, &stored));
    let span = location_measure(&source, f64::from(old), basic).unwrap();
    let mut text = String::from_utf8(source.bytes.to_vec()).unwrap();
    let record = text[span.clone()].replace(
        &format!("LENGTH_MEASURE({old}.)"),
        &format!("LENGTH_MEASURE({new}.)"),
    );
    assert_ne!(text[span.clone()], record);
    text.replace_range(span, &record);
    let mut part = load(text.into_bytes());
    assert!(location_measure(&part, f64::from(new), basic).is_some());
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        // The basic case has file position PMI. The nominal location leaves function open.
        assert_withheld(&analysis, question, !basic);
        (part, _) = saved_again(&part, &analysis);
        assert!(
            location_measure(&part, f64::from(new), basic).is_some(),
            "lost {new} mm on saving {question}"
        );
    }
}

#[test]
fn flatness_on_the_files_moved_datum_survives_two_saves() {
    let (stored, _) = accepted_all();
    assert_eq!(stored["datum.A"], json!([2]));
    assert_eq!(stored["datum.A.flatness"], "none");
    let mut answers = stored.clone();
    answers.insert("datum.A".into(), json!([4]));
    answers.insert("datum.A.flatness".into(), json!("0.05"));
    let mut part = load(written(&answers, &stored));
    let has_flatness = |part: &LoadedPart| {
        specify_core::existing::read_existing(part)
            .unwrap()
            .iter()
            .any(|e| e.r#type == "Flatness" && e.faces == [4] && e.value == Some(0.05))
    };
    assert!(has_flatness(&part));
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        assert_withheld(&analysis, "datum.A.flatness", false);
        (part, _) = saved_again(&part, &analysis);
        assert!(has_flatness(&part), "lost flatness on the file's datum");
    }
}

#[test]
fn an_extra_marked_note_survives_two_saves() {
    let (mut stored, _) = accepted_all();
    stored.insert("part.notes".into(), json!(["KEEP DRY"]));
    let mut answers = stored.clone();
    let notes = json!(["KEEP DRY", "DEBURR ALL OVER"]);
    answers.insert("part.notes".into(), notes.clone());
    let mut part = load(written(&answers, &stored));
    assert_eq!(
        specify_core::existing::part_settings_provenance(&part).unwrap()["notes"],
        json!(["specify-core:written:v1", "specify-core:written:v1"])
    );
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        assert_withheld(&analysis, "part.notes", false);
        (part, _) = saved_again(&part, &analysis);
        assert_eq!(
            specify_core::existing::part_settings(&part).unwrap()["notes"],
            notes
        );
    }
}

#[test]
fn a_marked_position_tolerance_contradicts_stored_none() {
    let hole = "hole.position:holes:12";
    let (mut answers, _) = accepted_all();
    answers.insert(hole.into(), json!("0.2"));
    let mut stored = answers.clone();
    stored.insert(hole.into(), json!("none"));
    let edited = load(written(&answers, &stored));
    let positions = |part: &LoadedPart| {
        specify_core::existing::read_existing(part)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces == [12])
            .map(|e| (e.value.unwrap(), e.provenance.is_some()))
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(&edited), [(0.2, true)]);

    let analysis = api::analyse(&edited).unwrap();
    let resume = &analysis["resume"];
    let (back, again) = saved_again(&edited, &analysis);
    assert_eq!(positions(&back), [(0.2, true)], "{resume}");
    // The marked position answers the question as file PMI under #204.
    assert_withheld(&analysis, hole, false);
    assert!(
        resume["file"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == hole)
    );
    let (back, _) = saved_again(&back, &again);
    assert_eq!(positions(&back), [(0.2, true)]);
}

#[test]
fn a_marked_note_contradicts_stored_no_notes() {
    let (stored, _) = accepted_all();
    let notes = json!(["KEEP DRY"]);
    let mut answers = stored.clone();
    answers.insert("part.notes".into(), notes.clone());
    let edited = load(written(&answers, &stored));
    let analysis = api::analyse(&edited).unwrap();
    // The file's complete notes answer the setting under #204.
    assert_withheld(&analysis, "part.notes", false);
    let (back, again) = saved_again(&edited, &analysis);
    assert_eq!(
        specify_core::existing::part_settings(&back).unwrap()["notes"],
        notes
    );
    assert_eq!(again["part"]["notes"], notes);
    let (back, _) = saved_again(&back, &again);
    assert_eq!(
        specify_core::existing::part_settings(&back).unwrap()["notes"],
        notes
    );
}

#[test]
fn mixed_provenance_notes_contradict_the_stored_list() {
    let (mut stored, _) = accepted_all();
    assert_eq!(stored["part.notes"], json!([]));
    let mut answers = stored.clone();
    answers.insert("part.notes".into(), json!(["KEEP DRY"]));
    let notes = json!(["KEEP DRY", "DEBURR ALL OVER"]);
    for (old, index) in [(json!([]), 2), (json!(["KEEP DRY"]), 1)] {
        stored.insert("part.notes".into(), old.clone());
        let edited = load(with_general_note(
            written(&answers, &stored),
            "DEBURR ALL OVER",
            index,
        ));
        assert_eq!(
            specify_core::existing::part_settings(&edited).unwrap()["notes"],
            notes
        );
        let provenance = specify_core::existing::part_settings_provenance(&edited).unwrap();
        assert!(provenance["notes"][0].is_string());
        assert!(provenance["notes"][1].is_null());

        let analysis = api::analyse(&edited).unwrap();
        let resume = &analysis["resume"];
        let (back, again) = saved_again(&edited, &analysis);
        assert_eq!(
            specify_core::existing::part_settings(&back).unwrap()["notes"],
            notes,
            "{resume}"
        );
        assert_eq!(analysis["part"]["notes"], notes);
        // Both notes are file entries, so the setting is already answered under #204.
        assert_withheld(&analysis, "part.notes", false);
        assert_eq!(again["part"]["notes"], notes, "{}", again["resume"]);
        let (back, _) = saved_again(&back, &again);
        assert_eq!(
            specify_core::existing::part_settings(&back).unwrap()["notes"],
            notes
        );
        assert_eq!(
            specify_core::existing::part_settings_provenance(&back).unwrap()["notes"],
            provenance["notes"]
        );
    }
}

fn with_general_note(bytes: Vec<u8>, text: &str, index: i64) -> Vec<u8> {
    use haecceity::pmi::{AttributeSet, AttributeValue, PartPmi};
    let note = AttributeSet {
        name: "semantic text".into(),
        on: None,
        items: vec![
            ("general note".into(), AttributeValue::Text(text.into())),
            ("note index".into(), AttributeValue::Integer(index)),
        ],
        description: None,
        general_property: None,
    };
    with_foreign(
        bytes,
        PartPmi {
            attributes: vec![note],
            ..Default::default()
        },
    )
}

/// "No notes" stored, then another tool adds a general note: the file's note contradicts the
/// stored `[]`. It is shown (`part.notes`), the answer is not resumed, and the note stays in
/// the file through a save.
#[test]
fn another_tools_general_note_contradicts_stored_no_notes() {
    let (all, bytes) = accepted_all();
    assert_eq!(all["part.notes"], json!([]));
    let edited = load(with_general_note(bytes, "DEBURR ALL OVER", 1));
    let analysis = api::analyse(&edited).unwrap();
    assert_eq!(
        analysis["part"]["notes"],
        json!(["DEBURR ALL OVER"]),
        "{}",
        analysis["part"]
    );
    // The imported note fills the part setting under #204.
    assert_withheld(&analysis, "part.notes", false);
    let (back, again) = saved_again(&edited, &analysis);
    assert_eq!(
        again["part"]["notes"],
        json!(["DEBURR ALL OVER"]),
        "{}",
        again["part"]
    );
    let (back, again) = saved_again(&back, &again);
    assert_eq!(
        again["part"]["notes"],
        json!(["DEBURR ALL OVER"]),
        "{}",
        again["part"]
    );
    // A session save of it keeps the note too.
    let dir = scratch("note");
    let input = dir.join("noted.step");
    std::fs::write(&input, back.bytes.as_slice()).unwrap();
    let shared = session_of(&input);
    let saved =
        session::save(&shared, None, &Default::default(), "page").unwrap_or_else(|e| panic!("{e}"));
    let reopened = api::analyse_file(Path::new(&saved.path), None).unwrap();
    assert_eq!(
        reopened["part"]["notes"],
        json!(["DEBURR ALL OVER"]),
        "{}",
        reopened["part"]
    );
}

/// A "general" hole (no PMI), then another tool adds a thread callout on it: a requirement of
/// another kind than "general"'s alternatives would write, on the question's faces, still
/// contradicts the stored answer.
#[test]
fn another_tools_thread_on_a_general_hole_contradicts_it() {
    use haecceity::p21::Document;
    use haecceity::pmi::{self, PartPmi};
    let (all, bytes) = accepted_all();
    assert_eq!(all["hole.function:holes:9"], "general");
    // A file stating hole 9's size (to name its feature) and hole 12's M5 thread.
    let mut fitted = all.clone();
    fitted.insert("hole.function:holes:9".into(), json!("fit:H7"));
    let source = written(&fitted, &fitted);
    let defs = haecceity::step::read_part_definitions(&source).unwrap();
    let read = pmi::read(&Document::parse(source.clone()).unwrap(), &defs).unwrap();
    let listed = specify_core::existing::read_existing(&load(source.clone())).unwrap();
    let k = listed
        .iter()
        .filter(|e| e.kind == "dimension")
        .position(|e| e.r#type == "Size_Diameter" && e.faces == [9])
        .unwrap();
    let size = read.parts[0]
        .dimensions
        .iter()
        .filter(|d| !matches!(d.tolerance, pmi::DimTolerance::None))
        .nth(k)
        .unwrap();
    let pmi::DimensionKind::Size { feature, .. } = &size.kind else {
        panic!("not a size: {size:?}");
    };
    let mut thread = read.parts[0].threads[0].clone();
    thread.feature = *feature;
    thread.partial_area = Some(*feature);
    thread.runout = None;
    let edited = load(with_foreign(
        bytes,
        PartPmi {
            features: read.parts[0].features.clone(),
            threads: vec![thread],
            ..Default::default()
        },
    ));
    let threads: Vec<_> = specify_core::existing::read_existing(&edited)
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == "internal thread" && e.faces == [9])
        .collect();
    assert_eq!(threads.len(), 1, "{threads:?}");
    let mut part = edited;
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        // The imported thread already establishes the hole's function under #204.
        assert_withheld(&analysis, "hole.function:holes:9", false);
        (part, _) = saved_again(&part, &analysis);
        assert_eq!(
            specify_core::existing::read_existing(&part)
                .unwrap()
                .iter()
                .filter(|e| e.r#type == "internal thread" && e.faces == [9])
                .count(),
            1
        );
    }
}

/// Another part than the plate, with holes and datums: written with every default approved,
/// and with `write --accept-defaults`'s stored answers, it reopens with nothing asked again
/// and no conflict.
#[test]
fn another_part_resumes_its_silent_answers() {
    let input = common::fixtures().join("parts/electronics-enclosure-base.step.gz");
    let source = specify_core::part::load(&input, None).unwrap();
    let analysis = api::analyse_file(&input, None).unwrap();
    assert!(
        analysis["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"].as_str().is_some_and(|id| id.starts_with("holes")))
    );
    let given = material();
    let all = specify_core::rules::with_all_defaults(&analysis, &given).unwrap();
    assert!(all.keys().any(|id| id.starts_with("datum.")), "{all:?}");
    let intent = api::apply(&analysis, &given, true).unwrap();
    let cli = api::stored_with_defaults(&source, &analysis, &given).unwrap();
    assert!(cli.len() > given.len(), "{cli:?}");
    for stored in [&all, &cli] {
        let bytes = api::write_bytes(
            source.bytes.to_vec(),
            &[(intent.clone(), Some(stored.clone()))],
        )
        .unwrap()
        .0;
        let back = load_bytes("enclosure.step", bytes).unwrap();
        let again = api::analyse(&back).unwrap();
        let resume = &again["resume"];
        assert!(resume.get("conflicts").is_none(), "{resume}");
        let answers = resume["answers"].as_object().unwrap();
        for id in stored.keys() {
            assert!(answers.contains_key(id), "{id} is not resumed: {resume}");
        }
        let (_, open) = asked(&again);
        assert!(open.is_empty(), "asked again: {open:?}");
    }
}

/// `bytes` with Specify's process UDA changed to cast by another tool: the item edited and
/// the provenance marker on its property link removed.
fn with_foreign_cast_process(bytes: Vec<u8>) -> Vec<u8> {
    use haecceity::p21::{Attribute, Document, Edit, RawEntity};
    use haecceity::pmi;
    use specify_core::process;
    let item = |value| {
        format!(
            "DESCRIPTIVE_REPRESENTATION_ITEM('{}','{value}')",
            process::ITEM
        )
    };
    let original = String::from_utf8(bytes).unwrap();
    assert_eq!(original.matches(&item(process::MACHINED)).count(), 1);
    let doc = Document::parse(
        original
            .replace(&item(process::MACHINED), &item(process::CAST))
            .into_bytes(),
    )
    .unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let i = read.parts[0]
        .attributes
        .iter()
        .position(|set| set.name == process::PROPERTY)
        .unwrap();
    let markers: Vec<_> = read.provenance.parts[0].attributes[i]
        .iter()
        .flat_map(|&id| doc.referrers(id))
        .copied()
        .filter(|&id| {
            matches!(doc.get(id), Some(RawEntity::Simple { name, attributes, .. })
                if name == "DESCRIPTION_ATTRIBUTE"
                    && matches!(attributes.first(),
                        Some(Attribute::String(s)) if s == "specify-core:written:v1"))
        })
        .collect();
    assert_eq!(markers.len(), 1);
    let mut edit = Edit::new();
    edit.remove(markers[0]);
    doc.apply(&edit).unwrap().bytes
}

/// The file's process and its process UDAs.
fn file_process(part: &LoadedPart) -> (usize, Value) {
    use haecceity::p21::Document;
    use haecceity::pmi;
    use specify_core::process;
    let doc = Document::parse(part.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let sets = read.parts[0]
        .attributes
        .iter()
        .filter(|set| set.name == process::PROPERTY)
        .count();
    (sets, process::read_pmi(&read.parts[0]).unwrap().value)
}

/// Both P9 rules on one file: another tool changed the process (#208) and added a flatness on
/// datum A against the stored "none" (no-PMI answers). Two reopen-and-save rounds keep both:
/// the process is a foreign conflict on reopening, and the flatness answer is withheld.
#[test]
fn foreign_process_and_flatness_survive_two_saves() {
    use specify_core::process;
    let (mut all, _) = accepted_all();
    assert_eq!(all["datum.A.flatness"], "none");
    all.insert(process::QUESTION.into(), json!(process::MACHINED));
    let bytes = written(&all, &all);
    let mut part = load(with_foreign_cast_process(with_foreign_flatness(bytes)));
    assert_eq!(file_process(&part), (1, json!(process::CAST)));
    for round in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        let conflict = analysis["resume"]["conflicts"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["id"] == process::QUESTION);
        if round == 0 {
            let conflict = conflict.expect("the changed process conflicts with the saved answer");
            assert_eq!(conflict["stored"], process::MACHINED);
            assert_eq!(conflict["file"], process::CAST);
            assert_eq!(conflict["foreign"], true);
        } else {
            // #208: a foreign process's answer is not stored again, so the file's process
            // stands unchallenged on later reopenings.
            assert!(conflict.is_none(), "{}", analysis["resume"]);
        }
        assert!(
            analysis["resume"]["answers"]
                .get(process::QUESTION)
                .is_none()
        );
        assert_eq!(analysis["part"]["process"], process::CAST);
        assert_eq!(analysis["process"]["source"], "file");
        assert_withheld(&analysis, "datum.A.flatness", false);
        let flatness = flatness_on_a(&analysis["existing"]);
        assert_eq!(flatness.len(), 1, "{}", analysis["existing"]);
        assert_eq!(flatness[0]["value"], 0.05);
        (part, _) = saved_again(&part, &analysis);
        assert_eq!(
            file_process(&part),
            (1, json!(process::CAST)),
            "round {round}"
        );
        assert!(
            specify_core::existing::read_existing(&part)
                .unwrap()
                .iter()
                .any(|e| e.r#type == "Flatness" && e.faces == [2] && e.value == Some(0.05)),
            "round {round}: lost the flatness"
        );
        // The withheld answer stays dormant in the file, still withheld on the next reopen.
        assert_eq!(
            specify_core::resume::stored(&part.bytes)[&0]["answers"]["datum.A.flatness"],
            "none"
        );
    }
}
