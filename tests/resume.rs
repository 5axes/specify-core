//! Answers stored in a written file and resumed from it: specify-core's `tests/test_resume.py`,
//! the cross-engine forms (Python's stored answers read here, this port's read by Python's
//! pattern), the fingerprint's JSON text, and a Python-written file rewritten by replace.

mod common;

use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::existing::{part_settings, read_existing};
use specify_core::part::{LoadedPart, load};
use specify_core::pyjson;
use specify_core::resume::{
    self, answers_for, fingerprint, fingerprint_of, fingerprint_v1, stored,
};
use specify_core::rules::{Intent, apply_with_defaults};
use specify_core::writer::write;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-resume-tests")
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

fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

fn apply(analysis: &Value, given: Value) -> Intent {
    apply_with_defaults(analysis, given.as_object().unwrap(), true).expect("an intent")
}

#[test]
fn long_saved_answers_resume_from_ordered_text_chunks() {
    let loaded = load(&fixture("plate"), None).unwrap();
    let analysis = specify_core::api::analyse(&loaded).unwrap();
    let given = json!({"part.material":"steel", "part.notes":["Keep dry. ".repeat(1000)]});
    let intent = apply(&analysis, given.clone());
    let (bytes, _) =
        specify_core::writer::write_bytes(&loaded, &intent, Some(given.as_object().unwrap()))
            .unwrap();
    let back = specify_core::part::load_bytes("chunked.step", bytes.clone()).unwrap();
    assert_eq!(
        answers_for(&back).unwrap(),
        Some(given.as_object().unwrap().clone())
    );
    assert!(String::from_utf8_lossy(&bytes).contains("haecceity text chunk/1"));
    let resumed = specify_core::api::analyse(&back).unwrap();
    let intent = apply(&resumed, given.clone());
    let (again, _) =
        specify_core::writer::write_bytes(&back, &intent, Some(given.as_object().unwrap()))
            .unwrap();
    assert_eq!(
        again, bytes,
        "unchanged chunked answers must resave without drift"
    );
}

fn feature(analysis: &Value, family: &str, diameter: f64) -> String {
    analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == family && f["record"]["diameter"].as_f64() == Some(diameter))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn flat(path: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    Regex::new(r"\s*\n\s*")
        .unwrap()
        .replace_all(&text, " ")
        .into_owned()
}

/// The analysis of a file the writer wrote, as `api.analyse` composes it (see tests/writer.rs).
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
        }
    }
    out
}

// --- test_resume.py --------------------------------------------------------------------------

/// Counts of what a written file holds, as Python's `_counts` (`saved` counts the stored
/// answers' items).
fn counts(path: &Path) -> (usize, usize, usize) {
    let text = flat(path);
    let n = |pattern: &str| Regex::new(pattern).unwrap().find_iter(&text).count();
    (
        n(r"= ?DATUM\("),
        n(r"POSITION_TOLERANCE"),
        n(r"DESCRIPTIVE_REPRESENTATION_ITEM\('pmi-assist answers'"),
    )
}

// test_resume.py::test_a_written_file_resumes_its_answers_and_is_rewritten_not_added_to
#[test]
fn a_written_file_resumes_its_answers_and_is_rewritten_not_added_to() {
    let dir = temp("resume");
    let base = analysis("parts/pin");
    let knurl = feature(&base, "turned_steps", 10.0);
    let given =
        json!({"part.material": "brass", format!("diameter.fit:{knurl}"): "knurl:straight"});
    let first = dir.join("first.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&base, given.clone()),
        &first,
        given.as_object(),
    )
    .unwrap();

    // Opened again: the answers, and nothing of the file's own PMI to keep.
    let loaded = load(&first, None).unwrap();
    assert_eq!(answers_for(&loaded).unwrap(), given.as_object().cloned());
    let again = reanalysed(&base, &loaded);
    assert_eq!(again["resume"], json!({ "answers": given }));
    assert_eq!(again["existing"], json!([]));

    // Changed and written again: the PMI is the new answers', once.
    let mut changed = given.clone();
    changed["part.material"] = json!("steel");
    let second = dir.join("second.step");
    let report = write(
        &loaded,
        &apply(&again, changed.clone()),
        &second,
        changed.as_object(),
    )
    .unwrap();
    assert!(report.written.contains(&"material steel".to_string()));
    assert_eq!(counts(&first), counts(&second));
    assert_eq!(counts(&second).2, 1);
    let text = flat(&second);
    assert!(text.contains("'steel'") && !text.contains("'brass'"));
    assert_eq!(
        answers_for(&load(&second, None).unwrap()).unwrap(),
        changed.as_object().cloned()
    );
}

