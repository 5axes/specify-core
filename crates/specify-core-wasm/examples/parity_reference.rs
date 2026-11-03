//! Native reference for scripts/wasm-parity.mjs. No browser or generated JS involved.
use std::io::{self, Read};

use serde_json::{Map, Value, json};
use specify_core::{api, part, writer};
use specify_core_wasm::{accept_defaults, interpret, session::Session};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
    if let Some(pair) = request["diff"].as_array() {
        let old = part::File::read(std::path::Path::new(pair[0].as_str().ok_or("old path")?))?;
        let new = part::File::read(std::path::Path::new(pair[1].as_str().ok_or("new path")?))?;
        let session = Session::open("old.step", old.bytes())?;
        let defaults = session.accept_defaults(&Map::new())?;
        println!(
            "{}",
            specify_core_wasm::session::diff_with_options(
                ("old.step", old.bytes()),
                ("new.step", new.bytes()),
                &defaults,
                "Spindle",
                specify_core::revision::DiffOptions { scenes: true },
            )?
        );
        return Ok(());
    }
    let name = request["name"].as_str().ok_or("missing name")?;
    let bytes = std::fs::read(request["path"].as_str().ok_or("missing path")?)?;
    let answers: Map<String, Value> = serde_json::from_value(request["answers"].clone())?;
    let session = Session::open(name, &bytes)?;
    // Load independently through specify-core too; the browser loader must agree with it.
    let loaded = part::load_bytes(name, bytes)?;
    let analysis = api::analyse(&loaded)?;
    let snapshot = session.snapshot();
    assert_eq!(snapshot["analysis"], analysis);
    assert_eq!(snapshot["mesh"], api::mesh(&loaded));
    let intent = api::preview(&analysis, &answers)?;
    let (bare_bytes, report) = writer::write_bytes(&loaded, &intent, Some(&answers))?;
    let accepted = accept_defaults(&analysis, &answers)?;
    assert_eq!(session.accept_defaults(&answers)?, accepted);
    let applied = api::apply(&analysis, &answers, true)?;
    let (accepted_bytes, accepted_report) =
        writer::write_bytes(&loaded, &applied, Some(&accepted))?;
    let accepted_write = session.write_pmi(&accepted, None, None)?;
    assert_eq!(accepted_write["bytes"], json!(accepted_bytes));
    assert_eq!(accepted_write["report"], accepted_report.to_dict());
    println!(
        "{}",
        json!({
            "snapshot": snapshot,
            "pmi_scene": session.pmi_scene(&answers)?,
            "review": session.review(&answers)?,
            "free_review": api::review(&analysis, &answers)?,
            "accepted": accepted,
            "defaults_without_answers": accept_defaults(&analysis, &Map::new())?,
            "accepted_review": session.review(&accepted)?,
            "accepted_free_review": api::review(&analysis, &accepted)?,
            "accepted_write": accepted_write,
            "accepted_settle": session.settle(&specify_core::tolerancing::State {
                answers: accepted.clone(), auto: Some(vec![]), gdt: None,
            })?,
            "interpret": interpret(&analysis, &answers,
                request["question_id"].as_str().ok_or("missing question_id")?,
                request["text"].as_str().ok_or("missing text")?)?,
            "bare": {"bytes": bare_bytes, "report": report.to_dict(),
                "source_sha256": loaded.binding.source_sha256},
            "project": session.write_pmi(&answers, request["project"].as_str(), None)?,
        })
    );
    Ok(())
}
