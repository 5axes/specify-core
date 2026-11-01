//! Writes the Rust-written STEP files of the cross-engine check (`tests/cross_engine.rs`).
//!
//! For each synthetic and NIST case of the Python oracle (`tests/fixtures/oracle/{parts,nist}`)
//! whose every part has Python's accept-defaults intent (`apply.json.gz`), the Rust writer
//! (`writer::write_parts`, library calls only) writes those intents into the case's input, every
//! part in one write as Python's case-level write does, with the oracle's base answers stored
//! (`meta.answers`). The result goes to `tests/fixtures/cross_engine/<case>/rust.step.gz`; a
//! write the Rust writer refuses goes to `rust.refused.txt` instead, its message, so a refusal
//! is a recorded result too, not a silently missing case.
//!
//! Python's read-back of each file is then captured with specify-core's own environment:
//!
//! ```text
//! cargo run --release --example write_fixtures
//! /Users/paul/repos/specify-core/.venv/bin/python tools/capture.py cross-engine tests/fixtures/cross_engine
//! ```
//!
//! **For quiddity-rust 0.4.3.** Regenerated with 56a4cbc0 and independently
//! recaptured with the same Python reference (7e2125d, `--rust-only`). All fourteen
//! byte goldens and both refusals remain unchanged. Recognition is unchanged;
//! these cases retain their prior written presentation and short UDA values.
//!
//! **For quiddity-rust 0.4.2.** Regenerated after merging #91/#94 and independently
//! recaptured with `--rust-only` using the same Python reference (7e2125d). All
//! fourteen successful byte goldens, readbacks and metadata, and both refusals,
//! are unchanged from main 3936c84. None of these inputs needs the temporary
//! multi-child datum adjustment. Historical Python-written specimens stay frozen.
//!
//! **For quiddity-rust 0.4.1.** Recaptured for regenerated ladders/callouts,
//! one numbered GENERAL NOTES block and generated plane views. Semantic intent and
//! recognition are unchanged; AP214 PRODUCT_TYPE entities upgrade with the schema.
//! The previous v0.2.6 recapture added: default presentation adds callouts,
//! surface symbols and notes; composite and location aspects change semantic structure.
//! These files include #65 ids/history and #77 dimension origin/source UDAs, checked byte
//! for byte with independent Python readbacks. Historical category-order and alpha.7
//! specimens remain unchanged.
//!
//! **Regenerating.** `tests/cross_engine.rs` checks that each committed file is still exactly
//! what the writer produces today (the decompressed bytes, byte for byte; the writer is
//! deterministic and a write's bytes depend only on the input and the intent). When a
//! deliberate change to the writer or to haecceity changes them, the test fails naming the
//! case: rerun this example, recapture Python's read-back (above) and review the
//! `known_differences/cross_engine.json` entries the new read-back meets, in one commit.
//!
//! The gzip wrapper is not part of the check (a different `flate2` may compress differently);
//! it is written with no file name and a zero time stamp, so rerunning on the same build gives
//! the same `.gz` bytes too.

// When `tests/cross_engine.rs` includes this file as a module, it uses the case list and the
// write but not `main`.
#![allow(dead_code)]

// Public so that `tests/cross_engine.rs`, which includes this file, uses this one copy.
#[path = "../tests/common/mod.rs"]
pub mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use specify_core::part::{self, LoadedPart};
use specify_core::rules::Intent;
use specify_core::writer::write_parts;

/// One case of the cross-engine check: an oracle case of a whole input file.
pub struct Written {
    /// The oracle case, e.g. `parts/plate`, `nist/nist_ctc_01_asme1_ap242`.
    pub name: String,
    /// The input both engines read.
    pub input: PathBuf,
    /// Python's accept-defaults intent of each part, in part order.
    pub intents: Vec<Value>,
    /// The answers stored with each part (the oracle's base answers).
    pub answers: Map<String, Value>,
}

/// `tests/fixtures/cross_engine`.
pub fn fixtures_dir() -> PathBuf {
    common::fixtures().join("cross_engine")
}

