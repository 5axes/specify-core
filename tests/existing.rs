//! Existing PMI and part settings (`src/existing.rs`, port of `existing.py`), checked against
//! specify-core.
//!
//! Ports the reading parts of `tests/test_merge.py` (what the file states is read; datums on a
//! set of faces, NIST CTC-01) and of `tests/test_mmc_finish_notes.py` (the part's notes and a
//! face's finish read back from a file specify-core wrote), then sweeps every oracle case:
//! cases whose input states PMI against `analysis.existing` and `analysis.part`, and every file
//! Python wrote (`written.step.gz`) against that case's `readback.json` (not `analysis`, which
//! is empty for a resumable file: docs/design.md, "Capture tool"). Each difference must match an
//! entry of `tests/fixtures/known_differences/existing.json`.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{
    Case, Difference, Tolerances, case, cases, check_known, compare, corpus_dir, is_error,
};
use serde_json::{Map, Value, json};
use specify_core::existing::{self, ExistingPmi};
use specify_core::part::{self, LoadedPart};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(name)
}

fn read(path: &Path, part: Option<usize>) -> (Vec<ExistingPmi>, Map<String, Value>) {
    let loaded = part::load(path, part).unwrap();
    (
        existing::read_existing(&loaded).unwrap(),
        existing::part_settings(&loaded).unwrap(),
    )
}

// --- test_merge.py (reading) -----------------------------------------------------------------

