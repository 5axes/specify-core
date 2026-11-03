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
    // Keep #95/#96's history-schema projection: only schema/role/specification
    // text is normalized. Every other STEP byte, counter, support and numeric ID
    // remains pinned; API and CLI bytes must still match exactly.
    for (fixture, all_answered, defaults, expected) in [
        (
            "plate",
            false,
            true,
            "476d188c1b42527116bd9564c17c6e0b0f049f7f3d9ffc191f4acff04dee5b81",
        ),
        (
            "assembly",
            true,
            false,
            "5eb56a396bed9761a277f2d99c9c7672b1ba3dc47ebd3a2f97002f99a96c27b8",
        ),
        (
            "assembly",
            true,
            true,
            "5eb56a396bed9761a277f2d99c9c7672b1ba3dc47ebd3a2f97002f99a96c27b8",
        ),
        (
            "assembly",
            false,
            true,
            "ec34b34e8972a6e85cd1ec4c464281f0bda0a025a5afaca71d6a7c176f7082eb",
        ),
        (
            "plate",
            true,
            false,
            "0d16d5c1cff1368017745881c9b45924ede847a76701eafca456e55ea8dc4285",
        ),
        (
            "plate",
            true,
            true,
            "0d16d5c1cff1368017745881c9b45924ede847a76701eafca456e55ea8dc4285",
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
