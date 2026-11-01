//! quiddity-rust#134 changes recognition measurements, not the saved physical targets.
mod common;

use serde_json::{Map, Value, json};
use specify_core::{api, existing, features, part, resume, revision, rules, writer};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/quiddity025")
        .join(name)
}

#[test]
fn v024_boss_answers_and_pmi_resume_after_chamfer_and_round_height_corrections() {
    for (name, old_height, height) in [("chamfer", 36.1, 37.6), ("round", 18.5, 20.)] {
        // These files were written with v0.2.4; never recapture them with the new kernel.
        let base: Value =
            serde_json::from_slice(&std::fs::read(fixture(&format!("{name}.json"))).unwrap())
                .unwrap();
        let loaded = part::load(&fixture(&format!("{name}.step.gz")), None).unwrap();
        let fs = features::recognise(&loaded).unwrap();
        let boss = fs.iter().find(|f| f.family == "bosses").unwrap();
        assert_eq!(base["boss"]["record"]["height"], old_height);
        assert_eq!(boss.record["height"], height);
        assert_eq!(json!(boss.id), base["boss"]["id"]);
        assert_eq!(json!(boss.faces), base["boss"]["faces"]);
        assert_eq!(boss.record["diameter"], base["boss"]["record"]["diameter"]);
        assert_eq!(boss.record["axis"], base["boss"]["record"]["axis"]);
        if name == "round" {
            assert!((boss.record["location"][2].as_f64().unwrap() - 24.).abs() < 1e-8);
            assert!((base["boss"]["record"]["location"][2].as_f64().unwrap() - 22.5).abs() < 1e-8);
        } else {
            // This boss points toward its untreated end; only its opposite span grows.
            // Its location is ~1e-11 mm from zero, with digits that differ by platform libm.
            for i in 0..3 {
                let (now, was) = (
                    boss.record["location"][i].as_f64().unwrap(),
                    base["boss"]["record"]["location"][i].as_f64().unwrap(),
                );
                assert!(
                    (now - was).abs() < 1e-8,
                    "{name} location[{i}]: {now} vs {was}"
                );
            }
        }
        assert_eq!(resume::evidence(&loaded).unwrap(), base["evidence"]);
        let answers = base["answers"].as_object().unwrap();
        assert_eq!(
            resume::answers_for(&loaded).unwrap().as_ref(),
            Some(answers)
        );
        let a = api::analyse(&loaded).unwrap();
        assert_eq!(a["resume"]["answers"], base["answers"]);
        assert!(a["resume"]["unresolved"].is_null());
        let mut unchanged = revision::diff(&loaded, &loaded, &Map::new()).unwrap();
        let numbers = unchanged
            .answers
            .shift_remove(specify_core::characteristics::CARRY)
            .unwrap();
        assert_eq!(numbers["schema"], 1);
        assert_eq!(numbers["faces"], base["evidence"]["faces"]);
        assert_eq!(&unchanged.answers, answers);
        assert!(unchanged.unresolved.is_empty());
        assert!(unchanged.check.is_empty());
        assert!(
            api::review(&a, answers).unwrap()["open"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            json!(existing::read_existing(&loaded).unwrap()),
            base["existing"]
        );
        let intent = rules::apply(&a, answers).unwrap();
        // Preserve the complete historical comparison on the legacy path. #75's
        // live path adds planner metadata and removes generated drill-floor rows;
        // it must still preserve every answered requirement and physical span.
        let mut legacy_analysis = a.clone();
        legacy_analysis
            .as_object_mut()
            .unwrap()
            .shift_remove("dimensioning");
        legacy_analysis
            .as_object_mut()
            .unwrap()
            .shift_remove("dimensioning_stated");
        let mut legacy = rules::apply(&legacy_analysis, answers).unwrap().to_dict();
        let mut prior = base["intent"].clone();
        legacy.as_object_mut().unwrap().shift_remove("binding");
        prior.as_object_mut().unwrap().shift_remove("binding");
        assert_eq!(legacy, prior, "{name}: complete legacy intent");
        assert_eq!(json!(intent.part), prior["part"]);
        assert_eq!(json!(intent.datums), prior["datums"]);
        let rows = |rs: &[Value], general: bool| {
            let mut rows: Vec<_> = rs
                .iter()
                .filter(|r| r.get("not_written_reason").is_none())
                .map(|r| {
                    let mut r = r.clone();
                    let map = r.as_object_mut().unwrap();
                    map.shift_remove("planner");
                    if general {
                        // Anonymous plane rows can acquire the shared support's name;
                        // physical endpoints, nominal and tolerance remain authoritative.
                        map.shift_remove("feature");
                        if r["kind"] == "location"
                            && r["faces"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| v.as_u64().unwrap())
                                .cmp(
                                    r["reference"]
                                        .as_array()
                                        .unwrap()
                                        .iter()
                                        .map(|v| v.as_u64().unwrap()),
                                )
                                .is_lt()
                        {
                            let faces = r["faces"].clone();
                            r["faces"] = r["reference"].clone();
                            r["reference"] = faces;
                        }
                    }
                    r
                })
                .collect();
            rows.sort_by_key(|r| {
                (
                    r["question"].to_string(),
                    r["kind"].to_string(),
                    r["faces"].to_string(),
                    r["reference"].to_string(),
                )
            });
            rows
        };
        let current = intent.to_dict();
        assert_eq!(
            rows(current["requirements"].as_array().unwrap(), false),
            rows(prior["requirements"].as_array().unwrap(), false),
            "{name}: all answered contracts"
        );
        let mut expected_general = prior["general"].as_array().unwrap().clone();
        if name == "chamfer" {
            // Face 16 is the M2 drill floor, not a part length (#75); the thread
            // carries its drilling depth. Keep every other historical span.
            let removed: Vec<_> = expected_general
                .iter()
                .filter(|r| r["faces"] == json!([16]))
                .collect();
            assert_eq!(removed.len(), 1);
            assert_eq!(removed[0]["distance"], 5.0);
            expected_general.retain(|r| r["faces"] != json!([16]));
            let unresolved: Vec<_> = intent
                .requirements
                .iter()
                .chain(&intent.general)
                .filter_map(|r| r.get("not_written_reason"))
                .collect();
            assert_eq!(unresolved.len(), 1);
            assert!(
                unresolved[0]
                    .as_str()
                    .unwrap()
                    .contains("General contract conflicts with an answered requirement")
            );
        }
        let written_general: Vec<_> = current["general"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r.get("not_written_reason").is_none())
            .cloned()
            .collect();
        assert_eq!(
            rows(&written_general, true),
            rows(&expected_general, true),
            "{name}: all independent physical spans"
        );
        let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(answers)).unwrap();
        let back = part::load_bytes("rewritten.step", bytes).unwrap();
        assert_eq!(resume::answers_for(&back).unwrap().as_ref(), Some(answers));
        let expected = existing::read_existing(&loaded).unwrap();
        if name == "chamfer" {
            // #75 emits the retained boss fit before the shared hole fit again,
            // matching the old file's order; both original numbers must survive.
            assert_eq!((expected[0].number, expected[1].number), (Some(4), Some(3)));
        }
        let rows = existing::read_existing(&back).unwrap();
        for row in rows.iter().filter(|r| r.kind == "dimension") {
            assert_eq!(
                row.origin,
                Some(specify_core::pmi_origin::Origin::Requirement)
            );
        }
        // #77 adds stored metadata to rewritten old files; compare every legacy field,
        // and verify the new metadata independently in tests/pmi_origin.rs.
        let legacy = rows
            .into_iter()
            .map(|mut row| {
                row.origin = None;
                row.source = None;
                row
            })
            .collect::<Vec<_>>();
        assert_eq!(legacy, expected);

        for evidence in [Some(&base["evidence"]), None] {
            let out = resume::rematch_answers(&loaded, &fs, answers, evidence, None).unwrap();
            assert_eq!(&out.answers, answers);
            assert!(out.unresolved.is_empty());
            assert_eq!(out.unverified.is_some(), evidence.is_none());
        }
        let mut wrong = base["evidence"].clone();
        wrong["faces"] = json!("another geometry");
        let out = resume::rematch_answers(&loaded, &fs, answers, Some(&wrong), None).unwrap();
        let key = "datum.A";
        assert!(!out.answers.contains_key(key));
        assert!(
            out.unresolved
                .iter()
                .any(|u| u.id == key && u.answer == json!([0]))
        );
    }
}

