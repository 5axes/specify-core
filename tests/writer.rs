//! The writer: specify-core's `tests/test_writer.py`, `test_requirements.py` (the semantic
//! content), `test_resume.py`, `test_merge.py` (re-expressed for haecceity's add mode),
//! `test_conformance.py` (as docs/design.md "Oracle and tests" keeps it), `test_drawn.py`
//! (datum symbols) and the write side of `test_threads.py` and `test_mmc_finish_notes.py`.
//!
//! Each test names the Python test it ports. Analyses are the oracle's (Python's, captured), so
//! intents come from the Rust rules on the analysis the Python test used; the analysis of a
//! file the writer wrote is that analysis with the file's binding and its PMI as the Rust reader
//! reads it (as `api.analyse` composes it: a file whose answers resume is analysed as its
//! geometry alone). What Python asserted of the text route, OpenCascade's text fixes and
//! leaders is not ported (not-applicable entries in `known_differences/writer.json`, raised by
//! `tests/write_oracle.rs`); every other assertion is kept, or re-expressed where haecceity
//! writes the same intent another way, which the comment says.

mod common;

use std::path::{Path, PathBuf};

use haecceity::p21::Document;
use haecceity::pmi::{self, DimTolerance};
use haecceity::step::read_part_definitions;
use regex::Regex;
use serde_json::{Map, Value, json};
use specify_core::existing::{part_settings, read_existing};
use specify_core::features::describe_faces;
use specify_core::part::{LoadedPart, load};
use specify_core::resume::{answers_for, stored};
use specify_core::rules::{Intent, apply_with_defaults, questions};
use specify_core::writer::{WriteError, WriteReport, write};

// --- helpers ---------------------------------------------------------------------------------

