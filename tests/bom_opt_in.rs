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
fn no_opt_in_matches_main_8d5a040_except_history_schema_through_api_and_cli() {
    let dir = std::env::temp_dir().join(format!("bom-opt-in-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let complete: Vec<Map<String, Value>> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-assembly-answers.json")).unwrap();
    let plate_complete: Map<String, Value> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-plate-answers.json")).unwrap();
    // For #95/#96, retain main's no-BOM byte guard across history schemas. These hashes
    // were computed from the original captured 8d5a040 outputs, after verifying their
    // original SHA-256 pins (f527d60c, ca33fa37, cfddc2ec, 294a33b8 respectively).
    // Only schema/role/specification text is normalized; all other STEP bytes, history
    // counters, supports and actual ID_ATTRIBUTE values remain pinned. API/CLI bytes
    // still match each other exactly in all six defaulted/complete cases.
    for (fixture, all_answered, defaults, expected) in [
        (
            "plate",
            false,
            true,
            "9334541fdbfbea7679e8671ed986b095de9a709b12f5f92a681c4854cd6778af",
        ),
        (
            "assembly",
            true,
            false,
            "f18c708898694e72a5f21f17754966e4a7957a1de5e26fa57525a6bc12721bdd",
        ),
        (
            "assembly",
            true,
            true,
            "f18c708898694e72a5f21f17754966e4a7957a1de5e26fa57525a6bc12721bdd",
        ),
        (
            "assembly",
            false,
            true,
            "325a5e135ff45c23c372cd2d43f2d7645fb3722756d6e078aef94ec01c8d765a",
        ),
        (
            "plate",
            true,
            false,
            "95fbb3b5ade988f34e0a9a35e621a67256964797228d213cb0a1649fb7701382",
        ),
        (
            "plate",
            true,
            true,
            "95fbb3b5ade988f34e0a9a35e621a67256964797228d213cb0a1649fb7701382",
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
