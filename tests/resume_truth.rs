//! ADR P9 §3: on reopening a file Specify wrote, the PMI is the truth. Stored answers the file
//! agrees with resume; the file's own additions are `existing`; a stored answer the file
//! changed or removed is a conflict for the person to decide, the file's state standing until
//! then (docs/answers.md, "Reopening a file Specify wrote").

mod common;

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::existing::read_existing;
use specify_core::part::{LoadedPart, load_bytes};
use specify_core::resume;

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

/// The plate written with `file`'s PMI (defaults taken) while storing `stored` as its
/// answers: a file Specify wrote whose PMI was then edited elsewhere.
const EDITED: &str = "resume-p9/edited-plate.step.gz";

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

/// The answers the edited plate stores.
fn stored() -> Map<String, Value> {
    map(json!({
        "part.material": "6082-T6",
        "part.locating": "position tolerances",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.05",
        "datum.B.control": "0.1",
        "hole.position:holes:12": "0.1",
    }))
}

/// The answers its PMI was written from: the hole's position changed and the flatness
/// removed. Another tool then added a fit on another hole (unmarked PMI).
fn edited() -> Map<String, Value> {
    let mut answers = stored();
    answers["hole.position:holes:12"] = json!("0.2");
    answers["datum.A.flatness"] = json!("none");
    answers
}

/// `bytes` with hole 9's ⌀8 H7, as Specify writes it, added by another tool: haecceity's own
/// writer, so it carries no Specify provenance marker.
fn with_foreign_fit(bytes: Vec<u8>, analysis: &Value) -> Vec<u8> {
    use haecceity::p21::Document;
    use haecceity::pmi;
    let mut fitted = stored();
    fitted.insert("hole.function:holes:9".into(), json!("fit:H7"));
    let source = write(std::fs::read(plate()).unwrap(), analysis, &fitted, &fitted);
    let defs = haecceity::step::read_part_definitions(&source).unwrap();
    let read = pmi::read(&Document::parse(source.clone()).unwrap(), &defs).unwrap();
    // The hole's size: `read_existing` lists Specify's toleranced dimensions in order.
    let listed = specify_core::existing::read_existing(&load(source.clone())).unwrap();
    let k = listed
        .iter()
        .filter(|e| e.kind == "dimension")
        .position(|e| e.r#type == "Size_Diameter" && e.faces == [9])
        .unwrap();
    let fit = read.parts[0]
        .dimensions
        .iter()
        .filter(|d| !matches!(d.tolerance, pmi::DimTolerance::None))
        .nth(k)
        .unwrap()
        .clone();
    let added = pmi::PartPmi {
        features: read.parts[0].features.clone(),
        dimensions: vec![fit],
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

fn write(
    loaded_bytes: Vec<u8>,
    analysis: &Value,
    answers: &Map<String, Value>,
    stored: &Map<String, Value>,
) -> Vec<u8> {
    let intent = api::apply(analysis, answers, true).unwrap();
    api::write_bytes(loaded_bytes, &[(intent, Some(stored.clone()))])
        .unwrap()
        .0
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    load_bytes("plate.step", bytes).unwrap()
}

fn edited_plate() -> LoadedPart {
    specify_core::part::load(&common::fixtures().join(EDITED), None).unwrap()
}

/// Writes `tests/fixtures/resume-p9/edited-plate.step` (run with `--ignored` to make it again).
#[test]
#[ignore]
fn regenerate_the_edited_plate() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let bytes = write(
        std::fs::read(plate()).unwrap(),
        &analysis,
        &edited(),
        &stored(),
    );
    let bytes = with_foreign_fit(bytes, &analysis);
    let path = common::fixtures().join(EDITED);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    std::io::Write::write_all(&mut gz, &bytes).unwrap();
    std::fs::write(path, gz.finish().unwrap()).unwrap();
}

/// `resume` resumes exactly the answers of `stored`.
fn resumes(resume: &Value, stored: &Map<String, Value>) {
    assert_eq!(resume["answers"], Value::Object(stored.clone()), "{resume}");
}

fn tolerances<'a>(existing: &'a Value, kind: &str, face: u64) -> Vec<&'a Value> {
    existing
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "geometric_tolerance" && e["type"] == kind)
        .filter(|e| e["faces"].as_array().unwrap().contains(&json!(face)))
        .collect()
}