/// A resumed file whose thread is answered away: the replace of a thread haecceity wrote, which
/// its removal plan refused until upstream need U14 was met (quiddity-rust 86b11ba); the
/// writer now makes it as any replace, and nothing the reader consumed survives.
#[test]
fn a_resumed_files_thread_can_be_answered_away() {
    let dir = temp("unthread");
    let base = analysis("parts/pin");
    let given = json!({"part.material": "brass"});
    let first = dir.join("first.step");
    let pin = load(&fixture("pin"), None).unwrap();
    let report = write(
        &pin,
        &apply(&base, given.clone()),
        &first,
        given.as_object(),
    )
    .unwrap();
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("internal thread"))
    );

    let loaded = load(&first, None).unwrap();
    let again = reanalysed(&base, &loaded);
    let hole = format!("hole.function:{}", feature(&base, "holes", 4.2));
    let mut changed = given.clone();
    changed[hole] = json!("general");
    let second = dir.join("second.step");
    let report = write(
        &loaded,
        &apply(&again, changed.clone()),
        &second,
        changed.as_object(),
    )
    .unwrap();
    assert!(!report.written.iter().any(|w| w.contains("thread")));
    let text = flat(&second);
    assert!(!text.contains("thread manufacturing"));
    assert_eq!(
        answers_for(&load(&second, None).unwrap()).unwrap(),
        changed.as_object().cloned()
    );
}

// test_resume.py::test_without_answers_nothing_is_stored
#[test]
fn without_answers_nothing_is_stored() {
    let dir = temp("noanswers");
    let analysis = analysis("parts/pin");
    let out = dir.join("plain.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&analysis, json!({"part.material": "brass"})),
        &out,
        None,
    )
    .unwrap();
    assert!(stored(&std::fs::read(&out).unwrap()).is_empty());
    assert!(answers_for(&load(&out, None).unwrap()).unwrap().is_none());
}

