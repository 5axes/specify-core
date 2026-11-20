//! Consumer regressions for quiddity #123 and #127; the recognition schema stays 6.
mod common;
use serde_json::{Map, json};
use specify_core::{api, features, part, resume, revision, rules, writer};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/quiddity023")
        .join(name)
}

fn seed() -> Map<String, serde_json::Value> {
    json!({"part.material":"steel"})
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn body_and_face_grooves_are_listed_without_duplicate_questions_or_intent() {
    for (name, grooves) in [("turned_body_disc.step", 0), ("face_grooves.step", 2)] {
        let loaded = part::load(&fixture(name), None).unwrap();
        let a = api::analyse(&loaded).unwrap();
        let fs = a["features"].as_array().unwrap();
        let bodies: Vec<_> = fs
            .iter()
            .filter(|f| f["family"] == "turned_bodies")
            .collect();
        assert_eq!(bodies.len(), 1);
        assert_eq!(
            bodies[0]["faces"],
            json!((0..loaded.part.faces.len()).collect::<Vec<_>>())
        );
        assert_eq!(bodies[0]["description"], "Ø22 × 7 turned body");
        assert!(!fs.iter().any(|f| f["family"] == "bosses"));
        let gs: Vec<_> = fs
            .iter()
            .filter(|f| f["family"] == "face_grooves")
            .collect();
        assert_eq!(gs.len(), grooves);
        for g in gs {
            assert_eq!(
                g["description"],
                "face groove Ø18.5/Ø13 × 0.25 deep, 45° walls"
            );
            assert_eq!(g["faces"].as_array().unwrap().len(), 3);
            assert_eq!(g["record"]["floor"].as_array().unwrap().len(), 1);
        }
        let mut without = a.clone();
        without["features"]
            .as_array_mut()
            .unwrap()
            .retain(|f| f["family"] != "face_grooves");
        assert_eq!(
            rules::questions(&a, &Map::new()).unwrap(),
            rules::questions(&without, &Map::new()).unwrap()
        );
        let answers = rules::with_all_defaults(&a, &seed()).unwrap();
        assert_eq!(
            answers,
            rules::with_all_defaults(&without, &seed()).unwrap()
        );
        assert_eq!(
            rules::apply(&a, &answers).unwrap().to_dict(),
            rules::apply(&without, &answers).unwrap().to_dict()
        );
        let intent = rules::apply(&a, &answers).unwrap();
        let (bytes, report) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
        assert!(!report.written.is_empty());
        let back = part::File::of_bytes(Path::new(name), bytes)
            .unwrap()
            .load(None)
            .unwrap();
        assert_eq!(resume::answers_for(&back).unwrap(), Some(answers));
        let diff = revision::diff(&loaded, &loaded).unwrap();
        assert!(diff.features.is_empty(), "{name}: {:?}", diff.features);
    }
}

#[test]
fn suppressed_od_boss_answers_are_visible_in_stored_and_shared_recovery() {
    let loaded = part::load(&fixture("turned_body_disc.step"), None).unwrap();
    // #123 suppresses this actual full-length Ø22 boss, but leaves its physical face
    // and the bore. A different family co-owning the face is never a migration target.
    let old_id = "bosses:1";
    assert!(matches!(
        loaded.part.faces[1].surface,
        haecceity::geom::Surface::Cylinder { radius: 11.0, .. }
    ));
    assert_eq!(loaded.part.frame_points_outward(1), Some(true));
    let key = format!("diameter.fit:{old_id}");
    let answers = json!({key.clone(): "g6", "part.material": "steel"})
        .as_object()
        .unwrap()
        .clone();
    // #123's whole-body context preserves the old OD datum defaults without
    // retaining the boss as a feature or recovering any of its saved answers.
    let a = api::analyse(&loaded).unwrap();
    let mut legacy = a.clone();
    legacy["features"] =
        serde_json::from_slice(&std::fs::read(fixture("turned_body_disc.v022.json")).unwrap())
            .unwrap();
    let defaults = rules::with_all_defaults(&a, &seed()).unwrap();
    assert_eq!(
        defaults,
        rules::with_all_defaults(&legacy, &seed()).unwrap()
    );
    assert_eq!(defaults["datum.A"], json!([1]));
    assert_eq!(
        rules::apply(&a, &defaults).unwrap().to_dict(),
        rules::apply(&legacy, &defaults).unwrap().to_dict()
    );
    let fs = features::recognise(&loaded).unwrap();
    assert!(!fs.iter().any(|f| f.id == old_id));
    let evidence = resume::evidence(&loaded).unwrap();
    for e in [Some(&evidence), None] {
        let out =
            resume::rematch_answers(&loaded, &fs, &answers, e, Some(std::slice::from_ref(&key)))
                .unwrap();
        assert_eq!(
            out.answers,
            json!({"part.material":"steel"})
                .as_object()
                .unwrap()
                .clone()
        );
        assert!(out.migrated.is_empty());
        assert_eq!(out.unresolved.len(), 1);
        assert_eq!(out.unresolved[0].id, key);
        assert_eq!(out.unresolved[0].answer, "g6");
        assert!(!out.unresolved[0].reason.is_empty());
        assert!(out.unresolved[0].automatic);
        assert!(out.automatic.unwrap().is_empty());
    }
    let a = api::analyse(&loaded).unwrap();
    let intent = rules::apply_with_defaults(&a, &seed(), true).unwrap();
    let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let back = part::File::of_bytes(Path::new("saved.step"), bytes)
        .unwrap()
        .load(None)
        .unwrap();
    let a = api::analyse(&back).unwrap();
    assert_eq!(a["resume"]["unresolved"][0]["id"], key);
    assert_eq!(a["resume"]["unresolved"][0]["answer"], "g6");
    assert!(a["resume"]["answers"].get(&key).is_none());
    let review = api::review(&a, a["resume"]["answers"].as_object().unwrap()).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answer_unresolved")
    );
}