/// test_datums_on_a_set_of_faces_are_read_from_the_text: CTC-01's datums B and C are each a
/// hole's two half-cylinders (OpenCascade's reader gives them no faces; haecceity reads them).
#[test]
fn datums_on_a_set_of_faces_are_read() {
    let input = case("nist/nist_ctc_01_asme1_ap242").input().unwrap();
    let (found, _) = read(&input, None);
    let datums: BTreeMap<&str, &[usize]> = found
        .iter()
        .filter(|e| e.kind == "datum")
        .map(|e| (e.r#type.as_str(), e.faces.as_slice()))
        .collect();
    let want: BTreeMap<&str, &[usize]> =
        BTreeMap::from([("A", &[97][..]), ("B", &[89, 90][..]), ("C", &[75, 76][..])]);
    assert_eq!(datums, want);
}

/// test_only_a_part_with_pmi_is_merged / test_what_the_file_states_is_not_asked_again: a bare
/// part states nothing; the plate as specify-core wrote it states its datums, material,
/// general tolerance and part notes.
#[test]
fn what_a_written_file_states_is_read() {
    let (bare, settings) = read(&fixture("plate.step"), None);
    assert!(bare.is_empty() && settings.is_empty());
    let written = case("parts/plate").written_step().unwrap();
    let (found, settings) = read(&written, None);
    let letters: BTreeSet<&str> = found
        .iter()
        .filter(|e| e.kind == "datum")
        .map(|e| e.r#type.as_str())
        .collect();
    assert_eq!(letters, BTreeSet::from(["A", "B", "C"]));
    for key in ["material", "general_tolerance", "surface_finish", "edges"] {
        assert!(settings.contains_key(key), "{key}: {settings:?}");
    }
    assert_eq!(settings["material"], "steel");
}

/// A part of an assembly reports no part settings, as Python (its text scan cannot tell whose
/// they are), but its own PMI only.
#[test]
fn a_part_of_an_assembly_has_no_settings() {
    let written = case("parts/assembly").written_step().unwrap();
    for k in 0..2 {
        let (found, settings) = read(&written, Some(k));
        assert!(settings.is_empty());
        assert!(found.iter().any(|e| e.kind == "datum"), "part {k}");
    }
}

/// test_the_parts_notes_are_written_and_read_back and test_a_face_finish_is_written_on_its_face
/// (reading): the plate written by specify-core (Python, live: skipped without its venv) with
/// a coating, a heat treatment, a finish default and a finish on its top face.
#[test]
fn the_parts_notes_and_a_face_finish_are_read_back() {
    let Some(python) = common::live_python() else {
        eprintln!("skipped: set SPECIFY_TEST_PYTHON for live Python checks");
        return;
    };
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("notes-plate.step");
    const SCRIPT: &str = r#"
import sys
from specify_core.api import analyse, apply, write
from specify_core.rules import questions
plate, out = sys.argv[1], sys.argv[2]
analysis = analyse(plate)
bore = next(q.id for q in questions(analysis, {}) if q.id.startswith("hole.function") and "Ø8" in q.prompt)
top = max((f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2])
answers = {
    "part.material": "Aluminium 6082-T6",
    bore: "fit:H7",
    "part.coating": "Anodise black (MIL-A-8625 Type II)",
    "part.heat_treatment": "Solution treat and age to T6",
    "part.surface_finish": "Ra 1.6",
    "extra.1": {"faces": [top["id"]], "kind": "finish", "value": "0.8"},
}
write(plate, apply(analysis, answers, accept_defaults=True), out)
print(top["id"])
"#;
    let output = Command::new(python)
        .arg("-c")
        .arg(SCRIPT)
        .arg(fixture("plate.step"))
        .arg(&out)
        .output()
        .expect("run specify-core's venv");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The last line is the top face's index (specify-core may print before it).
    let stdout = String::from_utf8_lossy(&output.stdout);
    let top: usize = stdout
        .lines()
        .last()
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap();
    let (found, settings) = read(&out, None);
    assert_eq!(
        Value::Object(settings),
        json!({
            "material": "Aluminium 6082-T6",
            "general_tolerance": "ISO 2768-m",
            "surface_finish": "Ra 1.6",
            "coating": "Anodise black (MIL-A-8625 Type II)",
            "heat_treatment": "Solution treat and age to T6",
            "edges": "Break sharp edges 0.2 max",
        })
    );
    assert!(
        found
            .iter()
            .any(|e| e.kind == "note" && e.r#type == "surface finish" && e.faces.contains(&top)),
        "{found:?}"
    );
}

/// Two reads give identical JSON.
#[test]
fn two_reads_give_identical_json() {
    let input = case("nist/nist_ctc_01_asme1_ap242").input().unwrap();
    let written = case("parts/stack").written_step().unwrap();
    for (path, parts) in [(input, 1), (written, 3)] {
        for k in 0..parts {
            let part = (parts > 1).then_some(k);
            let run = || serde_json::to_string(&read(&path, part)).unwrap();
            assert_eq!(run(), run(), "{} part {k}", path.display());
        }
    }
}

// --- the oracle sweep ------------------------------------------------------------------------

/// Each item keyed by kind, type and faces (and its occurrence, for a repeat), so the two
/// lists pair item by item whatever their order.
fn keyed(items: &Value) -> Value {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let out: Vec<Value> = items
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let base = format!(
                "{}|{}|{}|{}",
                item["kind"].as_str().unwrap_or_default(),
                item["type"].as_str().unwrap_or_default(),
                item["faces"],
                item.get("faces2").map_or(String::new(), Value::to_string)
            );
            let n = seen.entry(base.clone()).or_default();
            *n += 1;
            let mut item = item.clone();
            item.as_object_mut()
                .unwrap()
                .insert("key".into(), json!(format!("{base}#{n}")));
            item
        })
        .collect();
    Value::Array(out)
}

/// The differences between Python's `existing` and `part` and Rust's for one part, and one
/// difference per kind of finding haecceity reports on it.
fn differences(
    name: &str,
    python_existing: &Value,
    python_part: &Value,
    loaded: &LoadedPart,
) -> Vec<Difference> {
    let tolerances = Tolerances::default()
        .align("existing", "key")
        .with("existing*", 1e-9, 1e-9);
    // One reading per part: its PMI, settings and findings all come from it.
    let reading = existing::read(loaded);
    let rust_existing = match reading
        .as_ref()
        .map_err(|e| e.clone())
        .and_then(|r| r.existing(loaded))
    {
        Ok(found) => keyed(&serde_json::to_value(found).unwrap()),
        Err(e) => json!(e.to_string()),
    };
    let rust_part = match (&reading, loaded.part_count > 1) {
        (_, true) => Value::Object(Default::default()),
        (Ok(r), false) => Value::Object(r.settings(loaded)),
        (Err(e), false) => json!(e.to_string()),
    };
    let mut out = compare(
        name,
        &json!({"existing": keyed(python_existing), "part": python_part}),
        &json!({"existing": rust_existing, "part": rust_part}),
        &tolerances,
    );
    let mut kinds: BTreeMap<&str, (usize, String)> = BTreeMap::new();
    for f in reading.as_ref().map_or(&[][..], |r| r.findings()) {
        let entry = kinds
            .entry(f.kind.as_str())
            .or_insert((0, format!("{} #{:?}: {}", f.entity, f.ids, f.detail)));
        entry.0 += 1;
    }
    for (kind, (count, first)) in kinds {
        out.push(Difference {
            case: name.to_string(),
            path: format!("findings.{kind}"),
            python: None,
            rust: Some(json!({"count": count, "first": first})),
        });
    }
    out
}

/// The Rust part a case is of, as the features sweep finds it (NIST files XCAF splits are
/// one part here, question Q7: Python's part 0 is compared with Rust's only part, Python's
/// others are reported absent).
fn load_case(case: &Case, input: &Path) -> Result<LoadedPart, String> {
    let python_parts = case.meta["parts"].as_u64().unwrap_or(1) as usize;
    let rust_parts = part::parts(input).map_or(0, |p| p.len());
    match case.part() {
        Some(k) if rust_parts == 1 && python_parts > 1 => {
            if k > 0 {
                return Err(format!(
                    "Python's part {k} of {python_parts}; one part in Rust (Q7)"
                ));
            }
            part::load(input, Some(0)).map_err(|e| e.to_string())
        }
        k => part::load(input, k).map_err(|e| e.to_string()),
    }
}

/// The existing area's full sweep (one `check_known`).
#[test]
fn existing_pmi_matches_the_oracle() {
    let mut all = Vec::new();
    let (mut inputs, mut written) = (0, 0);
    for case in cases() {
        // The case's own input, against its analysis.
        if let Some(analysis) = case.artifact("analysis").filter(|a| !is_error(a))
            && let Some(input) = case.input()
        {
            match load_case(&case, &input) {
                Ok(loaded) => {
                    inputs += 1;
                    all.extend(differences(
                        &case.name,
                        &analysis["existing"],
                        &analysis["part"],
                        &loaded,
                    ));
                }
                Err(why) => all.push(Difference {
                    case: case.name.clone(),
                    path: "part".into(),
                    python: Some(json!("read")),
                    rust: Some(json!(why)),
                }),
            }
        }
        // The file Python wrote, against Python's own read-back of it.
        if let (Some(step), Some(Value::Array(readback))) =
            (case.written_step(), case.artifact("readback"))
        {
            for (k, entry) in readback.iter().enumerate() {
                let name = if readback.len() > 1 {
                    format!("{}@written#{k}", case.name)
                } else {
                    format!("{}@written", case.name)
                };
                if is_error(&entry["existing"]) || is_error(&entry["part"]) {
                    all.push(Difference {
                        case: name,
                        path: "readback".into(),
                        python: Some(entry.clone()),
                        rust: None,
                    });
                    continue;
                }
                let part = (readback.len() > 1).then_some(k);
                let loaded = part::load(&step, part).unwrap();
                written += 1;
                all.extend(differences(
                    &name,
                    &entry["existing"],
                    &entry["part"],
                    &loaded,
                ));
            }
        }
    }
    println!("existing: {inputs} inputs and {written} written parts compared");
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for d in all.iter().filter(|d| d.path.starts_with("findings.")) {
        *kinds.entry(d.path.clone()).or_default() += 1;
    }
    println!("finding kinds met (parts): {kinds:?}");
    if let Some(dir) = std::env::var_os("SPECIFY_DUMP") {
        let rows: Vec<Value> = all
            .iter()
            .map(|d| json!({"case": d.case, "path": d.path, "python": d.python, "rust": d.rust}))
            .collect();
        std::fs::write(
            PathBuf::from(dir).join("existing.json"),
            serde_json::to_string_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
    check_area("existing", &all);
}

/// `check_known(area)`, but without the corpus the entries for corpus cases, and the
/// cross-case (`*`) entries that then match nothing, are left out: their cases were skipped, so
/// they cannot be stale. With the corpus, every entry must match.
fn check_area(area: &str, differences: &[Difference]) {
    if corpus_dir().is_some() {
        return check_known(area, differences);
    }
    let path = common::known_differences_file(area);
    let entries: Vec<_> = common::load_known(&path)
        .into_iter()
        .filter(|e| !e.case.starts_with("corpus/"))
        .filter(|e| e.case != "*" || differences.iter().any(|d| e.matches(d)))
        .collect();
    println!("no corpus: corpus entries of {area} not checked");
    common::check_known_with(&path.display().to_string(), &entries, differences);
}

#[test]
fn solid_edge_lost_datum_is_unresolved_in_existing_and_cli_output() {
    let path = common::fixtures().join("nist/solid-edge-ctc01.step.gz");
    let loaded = part::load(&path, None).unwrap();
    let reading = existing::read(&loaded).unwrap();
    let labels: BTreeSet<_> = reading
        .existing(&loaded)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "datum")
        .map(|e| e.r#type)
        .collect();
    assert_eq!(labels, BTreeSet::from(["A".into(), "C".into()]));
    let unresolved: Vec<_> = reading
        .findings()
        .iter()
        .filter(|f| f.entity == "datum" && f.kind == haecceity::pmi::FindingKind::Unresolved)
        .collect();
    assert_eq!(unresolved.len(), 1);
    assert_eq!(
        unresolved[0].detail,
        "datum B is not read: what establishes it does not resolve"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("analyse")
        .arg(&path)
        .arg("-o")
        .arg(std::env::temp_dir().join(format!("pin045-existing-{}.json", std::process::id())))
        .output()
        .unwrap();
    assert!(output.status.success());
    let warnings = String::from_utf8(output.stderr).unwrap();
    assert!(warnings.contains(&unresolved[0].detail), "{warnings}");
}
