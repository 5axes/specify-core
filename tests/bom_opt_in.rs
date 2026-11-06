//! For #79: exercise question/completeness/intent/write through the actual public APIs and CLI.
mod common;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::{api, part::File, rules};
use std::{path::Path, process::Command};
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
#[test]
fn no_opt_in_matches_v046_presentation_through_api_and_cli() {
    let dir = std::env::temp_dir().join(format!("bom-opt-in-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let complete: Vec<Map<String, Value>> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-assembly-answers.json")).unwrap();
    let plate_complete: Map<String, Value> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-plate-answers.json")).unwrap();
    // v0.4.6 #184 places GENERAL NOTES clear of actual drawn strokes/text cells.
    // This moves the notes block in all four plate/assembly default/complete
    // cases below; only presentation changes. The six exact API/CLI byte
    // comparisons and all BOM opt-in guards remain enforced.
    // v0.4.8 changes all four hashes, for these reasons only (checked against v0.4.7's
    // bytes): #198 adds the default Notes saved view (one per file, on the first part of an
    // assembly) and repeats the material id as its description; #193 adds the PMI RP 4.1
    // reference to FILE_DESCRIPTION and names curve sets `note`/`label`; and on assemblies
    // the second part's callout model is named `PMI callouts (2)`.
    // Keep #95/#96's history-schema projection: only schema/role/specification
    // text is normalized. Every other STEP byte, counter, support and numeric ID
    // remains pinned; API and CLI bytes must still match exactly.
    for (fixture, all_answered, defaults, expected) in [
        (
            "plate",
            false,
            true,
            "f382716c25ab150445d4c11cf0d5bade94913816e8e3f630f3887f0eecdbbf49",
        ),
        (
            "assembly",
            true,
            false,
            "a9ed9c8ca0bfb433e3b28b71b36c314c754031dab7ec89076452da2e5e6de08d",
        ),
        (
            "assembly",
            true,
            true,
            "a9ed9c8ca0bfb433e3b28b71b36c314c754031dab7ec89076452da2e5e6de08d",
        ),
        (
            "assembly",
            false,
            true,
            "d9aca2bfdad4a407dfe6d0f3b1aaf623462550dc349df484d29d45ec58cf804c",
        ),
        (
            "plate",
            true,
            false,
            "98d4e965e40ed5acd747c1e446b227206193e6a9398bd19e5c940cdcfbfc0583",
        ),
        (
            "plate",
            true,
            true,
            "98d4e965e40ed5acd747c1e446b227206193e6a9398bd19e5c940cdcfbfc0583",
        ),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/parts/{fixture}.step"));
        let file = File::read(&path).unwrap();
        let mut jobs = Vec::new();
        let output = dir.join(format!("{fixture}-{all_answered}-{defaults}.step"));
        let mut pairs = Vec::new();
        for part in file.load_all().unwrap() {
            let analysis = api::analyse_reporting_in(&file, &part).unwrap().0;
            let answers = if all_answered {
                if fixture == "plate" {
                    plate_complete.clone()
                } else {
                    complete[part.binding.part].clone()
                }
            } else {
                json!({"part.material":"6061-T6"})
                    .as_object()
                    .unwrap()
                    .clone()
            };
            if all_answered {
                assert!(
                    rules::open_questions(&api::questions(&analysis, &answers).unwrap(), &answers)
                        .is_empty()
                );
            }
            let merged = rules::with_all_defaults(&analysis, &answers).unwrap();
            assert!(merged.keys().all(|k| !k.starts_with("bom.")));
            let intent = api::apply(&analysis, &answers, defaults).unwrap();
            assert!(!intent.part.contains_key("bom"));
            let apath = dir.join(format!("{fixture}-{}.analysis.json", part.binding.part));
            let bpath = dir.join(format!("{fixture}-{}.answers.json", part.binding.part));
            std::fs::write(&apath, analysis.to_string()).unwrap();
            std::fs::write(&bpath, json!(answers).to_string()).unwrap();
            pairs.push((apath, bpath));
            jobs.push((intent, Some(answers)));
        }
        let bytes = api::write_bytes(file.bytes().as_ref().clone(), &jobs)
            .unwrap()
            .0;
        assert_eq!(
            hash(&common::numbering::comparable_number_history(&bytes)),
            expected,
            "API {fixture}, complete={all_answered}, defaults={defaults}"
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_specify-core"));
        command.arg("write").arg(&path);
        for (a, b) in pairs {
            command.arg(a).arg(b);
        }
        command.arg("-o").arg(&output);
        if defaults {
            command.arg("--accept-defaults");
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(std::fs::read(output).unwrap(), bytes);
    }
}