#[test]
fn an_edited_file_shows_its_pmi_and_lists_the_conflicts() {
    let loaded = edited_plate();
    // The stored answers are as written: the file's own PMI decides what resumes.
    assert_eq!(resume::answers_for(&loaded).unwrap(), Some(stored()));
    let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
    let conflicts = analysis["resume"]["conflicts"].as_array().unwrap();
    assert_eq!(conflicts.len(), 2, "{conflicts:?}");
    assert_eq!(conflicts[0]["id"], "datum.A.flatness");
    assert_eq!(conflicts[0]["stored"], "0.05");
    assert_eq!(conflicts[0]["file"], Value::Null);
    assert_eq!(conflicts[1]["id"], "hole.position:holes:12");
    assert_eq!(conflicts[1]["stored"], "0.1");
    assert!(
        conflicts[1]["file"].as_str().unwrap().contains("0.2"),
        "{conflicts:?}"
    );
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.starts_with("saved answer "))
            .count(),
        2,
        "{warnings:?}"
    );
    // The other answers resume; the contradicted ones do not.
    let mut expected = stored();
    expected.remove("datum.A.flatness");
    expected.remove("hole.position:holes:12");
    resumes(&analysis["resume"], &expected);
    let answers = analysis["resume"]["answers"].as_object().unwrap();
    assert!(
        !answers.contains_key("datum.A.flatness")
            && !answers.contains_key("hole.position:holes:12")
    );
    // The file's state is what is shown: its changed tolerance (tagged with its question) and
    // its added fit are present; the removed flatness is not.
    let existing = &analysis["existing"];
    let position = tolerances(existing, "Position", 12);
    assert_eq!(position.len(), 1, "{existing}");
    assert_eq!(position[0]["value"], 0.2);
    assert_eq!(position[0]["question"], "hole.position:holes:12");
    assert!(tolerances(existing, "Flatness", 2).is_empty(), "{existing}");
    assert!(
        existing
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "dimension"
                && e["type"] == "Size_Diameter"
                && e["faces"] == json!([9])
                && e.get("question").is_none()
                && e.get("provenance").is_none()),
        "the added fit is present: {existing}"
    );
    // Not asked until decided; asked as conflicts instead.
    let answers = analysis["resume"]["answers"].as_object().unwrap().clone();
    let review = api::review(&analysis, &answers).unwrap();
    let asked: Vec<&str> = review["questions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|q| q["id"].as_str())
        .collect();
    assert!(!asked.contains(&"datum.A.flatness") && !asked.contains(&"hole.position:holes:12"));
    let questions = review["conflicts"].as_array().unwrap();
    assert_eq!(questions.len(), 2);
    assert!(questions.iter().all(|q| q["kind"] == "conflict"));
    assert_eq!(
        questions[0]["text"],
        "Saved answer 0.05: removed in the file. Restore it, or leave it removed?"
    );
    assert!(
        questions[1]["text"]
            .as_str()
            .unwrap()
            .starts_with("Saved answer 0.1, file says ")
    );
    assert_eq!(questions[1]["choices"][1]["answer"], "0.1");
}

/// Restoring answers the question with the stored value: the file's contradicting item gives
/// way, so the file written has the restored value and not both.
#[test]
fn restoring_writes_the_stored_answer_and_not_the_files() {
    let loaded = edited_plate();
    let analysis = api::analyse(&loaded).unwrap();
    let mut answers = analysis["resume"]["answers"].as_object().unwrap().clone();
    answers.insert("hole.position:holes:12".into(), json!("0.1"));
    answers.insert("datum.A.flatness".into(), json!("0.05"));
    let review = api::review(&analysis, &answers).unwrap();
    assert!(review.get("conflicts").is_none(), "{review}");
    let back = load(write(loaded.bytes.to_vec(), &analysis, &answers, &answers));
    let written = serde_json::to_value(read_existing(&back).unwrap()).unwrap();
    let position: Vec<_> = tolerances(&written, "Position", 12)
        .iter()
        .map(|e| e["value"].clone())
        .collect();
    assert_eq!(position, [json!(0.1)], "{written}");
    let flatness: Vec<_> = tolerances(&written, "Flatness", 2)
        .iter()
        .map(|e| e["value"].clone())
        .collect();
    assert_eq!(flatness, [json!(0.05)], "{written}");
    // Opened again: no conflict, every answer resumed.
    let again = api::analyse(&back).unwrap();
    assert!(
        again["resume"].get("conflicts").is_none(),
        "{}",
        again["resume"]
    );
    resumes(&again["resume"], &answers);
}

