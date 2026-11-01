//! A smoke test of the whole chain on the Rust engine's own analyses, before the command line
//! composes it (stage 4): for every synthetic part (each part of the assemblies), the analysis
//! is built by the library in Python's layout (`describe_faces`, `recognise`, mates and
//! contacts for a part of an assembly, `read_existing`, `part_settings`), then the rules ask
//! their questions, apply the defaults, and the writer writes the intent. Nothing may panic,
//! no rule may refuse the analysis as invalid (which would mean a record the rules read is
//! shaped differently by quiddity-rust), and every write must verify.
//!
//! Feature ids differ between engines by design (defining against constituent faces, upstream
//! need U2), so these analyses are not compared with Python's entry by entry; that the chain
//! runs on them is what is checked. The pin's tapped hole is such a hole (its faces lack the
//! floor), and the plate's general dimensions run over the plane levels the Rust analysis
//! gives.

mod common;

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use specify_core::existing::{part_settings, read_existing};
use specify_core::features::{describe_faces, recognise};
use specify_core::mates::{contacts, mates};
use specify_core::part::{LoadedPart, load_all};
use specify_core::rules::{Error, Intent, apply_with_defaults, questions};
use specify_core::writer::write_parts;

/// specify-core's analysis of `loaded` (Python's `api.analyse`), from the Rust engine.
fn analyse(loaded: &LoadedPart, all: &[LoadedPart]) -> Value {
    let mut faces: Vec<Value> = describe_faces(loaded)
        .unwrap()
        .iter()
        .map(|f| serde_json::to_value(f).unwrap())
        .collect();
    let features = recognise(loaded).unwrap();
    let mut analysis = Map::new();
    analysis.insert("schema".into(), json!(1));
    analysis.insert(
        "binding".into(),
        serde_json::to_value(&loaded.binding).unwrap(),
    );
    if loaded.part_count > 1 {
        let k = loaded.binding.part;
        for (index, contact) in contacts(all, k) {
            faces[index.parse::<usize>().unwrap()]["contact"] = contact;
        }
        analysis.insert("faces".into(), json!(faces));
        analysis.insert("features".into(), serde_json::to_value(features).unwrap());
        analysis.insert("mates".into(), Value::Object(mates(all, k)));
    } else {
        analysis.insert("faces".into(), json!(faces));
        analysis.insert("features".into(), serde_json::to_value(features).unwrap());
    }
    analysis.insert(
        "existing".into(),
        serde_json::to_value(read_existing(loaded).unwrap()).unwrap(),
    );
    analysis.insert("part".into(), Value::Object(part_settings(loaded).unwrap()));
    Value::Object(analysis)
}

fn synthetic_parts() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(common::fixtures().join("parts"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "step"))
        .collect();
    out.sort();
    out
}

fn temp() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-rust-analyses")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Questions, defaults and intent for one part's analysis; a refusal is reported by the
/// caller, naming the part.
fn intent(analysis: &Value) -> Result<Intent, Error> {
    questions(analysis, &Map::new())?;
    let given = json!({"part.material": "steel"});
    apply_with_defaults(analysis, given.as_object().unwrap(), true)
}

fn write_file(path: &Path, dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let parts = load_all(path).unwrap();
    let mut intents = Vec::new();
    for loaded in &parts {
        let analysis = analyse(loaded, &parts);
        match intent(&analysis) {
            Ok(i) => intents.push(i),
            Err(e) => problems.push(format!("{name} part {}: {e}", loaded.binding.part)),
        }
    }
    if !problems.is_empty() {
        return problems;
    }
    let answers = json!({"part.material": "steel"});
    let jobs: Vec<_> = parts
        .iter()
        .zip(&intents)
        .map(|(l, i)| (l, i, answers.as_object()))
        .collect();
    let out = dir.join(format!("{name}.step"));
    match write_parts(&jobs, &out) {
        Ok(report) => {
            assert!(!report.written.is_empty(), "{name}: nothing written");
            // Every file written passes the EXPRESS check (test_conformance.py, as kept).
            let doc = haecceity::p21::Document::parse(std::fs::read(&out).unwrap()).unwrap();
            let violations = haecceity::express::validate_document(doc.graph());
            let rules = haecceity::express_rules::check_all(&doc);
            if !violations.is_empty() || !rules.is_empty() {
                problems.push(format!("{name}: {violations:?} {rules:?}"));
            }
            println!(
                "{name}: {} written, {} not written",
                report.written.len(),
                report.not_written.len()
            );
        }
        Err(e) => problems.push(format!("{name}: {e}")),
    }
    problems
}