// test_resume.py::test_answers_for_other_faces_are_not_resumed
#[test]
fn answers_for_other_faces_are_not_resumed() {
    let dir = temp("forged");
    let analysis = analysis("parts/pin");
    let given = json!({"part.material": "brass"});
    let out = dir.join("x.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    let mut payload = Value::Object(stored(text.as_bytes()).remove(&0).unwrap());
    payload["faces"] = json!("0".repeat(64));
    let forged = resume::encode(&payload);
    let pattern = Regex::new(r"('pmi-assist answers',')[A-Za-z0-9+/=]*'").unwrap();
    let changed = pattern
        .replace(&text, format!("${{1}}{forged}'").as_str())
        .into_owned();
    assert_ne!(changed, text);
    std::fs::write(&out, changed).unwrap();
    assert!(stored(&std::fs::read(&out).unwrap()).contains_key(&0));
    assert!(answers_for(&load(&out, None).unwrap()).unwrap().is_none());
}

// Python resumes what the Rust writer stores: its pattern matches the stored item.
#[test]
fn the_stored_item_is_the_one_pythons_pattern_reads() {
    let dir = temp("pattern");
    let analysis = analysis("parts/plate");
    let given = json!({"part.material": "steel"});
    let out = dir.join("x.step");
    let plate = load(&fixture("plate"), None).unwrap();
    write(
        &plate,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    // resume.py's _SAVED, over the text with line breaks removed.
    let text = Regex::new(r"\s*\n\s*")
        .unwrap()
        .replace_all(&std::fs::read_to_string(&out).unwrap(), "")
        .into_owned();
    let saved = Regex::new(
        r"DESCRIPTIVE_REPRESENTATION_ITEM\(\s*'pmi-assist answers'\s*,\s*'([A-Za-z0-9+/=]*)'\s*\)",
    )
    .unwrap();
    let found: Vec<_> = saved.captures_iter(&text).collect();
    assert_eq!(found.len(), 1);
    let payload = stored(text.as_bytes()).remove(&0).unwrap();
    assert_eq!(found[0][1], resume::encode(&Value::Object(payload)));
}

// --- the fingerprint, across engines ---------------------------------------------------------------

/// The plate's faces as Python's `describe_faces` gives them (kind, centroid rounded to 3
/// decimals: `round(c, 3)` of OpenCascade's centroids, signs of zero included), and the
/// fingerprint Python stored for them in the file it wrote (`parts/plate/written.step.gz`).
const PYTHON_PLATE: &str = r#"[["plane", [-50.0, -0.0, -0.0]], ["plane", [0.0, -30.0, -0.0]], ["plane", [-0.109, -0.0, 5.0]], ["plane", [0.0, 30.0, -0.0]], ["plane", [-0.109, -0.0, -5.0]], ["plane", [50.0, -0.0, -0.0]], ["cylinder", [-30.0, -15.0, -0.0]], ["cylinder", [0.0, -15.0, -0.0]], ["cylinder", [30.0, -15.0, -0.0]], ["cylinder", [0.0, 0.0, -0.0]], ["cylinder", [-30.0, 15.0, -0.0]], ["cylinder", [0.0, 15.0, -0.0]], ["cylinder", [45.0, -0.0, -0.0]], ["cylinder", [30.0, 15.0, -0.0]]]"#;

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// The fingerprint hashes Python's exact `json.dumps` text: given Python's faces, pyjson writes
// the text Python hashed.
#[test]
fn the_fingerprint_text_is_pythons() {
    let faces: Value = serde_json::from_str(PYTHON_PLATE).unwrap();
    assert_eq!(pyjson::dumps(&faces), PYTHON_PLATE);
    let written = common::case("parts/plate").written_step().unwrap();
    let payload = stored(&std::fs::read(&written).unwrap())
        .remove(&0)
        .unwrap();
    assert_eq!(payload["faces"], json!(sha(PYTHON_PLATE)));
}

// Python's stored answers are read in Python's form, in every file Python wrote.
#[test]
fn pythons_stored_answers_are_read() {
    for case in common::cases() {
        if case.part().is_some() {
            continue;
        }
        let (Some(written), Some(Value::Array(back))) =
            (case.written_step(), case.artifact("readback"))
        else {
            continue;
        };
        let found = stored(&std::fs::read(&written).unwrap());
        for (k, python) in back.iter().enumerate() {
            if python["resume"].is_object() {
                assert_eq!(
                    found.get(&k).map(|p| &p["answers"]),
                    Some(&python["resume"]),
                    "{} part {k}",
                    case.name
                );
            }
        }
    }
}

// Why a schema 1 file Python wrote does not resume here (known difference, Q14): its payload is
// read and its face count matches, but its fingerprint is of OpenCascade's centroids, whose
// zero components carry OpenCascade's signs; the Rust engine's centroids round to the same
// numbers with other signs of zero.
#[test]
fn a_python_fingerprint_differs_only_in_signs_of_zero() {
    let loaded = load(&fixture("plate"), None).unwrap();
    let ours = fingerprint_v1(&loaded).unwrap();
    assert_ne!(ours, sha(PYTHON_PLATE));
    let python: Value = serde_json::from_str(PYTHON_PLATE).unwrap();
    let rust: Vec<Value> = specify_core::features::describe_faces(&loaded)
        .unwrap()
        .iter()
        .map(|f| {
            json!([
                f.kind,
                f.centroid
                    .iter()
                    .map(|c| specify_core::pyfmt::round_half_even(*c, 3))
                    .collect::<Vec<_>>()
            ])
        })
        .collect();
    let python = python.as_array().unwrap();
    assert_eq!(python.len(), rust.len());
    for (p, r) in python.iter().zip(&rust) {
        assert_eq!(p[0], r[0]);
        for (a, b) in p[1]
            .as_array()
            .unwrap()
            .iter()
            .zip(r[1].as_array().unwrap())
        {
            assert_eq!(a.as_f64(), b.as_f64(), "{p} {r}"); // -0.0 == 0.0
        }
    }
    let written = common::case("parts/plate").written_step().unwrap();
    let back = load(&written, None).unwrap();
    let payload = stored(&back.bytes).remove(&0).unwrap();
    assert_eq!(payload["face_count"], json!(back.binding.face_count));
    assert!(answers_for(&back).unwrap().is_none());
}

// A file Python wrote whose stored (schema 1) fingerprint is the Rust engine's resumes, and is
// rewritten by replace: Python's text route, presentation and stored answers go, and the
// answers stored are this port's only (schema 2).
#[test]
fn a_python_written_file_that_resumes_is_rewritten_by_replace() {
    let dir = temp("python-written");
    let written = common::case("parts/plate").written_step().unwrap();
    let text = std::fs::read_to_string(&written).unwrap();
    let ours = fingerprint_v1(&load(&fixture("plate"), None).unwrap()).unwrap();
    let mut payload = Value::Object(stored(text.as_bytes()).remove(&0).unwrap());
    assert_eq!(payload["schema"], json!(1));
    payload["faces"] = json!(ours);
    let pattern = Regex::new(r"('pmi-assist answers',')[A-Za-z0-9+/=]*'").unwrap();
    let patched = dir.join("python.step");
    std::fs::write(
        &patched,
        pattern
            .replace(
                &text,
                format!("${{1}}{}'", resume::encode(&payload)).as_str(),
            )
            .as_ref(),
    )
    .unwrap();
    let loaded = load(&patched, None).unwrap();
    let given = json!({"part.material": "brass"});
    assert_eq!(
        answers_for(&loaded).unwrap(),
        Some(Map::from_iter([(
            "part.material".to_string(),
            json!("steel")
        )]))
    );
    let base = reanalysed(&analysis("parts/plate"), &loaded);
    let out = dir.join("out.step");
    write(
        &loaded,
        &apply(&base, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    let after = flat(&out);
    assert!(!after.contains("'manufacturing requirement'"));
    assert!(!after.contains("PROPERTY_DEFINITION('pmi-assist answers','schema 1'"));
    assert_eq!(
        after
            .matches("DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers'")
            .count(),
        1
    );
    assert!(after.contains("'brass'") && !after.contains("'steel'"));
    let back = load(&out, None).unwrap();
    assert_eq!(stored(&back.bytes)[&0]["schema"], json!(2));
    assert_eq!(answers_for(&back).unwrap(), given.as_object().cloned());
}

// --- schema 2 (Q14) --------------------------------------------------------------------------

/// A file the writer wrote for the plate with `given` stored, and its text.
fn written_plate(dir: &Path, given: &Value) -> (PathBuf, String) {
    let out = dir.join("plate.step");
    let plate = load(&fixture("plate"), None).unwrap();
    write(
        &plate,
        &apply(&analysis("parts/plate"), given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    (out, text)
}

/// `text` with its stored payload replaced by `payload`.
fn with_payload(text: &str, payload: &Value) -> String {
    let pattern = Regex::new(r"('pmi-assist answers',')[A-Za-z0-9+/=]*'").unwrap();
    let encoded = resume::encode(payload);
    assert!(pattern.is_match(text));
    let changed = pattern
        .replace(text, format!("${{1}}{encoded}'").as_str())
        .into_owned();
    assert!(changed.contains(&encoded));
    changed
}

// The writer stores schema 2, with the schema 2 fingerprint: zeros unsigned.
#[test]
fn the_writer_stores_schema_two() {
    let dir = temp("schema2");
    let given = json!({"part.material": "steel"});
    let (out, _) = written_plate(&dir, &given);
    let loaded = load(&out, None).unwrap();
    let payload = stored(&loaded.bytes).remove(&0).unwrap();
    assert_eq!(payload["schema"], json!(2));
    assert_eq!(payload["faces"], json!(fingerprint(&loaded).unwrap()));
    assert_ne!(payload["faces"], json!(fingerprint_v1(&loaded).unwrap()));
    assert_eq!(answers_for(&loaded).unwrap(), given.as_object().cloned());
}

// Each schema is checked against its own fingerprint: a schema 1 payload is verified on the
// schema 1 fingerprint and not on schema 2's, and the reverse. A schema 1 payload in this
// port's attribute form whose hash is not reproduced (another kernel's signs of zero) still
// resumes, without face evidence (#35): `unverified` says so, and its answers are matched by
// ID (here a datum on a face the part has, applied either way).
#[test]
fn each_schema_is_checked_against_its_own_fingerprint() {
    let dir = temp("schemas");
    let given = json!({"part.material": "steel"});
    let (out, text) = written_plate(&dir, &given);
    let loaded = load(&out, None).unwrap();
    let (v1, v2) = (
        fingerprint_v1(&loaded).unwrap(),
        fingerprint(&loaded).unwrap(),
    );
    let mut payload = Value::Object(stored(text.as_bytes()).remove(&0).unwrap());
    payload["answers"]["datum.A"] = json!([0]);
    for (schema, faces, resumes, verified) in [
        (1, &v1, true, true),
        (1, &v2, true, false),
        (2, &v1, false, false),
        (2, &v2, true, true),
    ] {
        payload["schema"] = json!(schema);
        payload["faces"] = json!(faces);
        let path = dir.join(format!("s{schema}-{resumes}.step"));
        std::fs::write(&path, with_payload(&text, &payload)).unwrap();
        let back = load(&path, None).unwrap();
        let resume = &specify_core::api::analyse(&back).unwrap()["resume"];
        assert_eq!(
            resume.is_object() && resume["unverified"].is_null(),
            verified,
            "schema {schema}: {resume}"
        );
        assert_eq!(resume.is_object(), resumes, "schema {schema}: {resume}");
        if resumes {
            // Verified or matched by ID, the datum is applied.
            assert_eq!(resume["answers"]["datum.A"], json!([0]));
            assert!(resume["unresolved"].is_null(), "schema {schema}: {resume}");
            assert_eq!(resume["answers"]["part.material"], "steel");
        }
        assert_eq!(
            answers_for(&back).unwrap().is_some(),
            resumes,
            "schema {schema}, {}",
            if faces == &v1 {
                "schema 1 fingerprint"
            } else {
                "schema 2 fingerprint"
            }
        );
    }
    // Schema 3 now carries authored names; a future unsupported schema is passed over.
    payload["schema"] = json!(4);
    payload["faces"] = json!(v2);
    let path = dir.join("s4.step");
    std::fs::write(&path, with_payload(&text, &payload)).unwrap();
    assert!(stored(&std::fs::read(&path).unwrap()).is_empty());
}

// The schema 2 fingerprint of the faces Python's engine gives the plate (OpenCascade's signs of
// zero) is the Rust engine's: the documented canonical form (docs/design.md) makes the two
// engines' fingerprints of the same faces equal, where schema 1's differ.
#[test]
fn schema_two_fingerprints_pythons_faces_as_the_rust_engines() {
    let loaded = load(&fixture("plate"), None).unwrap();
    let python: Value = serde_json::from_str(PYTHON_PLATE).unwrap();
    assert_eq!(fingerprint_of(&python, 2), fingerprint(&loaded).unwrap());
    assert_eq!(fingerprint_of(&python, 1), sha(PYTHON_PLATE));
    assert_ne!(fingerprint_of(&python, 1), fingerprint_v1(&loaded).unwrap());
    // The text schema 2 hashes, exactly.
    assert_eq!(
        fingerprint(&loaded).unwrap(),
        sha(&PYTHON_PLATE.replace("-0.0", "0.0"))
    );
}

/// These files were committed on the old dependency before upgrading. In particular CTC-02's
/// ambiguous rows change under quiddity #82 and cgb217's frame/fillets change under #81.
#[test]
fn alpha7_saved_answers_survive_the_schema6_upgrade() {
    for (name, count) in [
        ("golden_oriented_slots", 13),
        ("cgb217", 37),
        ("ctc02_ap242", 34),
    ] {
        let dir = common::fixtures().join("resume_alpha7");
        let meta: Value =
            serde_json::from_slice(&std::fs::read(dir.join(format!("{name}.json"))).unwrap())
                .unwrap();
        if name == "golden_oriented_slots" {
            // These are the actual 0.1.1 answers: all 13 are part/datum settings,
            // not feature-answer IDs. Do not invent an oriented_slots answer migration.
            assert!(
                meta["answers"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .all(|id| !id.contains(':'))
            );
            assert_eq!(
                meta["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|f| f["family"] == "oriented_slots")
                    .count(),
                3
            );
        }
        let path = dir.join(format!("{name}.step.gz"));
        let loaded = load(&path, None).unwrap();
        let saved = stored(&loaded.bytes).remove(&0).unwrap();
        assert_eq!(saved["answers"], meta["answers"]);
        let (analysis, warnings) = specify_core::api::analyse_reporting(&loaded).unwrap();
        if name == "golden_oriented_slots" {
            for old in meta["features"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["family"] == "oriented_slots")
            {
                // #91 restores the plate frame and #100 claims both end walls: the same
                // four physical walls as alpha.7, with the #92 family rename.
                // schema6 checks their actual end planes and rigid motions.
                let matches: Vec<_> = analysis["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|f| {
                        f["family"] == "slots"
                            && f["faces"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .all(|face| old["faces"].as_array().unwrap().contains(face))
                    })
                    .collect();
                assert_eq!(matches.len(), 1, "old support {}", old["id"]);
                assert_eq!(matches[0]["faces"], old["faces"]);
            }
        }
        let resumed = &analysis["resume"];
        assert!(resumed.is_object(), "{name}: saved answers disappeared");
        let applied = resumed["answers"].as_object().unwrap();
        assert_eq!(applied.len(), count, "{name}");
        let unresolved = resumed["unresolved"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let migrated = resumed["migrated"].as_array().cloned().unwrap_or_default();
        for (old, value) in meta["answers"].as_object().unwrap() {
            let new = migrated
                .iter()
                .find(|pair| pair[0] == *old)
                .and_then(|pair| pair[1].as_str())
                .unwrap_or(old);
            if let Some(answer) = applied.get(new) {
                assert_eq!(answer, value, "{name}: {old}");
                if let Some((_, old_feature)) = old.split_once(':') {
                    let old_support = meta["features"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|f| f["id"] == old_feature)
                        .unwrap();
                    let new_feature = new.split_once(':').unwrap().1;
                    let new_support = analysis["features"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|f| f["id"] == new_feature)
                        .unwrap();
                    assert_eq!(old_support["family"], new_support["family"]);
                    // This is the geometric-support rule, never matching by dimension/order.
                    for face in old_support["faces"].as_array().unwrap() {
                        assert!(
                            new_support["faces"].as_array().unwrap().contains(face),
                            "{name}: {old} attached to wrong support {new}"
                        );
                    }
                }
            } else {
                let report = unresolved
                    .iter()
                    .find(|u| u["id"] == *old)
                    .unwrap_or_else(|| panic!("{name}: silently dropped {old}"));
                assert_eq!(report["answer"], *value);
                assert!(!report["reason"].as_str().unwrap().is_empty());
                assert!(
                    warnings
                        .iter()
                        .any(|w| w.contains(old) && w.contains("left open"))
                );
                assert!(!applied.contains_key(old));
            }
        }
        assert_eq!(
            applied.len() + unresolved.len(),
            meta["answers"].as_object().unwrap().len()
        );
        // An unresolved old feature has no accepted answer in the current interview.
        let qs = specify_core::api::questions(&analysis, applied).unwrap();
        let open = specify_core::rules::open_questions(&qs, applied);
        for u in &unresolved {
            if open.iter().any(|q| q.id == u["id"].as_str().unwrap()) {
                assert!(!applied.contains_key(u["id"].as_str().unwrap()));
            }
        }
        // Saving again uses current recognition, and the exact old answers remain readable.
        let intent = specify_core::api::apply(&analysis, applied, true).unwrap();
        let (bytes, _) =
            specify_core::writer::write_bytes(&loaded, &intent, Some(applied)).unwrap();
        let rewritten = specify_core::part::load_bytes(name, bytes).unwrap();
        assert_eq!(answers_for(&rewritten).unwrap().as_ref(), Some(applied));
        eprintln!(
            "{name}: {} reattached, {} unresolved with old ID, {} migrated",
            applied.len(),
            unresolved.len(),
            migrated.len()
        );
    }
}