#[test]
fn saved_boss_dimensions_on_a_really_shortened_revision_keep_their_targets() {
    let old = part::load(&fixture("chamfer.step.gz"), None).unwrap();
    let new = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/revisions/gramil_shaft.shorter.step.gz"),
        None,
    )
    .unwrap();
    let r = revision::diff(&old, &new, &Map::new()).unwrap();
    assert_eq!(r.pmi.answers_from, "stored");
    assert_eq!(r.answers["datum.A"], json!([0]));
    assert_eq!(
        r.answers["extra.boss-fit"],
        json!({"faces":[0],"kind":"fit","value":"h6"})
    );
    let boss = r.features.iter().find(|f| f.family == "bosses").unwrap();
    assert_eq!(boss.changes["sizes.height"].old, 37.6);
    assert_eq!(boss.changes["sizes.height"].new, 35.6);
    assert!(r.markdown("Shaft", None).contains("height 37.6 → 35.6"));
    let intent = api::preview(&api::analyse(&new).unwrap(), &r.answers).unwrap();
    let height = intent
        .requirements
        .iter()
        .find(|r| r["question"] == "extra.boss-height")
        .unwrap();
    assert_eq!(height["faces"], json!([13]));
    assert_eq!(height["reference"], json!([7, 11]));
    assert_eq!(height["distance"], 35.6);
    assert_eq!(height["upper"], 0.05);
    assert_eq!(height["lower"], -0.05);
}

#[test]
fn turned_inside_fillets_use_the_existing_concave_description_path() {
    for (case, faces, radius) in [
        ("corpus/nist/nist_ftc_06_asme1_rd", vec![77, 78], 3.175),
        ("corpus/cadgenbench/flanged_spool_132", vec![86, 91], 10.),
    ] {
        let Some(input) = common::case(case).input() else {
            continue;
        };
        let loaded = part::load(&input, None).unwrap();
        let a = api::analyse(&loaded).unwrap();
        for face in faces {
            let f = a["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| {
                    f["family"] == "fillets"
                        && f["faces"].as_array().unwrap().contains(&json!(face))
                })
                .unwrap();
            assert_eq!(f["record"]["turned"], true);
            assert_eq!(f["record"]["side"], "concave");
            assert_eq!(f["record"]["radius"], radius);
            assert_eq!(f["description"], format!("R{radius} concave fillet"));
        }
        // Fillets have no dedicated interview operation. Their side must not change
        // the existing face-based surface questions, answers or PMI.
        let mut convex = a.clone();
        for f in convex["features"].as_array_mut().unwrap() {
            if f["family"] == "fillets" && f["record"]["turned"] == true {
                f["record"]["side"] = json!("convex");
                f.as_object_mut().unwrap().shift_remove("description");
            }
        }
        assert_eq!(
            rules::questions(&a, &Map::new()).unwrap(),
            rules::questions(&convex, &Map::new()).unwrap()
        );
    }
}