#[test]
fn the_rules_and_the_writer_run_on_rust_analyses() {
    let dir = temp();
    let mut problems = Vec::new();
    for path in synthetic_parts() {
        problems.extend(write_file(&path, &dir));
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The pin's tapped hole as the Rust engine recognises it: its defining faces only, the bore
/// without the floor (U2), and still a tapped hole with its thread written.
#[test]
fn a_hole_without_its_floor_is_tapped_and_written() {
    let dir = temp().join("pin");
    std::fs::create_dir_all(&dir).unwrap();
    let path = common::fixtures().join("parts").join("pin.step");
    let parts = load_all(&path).unwrap();
    let analysis = analyse(&parts[0], &parts);
    let hole = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == "holes")
        .unwrap();
    let faces = hole["faces"].as_array().unwrap();
    assert!(
        faces
            .iter()
            .all(|f| analysis["faces"][f.as_u64().unwrap() as usize]["kind"] == "cylinder")
    );
    let i = intent(&analysis).unwrap();
    assert!(
        i.requirements
            .iter()
            .any(|r| r["kind"] == "thread" && r["side"] == "internal")
    );
    let report = write_parts(&[(&parts[0], &i, None)], &dir.join("pin.step")).unwrap();
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("internal thread M5x0.8-6H on holes:"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The plate's general dimensions from the Rust analysis: its lengths (100 × 60 × 10) as ISO
/// 2768 locations between plane levels, each on plane faces from plane faces, toleranced by
/// the general tolerance.
#[test]
fn general_dimensions_run_over_the_plates_plane_levels() {
    let path = common::fixtures().join("parts").join("plate.step");
    let parts = load_all(&path).unwrap();
    let analysis = analyse(&parts[0], &parts);
    let i = intent(&analysis).unwrap();
    let plane = |f: &Value| analysis["faces"][f.as_u64().unwrap() as usize]["kind"] == "plane";
    // A length is a location of no feature: between two plane levels.
    let lengths: Vec<_> = i
        .general
        .iter()
        .filter(|g| g["kind"] == "location" && g["feature"] == "")
        .collect();
    let mut distances: Vec<f64> = lengths
        .iter()
        .map(|g| g["distance"].as_f64().unwrap())
        .collect();
    distances.sort_by(f64::total_cmp);
    assert_eq!(distances, [10.0, 60.0, 100.0], "{:?}", i.general);
    for g in lengths {
        let faces = g["faces"].as_array().unwrap();
        let reference = g["reference"].as_array().unwrap();
        assert!(!faces.is_empty() && !reference.is_empty(), "{g:?}");
        assert!(
            faces.iter().all(plane) && reference.iter().all(plane),
            "{g:?}"
        );
        assert_eq!(g["general"], json!(true));
        assert_eq!(g["upper"].as_f64(), g["lower"].as_f64().map(|l| -l));
    }
}

/// Alpha.8 publishes physical thread measurements without changing Specify's interview
/// contract. Both thread-bearing corpus inputs must complete the existing writer path.
#[test]
fn modelled_thread_parts_still_analyse_interview_and_write() {
    let Some(corpus) = common::corpus_dir() else {
        eprintln!("skipped: no corpus (set QUIDDITY_CORPUS or SPECIFY_CORPUS)");
        return;
    };
    let dir = std::env::temp_dir()
        .join("specify-core-modelled-threads")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    // #75 replaces cgb243's member offsets with shared pattern origins/pitches
    // and Basic bolt circles, and consolidates General rows. Recognition and
    // interview counts remain unchanged. Exact unsupported proposals stay visible.
    for (
        file,
        count,
        freeforms,
        question_count,
        requirements,
        general,
        datums,
        written,
        not_written,
    ) in [
        (
            "cadgenbench_inputs/cgb243.step.gz",
            4,
            66,
            39,
            57,
            48,
            3,
            62,
            &[
                "shared planner dim:multiple[hole:101.102.283.284,hole:87.88.297.298,hole:89.90.295.296,hole:91.92.293.294,hole:93.94.291.292,hole:95.96.289.290,hole:97.98.287.288,hole:99.100.285.286]/bore.diameter: General contract conflicts with an answered requirement; retained answered intent instead",
                "shared planner dim:hole:128.220/bore.depth: bottom is unknown; bore extent is not an established blind drilling depth",
            ][..],
        ),
        (
            "cadgenbench/threaded_connector_109.step",
            1,
            4,
            18,
            0,
            17,
            2,
            6,
            &[][..],
        ),
    ] {
        let path = corpus.join(file);
        let analysis = specify_core::api::analyse_file(&path, None).unwrap();
        let features = analysis["features"].as_array().unwrap();
        assert_eq!(
            features
                .iter()
                .filter(|f| f["family"] == "modelled_threads")
                .count(),
            count,
            "{file}"
        );
        assert_eq!(
            features
                .iter()
                .filter(|f| f["family"] == "freeform_surfaces")
                .count(),
            freeforms,
            "{file}"
        );
        // A more specific feature classification must retain every old freeform face.
        let case_name = format!(
            "corpus/{}",
            file.trim_end_matches(".gz").trim_end_matches(".step")
        );
        let captured = common::case(&case_name).artifact("analysis").unwrap();
        let claimed: std::collections::BTreeSet<_> = features
            .iter()
            .filter(|f| f["family"] == "modelled_threads")
            .flat_map(|f| {
                f["faces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
            })
            .collect();
        let removed: Vec<_> = captured["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| {
                f["family"] == "freeform_surfaces" && !features.iter().any(|r| r["id"] == f["id"])
            })
            .collect();
        assert_eq!(removed.len(), if count == 4 { 8 } else { 136 });
        for old in removed {
            assert!(
                old["faces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|f| claimed.contains(&f.as_u64().unwrap())),
                "{file}: lost {old}"
            );
        }
        let roundtrip: Value = serde_json::from_str(&analysis.to_string()).unwrap();
        let qs = questions(&roundtrip, &Map::new()).unwrap();
        assert_eq!(qs.len(), question_count, "{file}: interview changed");
        let applied = intent(&roundtrip).unwrap();
        assert_eq!(applied.requirements.len(), requirements, "{file}");
        assert_eq!(applied.general.len(), general, "{file}");
        assert_eq!(applied.datums.len(), datums, "{file}");
        let loaded = specify_core::part::load(&path, None).unwrap();
        let answers = json!({"part.material": "steel"});
        let output = dir.join(format!(
            "{}.step",
            path.file_stem().unwrap().to_string_lossy()
        ));
        let report = write_parts(&[(&loaded, &applied, answers.as_object())], &output).unwrap();
        // Besides the one line counting the dimensions under the general tolerance (#63).
        let lines = report
            .written
            .iter()
            .filter(|l| !l.ends_with("under the general tolerance"))
            .count();
        assert_eq!(lines, written, "{file}");
        assert_eq!(
            report.not_written, not_written,
            "{file}: exact unresolved proposals"
        );
        let resumed = specify_core::part::load(&output, None).unwrap();
        assert_eq!(
            specify_core::resume::answers_for(&resumed).unwrap(),
            answers.as_object().cloned(),
            "{file}"
        );
        println!(
            "{file}: {count} measured threads, {freeforms} freeforms, {} questions, {} requirements, {} written, {} not written; answers resume",
            qs.len(),
            applied.requirements.len(),
            report.written.len(),
            report.not_written.len()
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