/// Keeping is recorded in `_specify.keep`; nothing else changes and the file's PMI is written.
#[test]
fn keeping_writes_the_files_pmi() {
    let loaded = edited_plate();
    let analysis = api::analyse(&loaded).unwrap();
    let mut answers = analysis["resume"]["answers"].as_object().unwrap().clone();
    answers.insert(
        "_specify".into(),
        json!({"keep": ["hole.position:holes:12", "datum.A.flatness"]}),
    );
    let review = api::review(&analysis, &answers).unwrap();
    assert!(review.get("conflicts").is_none(), "{review}");
    // The answers are stored as given; the writer keeps `_specify` out of them.
    let back = load(write(loaded.bytes.to_vec(), &analysis, &answers, &answers));
    let written = serde_json::to_value(read_existing(&back).unwrap()).unwrap();
    let position: Vec<_> = tolerances(&written, "Position", 12)
        .iter()
        .map(|e| e["value"].clone())
        .collect();
    assert_eq!(position, [json!(0.2)], "{written}");
    assert!(tolerances(&written, "Flatness", 2).is_empty(), "{written}");
    // Kept, the file's tolerance is the file's own PMI: written without Specify's marker, so
    // any reader, and Specify reopening it, sees it as such. The removal kept is stored with
    // the answers (`kept`), having no item to carry it.
    let marked = |l: &LoadedPart| -> Vec<bool> {
        read_existing(l)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces == [12])
            .map(|e| e.provenance.is_some())
            .collect()
    };
    assert_eq!(marked(&back), [false]);
    let reopened = |l: &LoadedPart| {
        let again = api::analyse(l).unwrap();
        assert!(
            again["resume"].get("conflicts").is_none(),
            "{}",
            again["resume"]
        );
        let answers = again["resume"]["answers"].as_object().unwrap().clone();
        let review = api::review(&again, &answers).unwrap();
        let asked: Vec<&str> = review["questions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|q| q["id"].as_str())
            .collect();
        for id in ["hole.position:holes:12", "datum.A.flatness"] {
            assert!(!asked.contains(&id), "{id} is asked again: {asked:?}");
        }
        let position: Vec<_> = tolerances(&again["existing"], "Position", 12)
            .iter()
            .map(|e| e["value"].clone())
            .collect();
        assert_eq!(position, [json!(0.2)], "{}", again["existing"]);
        assert!(tolerances(&again["existing"], "Flatness", 2).is_empty());
        (again, answers)
    };
    let (again, answers) = reopened(&back);
    // Saved again: unchanged.
    let second = load(write(back.bytes.to_vec(), &again, &answers, &answers));
    let written = serde_json::to_value(read_existing(&second).unwrap()).unwrap();
    let position: Vec<_> = tolerances(&written, "Position", 12)
        .iter()
        .map(|e| e["value"].clone())
        .collect();
    assert_eq!(position, [json!(0.2)], "{written}");
    assert!(tolerances(&written, "Flatness", 2).is_empty(), "{written}");
    assert_eq!(marked(&second), [false]);
    reopened(&second);
}

/// A file written with defaults taken but only the given answers stored (as every writer did
/// before ADR P9's approved defaults): the defaulted requirements read back as present, not as
/// open questions, and nothing conflicts. Each default the file states as the defaults would
/// write it is in `resume.present` with its value: the file answers its question, which can
/// still be answered otherwise.
#[test]
fn defaults_written_without_answers_read_back_as_present() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let given = stored();
    let back = load(write(
        std::fs::read(plate()).unwrap(),
        &analysis,
        &given,
        &given,
    ));
    let again = api::analyse(&back).unwrap();
    assert!(
        again["resume"].get("conflicts").is_none(),
        "{}",
        again["resume"]
    );
    resumes(&again["resume"], &given);
    let present: Vec<&str> = again["resume"]["present"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let review = api::review(&again, again["resume"]["answers"].as_object().unwrap()).unwrap();
    let open: Vec<&str> = review["open"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for id in [
        "hole.position:hole_patterns.grid:6.7.8.10.11.13",
        "hole.function:hole_patterns.grid:6.7.8.10.11.13",
        "hole.function:holes:12",
        "part.general_tolerance",
    ] {
        assert!(present.contains(&id), "{id} is not present: {present:?}");
        assert!(!open.contains(&id), "{id} is open: {open:?}");
    }
    // Present, so not repeated as the file's own PMI.
    assert!(
        tolerances(&again["existing"], "Position", 6).is_empty(),
        "{}",
        again["existing"]
    );
}

/// An unchanged round trip resumes every answer with no conflict, across the corpus parts the
/// write oracle writes (with QUIDDITY_CORPUS or SPECIFY_CORPUS; the fixtures otherwise).
#[test]
fn an_unchanged_round_trip_resumes_every_answer() {
    let mut parts = 0;
    let mut failures = Vec::new();
    for c in common::cases() {
        if c.part().is_some() || c.meta["parts"].as_u64().unwrap_or(1) != 1 {
            continue;
        }
        let Some(input) = c.input() else { continue };
        let Some(answers) = c.meta["answers"].as_object().cloned() else {
            continue;
        };
        let Ok(analysis) = api::analyse_file(&input, None) else {
            continue;
        };
        let Ok(intent) = api::apply(&analysis, &answers, true) else {
            continue;
        };
        let Ok(source) = specify_core::part::load(&input, None) else {
            continue;
        };
        let bytes = source.bytes.to_vec();
        let Ok((written, _)) = api::write_bytes(bytes, &[(intent, Some(answers.clone()))]) else {
            continue;
        };
        let back = load(written);
        let Some(expected) = resume::answers_for(&back).unwrap() else {
            continue;
        };
        parts += 1;
        let again = api::analyse(&back).unwrap();
        if again["resume"].get("conflicts").is_some()
            || again["resume"]["answers"] != Value::Object(expected)
        {
            failures.push(format!("{}: {}", c.name, again["resume"]));
        }
    }
    println!(
        "unchanged round trips: {parts} parts, {} with conflicts",
        failures.len()
    );
    assert!(parts > 0);
    assert!(failures.is_empty(), "{failures:#?}");
}
