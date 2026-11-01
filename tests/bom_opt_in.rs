//! For #79: exercise question/completeness/intent/write through the actual public APIs and CLI.
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
fn no_opt_in_is_byte_identical_to_main_8d5a040_through_api_and_cli() {
    let dir = std::env::temp_dir().join(format!("bom-opt-in-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let complete: Vec<Map<String, Value>> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-assembly-answers.json")).unwrap();
    let plate_complete: Map<String, Value> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-plate-answers.json")).unwrap();
    // SHA-256 of pristine main 8d5a040 CLI writes, including planner dimensions and
    // owners of answers/characteristic history. Captured with fresh per-engine analyses:
    // all six single-part/assembly and complete/defaulted CLI writes match byte for byte.
    for (fixture, all_answered, defaults, expected) in [
        (
            "plate",
            false,
            true,
            "f527d60c401ded8dc99d0ec8e3e8e380edabf6aad9af8667dbd55c2cc34eb023",
        ),
        (
            "assembly",
            true,
            false,
            "ca33fa372e9d1be19ff1accc2e59ac7d945e977b64bf3565fc482a709a866b89",
        ),
        (
            "assembly",
            true,
            true,
            "ca33fa372e9d1be19ff1accc2e59ac7d945e977b64bf3565fc482a709a866b89",
        ),
        (
            "assembly",
            false,
            true,
            "cfddc2ec06aed65d58d85b43f75e54ace80fe91f7047cc62d7e612822711b111",
        ),
        (
            "plate",
            true,
            false,
            "294a33b82728b26c94c3202f60d3e27bbed1036f8253ea0ad0b8f5a0b9bb59cf",
        ),
        (
            "plate",
            true,
            true,
            "294a33b82728b26c94c3202f60d3e27bbed1036f8253ea0ad0b8f5a0b9bb59cf",
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
            hash(&bytes),
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