#[test]
fn profiles_and_inside_rounds_keep_material_side_and_plain_descriptions() {
    let loaded = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/nist/nist_ctc_05_asme1_ap242.stp.gz"),
        None,
    )
    .unwrap();
    let a = api::analyse(&loaded).unwrap();
    let profiles: Vec<_> = a["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["family"] == "cylindrical_profiles")
        .collect();
    // #127: four R203.2 skins, two R19.05 concave profiles and one R127 skin.
    assert_eq!(profiles.len(), 7);
    assert_eq!(
        profiles
            .iter()
            .filter(|f| f["record"]["radius"] == 203.2)
            .count(),
        4
    );
    let expected = [
        (13, 203.2, "convex"),
        (15, 203.2, "convex"),
        (18, 203.2, "convex"),
        (19, 203.2, "convex"),
        (24, 19.05, "concave"),
        (28, 19.05, "concave"),
        (71, 127., "convex"),
    ];
    for (face, radius, side) in expected {
        let f = profiles
            .iter()
            .find(|f| f["id"] == format!("cylindrical_profiles:{face}"))
            .unwrap();
        assert_eq!(
            f["description"],
            format!("R{radius} {side} profile surface")
        );
        assert_eq!(f["record"]["radius"], radius);
        assert_eq!(f["record"]["side"], side);
        assert!(f["record"]["sweep"].as_f64().unwrap() < std::f64::consts::PI);
        assert_eq!(
            loaded.part.frame_points_outward(face),
            Some(side == "convex")
        );
    }
    // #127 also publishes the four spherical inside corner caps as fillets.
    let caps: Vec<_> = a["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| {
            f["family"] == "fillets"
                && f["record"]["side"] == "concave"
                && f["record"]["radius"] == 6.35
        })
        .flat_map(|f| f["faces"].as_array().unwrap())
        .map(|i| i.as_u64().unwrap() as usize)
        .filter(|&i| {
            matches!(
                loaded.part.faces[i].surface,
                haecceity::geom::Surface::Sphere { .. }
            )
        })
        .collect();
    assert_eq!(caps.len(), 4);
    for &i in &caps {
        assert_eq!(loaded.part.frame_points_outward(i), Some(false));
    }
    // Imported semantic radius PMI is attached to source faces, independent of convexity
    // and recognition family. Check a cylindrical round and a sphere cap through STEP.
    let cylinder = a["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| {
            f["family"] == "fillets"
                && f["record"]["side"] == "concave"
                && f["record"]["radius"] == 6.35
        })
        .flat_map(|f| f["faces"].as_array().unwrap())
        .map(|i| i.as_u64().unwrap() as usize)
        .find(|&i| {
            matches!(
                loaded.part.faces[i].surface,
                haecceity::geom::Surface::Cylinder { .. }
            )
        })
        .unwrap();
    use haecceity::pmi;
    let mut p = pmi::PartPmi::default();
    for (i, kind) in [
        (cylinder, pmi::SizeKind::Radius),
        (caps[0], pmi::SizeKind::SphericalRadius),
    ] {
        let radius = match loaded.part.faces[i].surface {
            haecceity::geom::Surface::Cylinder { radius, .. }
            | haecceity::geom::Surface::Sphere { radius, .. } => radius,
            _ => unreachable!(),
        };
        let feature = pmi::FeatureId(p.features.len());
        p.features.push(pmi::Feature::Items(vec![pmi::Anchor::Face(
            pmi::FaceIndex(i),
        )]));
        p.dimensions.push(pmi::Dimension {
            kind: pmi::DimensionKind::Size {
                feature,
                kind,
                path: None,
                angle: None,
            },
            nominal: Some(pmi::Value::length(
                pmi::Decimal::parse(&radius.to_string()).unwrap(),
                pmi::LengthUnit::Millimetre,
            )),
            tolerance: pmi::DimTolerance::None,
            qualifier: None,
            modifiers: vec![],
            principle: None,
        });
    }
    let doc = haecceity::p21::Document::parse(loaded.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&loaded.bytes).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), p)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let imported = part::load_bytes("radius.step", doc.apply(&edit).unwrap().bytes).unwrap();
    let existing = specify_core::existing::read_existing(&imported).unwrap();
    for (i, kind) in [(cylinder, "Size_Radius"), (caps[0], "Size_SphericalRadius")] {
        assert!(
            existing
                .iter()
                .any(|p| p.r#type == kind && p.faces == [i] && p.value == Some(6.35))
        );
    }
    let Some(path) = common::case("corpus/nist/nist_ctc_05_asme1_rd#0").input() else {
        return;
    };
    let loaded = part::load(&path, None).unwrap();
    let a = api::analyse(&loaded).unwrap();
    for face in [11, 71, 72, 74, 75, 76, 172, 173, 175, 176, 179, 181] {
        let f = a["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| {
                f["family"] == "fillets" && f["faces"].as_array().unwrap().contains(&json!(face))
            })
            .unwrap();
        assert_eq!(f["record"]["side"], "concave");
        assert!(
            f["description"]
                .as_str()
                .unwrap()
                .contains("concave fillet")
        );
        assert_eq!(loaded.part.frame_points_outward(face), Some(false));
    }
}
