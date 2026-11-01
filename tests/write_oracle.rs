//! The writer against the Python oracle: every captured write (each case's accept-defaults
//! intent and each scenario that writes) is written again by the Rust writer, from Python's
//! own intent, and read back with the Rust reader (`existing::read_existing`,
//! `existing::part_settings`, `resume::answers_for`); the report and the read-back are compared
//! with Python's (`write.json`, `readback.json`: Python's direct read-back of the file it
//! wrote). Then each file Python wrote (`written.step.gz`) is resumed by the Rust engine.
//! Every difference must have a verdict in `known_differences/writer.json`.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{
    Case, Difference, Tolerances, case, cases, check_known, compare, corpus_dir, explained,
    is_error, scenarios,
};
use serde_json::{Map, Value, json};
use specify_core::existing::{part_settings, read_existing};
use specify_core::part::{LoadedPart, load};
use specify_core::resume::answers_for;
use specify_core::rules::Intent;
use specify_core::writer::{WriteError, write_parts};

/// One captured write: the input, and per part written its intent and the answers stored.
struct Captured {
    name: String,
    input: PathBuf,
    /// Python's `{"part": k}` of each job: (part, intent, answers stored).
    jobs: Vec<(usize, Value, Option<Value>)>,
    report: Value,
    readback: Option<Value>,
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-write-oracle")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The case-level writes: a file of one part with its `apply`, a file of several with each
/// part's `apply`, the base answers stored for each.
fn case_writes() -> Vec<Captured> {
    let all = cases();
    let mut out = Vec::new();
    for c in &all {
        if c.part().is_some() {
            continue;
        }
        let Some(report) = c.artifact("write") else {
            continue;
        };
        if report.get("skipped").is_some() {
            continue;
        }
        let count = c.meta["parts"].as_u64().unwrap_or(1) as usize;
        let intents: Vec<Option<Value>> = if count == 1 {
            vec![c.artifact("apply")]
        } else {
            (0..count)
                .map(|k| {
                    all.iter()
                        .find(|p| p.name == format!("{}#{k}", c.name))
                        .and_then(|p| p.artifact("apply"))
                })
                .collect()
        };
        // A part whose intent Python could not make has nothing to write in either engine.
        if intents.iter().any(|i| i.as_ref().is_none_or(is_error)) {
            continue;
        }
        let Some(input) = c.input() else { continue };
        let answers = c.meta["answers"].clone();
        out.push(Captured {
            name: c.name.clone(),
            input,
            jobs: intents
                .into_iter()
                .enumerate()
                .map(|(k, i)| (k, i.unwrap(), Some(answers.clone())))
                .collect(),
            report,
            readback: c.artifact("readback"),
        });
    }
    out
}

/// The scenarios that write: their intent, stored or not as the scenario says.
fn scenario_writes() -> Vec<Captured> {
    let mut out = Vec::new();
    for s in scenarios() {
        let Some(how) = s.spec.get("write").and_then(Value::as_str) else {
            continue;
        };
        let Some(result) = &s.result else { continue };
        let Some(intent) = result.get("apply").filter(|a| !is_error(a)) else {
            continue;
        };
        let c: Case = case(&s.case);
        let Some(input) = c.input() else { continue };
        out.push(Captured {
            name: format!("scenarios/{}", s.name),
            input,
            jobs: vec![(
                c.part().unwrap_or(0),
                intent.clone(),
                (how == "with answers").then(|| s.answers.clone()),
            )],
            report: result["write"].clone(),
            readback: result.get("readback").cloned(),
        });
    }
    out
}

/// What the Rust engine reads back of part `k` of `path`, in the read-back's layout.
fn rust_readback(path: &Path, k: usize, count: usize) -> Value {
    let loaded = match load(path, (count > 1).then_some(k)) {
        Ok(l) => l,
        Err(e) => return json!({"error": "LoadError", "message": e.to_string()}),
    };
    readback_of(&loaded)
}

fn readback_of(loaded: &LoadedPart) -> Value {
    let existing = match read_existing(loaded) {
        Ok(e) => serde_json::to_value(common::numbering::legacy_existing(e)).unwrap(),
        Err(e) => json!({"error": "ExistingError", "message": e.to_string()}),
    };
    let part = match part_settings(loaded) {
        Ok(p) => Value::Object(p),
        Err(e) => json!({"error": "ExistingError", "message": e.to_string()}),
    };
    let resume = match answers_for(loaded) {
        Ok(Some(a)) => Value::Object(a),
        Ok(None) => Value::Null,
        Err(e) => json!({"error": "FeatureError", "message": e.to_string()}),
    };
    json!({"existing": existing, "part": part, "resume": resume})
}

/// The differences between two lists of report lines, as multisets: `path[line]`.
fn lines(name: &str, path: &str, python: &Value, rust: &Value) -> Vec<Difference> {
    let count = |v: &Value| {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for l in v.as_array().into_iter().flatten() {
            *m.entry(l.as_str().unwrap_or_default().to_string())
                .or_default() += 1;
        }
        m
    };
    let (p, r) = (count(python), count(rust));
    let mut out = Vec::new();
    for line in p
        .keys()
        .chain(r.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (a, b) = (
            p.get(line).copied().unwrap_or(0),
            r.get(line).copied().unwrap_or(0),
        );
        if a != b {
            out.push(Difference {
                case: name.into(),
                path: format!("{path}[{line}]"),
                python: (a > 0).then(|| json!(a)),
                rust: (b > 0).then(|| json!(b)),
            });
        }
    }
    out
}

/// Each existing-PMI item keyed by kind, type and faces (and its occurrence, for a repeat), as
/// the existing-PMI sweep keys them (`tests/existing.rs`), so the two lists pair item by item
/// whatever their order and the reading differences that sweep explains are recognised here.
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

/// The ISO 286 limits each fit of `intent` means, by its sorted faces: the writer writes a fit
/// as its class alone (and refuses an intent whose limits are not its class's), so a fit reads
/// back with no limits, and the read-back is compared as the limits its class states.
fn fits(intent: &Value) -> Vec<(Vec<u64>, f64, f64, f64)> {
    let mut out = Vec::new();
    for r in intent["requirements"].as_array().into_iter().flatten() {
        let (Some(fit), Some(nominal)) = (r["fit"].as_str(), r["nominal"].as_f64()) else {
            continue;
        };
        if r["kind"] != "size" || fit.is_empty() {
            continue;
        }
        let mut faces: Vec<u64> = r["faces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect();
        faces.sort_unstable();
        if let Ok((upper, lower)) = specify_core::standards::fit_limits(fit, nominal) {
            out.push((faces, nominal, upper, lower));
        }
    }
    out
}

/// A Rust read-back with each size that reads back as a class alone (no limits, on the faces
/// of one of `fits`) given the limits that class states.
fn with_class_limits(rust: &Value, fits: &[(Vec<u64>, f64, f64, f64)]) -> Value {
    let mut rust = rust.clone();
    for item in rust["existing"].as_array_mut().into_iter().flatten() {
        if item["kind"] != "dimension"
            || !item["type"].as_str().unwrap_or("").starts_with("Size_")
            || !item["upper"].is_null()
            || !item["lower"].is_null()
        {
            continue;
        }
        let mut faces: Vec<u64> = item["faces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect();
        faces.sort_unstable();
        if let Some((_, _, upper, lower)) = fits.iter().find(|(f, nominal, _, _)| {
            *f == faces
                && item["value"]
                    .as_f64()
                    .is_some_and(|v| (v - nominal).abs() < 1e-6)
        }) {
            item["upper"] = json!(upper);
            item["lower"] = json!(lower);
        }
    }
    rust
}

/// The read-backs of one part compared: `existing` items paired by key, `part` and `resume`
/// as values. A resume Python's own read-back did not make (it could not resume the file it
/// wrote) is told apart from one it made differently.
fn compare_readback(name: &str, python: &Value, rust: &Value) -> Vec<Difference> {
    let tolerances = Tolerances::default()
        .align("existing", "key")
        .with("existing*", 1e-9, 1e-9);
    let view = |v: &Value| {
        json!({
            "existing": if v["existing"].is_array() { keyed(&v["existing"]) } else { v["existing"].clone() },
            "part": v["part"],
            "resume": v["resume"],
        })
    };
    let mut diffs = compare(name, &view(python), &view(rust), &tolerances);
    for d in &mut diffs {
        if d.path == "resume" && d.python == Some(Value::Null) {
            d.path = "resume[python did not resume]".into();
        }
    }
    diffs
}

fn prefixed(diffs: Vec<Difference>, at: &str) -> Vec<Difference> {
    diffs
        .into_iter()
        .map(|mut d| {
            d.path = if d.path.is_empty() {
                at.to_string()
            } else {
                format!("{at}.{}", d.path)
            };
            d
        })
        .collect()
}

/// One captured write done again by the Rust writer, and its differences.
fn rewrite(c: &Captured, dir: &Path) -> Vec<Difference> {
    let mut out = Vec::new();
    let count = c.jobs.iter().map(|(k, _, _)| *k).max().map_or(1, |k| k + 1);
    // The file's part count, which decides whether a part is loaded by index.
    // Count and load jobs through one parsed file, including multi-part NIST cases.
    let file = specify_core::part::File::read(&c.input).map_err(|e| e.to_string());
    let parts = file.as_ref().map_or(count, |f| f.parts().len());
    let loaded: Vec<Result<LoadedPart, String>> = c
        .jobs
        .iter()
        .map(|(k, _, _)| {
            file.as_ref()
                .map_err(Clone::clone)
                .and_then(|f| f.load((parts > 1).then_some(*k)).map_err(|e| e.to_string()))
        })
        .collect();
    let intents: Vec<Result<Intent, String>> = c
        .jobs
        .iter()
        .map(|(_, i, _)| Intent::from_dict(i).map_err(|e| e.to_string()))
        .collect();
    let answers: Vec<Option<Map<String, Value>>> = c
        .jobs
        .iter()
        .map(|(_, _, a)| a.as_ref().and_then(Value::as_object).cloned())
        .collect();
    let output = dir.join(format!("{}.step", c.name.replace(['/', '#'], "_")));
    let result: Result<Value, Value> = (|| {
        let mut jobs = Vec::new();
        for ((l, i), a) in loaded.iter().zip(&intents).zip(&answers) {
            let l = l
                .as_ref()
                .map_err(|e| json!({"error": "LoadError", "message": e}))?;
            let i = i
                .as_ref()
                .map_err(|e| json!({"error": "Invalid", "message": e}))?;
            jobs.push((l, i, a.as_ref()));
        }
        write_parts(&jobs, &output)
            .map(|r| {
                let mut d = r.to_dict();
                d["output"] = json!("{output}/written.step");
                d
            })
            .map_err(|e| {
                let kind = match &e {
                    WriteError::Invalid(_) => "Invalid",
                    WriteError::Refused(_) => "Refused",
                    WriteError::Verification(_) => "VerificationError",
                    WriteError::Failed(_) => "Failed",
                };
                json!({"error": kind, "message": e.to_string()})
            })
    })();
    match (&c.report, &result) {
        (p, Ok(r)) if !is_error(p) => {
            for key in ["written", "not_written", "warnings"] {
                out.extend(lines(&c.name, &format!("write.{key}"), &p[key], &r[key]));
            }
        }
        (p, Err(r)) if is_error(p) => {
            if p["message"] != r["message"] {
                // The Rust message in the path, so that an entry names why Rust refused.
                out.push(Difference {
                    case: c.name.clone(),
                    path: format!("write.message[{}]", r["message"].as_str().unwrap_or("")),
                    python: Some(p.clone()),
                    rust: Some(r.clone()),
                });
            }
        }
        (p, r) => out.push(Difference {
            case: c.name.clone(),
            path: "write".into(),
            python: Some(p.clone()),
            rust: Some(match r {
                Ok(v) | Err(v) => v.clone(),
            }),
        }),
    }
    if result.is_ok() {
        // test_conformance.py, as kept: every file written passes the EXPRESS check, but for
        // what the input itself violates and the writer keeps byte for byte (NIST CTC-01 and
        // CTC-03 state INTEGER_REPRESENTATION_ITEMs with REAL values).
        let input = loaded
            .iter()
            .flatten()
            .next()
            .map_or_else(Vec::new, |l| violations(l.bytes.to_vec()));
        let problems: Vec<String> = violations(std::fs::read(&output).unwrap())
            .into_iter()
            .filter(|v| !input.contains(v))
            .collect();
        if !problems.is_empty() {
            out.push(Difference {
                case: c.name.clone(),
                path: "conformance".into(),
                python: None,
                rust: Some(json!(problems)),
            });
        }
    }
    if let (Ok(_), Some(Value::Array(back))) = (&result, &c.readback) {
        for (k, python) in back.iter().enumerate() {
            let fits = c.jobs.get(k).map_or_else(Vec::new, |(_, i, _)| fits(i));
            let rust = with_class_limits(&rust_readback(&output, k, back.len()), &fits);
            let name = if back.len() > 1 {
                format!("{}#{k}", c.name)
            } else {
                c.name.clone()
            };
            out.extend(compare_readback(&name, python, &rust));
        }
    }
    if let Some(dump) = std::env::var_os("SPECIFY_DUMP")
        && output.is_file()
    {
        let dump = PathBuf::from(dump).join("writes");
        std::fs::create_dir_all(&dump).unwrap();
        std::fs::copy(&output, dump.join(output.file_name().unwrap())).unwrap();
        let rows: Vec<_> = out
            .iter()
            .map(|d| {
                json!({"case":d.case,"path":d.path,
            "python":d.python,"rust":d.rust})
            })
            .collect();
        std::fs::write(
            dump.join(output.file_name().unwrap())
                .with_extension("differences.json"),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
    let _ = std::fs::remove_file(&output);
    out
}

/// The AP242 instances and EXPRESS rules haecceity finds violated in the Part 21 `bytes`.
fn violations(bytes: Vec<u8>) -> Vec<String> {
    let doc = match haecceity::p21::Document::parse(bytes) {
        Ok(d) => d,
        Err(e) => return vec![e.to_string()],
    };
    let mut out: Vec<String> = haecceity::express::validate_document(doc.graph())
        .iter()
        .map(|v| format!("{v:?}"))
        .collect();
    out.extend(
        haecceity::express_rules::check_all(&doc)
            .iter()
            .map(|v| format!("{v:?}")),
    );
    out
}

/// The files Python wrote, resumed by the Rust engine: `answers_for` of each part against
/// Python's own read-back of its resume.
fn python_written_resume() -> Vec<Difference> {
    let mut out = Vec::new();
    for c in cases() {
        if c.part().is_some() {
            continue;
        }
        let (Some(written), Some(Value::Array(back))) = (c.written_step(), c.artifact("readback"))
        else {
            continue;
        };
        for (k, python) in back.iter().enumerate() {
            let rust = match load(&written, (back.len() > 1).then_some(k)) {
                Ok(l) => match answers_for(&l) {
                    Ok(Some(a)) => Value::Object(a),
                    Ok(None) => Value::Null,
                    Err(e) => json!({"error": "FeatureError", "message": e.to_string()}),
                },
                Err(e) => json!({"error": "LoadError", "message": e.to_string()}),
            };
            let name = if back.len() > 1 {
                format!("{}#{k}", c.name)
            } else {
                c.name.clone()
            };
            out.extend(prefixed(
                compare(&name, &python["resume"], &rust, &Tolerances::default()),
                "python_written.resume",
            ));
        }
    }
    out
}

/// specify-core's test assertions this port does not carry over, each raised as a difference
/// so that its verdict (not-applicable, with the reason) is in `writer.json` with the rest:
/// what they assert is Python's own text route (the 'manufacturing requirement' sentences and
/// their leaders), which haecceity does not write (its decision 5: standard forms only), or
/// Python's drawing of datum symbols, which haecceity does itself (decision 6).
const DROPPED: &[(&str, &str)] = &[
    (
        "test_requirements.py::test_sentences_are_in_the_form_draftwright_reads",
        "requirements.sentence() matches Draftwright's parser for threads and knurls",
    ),
    (
        "test_requirements.py::test_sentences_claim_only_what_is_known",
        "a sentence claims a drill point and chamfers only when known",
    ),
    (
        "test_requirements.py::test_the_text_route_and_its_callouts",
        "'manufacturing requirement' properties of each kind, and a leader callout per note",
    ),
    (
        "test_requirements.py::test_verification_notices_a_missing_requirement",
        "_verify_appended finds a sentence missing from the file",
    ),
    (
        "test_requirements.py::test_the_default_finish_is_in_the_form_draftwright_draws",
        "the default finish sentence 'Ra 3.2 um unless otherwise specified'",
    ),
    (
        "test_threads.py::test_a_typed_thread_depth_is_written_and_read_back",
        "the sentence '5.6 mm minimum full thread' in the file",
    ),
    (
        "test_mmc_finish_notes.py::test_a_face_finish_is_written_on_its_face",
        "the face finish sentence 'Ra 0.8 \\X2\\00B5\\X0\\m' and its note on the face",
    ),
    (
        "test_conformance.py::test_callout_presentation_is_named",
        "TESSELLATED_GEOMETRIC_SET('note') for a note's leader",
    ),
    (
        "test_drawn.py::test_a_note_is_its_leader_and_no_words",
        "each note is drawn as a two-point leader",
    ),
    (
        "test_drawn.py::test_letters_are_strokes_scaled_to_their_height",
        "lettering.line() strokes for datum symbols",
    ),
];

fn dropped() -> Vec<Difference> {
    DROPPED
        .iter()
        .map(|(test, assertion)| Difference {
            case: format!("tests/{test}"),
            path: "not ported".into(),
            python: Some(json!(assertion)),
            rust: None,
        })
        .collect()
}

/// `check_known("writer")`, but without the corpus the entries for corpus cases and
/// for scenarios on corpus cases, and the cross-case (`*`) entries that then match nothing, are
/// left out: their cases were skipped, so they cannot be stale. With the corpus, every entry
/// must match.
fn check_area(differences: &[Difference]) {
    if corpus_dir().is_some() {
        return check_known("writer", differences);
    }
    let on_corpus: Vec<String> = scenarios()
        .into_iter()
        .filter(|s| s.case.starts_with("corpus/"))
        .map(|s| format!("scenarios/{}", s.name))
        .collect();
    let path = common::known_differences_file("writer");
    let entries: Vec<_> = common::load_known(&path)
        .into_iter()
        .filter(|e| !e.case.starts_with("corpus/") && !on_corpus.contains(&e.case))
        .filter(|e| e.case != "*" || differences.iter().any(|d| e.matches(d)))
        .collect();
    println!("no corpus: corpus entries of writer not checked");
    common::check_known_with(&path.display().to_string(), &entries, differences);
}

#[test]
fn writes_match_the_oracle() {
    let dir = temp_dir();
    let mut captured = case_writes();
    captured.extend(scenario_writes());
    let mut differences = Vec::new();
    for c in &captured {
        differences.extend(rewrite(c, &dir));
    }
    differences.extend(python_written_resume());
    println!("{} writes compared", captured.len());
    let _ = std::fs::remove_dir_all(&dir);
    // How the file's own PMI reads (NIST's) is the existing-PMI sweep's to explain; what is
    // left is the writer's.
    let mut differences = explained(&["existing"], &differences);
    differences.extend(dropped());
    if let Some(dir) = std::env::var_os("SPECIFY_DUMP") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rows: Vec<_> = differences
            .iter()
            .map(|d| json!({"case":d.case,"path":d.path,"python":d.python,"rust":d.rust}))
            .collect();
        std::fs::write(
            dir.join("writer.json"),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
    check_area(&differences);
}

#[test]
fn class_limits_never_attach_to_another_size_on_the_same_faces() {
    let read = json!({"existing":[
        {"kind":"dimension","type":"Size_Diameter","faces":[1,2],"value":90.},
        {"kind":"dimension","type":"Size_Diameter","faces":[1,2],"value":10.}
    ]});
    let actual = with_class_limits(&read, &[(vec![1, 2], 10., 0.09, 0.)]);
    assert!(actual["existing"][0].get("upper").is_none());
    assert!(actual["existing"][0].get("lower").is_none());
    assert_eq!(actual["existing"][1]["upper"], json!(0.09));
    assert_eq!(actual["existing"][1]["lower"], json!(0.));
}