/// A fresh directory for one test.
/// The report's line counting the dimensions under the general tolerance (#63): 1 or 0.
fn general_line(written: &[String]) -> usize {
    written
        .iter()
        .filter(|l| l.ends_with("under the general tolerance"))
        .count()
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-writer-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(name: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{name}.step"))
}

/// The oracle's analysis of a case (Python's).
fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

fn apply(analysis: &Value, given: Value) -> Intent {
    apply_with_defaults(analysis, &answers(given), true).expect("an intent")
}

/// The analysis of `loaded` (a file the writer wrote, of the same faces as `base`'s): `base`
/// with the file's binding and the file's PMI as the Rust reader reads it; a file whose
/// answers resume is analysed as its geometry alone and carries them (as `api.analyse`).
fn reanalysed(base: &Value, loaded: &LoadedPart) -> Value {
    let mut out = base.clone();
    out["binding"] = serde_json::to_value(&loaded.binding).unwrap();
    match answers_for(loaded).unwrap() {
        Some(a) => {
            out["existing"] = json!([]);
            out["part"] = json!({});
            out["resume"] = json!({ "answers": a });
        }
        None => {
            out["existing"] = serde_json::to_value(read_existing(loaded).unwrap()).unwrap();
            out["part"] = Value::Object(part_settings(loaded).unwrap());
            out.as_object_mut().unwrap().remove("resume");
        }
    }
    out
}

/// A feature id by family and recorded diameter.
fn feature(analysis: &Value, family: &str, diameter: f64) -> String {
    analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == family && f["record"]["diameter"].as_f64() == Some(diameter))
        .unwrap_or_else(|| panic!("no {family} of diameter {diameter}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The id of the plane face highest in z.
fn top_face(analysis: &Value) -> i64 {
    let faces = analysis["faces"].as_array().unwrap();
    let z = |f: &Value| f["centroid"][2].as_f64().unwrap();
    faces
        .iter()
        .filter(|f| f["kind"] == "plane")
        .max_by(|a, b| z(a).total_cmp(&z(b)))
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

/// The question whose id starts `prefix` and whose prompt contains `text`.
fn question(analysis: &Value, prefix: &str, text: &str) -> String {
    questions(analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with(prefix) && q.prompt.contains(text))
        .unwrap_or_else(|| panic!("no question {prefix} .. {text}"))
        .id
}

/// The file's text with its line breaks folded, as Python's tests read it.
fn flat(path: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    Regex::new(r"\s*\n\s*")
        .unwrap()
        .replace_all(&text, " ")
        .into_owned()
}

/// haecceity's reading of part `k` of the file.
fn pmi_of(path: &Path, k: usize) -> pmi::PartPmi {
    let bytes = std::fs::read(path).unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    pmi::read(&doc, &defs).unwrap().parts.swap_remove(k)
}

fn types(path: &Path) -> Vec<String> {
    read_existing(&load(path, None).unwrap())
        .unwrap()
        .into_iter()
        .map(|e| e.r#type)
        .collect()
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The plate's intent with only its material answered (Python's `_plate_intent`).
fn plate_intent() -> Intent {
    apply(&analysis("parts/plate"), json!({"part.material": "steel"}))
}

/// A datum on the largest plane and a position of the first cylinder against it (Python's
/// `_datum_and_position`), from the Rust engine's own faces of `loaded`.
fn datum_and_position(loaded: &LoadedPart) -> Intent {
    let faces = describe_faces(loaded).unwrap();
    let plane = faces
        .iter()
        .filter(|f| f.kind == "plane")
        .max_by(|a, b| a.area.total_cmp(&b.area))
        .unwrap();
    let bore = faces.iter().find(|f| f.kind == "cylinder").unwrap();
    let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.datums = vec![answers(json!({"letter": "A", "faces": [plane.id]}))];
    intent.requirements = vec![answers(json!({
        "kind": "position", "feature": "bore", "faces": [bore.id], "tolerance": 0.1,
        "diametral": true, "datums": ["A"],
    }))];
    intent
}

// --- test_writer.py --------------------------------------------------------------------------

/// `spool_written`: every kind of requirement v1 writes, including a g6 shaft fit, on corpus
/// `cadgenbench/flanged_spool_132` (skipped without the corpus).
fn spool_written(dir: &Path) -> Option<(Intent, WriteReport, PathBuf, Value)> {
    let case = common::case("corpus/cadgenbench/flanged_spool_132");
    let input = case.input()?;
    let analysis = case.artifact("analysis").unwrap();
    let ids: Vec<String> = questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .map(|q| q.id)
        .collect();
    let pattern = ids
        .iter()
        .find(|i| i.starts_with("hole.function:hole_patterns"))
        .unwrap();
    let bore = ids
        .iter()
        .find(|i| i.starts_with("hole.function:holes"))
        .unwrap();
    let turned: Vec<&String> = ids
        .iter()
        .filter(|i| i.starts_with("diameter.fit"))
        .collect();
    let given = json!({
        "part.material": "6082-T6",
        pattern.clone(): "fit:H7",
        bore.clone(): "fit:H7",
        turned[0].clone(): "p6",
        turned[1].clone(): "g6",
        "datum.A.flatness": "0.02",
    });
    let intent = apply(&analysis, given);
    let out = dir.join("spool-pmi.step");
    let report = write(&load(&input, None).unwrap(), &intent, &out, None).unwrap();
    Some((intent, report, out, analysis))
}

// test_writer.py::test_write_verifies_and_reports, test_limits_are_stated_correctly_in_the_file,
// test_existing_pmi_is_read_back_onto_the_same_faces, test_existing_pmi_suppresses_questions
#[test]
fn the_spool_is_written_verified_and_read_back() {
    let dir = temp("spool");
    let Some((intent, report, out, analysis)) = spool_written(&dir) else {
        println!("skipped: no corpus");
        return;
    };
    // Datums, requirements, material, general tolerance and the part's notes (surface finish
    // and edges at their defaults, Q3), and the one line counting the dimensions under the general tolerance, #63).
    assert_eq!(
        report.written.len(),
        // This frozen analysis predates the planner. Its missing Basic Ø90 radial
        // location gets one report line of its own (#75); General dimensions
        // still share the aggregate report line below (#63).
        intent.datums.len() + intent.requirements.len() + 5 + general_line(&report.written)
    );
    assert!(report.written.contains(&"material 6082-T6".to_string()));
    assert!(
        report
            .written
            .contains(&"surface finish Ra 3.2".to_string())
    );
    assert!(report.not_written.is_empty(), "{:?}", report.not_written);
    assert!(report.warnings.is_empty());

    // The limits: each fit is written as its ISO 286 class (Python wrote bare limits, as
    // OpenCascade writes the grade wrongly), including g6 wholly below nominal and p6 above.
    let read = pmi_of(&out, 0);
    assert_eq!(
        read.dimensions
            .iter()
            .filter(|d| d.tolerance == DimTolerance::Basic
                && d.nominal
                    .as_ref()
                    .is_some_and(|n| matches!(&n.quantity,pmi::Quantity::Length(l)
            if l.value.to_f64()==90.)))
            .count(),
        1
    );
    let classes: Vec<String> = read
        .dimensions
        .iter()
        .filter_map(|d| match &d.tolerance {
            DimTolerance::Fit { class, .. } => Some(class.to_string()),
            _ => None,
        })
        .collect();
    for req in intent.requirements.iter().filter(|r| r["kind"] == "size") {
        let fit = req["fit"].as_str().unwrap();
        assert!(classes.iter().any(|c| c == fit), "{fit} not in {classes:?}");
    }
    assert!(classes.iter().any(|c| c == "g6") && classes.iter().any(|c| c == "p6"));

    // Read back onto the same faces.
    let back = load(&out, None).unwrap();
    let found = read_existing(&back).unwrap();
    let positions: Vec<_> = found.iter().filter(|e| e.r#type == "Position").collect();
    // Axis-first turned defaults: the coaxial bore is positioned about A alone;
    // the off-axis pattern retains the full A|B frame.
    assert!(positions.iter().any(|e| e.datums == ["A"]));
    assert!(positions.iter().any(|e| e.datums == ["A", "B"]));
    for position in &positions {
        assert!(intent.requirements.iter().any(|r| r["kind"] == "position"
            && r["faces"] == json!(position.faces)
            && r["datums"] == json!(position.datums)));
    }
    let mut wanted: Vec<Vec<usize>> = intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "position")
        .map(|r| serde_json::from_value(r["faces"].clone()).unwrap())
        .collect();
    let mut got: Vec<Vec<usize>> = positions.iter().map(|e| e.faces.clone()).collect();
    wanted.sort();
    wanted.dedup();
    got.sort();
    got.dedup();
    assert_eq!(got, wanted);

    // Re-analysing the enriched file does not ask again for sizes it states.
    let before: Vec<String> = questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .map(|q| q.id)
        .collect();
    assert!(before.iter().any(|i| i.starts_with("hole.function")));
    let after = questions(&reanalysed(&analysis, &back), &Map::new()).unwrap();
    assert!(
        !after
            .iter()
            .any(|q| q.id.starts_with("hole.function:holes"))
    );
}

// test_writer.py::test_intent_for_another_file_is_refused
#[test]
fn intent_for_another_file_is_refused() {
    let dir = temp("another");
    let pin = load(&fixture("pin"), None).unwrap();
    match write(&pin, &plate_intent(), &dir.join("x.step"), None) {
        Err(WriteError::Invalid(m)) => assert!(m.contains("different file"), "{m}"),
        other => panic!("not refused: {other:?}"),
    }
    assert!(listing(&dir).is_empty());
}

// test_writer.py::test_unreferenced_datums_are_warned
#[test]
fn unreferenced_datums_are_warned() {
    let dir = temp("unreferenced");
    let analysis = analysis("parts/plate");
    let plate = load(&fixture("plate"), None).unwrap();
    let mut given = json!({"part.material": "steel"});
    for q in questions(&analysis, &Map::new()).unwrap() {
        if q.id.starts_with("hole.function") {
            given[q.id] = json!("general");
        }
    }
    let report = write(
        &plate,
        &apply(&analysis, given.clone()),
        &dir.join("p.step"),
        None,
    )
    .unwrap();
    assert_eq!(report.warnings.len(), 3, "{:?}", report.warnings);
    given["datum.B"] = json!([]);
    given["datum.C"] = json!([]);
    given["datum.A"] = json!([top_face(&analysis)]);
    let report = write(&plate, &apply(&analysis, given), &dir.join("q.step"), None).unwrap();
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    // haecceity writes and reads an unreferenced datum (Python's reader drops it).
    let back = read_existing(&load(&dir.join("q.step"), None).unwrap()).unwrap();
    assert!(back.iter().any(|e| e.kind == "datum" && e.r#type == "A"));
}

// test_writer.py::test_pmi_is_written_on_an_assembly_holding_one_solid
#[test]
fn pmi_is_written_on_an_assembly_holding_one_solid() {
    let dir = temp("wrapped");
    let loaded = load(&fixture("wrapped"), None).unwrap();
    let report = write(
        &loaded,
        &datum_and_position(&loaded),
        &dir.join("out.step"),
        None,
    )
    .unwrap();
    assert_eq!(report.written, ["datum A", "position 0.1 on bore"]);
}

// test_writer.py::test_tolerances_without_any_dimension_keep_their_size
#[test]
fn tolerances_without_any_dimension_keep_their_size() {
    let dir = temp("nodimension");
    let loaded = load(&fixture("plate"), None).unwrap();
    let out = dir.join("out.step");
    write(&loaded, &datum_and_position(&loaded), &out, None).unwrap();
    let back = read_existing(&load(&out, None).unwrap()).unwrap();
    let positions: Vec<_> = back.iter().filter(|e| e.r#type == "Position").collect();
    assert_eq!(positions.len(), 1);
    assert!((positions[0].value.unwrap() - 0.1).abs() < 1e-12);
}

// test_writer.py::test_intent_from_another_loader_version_is_refused
#[test]
fn intent_from_another_loader_version_is_refused() {
    let dir = temp("loader");
    let mut intent = plate_intent();
    intent.binding["loader_version"] = json!(1);
    let plate = load(&fixture("plate"), None).unwrap();
    match write(&plate, &intent, &dir.join("out.step"), None) {
        Err(WriteError::Invalid(m)) => assert_eq!(
            m,
            "intent was made by loader version 1, not 2: analyse the file again"
        ),
        other => panic!("not refused: {other:?}"),
    }
    assert!(listing(&dir).is_empty());
}

/// An intent haecceity refuses as invalid once everything else is checked: a flatness citing a
/// datum (`flatness_tolerance` WR1 forbids datums).
fn refused_intent(base: Intent) -> Intent {
    let mut intent = base;
    let faces = intent.datums[0]["faces"].clone();
    intent.requirements.push(answers(json!({
        "kind": "flatness", "feature": "", "faces": faces, "tolerance": 0.05, "datums": ["A"],
    })));
    intent
}

// test_writer.py::test_a_failed_write_leaves_the_destination_and_the_source. Python injects a
// failing `verify`; here the failures are real: haecceity refusing the intent, and a
// destination that cannot be replaced (a directory) once the file is written and verified.
#[test]
fn a_failed_write_leaves_the_destination_and_the_source() {
    let dir = temp("failed");
    let source = dir.join("part.step");
    std::fs::copy(fixture("plate"), &source).unwrap();
    let destination = dir.join("out.step");
    std::fs::write(&destination, "what was there before").unwrap();
    let before = std::fs::read(&source).unwrap();
    let loaded = load(&source, None).unwrap();
    let intent = refused_intent(plate_intent());
    for target in [&destination, &source] {
        match write(&loaded, &intent, target, None) {
            Err(WriteError::Invalid(m)) => assert!(m.contains("Flatness takes no datums"), "{m}"),
            other => panic!("not refused: {other:?}"),
        }
    }
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "what was there before"
    );
    assert_eq!(std::fs::read(&source).unwrap(), before);
    assert_eq!(listing(&dir), ["out.step", "part.step"]);

    // Written and verified, but the destination is a directory: nothing is left behind.
    let blocked = dir.join("blocked.step");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("inside"), "x").unwrap();
    assert!(matches!(
        write(&loaded, &plate_intent(), &blocked, None),
        Err(WriteError::Failed(_))
    ));
    assert_eq!(listing(&dir), ["blocked.step", "out.step", "part.step"]);
    assert_eq!(listing(&blocked), ["inside"]);
}

// test_writer.py::test_a_failed_merge_leaves_a_part_with_pmi_as_it_was: the add-mode path
// (a part with PMI of its own and no answers to resume) is staged too.
#[test]
fn a_failed_add_leaves_a_part_with_pmi_as_it_was() {
    let dir = temp("failedadd");
    let source = dir.join("part.step");
    let plate = load(&fixture("plate"), None).unwrap();
    write(&plate, &plate_intent(), &source, None).unwrap(); // PMI, and no answers to resume
    let loaded = load(&source, None).unwrap();
    assert!(!read_existing(&loaded).unwrap().is_empty());
    assert!(answers_for(&loaded).unwrap().is_none());
    let before = std::fs::read(&source).unwrap();
    let analysis = reanalysed(&analysis("parts/plate"), &loaded);
    let intent = refused_intent(apply(&analysis, json!({"part.material": "steel"})));
    assert!(matches!(
        write(&loaded, &intent, &source, None),
        Err(WriteError::Invalid(_))
    ));
    assert_eq!(std::fs::read(&source).unwrap(), before);
    assert_eq!(listing(&dir), ["part.step"]);
}

// test_writer.py::test_the_output_keeps_its_permissions_and_its_symlink
#[cfg(unix)]
#[test]
fn the_output_keeps_its_permissions_and_its_symlink() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp("symlink");
    let target = dir.join("real.step");
    std::fs::write(&target, "old").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.join("link.step");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let plate = load(&fixture("plate"), None).unwrap();
    let report = write(&plate, &plate_intent(), &link, None).unwrap();
    assert_eq!(report.output, link);
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(std::fs::read_to_string(&target).unwrap().contains("DATUM("));
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(listing(&dir), ["link.step", "real.step"]);
}

// test_writer.py::test_the_command_line_says_why_it_cannot_write is the command line's
// (stage 4); its refusal is `intent_from_another_loader_version_is_refused` above.

// --- binding checks, with Python's messages -----------------------------------------------------

#[test]
fn the_binding_is_checked_with_pythons_messages() {
    let dir = temp("binding");
    let plate = load(&fixture("plate"), None).unwrap();
    let refused = |intent: &Intent| match write(&plate, intent, &dir.join("x.step"), None) {
        Err(WriteError::Invalid(m)) => m,
        other => panic!("not refused: {other:?}"),
    };
    let mut intent = plate_intent();
    intent.binding["part"] = json!(1);
    assert_eq!(refused(&intent), "intent was made for part 1, not part 0");
    let mut intent = plate_intent();
    intent.binding["face_count"] = json!(13);
    assert_eq!(
        refused(&intent),
        "intent face count does not match the file"
    );
    let mut intent = plate_intent();
    intent
        .binding
        .as_object_mut()
        .unwrap()
        .remove("loader_version");
    assert_eq!(
        refused(&intent),
        "intent was made by loader version None, not 2: analyse the file again"
    );
    // An intent made before parts were told apart is part 0's
    // (test_assembly.py::test_an_intent_made_before_parts_were_told_apart_still_writes).
    let mut intent = plate_intent();
    intent.binding.as_object_mut().unwrap().remove("part");
    assert!(write(&plate, &intent, &dir.join("old.step"), None).is_ok());
    assert!(matches!(
        specify_core::writer::write_parts(&[], &dir.join("x.step")),
        Err(WriteError::Invalid(m)) if m == "nothing to write"
    ));
}

// --- test_requirements.py (semantic content) ----------------------------------------------------

/// `pin_written`: an M6 thread and a straight knurl on the pin.
fn pin_written(dir: &Path) -> (Value, WriteReport, PathBuf) {
    let analysis = analysis("parts/pin");
    let given = json!({
        "part.material": "CW614N leaded brass",
        format!("diameter.fit:{}", feature(&analysis, "turned_steps", 6.0)): "thread:M6",
        format!("diameter.fit:{}", feature(&analysis, "turned_steps", 10.0)): "knurl:straight",
    });
    let out = dir.join("pin-pmi.step");
    let pin = load(&fixture("pin"), None).unwrap();
    let report = write(&pin, &apply(&analysis, given), &out, None).unwrap();
    (analysis, report, out)
}

// test_requirements.py::test_everything_is_written_and_verified. The straight knurl is written
// as AP242's turned_knurl, its parameters from DIN 82 (questions Q4, Q15).
#[test]
fn everything_is_written_and_verified() {
    let dir = temp("pin");
    let (_, report, _) = pin_written(&dir);
    for wanted in [
        "internal thread M5x0.8-6H",
        "external thread M6x1-6g",
        "material CW614N leaded brass",
        "general tolerance ISO 2768-m",
        "straight knurl on turned_steps:0",
    ] {
        assert!(
            report.written.iter().any(|w| w.starts_with(wanted)),
            "{wanted}"
        );
    }
    assert!(!report.not_written.iter().any(|w| w.contains("knurl")));
}

// test_requirements.py::test_the_standard_constructs, re-expressed: threads as AP242's THREAD
// with their manufacturing data as a user defined attribute set (U8), the general tolerance
// as the 'default tolerances' class, the material as 'material name'.
#[test]
fn the_standard_constructs() {
    let dir = temp("constructs");
    let (_, _, out) = pin_written(&dir);
    let text = flat(&out);
    assert_eq!(
        Regex::new(r"= ?THREAD\(").unwrap().find_iter(&text).count(),
        2
    );
    assert_eq!(
        Regex::new(r"GENERAL_PROPERTY\('','thread manufacturing',\$\)")
            .unwrap()
            .find_iter(&text)
            .count(),
        2
    );
    assert!(Regex::new(r"MEASURE_WITH_UNIT\(LENGTH_MEASURE\(5\.5\),#\d+\)REPRESENTATION_ITEM\('minimum full thread'\)").unwrap().is_match(&text));
    assert!(text.contains("DESCRIPTIVE_REPRESENTATION_ITEM('tolerance class','ISO 2768-m')"));
    assert!(text.contains("PROPERTY_DEFINITION('material property','material name'"));
    assert!(!text.contains("DEFAULT_TOLERANCE_TABLE"));
    let read = pmi_of(&out, 0);
    let fits: Vec<&str> = read.threads.iter().map(|t| t.fit_class.as_str()).collect();
    assert!(fits.contains(&"6H") && fits.contains(&"6g"), "{fits:?}");
}

// test_requirements.py::test_the_file_still_reads_with_the_same_faces,
// test_faces_resolve_inside_an_assembly
#[test]
fn the_file_still_reads_with_the_same_faces() {
    let dir = temp("faces");
    let (_, _, out) = pin_written(&dir);
    let pin = load(&fixture("pin"), None).unwrap();
    let back = load(&out, None).unwrap();
    assert_eq!(back.binding.face_count, pin.binding.face_count);
    assert_eq!(back.definition.faces.len(), pin.binding.face_count);
    let wrapped = load(&fixture("wrapped"), None).unwrap();
    assert_eq!(wrapped.definition.faces.len(), wrapped.binding.face_count);
}

// test_requirements.py::test_its_callouts_read_back_as_notes_not_as_sizes_of_nothing
#[test]
fn threads_read_back_as_notes_not_as_sizes_of_nothing() {
    let dir = temp("notes");
    let (_, _, out) = pin_written(&dir);
    let existing = read_existing(&load(&out, None).unwrap()).unwrap();
    assert!(
        !existing
            .iter()
            .any(|e| e.r#type == "CommonLabel" || e.kind == "dimension")
    );
    let kinds: Vec<&str> = existing
        .iter()
        .filter(|e| e.kind == "note")
        .map(|e| e.r#type.as_str())
        .collect();
    assert!(kinds.contains(&"internal thread") && kinds.contains(&"external thread"));
}

// test_requirements.py::test_a_finish_on_faces_is_not_a_second_default: the default is the
// part's finish (a surface texture on the part) and the one on faces is a note (a surface
// texture on them), each written once (Q3).
#[test]
fn a_finish_on_faces_is_not_a_second_default() {
    let dir = temp("finish");
    let analysis = analysis("parts/pin");
    let top = top_face(&analysis);
    let given = json!({
        "part.material": "brass",
        "extra.0": {"faces": [top], "kind": "finish", "value": "Ra 0.8"},
    });
    let pin = load(&fixture("pin"), None).unwrap();
    let out = dir.join("finishes.step");
    let report = write(&pin, &apply(&analysis, given), &out, None).unwrap();
    assert!(report.not_written.is_empty(), "{:?}", report.not_written);
    assert!(
        report
            .written
            .contains(&format!("surface finish Ra 0.8 on faces [{top}]"))
    );
    let textures = &pmi_of(&out, 0).surface_textures;
    assert_eq!(textures.len(), 2);
    assert_eq!(textures.iter().filter(|t| t.on.is_none()).count(), 1);
    let back = load(&out, None).unwrap();
    assert_eq!(
        part_settings(&back).unwrap()["surface_finish"],
        json!("Ra 3.2")
    );
    assert!(
        read_existing(&back)
            .unwrap()
            .iter()
            .any(|e| e.kind == "note" && e.r#type == "surface finish" && e.faces == [top as usize])
    );
}

// --- test_merge.py, for haecceity's add mode ------------------------------------------------------

/// `enriched`: the plate with datums A, B, C and a fitted, positioned Ø8 bore, written without
/// answers (so its PMI is, to a later write, the file's own).
fn enriched(dir: &Path) -> PathBuf {
    let analysis = analysis("parts/plate");
    let given = json!({
        "part.material": "steel",
        question(&analysis, "hole.function", "Ø8"): "fit:H7",
        question(&analysis, "hole.function", "6×"): "general",
    });
    let out = dir.join("first.step");
    let plate = load(&fixture("plate"), None).unwrap();
    write(&plate, &apply(&analysis, given), &out, None).unwrap();
    out
}

/// A fit is written as its ISO 286 class alone, so an intent whose deviations are not the
/// class's is refused rather than written with a meaning it does not state.
#[test]
fn a_fit_whose_limits_are_not_its_class_is_refused() {
    let dir = temp("fit-limits");
    let analysis = analysis("parts/plate");
    let given =
        json!({"part.material": "steel", question(&analysis, "hole.function", "Ø8"): "fit:H7"});
    let mut intent = apply(&analysis, given);
    let size = intent
        .requirements
        .iter_mut()
        .find(|r| r["kind"] == "size" && r["fit"] == "H7")
        .expect("the H7 size");
    size.insert("upper".into(), json!(0.05));
    let out = dir.join("out.step");
    let plate = load(&fixture("plate"), None).unwrap();
    let err = write(&plate, &intent, &out, None).unwrap_err();
    assert!(
        matches!(&err, WriteError::Invalid(m) if m.contains("H7") && m.contains("not the class's")),
        "{err:?}"
    );
    assert!(!out.exists());
}

// test_merge.py::test_only_a_part_with_pmi_is_merged, test_what_the_file_states_is_not_asked_
// again, test_the_new_pmi_cites_the_files_datums, test_the_original_file_is_kept_byte_for_byte
#[test]
fn pmi_is_added_to_a_part_that_has_some() {
    let dir = temp("merge");
    let first = enriched(&dir);
    let plate = load(&fixture("plate"), None).unwrap();
    assert!(read_existing(&plate).unwrap().is_empty());
    let loaded = load(&first, None).unwrap();
    assert!(!read_existing(&loaded).unwrap().is_empty());

    let analysis = reanalysed(&analysis("parts/plate"), &loaded);
    let qs = questions(&analysis, &Map::new()).unwrap();
    let ids: Vec<&str> = qs.iter().map(|q| q.id.as_str()).collect();
    for stated in [
        "datum.A",
        "datum.B",
        "datum.C",
        "part.material",
        "part.general_tolerance",
    ] {
        assert!(!ids.contains(&stated), "{stated} asked again");
    }
    // What the first pass wrote is read back, its notes too (Q3); "no coating" and "no heat
    // treatment" and an empty free-text note list write nothing, so they are offered again.
    for q in ids.iter().filter(|i| i.starts_with("part.")) {
        assert!(
            ["part.coating", "part.heat_treatment", "part.notes"].contains(q),
            "{q}"
        );
    }
    assert!(!qs.iter().any(|q| q.prompt.contains("Ø8")));
    let six = qs
        .iter()
        .find(|q| q.prompt.contains("6×"))
        .unwrap()
        .id
        .clone();
    let intent = apply(&analysis, json!({ six: "clearance:M6" }));
    let second = dir.join("second.step");
    let report = write(&loaded, &intent, &second, None).unwrap();
    assert!(
        !report
            .written
            .iter()
            .any(|w| w.starts_with("material") || w.starts_with("general tolerance"))
    );

    // The new PMI cites the file's datums: none is written again.
    let joined = report.written.join(" ");
    assert!(
        Regex::new(r"position [\d.]+ MMC on hole_patterns")
            .unwrap()
            .is_match(&joined)
    );
    assert!(!report.written.iter().any(|w| w.starts_with("datum ")));
    let letters: Vec<String> = Regex::new(r"DATUM\('[^']*',[^;]*'(\w)'\);")
        .unwrap()
        .captures_iter(&std::fs::read_to_string(&second).unwrap())
        .map(|c| c[1].to_string())
        .collect();
    let mut sorted = letters.clone();
    sorted.sort();
    assert_eq!(sorted, ["A", "B", "C"]);

    // The original file is kept byte for byte around what was added, and all it stated reads
    // back with more beside it.
    let (before, after) = (
        std::fs::read_to_string(&first).unwrap(),
        std::fs::read_to_string(&second).unwrap(),
    );
    let end = before.rfind("ENDSEC;").unwrap();
    assert!(after.starts_with(&before[..end]) && after.ends_with(&before[end..]));
    let old = read_existing(&loaded).unwrap();
    let new = read_existing(&load(&second, None).unwrap()).unwrap();
    assert!(old.iter().all(|e| new.contains(e)) && new.len() > old.len());
    // Its PMI is not the writer's own: no answers are stored.
    assert!(stored(after.as_bytes()).is_empty());
}

// test_merge.py::test_tolerances_written_simply_survive_the_merge
#[test]
fn tolerances_with_datums_survive_an_add() {
    let dir = temp("simple");
    let first = enriched(&dir);
    let loaded = load(&first, None).unwrap();
    let analysis = reanalysed(&analysis("parts/plate"), &loaded);
    let faces = analysis["faces"].as_array().unwrap();
    let bore = faces
        .iter()
        .find(|f| f["kind"] == "cylinder" && (f["radius"].as_f64().unwrap() - 4.0).abs() < 1e-6)
        .unwrap()["id"]
        .clone();
    let top = top_face(&analysis);
    let given = json!({
        "extra.0": {"faces": [bore], "kind": "runout", "value": "0.02", "datums": ["A"]},
        "extra.1": {"faces": [bore], "kind": "total_runout", "value": "0.02", "datums": ["A"]},
        "extra.2": {"faces": [top], "kind": "parallelism", "value": "0.02", "datums": ["A"]},
    });
    let out = dir.join("simple.step");
    write(&loaded, &apply(&analysis, given), &out, None).unwrap();
    let read = types(&out);
    for t in ["CircularRunout", "TotalRunout", "Parallelism"] {
        assert!(read.iter().any(|x| x == t), "{t} in {read:?}");
    }
}

/// NIST CTC-01 (AP242 with semantic PMI), from the committed fixture.
fn ctc01() -> (common::Case, PathBuf) {
    let case = common::case("nist/nist_ctc_01_asme1_ap242");
    let input = case.input().unwrap();
    (case, input)
}

// test_merge.py::test_a_nist_part_keeps_all_its_pmi, test_a_files_own_datums_are_not_drawn_
// again, test_a_file_with_pmi_from_elsewhere_stores_no_answers
#[test]
fn a_nist_part_keeps_all_its_pmi() {
    let dir = temp("ctc01");
    let (case, input) = ctc01();
    let analysis = case.artifact("analysis").unwrap();
    let asked: Vec<String> = questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .map(|q| q.id.split(':').next().unwrap().to_string())
        .collect();
    let allowed = [
        "part.material",
        "part.general_tolerance",
        "part.surface_finish",
        "part.coating",
        "part.heat_treatment",
        "part.edges",
        "part.notes",
        "datum.B.control",
        "datum.C.control",
    ];
    assert!(
        asked.iter().all(|a| allowed.contains(&a.as_str())),
        "{asked:?}"
    );
    let given = json!({"part.material": "CZ121"});
    let loaded = load(&input, None).unwrap();
    let out = dir.join("ctc01.step");
    let report = write(
        &loaded,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    assert_eq!(
        report.written,
        [
            // #75: three raw counted General bore groups overlap stored fits and
            // cannot replace their contracts. Report them; append the 14 reconciled
            // requirements and preserve the 12 original dimensions.
            "14 dimensions under the general tolerance",
            "material CZ121",
            "general tolerance ISO 2768-m",
            "surface finish Ra 3.2",
            "edges Break sharp edges 0.2 max"
        ]
    );
    let old = read_existing(&loaded).unwrap();
    let new = read_existing(&load(&out, None).unwrap()).unwrap();
    assert_eq!(old, new);
    assert_eq!(pmi_of(&out, 0).dimensions.len(), 26); // 12 original + 14 reconciled quantities.
    assert_eq!(report.not_written.iter().cloned().collect::<std::collections::BTreeSet<_>>(),[
        "shared planner dim:multiple[hole:50.51.56.57,hole:52.53.58.59]/bore.diameter: General contract conflicts with an answered requirement; retained answered intent instead",
        "shared planner dim:pattern[hole:0.1,hole:91.92,hole:93.94,hole:95.96]/bore.diameter: General contract conflicts with an answered requirement; retained answered intent instead",
        "shared planner dim:pattern[hole:71.72,hole:73.74,hole:75.76,hole:89.90]/bore.diameter: General contract conflicts with an answered requirement; retained answered intent instead",
    ].into_iter().map(String::from).collect());
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("DRAUGHTING_CALLOUT('Datum "));
    assert!(stored(text.as_bytes()).is_empty());
    // #75 adds dimension-owned origin/planner UDAs. As for #77, AP242 ed1 excludes
    // those owners, so the existing writer policy upgrades only FILE_SCHEMA to ed4.
    // Require that exact header change and every other original byte to survive.
    let original = std::fs::read_to_string(&input).unwrap();
    let schema = "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }";
    assert_eq!(original.matches(schema).count(), 1);
    let expected = original.replacen(schema, haecceity::express::TARGET_SCHEMA, 1);
    let end = expected.rfind("ENDSEC;").unwrap();
    assert!(text.starts_with(&expected[..end]) && text.ends_with(&expected[end..]));
}

// --- test_threads.py, test_mmc_finish_notes.py (write side) ------------------------------------

// test_threads.py::test_a_typed_thread_depth_is_written_and_read_back (the write side). The
// sentence "5.6 mm minimum full thread" is the text route, not written (not-applicable).
#[test]
fn a_typed_thread_depth_is_written_and_read_back() {
    let dir = temp("depth");
    let analysis = analysis("parts/pin");
    let hole = format!("hole.function:{}", feature(&analysis, "holes", 4.2));
    let given = json!({"part.material": "brass", hole: "tapped:M5@5.6"});
    let out = dir.join("deep.step");
    let pin = load(&fixture("pin"), None).unwrap();
    let report = write(
        &pin,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    assert!(report.not_written.is_empty(), "{:?}", report.not_written);
    let text = flat(&out);
    assert!(Regex::new(r"MEASURE_WITH_UNIT\(LENGTH_MEASURE\(5\.6\),#\d+\)REPRESENTATION_ITEM\('minimum full thread'\)").unwrap().is_match(&text));
    assert!(Regex::new(r"MEASURE_WITH_UNIT\(LENGTH_MEASURE\(8\.\),#\d+\)REPRESENTATION_ITEM\('tapping drill depth'\)").unwrap().is_match(&text));
    assert_eq!(
        answers_for(&load(&out, None).unwrap()).unwrap(),
        given.as_object().cloned()
    );
}

/// test_mmc_finish_notes.py's `written`: MMC positions, every part note and a face finish.
fn notes_written(dir: &Path) -> (Intent, WriteReport, PathBuf, i64) {
    let analysis = analysis("parts/plate");
    let top = top_face(&analysis);
    let given = json!({
        "part.material": "Aluminium 6082-T6",
        question(&analysis, "hole.function", "Ø8"): "fit:H7",
        "part.coating": "Anodise black (MIL-A-8625 Type II)",
        "part.heat_treatment": "Solution treat and age to T6",
        "part.surface_finish": "Ra 1.6",
        "extra.1": {"faces": [top], "kind": "finish", "value": "0.8"},
    });
    let intent = apply(&analysis, given);
    let out = dir.join("plate.step");
    let plate = load(&fixture("plate"), None).unwrap();
    let report = write(&plate, &intent, &out, None).unwrap();
    (intent, report, out, top)
}

// test_mmc_finish_notes.py::test_clearance_holes_are_positioned_at_mmc_and_fitted_ones_are_not
// (write side), test_the_parts_notes_are_written_and_read_back, test_a_face_finish_is_written_
// on_its_face (re-expressed: a surface texture on the face, Q3, not Python's text route) and
// test_nothing_is_written_for_no_coating_or_heat_treatment
#[test]
fn mmc_positions_are_written_and_notes_are_read_back() {
    let dir = temp("mmc");
    let (intent, report, out, top) = notes_written(&dir);
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("position") && w.contains(" MMC on "))
    );
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("MAXIMUM_MATERIAL_REQUIREMENT")
    );
    assert!(
        intent.part["coating"]
            .as_str()
            .unwrap()
            .starts_with("Anodise black")
    );
    assert!(report.not_written.is_empty(), "{:?}", report.not_written);
    for line in [
        "surface finish Ra 1.6".to_string(),
        "edges Break sharp edges 0.2 max".to_string(),
        format!("surface finish Ra 0.8 on faces [{top}]"),
    ] {
        assert!(report.written.contains(&line), "{line}");
    }
    let back = load(&out, None).unwrap();
    assert_eq!(
        part_settings(&back).unwrap(),
        answers(json!({
            "material": "Aluminium 6082-T6",
            "general_tolerance": "ISO 2768-m",
            "surface_finish": "Ra 1.6",
            "coating": "Anodise black (MIL-A-8625 Type II)",
            "heat_treatment": "Solution treat and age to T6",
            "edges": "Break sharp edges 0.2 max",
        }))
    );
    assert!(
        read_existing(&back)
            .unwrap()
            .iter()
            .any(|e| e.kind == "note"
                && e.r#type == "surface finish"
                && e.faces.contains(&(top as usize)))
    );

    let plain = apply(&analysis("parts/plate"), json!({"part.material": "steel"}));
    assert!(!plain.part.contains_key("coating") && !plain.part.contains_key("heat_treatment"));
    let p = dir.join("p.step");
    write(&load(&fixture("plate"), None).unwrap(), &plain, &p, None).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(
        !Regex::new(r"'(surface treatment|heat treatment|coating)'")
            .unwrap()
            .is_match(&text)
    );
}

// --- test_conformance.py, as kept -----------------------------------------------------------------

/// Every instance of the file is a valid AP242 instance and no EXPRESS rule haecceity checks
/// is violated.
fn assert_conformant(path: &Path) {
    let doc = Document::parse(std::fs::read(path).unwrap()).unwrap();
    let violations = haecceity::express::validate_document(doc.graph());
    assert!(violations.is_empty(), "{}: {violations:?}", path.display());
    let rules = haecceity::express_rules::check_all(&doc);
    assert!(rules.is_empty(), "{}: {rules:?}", path.display());
}

// test_conformance.py::test_no_malformed_runout_zone_is_written: the zone is written as its
// tolerance zone form only, every reference resolves, and the file conforms.
#[test]
fn no_malformed_runout_zone_is_written() {
    let dir = temp("zones");
    let analysis = analysis("parts/pin");
    let runout = questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with("diameter.runout"))
        .unwrap()
        .id;
    let intent = apply(&analysis, json!({"part.material": "brass", runout: "0.02"}));
    assert!(
        intent
            .requirements
            .iter()
            .any(|r| r.get("diametral") == Some(&json!(true)))
    );
    assert!(intent.requirements.iter().any(|r| r["kind"] == "runout"));
    let out = dir.join("zones.step");
    let report = write(&load(&fixture("pin"), None).unwrap(), &intent, &out, None).unwrap();
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("runout 0.02 on "))
    );
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("RUNOUT_ZONE"));
    assert!(!text.contains("PLANE_ANGLE_UNIT() SI_UNIT($,.METRE.)"));
    assert!(text.contains("TOLERANCE_ZONE_FORM('cylindrical or circular')"));
    let data = text.split_once("DATA;").unwrap().1;
    let ids: std::collections::BTreeSet<&str> = Regex::new(r"(?m)^#(\d+)\s*=")
        .unwrap()
        .captures_iter(&text)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();
    for r in Regex::new(r"#(\d+)").unwrap().captures_iter(data) {
        assert!(
            ids.contains(r.get(1).unwrap().as_str()),
            "#{} dangles",
            &r[1]
        );
    }
    assert_conformant(&out);
}

// test_conformance.py::test_callout_presentation_is_named, as far as it applies: no unnamed
// tessellated presentation (the datum symbols are named 'datum'); the note leaders it checks
// are not written.
#[test]
fn presentation_is_named() {
    let dir = temp("named");
    let (_, _, out) = pin_written(&dir);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("TESSELLATED_GEOMETRIC_SET(''"));
    assert!(!text.contains("TESSELLATED_ANNOTATION_OCCURRENCE(''"));
    assert!(text.contains("TESSELLATED_GEOMETRIC_SET('datum'"));
    assert_conformant(&out);
}

// test_conformance.py::test_limits_are_typed_lengths_and_runout_is_simple
#[test]
fn limits_are_typed_lengths_and_runout_is_simple() {
    let dir = temp("typed");
    let analysis = analysis("parts/pin");
    let qs = questions(&analysis, &Map::new()).unwrap();
    let runout = qs
        .iter()
        .find(|q| q.id.starts_with("diameter.runout"))
        .unwrap()
        .id
        .clone();
    let fit = qs
        .iter()
        .find(|q| q.id.starts_with("diameter.fit"))
        .unwrap()
        .id
        .clone();
    let intent = apply(
        &analysis,
        json!({"part.material": "brass", runout: "0.02", fit: "g6"}),
    );
    assert!(intent.requirements.iter().any(|r| r["kind"] == "size"));
    let out = dir.join("typed.step");
    write(&load(&fixture("pin"), None).unwrap(), &intent, &out, None).unwrap();
    let text = flat(&out);
    assert!(
        !Regex::new(r"= ?MEASURE_WITH_UNIT\(\s*[-+0-9.]")
            .unwrap()
            .is_match(&text)
    );
    assert!(text.contains("LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE("));
    assert!(
        !Regex::new(r"(RUNOUT|PARALLELISM|PERPENDICULARITY)_TOLERANCE\s*\(\s*\)")
            .unwrap()
            .is_match(&text)
    );
    assert!(
        Regex::new(r"= ?CIRCULAR_RUNOUT_TOLERANCE\('")
            .unwrap()
            .is_match(&text)
    );
    // g6 lies wholly below nominal; it is written as the class.
    assert!(
        text.contains("LIMITS_AND_FITS('g','shaft','6',"),
        "no g6 class"
    );
    assert_conformant(&out);
}

// test_conformance.py::test_every_datum_referenced_tolerance_is_written_simply
#[test]
fn every_datum_referenced_tolerance_is_written_simply() {
    let dir = temp("kinds");
    let analysis = analysis("parts/plate");
    let planes: Vec<&Value> = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "plane")
        .collect();
    let top = top_face(&analysis);
    let side = planes
        .iter()
        .find(|f| f["direction"][2].as_f64().unwrap().abs() < 0.1)
        .unwrap()["id"]
        .clone();
    let bore = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "cylinder")
        .unwrap()["id"]
        .clone();
    let mut given = json!({"part.material": "steel"});
    for (n, (face, kind)) in [
        (json!(top), "parallelism"),
        (side, "perpendicularity"),
        (bore.clone(), "runout"),
        (bore, "total_runout"),
    ]
    .into_iter()
    .enumerate()
    {
        given[format!("extra.{n}")] =
            json!({"faces": [face], "kind": kind, "value": "0.05", "datums": ["A"]});
    }
    let out = dir.join("kinds.step");
    write(
        &load(&fixture("plate"), None).unwrap(),
        &apply(&analysis, given),
        &out,
        None,
    )
    .unwrap();
    let text = flat(&out);
    assert!(
        !Regex::new(r"(PARALLELISM|PERPENDICULARITY|RUNOUT)_TOLERANCE\s*\(\s*\)")
            .unwrap()
            .is_match(&text)
    );
    for kind in [
        "PARALLELISM",
        "PERPENDICULARITY",
        "CIRCULAR_RUNOUT",
        "TOTAL_RUNOUT",
    ] {
        assert!(
            Regex::new(&format!(r"= ?{kind}_TOLERANCE\('"))
                .unwrap()
                .is_match(&text),
            "{kind}"
        );
    }
    assert_conformant(&out);
}