/// The directory of case `name` under `tests/fixtures/cross_engine`.
pub fn case_dir(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

/// Every synthetic and NIST oracle case of a whole file whose parts all have Python's intent,
/// sorted by name. A part whose intent Python could not make (NIST CTC-05's fit above 500 mm,
/// FTC-09's failed analysis) leaves its file out: there is no intent to write in either engine.
pub fn cases() -> Vec<Written> {
    let all = common::cases();
    let mut out = Vec::new();
    for case in &all {
        if case.part().is_some() || !matches!(case.kind(), "parts" | "nist") {
            continue;
        }
        let count = case.meta["parts"].as_u64().unwrap_or(1) as usize;
        let intents: Option<Vec<Value>> = if count == 1 {
            case.artifact("apply").map(|a| vec![a])
        } else {
            (0..count)
                .map(|k| {
                    all.iter()
                        .find(|p| p.name == format!("{}#{k}", case.name))
                        .and_then(|p| p.artifact("apply"))
                })
                .collect()
        };
        let Some(intents) = intents.filter(|i| !i.iter().any(common::is_error)) else {
            continue;
        };
        let input = case
            .input()
            .unwrap_or_else(|| panic!("{}: committed input missing", case.name));
        let answers = case.meta["answers"]
            .as_object()
            .cloned()
            .unwrap_or_else(|| panic!("{}: meta.answers is not an object", case.name));
        out.push(Written {
            name: case.name.clone(),
            input,
            intents,
            answers,
        });
    }
    out
}

/// The Rust writer's output for `case`: the bytes written to `output`, or why it refused.
/// Each intent is written to the part its binding names. An intent whose part the Rust engine's
/// reading of the input does not have (a file it splits into parts differently from
/// OpenCascade, question Q7) is refused here, naming the part, before `write_parts` is called,
/// rather than written to the wrong part; so is an intent with no part in its binding on a
/// file of several parts. The writer's own checks then apply to what is written.
pub fn write(case: &Written, output: &Path) -> Result<Vec<u8>, String> {
    let loaded: Vec<LoadedPart> = part::load_all(&case.input).map_err(|e| e.to_string())?;
    let intents: Vec<Intent> = case
        .intents
        .iter()
        .map(Intent::from_dict)
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let mut jobs = Vec::new();
    for intent in &intents {
        // A binding with no part means the only part; on a file of several parts it is
        // ambiguous, and refused rather than guessed.
        let k = match intent.binding.get("part") {
            Some(part) => part.as_u64().ok_or_else(|| {
                format!("intent's part {part} is not a part index ({})", case.name)
            })? as usize,
            None if loaded.len() == 1 => 0,
            None => {
                return Err(format!(
                    "intent names no part, and the Rust engine reads {} parts in {}'s input",
                    loaded.len(),
                    case.name
                ));
            }
        };
        let Some(part) = loaded.get(k) else {
            return Err(format!(
                "intent was made for part {k}, and the Rust engine reads {} part(s) in {}'s input",
                loaded.len(),
                case.name
            ));
        };
        jobs.push((part, intent, Some(&case.answers)));
    }
    write_parts(&jobs, output).map_err(|e| e.to_string())?;
    std::fs::read(output).map_err(|e| format!("{}: {e}", output.display()))
}

/// `bytes` gzipped reproducibly: no file name, time stamp zero, level 9.
pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes).expect("gzip in memory");
    encoder.finish().expect("gzip in memory")
}

/// A scratch directory for the staged outputs, under the system's temporary directory.
pub fn scratch() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-cross-engine")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn main() {
    let scratch = scratch();
    let cases = cases();
    for case in &cases {
        let dir = case_dir(&case.name);
        std::fs::create_dir_all(&dir).expect("fixture directory");
        let output = scratch.join(format!("{}.step", case.name.replace(['/', '#'], "_")));
        let (step, refused) = (dir.join("rust.step.gz"), dir.join("rust.refused.txt"));
        match write(case, &output) {
            Ok(bytes) => {
                std::fs::write(&step, gzip(&bytes)).expect("rust.step.gz");
                let _ = std::fs::remove_file(&refused);
                println!("{}: {} bytes written", case.name, bytes.len());
            }
            Err(why) => {
                std::fs::write(&refused, format!("{why}\n")).expect("rust.refused.txt");
                let _ = std::fs::remove_file(&step);
                println!("{}: refused: {why}", case.name);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    println!("{} cases", cases.len());
}
