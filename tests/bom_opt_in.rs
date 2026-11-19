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
    // quiddity 0.5.1 (pinned at v0.5.2) lays GD&T frames out CAD-like: 2.5 mm text, 5 mm
    // boxes and font strokes. All four hashes change for that only: the written STEP differs
    // from v0.5.0's (the previous pin) in its tessellated annotation curves (COORDINATES_LIST
    // points, TESSELLATED_CURVE_SET index lists) and the CARTESIAN_POINT placement points of
    // those annotations, and nothing else (checked on the plate and pin goldens: same entity
    // count, same entity kinds changed).
    // #216: the two complete-assembly byte pins change only for the imported
    // pin answer keys holes:3 -> holes:3.5. The frozen legacy fixture stays keyed
    // as saved and passes through the public matcher; every value and PMI stays exact.
    // Keep #95/#96's history-schema projection: only schema/role/specification
    // text is normalized. Every other STEP byte, counter, support and numeric ID
    // remains pinned; API and CLI bytes must still match exactly.
    for (fixture, all_answered, defaults, expected) in [
        (
            "plate",
            false,
            true,
            "1227a34aa63a6ce010edf3f33780ac7ee828506eb4b07a237cb3d874ad8e46f3",
        ),
        (
            "assembly",
            true,
            false,
            "69c5773fe7fecc4f66dea2fa2dc38993d6cf382802956742cc3eef893b59d484",
        ),
        (
            "assembly",
            true,
            true,
            "69c5773fe7fecc4f66dea2fa2dc38993d6cf382802956742cc3eef893b59d484",
        ),
        (
            "assembly",
            false,
            true,
            "99e5dff19f382adfa0fbeac36ffa709233816ed364269f07cfe63596e16bf4ad",
        ),
        (
            "plate",
            true,
            false,
            "45be4b84df123aa284b0551e1189d3df33be55de0da9f4916c219a22870049e1",
        ),
        (
            "plate",
            true,
            true,
            "45be4b84df123aa284b0551e1189d3df33be55de0da9f4916c219a22870049e1",
        ),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/parts/{fixture}.step"));
        let file = File::read(&path).unwrap();
        let mut jobs = Vec::new();
        let mut cli_jobs = Vec::new();
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
            // The frozen fixtures are saved plain answer files, with no face evidence.
            // Import them through the public ID matcher before the strict current-ID
            // question/apply APIs. Keep their old keys to exercise floor-ID migration.
            let answers = if all_answered {
                let imported = specify_core::resume::rematch_answers(
                    &part,
                    &specify_core::features::recognise(&part).unwrap(),
                    &answers,
                    None,
                    None,
                )
                .unwrap();
                assert!(imported.unresolved.is_empty(), "{:?}", imported.unresolved);
                assert_eq!(imported.answers.len(), answers.len());
                for (old, new) in &imported.migrated {
                    assert_eq!(new, &format!("{}.5", old));
                    assert!(old.ends_with(":holes:3"));
                    assert_eq!(imported.answers[new], answers[old]);
                }
                imported.answers
            } else {
                answers
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
            // With every default taken, the command line also stores those that write no PMI
            // (ADR P9): the same PMI, more stored answers.
            let stored = if defaults {
                api::stored_with_defaults(&part, &analysis, &answers).unwrap()
            } else {
                answers.clone()
            };
            cli_jobs.push((intent.clone(), Some(stored)));
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
        let cli_bytes = api::write_bytes(file.bytes().as_ref().clone(), &cli_jobs)
            .unwrap()
            .0;
        assert_eq!(std::fs::read(output).unwrap(), cli_bytes);
    }
}