// --- test_drawn.py: datum symbols --------------------------------------------------------------

// test_drawn.py::test_a_datum_symbol_presents_its_datum_feature, against haecceity's symbols.
// (test_a_note_is_its_leader_and_no_words: no note leaders are written, not-applicable.)
#[test]
fn a_datum_symbol_presents_its_datum_feature() {
    let dir = temp("drawn");
    // Exercise both axis and plane symbols independently of seat defaults: pin_written
    // makes both diameters rough, so neither can now supply a default datum axis.
    let intent = apply(
        &analysis("parts/pin"),
        json!({"part.material": "brass", "datum.A": [0], "datum.B": [2]}),
    );
    let out = dir.join("datum-symbols.step");
    write(&load(&fixture("pin"), None).unwrap(), &intent, &out, None).unwrap();
    let text = flat(&out);
    let names: Vec<(String, String)> = Regex::new(r"#(\d+)\s*=\s*DRAUGHTING_CALLOUT\('([^']+)'")
        .unwrap()
        .captures_iter(&text)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect();
    let symbols: Vec<&String> = names
        .iter()
        .filter(|(_, n)| n.starts_with("Datum "))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(symbols.len(), 2, "{names:?}");
    let linked: std::collections::BTreeMap<String, String> =
        Regex::new(r"DRAUGHTING_MODEL_ITEM_ASSOCIATION\('[^']*','',#(\d+),#\d+,#(\d+)\)")
            .unwrap()
            .captures_iter(&text)
            .map(|c| (c[2].to_string(), c[1].to_string()))
            .collect();
    for callout in symbols {
        let aspect = &linked[callout];
        assert!(
            Regex::new(&format!(r"#{aspect}\s*=\s*DATUM_FEATURE\("))
                .unwrap()
                .is_match(&text),
            "callout #{callout} presents #{aspect}, not a datum feature"
        );
    }
}

