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
fn no_opt_in_matches_v044_presentation_through_api_and_cli() {
    let dir = std::env::temp_dir().join(format!("bom-opt-in-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let complete: Vec<Map<String, Value>> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-assembly-answers.json")).unwrap();
    let plate_complete: Map<String, Value> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-plate-answers.json")).unwrap();
    // v0.4.4 adds thread ink and repacks neighbouring annotations. Four distinct
    // hashes pin these six no-BOM cases. Independent v0.4.3-main CLI writes retain
    // identical bare recognition, semantic PMI and part settings in all six cases.
    // Keep #95/#96's history-schema projection: only schema/role/specification
    // text is normalized. Every other STEP byte, counter, support and numeric ID
    // remains pinned; API and CLI bytes must still match exactly.
    for (fixture, all_answered, defaults, expected) in [
        (
            "plate",
            false,
            true,
            "9554820a86f79f36290f292c95314d29036d53f697e701efcc5a431db31dafcd",
        ),
        (
            "assembly",
            true,
            false,
            "1d627fdc6520470e1098cd2fa670d9c4eb8cbcf95c495853b6b5965a43a59a22",
        ),
        (
            "assembly",
            true,
            true,
            "1d627fdc6520470e1098cd2fa670d9c4eb8cbcf95c495853b6b5965a43a59a22",
        ),
        (
            "assembly",
            false,
            true,
            "17c82d9b786db0bef46de003b9b6319abfe100aff1576ca478f53595b2b92c38",
        ),
        (
            "plate",
            true,
            false,
            "faf228b938a5a6366837f0085ef9766b9bc74d9044dc0c2507c15c68d780ac72",
        ),
        (
            "plate",
            true,
            true,
            "faf228b938a5a6366837f0085ef9766b9bc74d9044dc0c2507c15c68d780ac72",
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