// --- determinism ---------------------------------------------------------------------------------

#[test]
fn writing_twice_gives_identical_bytes() {
    let dir = temp("twice");
    let analysis = analysis("parts/plate");
    let given = json!({"part.material": "steel"});
    let intent = apply(&analysis, given.clone());
    let plate = load(&fixture("plate"), None).unwrap();
    let (a, b) = (dir.join("a.step"), dir.join("b.step"));
    write(&plate, &intent, &a, given.as_object()).unwrap();
    write(&plate, &intent, &b, given.as_object()).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

// --- requirements on no faces (question Q17) -----------------------------------------------------

/// Python's own accept-defaults intent of a corpus case whose rules give a hole's requirements
/// `faces: []` is reconciled by stable recognition identity where possible. Truly unbound
/// requirements remain Q17 diagnostics; unsupported planner quantities are reported too.
/// Every writable requirement is verified by reading back. Skipped without the corpus.
fn write_the_rest(name: &str, faceless: &[&str]) {
    let case = common::case(name);
    let Some(input) = case.input() else {
        return;
    };
    let given = case.meta["answers"].as_object().cloned();
    let loaded = load(&input, None).unwrap();
    // Retain the frozen radius-filter evidence. The shared input supplies exact
    // recognition identities, so generated answered bores can recover their supports.
    let mut analysis = case.artifact("analysis").unwrap();
    analysis["dimensioning"] = specify_core::api::analyse(&loaded).unwrap()["dimensioning"].clone();
    let intent =
        specify_core::rules::apply_with_defaults(&analysis, given.as_ref().unwrap(), true).unwrap();
    let dir = temp(&name.replace('/', "_"));
    let out = dir.join("written.step");
    let report = write(&load(&input, None).unwrap(), &intent, &out, given.as_ref()).unwrap();

    let reason = " names no faces, so nothing in the file can carry it (question Q17)";
    let listed: Vec<&String> = report
        .not_written
        .iter()
        .filter(|l| l.ends_with(reason))
        .collect();
    let wanted: Vec<String> = faceless.iter().map(|w| format!("{w}{reason}")).collect();
    assert_eq!(listed, wanted.iter().collect::<Vec<_>>());
    let deferred: Vec<_> = intent
        .requirements
        .iter()
        .filter_map(|r| r.get("not_written_reason").and_then(Value::as_str))
        .collect();
    assert!(
        deferred
            .iter()
            .all(|reason| report.not_written.iter().any(|r| r == *reason))
    );
    // Reported deferrals do not masquerade as written requirements. Every other
    // requirement, datum, setting and note still contributes its exact report line.
    let notes = specify_core::rules::NOTES
        .iter()
        .filter(|k| intent.part.get(**k).is_some_and(|v| v.as_str() != Some("")))
        .count();
    assert_eq!(
        report.written.len(),
        intent.datums.len() + intent.requirements.len() - faceless.len() - deferred.len()
            + 2
            + notes
            + general_line(&report.written),
        "{:?}",
        report.written
    );
    assert!(report.written.contains(&"material steel".to_string()));

    // What reads back is what was written: no dimension or tolerance of the faceless items.
    let read = pmi_of(&out, 0);
    let on_faces = |kinds: &[&str]| {
        intent
            .requirements
            .iter()
            .filter(|r| kinds.contains(&r["kind"].as_str().unwrap_or("")))
            .filter(|r| r["faces"].as_array().is_some_and(|f| !f.is_empty()))
            .count()
    };
    // Those with a tolerance of their own: the rest are under the general tolerance (#63).
    let toleranced = read
        .dimensions
        .iter()
        .filter(|d| !matches!(d.tolerance, DimTolerance::None))
        .count();
    assert_eq!(toleranced, on_faces(&["size", "location"]));
    assert_eq!(
        read.tolerances.len(),
        on_faces(&[
            "position",
            "flatness",
            "perpendicularity",
            "parallelism",
            "profile",
            "runout",
            "total_runout"
        ])
    );
    assert_eq!(read.datums.len(), intent.datums.len());
    assert_conformant(&out);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pattern_on_no_faces_is_listed_and_the_rest_written() {
    write_the_rest(
        "corpus/cadgenbench_inputs/cgb217",
        &[
            "the size on hole_patterns.set:105.106.107.108.109.110",
            "the position tolerance on hole_patterns.set:105.106.107.108.109.110",
        ],
    );
}

#[test]
fn a_recognised_bore_recovers_its_answered_supports_instead_of_becoming_q17() {
    // Its H11 size, two basics and position were previously lost to an exact
    // radius comparison. Stable face-derived identity makes them writable (#75).
    write_the_rest("corpus/mfcadpp/1004", &[]);
}

#[test]
fn an_unbound_raw_requirement_is_listed_and_the_rest_written() {
    let dir = temp("unbound");
    let p = load(&fixture("plate"), None).unwrap();
    let mut i = Intent::new(json!(p.binding));
    i.requirements.push(answers(
        json!({"kind":"size","feature":"unbound","faces":[],"nominal":8.,"upper":0.1,"lower":-0.1}),
    ));
    i.requirements.push(answers(
        json!({"kind":"flatness","faces":[0],"tolerance":0.01}),
    ));
    let out = dir.join("written.step");
    let report = write(&p, &i, &out, None).unwrap();
    assert_eq!(
        report.not_written,
        ["the size on unbound names no faces, so nothing in the file can carry it (question Q17)"]
    );
    let read = pmi_of(&out, 0);
    assert!(read.dimensions.is_empty());
    assert_eq!(read.tolerances.len(), 1);
    assert_conformant(&out);
}

// --- knurls (questions Q4, Q15) ----------------------------------------------------------------

/// The pin with a `pattern` knurl on Ø10, its intent and the file written.
fn pin_knurled(dir: &Path, pattern: &str) -> (Intent, WriteReport, PathBuf) {
    let analysis = analysis("parts/pin");
    let given = json!({
        "part.material": "brass",
        format!("diameter.fit:{}", feature(&analysis, "turned_steps", 10.0)): format!("knurl:{pattern}"),
    });
    let intent = apply(&analysis, given.clone());
    let out = dir.join(format!("{pattern}.step"));
    let pin = load(&fixture("pin"), None).unwrap();
    let report = write(&pin, &intent, &out, given.as_object()).unwrap();
    (intent, report, out)
}

/// The knurl read back from the file, with its parameters as plain numbers.
fn knurl_back(out: &Path) -> (pmi::KnurlPattern, Vec<f64>, Option<u32>, Option<f64>, bool) {
    let read = pmi_of(out, 0);
    assert_eq!(read.knurls.len(), 1, "{:?}", read.knurls);
    let k = &read.knurls[0];
    let mm = |l: &pmi::Length| l.value.to_string().parse::<f64>().unwrap();
    let lengths = [
        Some(&k.major_diameter),
        Some(&k.nominal_diameter),
        Some(&k.diametral_pitch),
        k.tooth_depth.as_ref(),
        k.root_fillet.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(mm)
    .collect();
    let angle = k.helix_angle.as_ref().map(|a| a.rad().to_degrees());
    (
        k.pattern,
        lengths,
        k.number_of_teeth.map(|c| c.0),
        angle,
        k.helix_hand.is_some(),
    )
}

/// A straight knurl is written through turned_knurl with its diameter, pitch and DIN 82's
/// number of teeth, tooth depth and root fillet (six parameters, WR2), no helix angle or hand,
/// and reads back as written; its faces read back as a knurl note; its answers resume.
#[test]
fn a_straight_knurl_is_written_through_turned_knurl() {
    let dir = temp("knurl-straight");
    let (_, report, out) = pin_knurled(&dir, "straight");
    assert!(
        report
            .written
            .contains(&"straight knurl on turned_steps:0".to_string())
    );
    assert!(
        !report.warnings.iter().any(|w| w.contains("knurl")),
        "{:?}",
        report.warnings
    );
    let (pattern, lengths, teeth, angle, hand) = knurl_back(&out);
    assert_eq!(pattern, pmi::KnurlPattern::Straight);
    assert_eq!(lengths, [10.0, 10.0, 1.0, 0.5, 0.1]);
    assert_eq!((teeth, angle, hand), (Some(31), None, false));
    assert!(flat(&out).contains(&format!(
        "TURNED_KNURL('{}','straight')",
        specify_core::writer::PMI_PROVENANCE
    )));
    let existing = read_existing(&load(&out, None).unwrap()).unwrap();
    assert!(
        existing
            .iter()
            .any(|e| e.kind == "note" && e.r#type == "knurl")
    );
    // Written again from its stored answers (a replace of a part with a knurl).
    let back = load(&out, None).unwrap();
    let given = answers_for(&back).unwrap().expect("answers stored");
    let again = dir.join("again.step");
    let report = write(
        &back,
        &apply(
            &reanalysed(&analysis("parts/pin"), &back),
            Value::Object(given.clone()),
        ),
        &again,
        Some(&given),
    )
    .unwrap();
    assert!(
        report
            .written
            .contains(&"straight knurl on turned_steps:0".to_string())
    );
    assert_eq!(knurl_back(&again).2, Some(31));
}

/// A diamond knurl states a 30° helix angle and no hand: two crossed helices (WR9; hand is
/// optional), seven parameters.
#[test]
fn a_diamond_knurl_states_a_helix_angle_and_no_hand() {
    let dir = temp("knurl-diamond");
    let (_, report, out) = pin_knurled(&dir, "diamond");
    assert!(
        report
            .written
            .contains(&"diamond knurl on turned_steps:0".to_string())
    );
    let (pattern, lengths, teeth, angle, hand) = knurl_back(&out);
    assert_eq!(pattern, pmi::KnurlPattern::Diamond);
    assert_eq!(lengths, [10.0, 10.0, 1.0, 0.5, 0.1]);
    assert_eq!(teeth, Some(31));
    assert!((angle.unwrap() - 30.0).abs() < 1e-9, "{angle:?}");
    assert!(!hand);
}

/// An intent without the derived parameters (Python's, or one stored before) is filled in from
/// DIN 82 on write, and a warning says so; values the intent states are written as stated.
#[test]
fn a_knurl_without_its_parameters_is_filled_in_on_write() {
    let dir = temp("knurl-fill");
    let analysis = analysis("parts/pin");
    let ten = feature(&analysis, "turned_steps", 10.0);
    let mut intent = apply(
        &analysis,
        json!({"part.material": "brass", format!("diameter.fit:{ten}"): "knurl:diamond"}),
    );
    let at = intent
        .requirements
        .iter()
        .position(|r| r["kind"] == "knurl")
        .unwrap();
    for key in [
        "teeth",
        "tooth_depth",
        "root_fillet",
        "helix_angle",
        "standard",
        "derived",
    ] {
        intent.requirements[at].remove(key);
    }
    let pin = load(&fixture("pin"), None).unwrap();
    let out = dir.join("filled.step");
    let report = write(&pin, &intent, &out, None).unwrap();
    assert_eq!(
        report
            .warnings
            .iter()
            .filter(|w| w.contains("knurl"))
            .collect::<Vec<_>>(),
        [&format!(
            "diamond knurl on {ten}: the intent states no number of teeth, tooth depth, root \
             fillet, helix angle; written with 31 teeth, tooth depth 0.5, root fillet 0.1, helix \
             angle 30° and no hand from DIN 82 (questions Q4, Q15)"
        )]
    );
    assert_eq!(knurl_back(&out).2, Some(31));
    // Stated values are the intent's: written as stated, only the rest filled in.
    intent.requirements[at].insert("teeth".into(), json!(32));
    intent.requirements[at].insert("tooth_depth".into(), json!(0.42));
    let out = dir.join("stated.step");
    let report = write(&pin, &intent, &out, None).unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("states no root fillet, helix angle;")),
        "{:?}",
        report.warnings
    );
    let (_, lengths, teeth, _, _) = knurl_back(&out);
    assert_eq!((teeth, lengths[3]), (Some(32), 0.42));
}

/// A knurl the standard does not give (a pitch off DIN 82's series, a pattern other than
/// straight or diamond) and whose intent does not state the parameters is listed in
/// `not_written`, saying why; the rest is written. A bad stated value is refused.
#[test]
fn a_knurl_off_the_standard_is_listed_not_guessed() {
    let dir = temp("knurl-off");
    let analysis = analysis("parts/pin");
    let ten = feature(&analysis, "turned_steps", 10.0);
    let base = apply(
        &analysis,
        json!({"part.material": "brass", format!("diameter.fit:{ten}"): "knurl:straight"}),
    );
    let at = base
        .requirements
        .iter()
        .position(|r| r["kind"] == "knurl")
        .unwrap();
    let pin = load(&fixture("pin"), None).unwrap();
    let mut intent = base.clone();
    intent.requirements[at].remove("teeth");
    intent.requirements[at].insert("pitch".into(), json!(0.7));
    let report = write(&pin, &intent, &dir.join("pitch.step"), None).unwrap();
    assert_eq!(
        report
            .not_written
            .iter()
            .filter(|w| w.contains("knurl"))
            .collect::<Vec<_>>(),
        [&format!(
            "straight knurl on {ten}: a pitch of 0.7 mm is not one of DIN 82's (0.5, 0.6, 0.8, \
             1, 1.2, 1.6 mm), so AP242's turned_knurl cannot be completed (it needs at least \
             three of the number of teeth, tooth depth, root fillet, helix angle and hand, WR2); \
             state them in the intent (questions Q4, Q15)"
        )]
    );
    assert!(report.written.iter().any(|w| w == "material brass"));
    let mut intent = base.clone();
    intent.requirements[at].insert("pattern".into(), json!("wavy"));
    let report = write(&pin, &intent, &dir.join("wavy.step"), None).unwrap();
    assert!(report.not_written.iter().any(|w| w.starts_with(&format!(
        "wavy knurl on {ten}: the pattern 'wavy' is not straight or diamond"
    ))));
    let mut intent = base;
    intent.requirements[at].insert("teeth".into(), json!(31.5));
    assert!(matches!(
        write(&pin, &intent, &dir.join("bad.step"), None),
        Err(WriteError::Invalid(m)) if m.contains("teeth 31.5 is not a whole number")
    ));
}

#[test]
fn plus_minus_plate_writes_datum_features_without_geometric_tolerances() {
    let loaded = load(&fixture("plate"), None).unwrap();
    let analysis = specify_core::api::analyse(&loaded).unwrap();
    let mut given = answers(
        json!({"part.material": "steel", "part.locating": specify_core::rules::LOCATING[1]}),
    );
    let qs = questions(&analysis, &given).unwrap();
    for letter in ["A", "B", "C"] {
        let q = qs
            .iter()
            .find(|q| q.id == format!("datum.{letter}"))
            .unwrap();
        assert!(
            q.default
                .as_ref()
                .unwrap()
                .as_array()
                .is_some_and(|f| !f.is_empty())
        );
        assert!(
            q.prompt
                .starts_with("Datums: where is the part measured from?")
        );
        assert!(q.why.starts_with("Datums: the part is measured from"));
    }
    let output = temp("plus-minus-datums").join("plate.step");
    for remove in [false, true] {
        if remove {
            for letter in ["A", "B", "C"] {
                given.insert(format!("datum.{letter}"), json!([]));
            }
        }
        let intent = apply_with_defaults(&analysis, &given, true).unwrap();
        assert_eq!(intent.datums.len(), if remove { 0 } else { 3 });
        write(&loaded, &intent, &output, Some(&given)).unwrap();
        let written = load(&output, None).unwrap();
        let items = read_existing(&written).unwrap();
        let datums: Vec<_> = items.iter().filter(|e| e.kind == "datum").collect();
        assert_eq!(datums.len(), intent.datums.len());
        for d in &intent.datums {
            assert!(
                datums
                    .iter()
                    .any(|e| e.r#type == d["letter"].as_str().unwrap()
                        && json!(e.faces) == d["faces"])
            );
        }
        let step = std::fs::read_to_string(&output).unwrap();
        assert_eq!(step.matches("DATUM_FEATURE(").count(), intent.datums.len());
        assert!(!step.contains("GEOMETRIC_TOLERANCE("));
        assert!(items.iter().all(|e| e.kind != "geometric_tolerance"));
    }
}
