//! Add-mode answer ownership and lossless repeated edits on real foreign PMI.
mod common;
use haecceity::p21::Document;
use serde_json::{Map, Value, json};
use specify_core::{api, characteristics, existing, part, resume, writer};

fn assert_entities_identical(before: &[u8], after: &[u8]) {
    let before = Document::parse(before.to_vec()).unwrap();
    let after = Document::parse(after.to_vec()).unwrap();
    // Compare the parser's complete original entity spans, including comments/whitespace
    // within a record, at the same entity ids. This covers ALL original entities, beyond
    // just semantic roots or counts. FILE_SCHEMA can legitimately be upgraded separately.
    for id in before.ids() {
        assert_eq!(
            before.span(id).map(|s| &before.bytes()[s]),
            after.span(id).map(|s| &after.bytes()[s]),
            "foreign entity #{id} changed"
        );
    }
}

fn round_trip(loaded: part::LoadedPart) {
    eprintln!("opening {}", loaded.path.display());
    let original = loaded.bytes.to_vec();
    let initial = api::analyse(&loaded).unwrap();
    let foreign = initial["existing"].clone();
    assert!(!foreign.as_array().unwrap().is_empty());
    let face = initial["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "plane")
        .unwrap()["id"]
        .clone();
    let given = json!({"extra.1":{"kind":"flatness","faces":[face],"value":"0.031"}})
        .as_object()
        .unwrap()
        .clone();
    let intent = api::preview(&initial, &given).unwrap();
    eprintln!("first write");
    let bytes = writer::write_bytes(&loaded, &intent, Some(&given))
        .unwrap()
        .0;
    assert_entities_identical(&original, &bytes);
    let part_index = loaded.binding.part;
    let reopened = part::File::of_bytes(std::path::Path::new("added.step"), bytes.clone())
        .unwrap()
        .load(Some(part_index))
        .unwrap();
    assert_eq!(resume::answers_for(&reopened).unwrap(), Some(given.clone()));
    eprintln!("analyse resumed");
    let analysis = api::analyse(&reopened).unwrap();
    assert_eq!(analysis["resume"]["answers"], json!(given));
    assert_eq!(analysis["existing"], foreign);
    assert_eq!(analysis["part"], initial["part"]);
    let own = existing::read_existing(&reopened)
        .unwrap()
        .into_iter()
        .filter(|p| p.provenance.as_deref() == Some(writer::PMI_PROVENANCE))
        .collect::<Vec<_>>();
    assert_eq!(own.len(), 1);
    assert!((own[0].value.unwrap() - 0.031).abs() < 1e-12);
    let number = own[0].number.expect("Specify characteristic numbered");
    let initial_numbers = characteristics::read(&loaded).unwrap();
    let numbers = characteristics::read(&reopened).unwrap();
    assert_eq!(numbers.len(), initial_numbers.len() + 1);
    assert!(numbers.iter().any(|c| c.number == number));
    // Foreign requirements still answer exactly the same questions after resume.
    let ids = |a: &Value| {
        api::questions(a, &Map::new())
            .unwrap()
            .into_iter()
            .map(|q| q.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&initial), ids(&analysis));
    let mut edited = given.clone();
    edited["extra.1"]["value"] = json!("0.047");
    let intent = api::preview(&analysis, &edited).unwrap();
    eprintln!("edited write");
    let changed = writer::write_bytes(&reopened, &intent, Some(&edited))
        .unwrap()
        .0;
    assert_entities_identical(&original, &changed);
    let again = part::File::of_bytes(std::path::Path::new("changed.step"), changed)
        .unwrap()
        .load(Some(part_index))
        .unwrap();
    assert_eq!(resume::answers_for(&again).unwrap(), Some(edited));
    assert_eq!(api::analyse(&again).unwrap()["existing"], foreign);
    let changed = existing::read_existing(&again)
        .unwrap()
        .into_iter()
        .filter(|p| p.provenance.as_deref() == Some(writer::PMI_PROVENANCE))
        .collect::<Vec<_>>();
    assert_eq!(
        changed.len(),
        1,
        "edited requirement must replace its predecessor"
    );
    assert_eq!(changed[0].number, Some(number));
    assert!((changed[0].value.unwrap() - 0.047).abs() < 1e-12);
    assert_eq!(characteristics::read(&again).unwrap().len(), numbers.len());
}

fn nist(name: &str) {
    let case = common::case(name);
    let path = case.input().expect("required NIST fixture/corpus");
    round_trip(part::load(&path, Some(0)).unwrap());
}

#[test]
fn ctc01_foreign_pmi_answers_resume_and_edits_preserve_entity_text() {
    nist("nist/nist_ctc_01_asme1_ap242");
}

#[test]
fn ctc02_foreign_pmi_answers_resume_and_edits_preserve_entity_text() {
    nist("nist/nist_ctc_02_asme1_ap242#0");
}

#[test]
fn ftc09_foreign_pmi_answers_resume_and_edits_preserve_entity_text() {
    nist("nist/nist_ftc_09_asme1_ap242-e1");
}

#[test]
fn ctc01_defaulted_add_replaces_all_own_items_and_keeps_numbers() {
    let case = common::case("nist/nist_ctc_01_asme1_ap242");
    let loaded = part::load(&case.input().unwrap(), None).unwrap();
    let original = loaded.bytes.to_vec();
    let initial = api::analyse(&loaded).unwrap();
    let mut answers = json!({"part.material":"brass", "part.notes":["First instruction"]})
        .as_object()
        .unwrap()
        .clone();
    let intent = api::apply(&initial, &answers, true).unwrap();
    let bytes = writer::write_bytes(&loaded, &intent, Some(&answers))
        .unwrap()
        .0;
    let reopened = part::load_bytes("defaults.step", bytes).unwrap();
    let numbers = characteristics::read(&reopened).unwrap();
    assert!(!numbers.is_empty());
    let analysis = api::analyse(&reopened).unwrap();
    assert_eq!(analysis["existing"], initial["existing"]);
    assert_eq!(analysis["part"], initial["part"]);
    answers.insert("part.material".into(), json!("steel"));
    answers.insert("part.notes".into(), json!(["Revised instruction"]));
    let intent = api::apply(&analysis, &answers, true).unwrap();
    let bytes = writer::write_bytes(&reopened, &intent, Some(&answers))
        .unwrap()
        .0;
    assert_entities_identical(&original, &bytes);
    let reopened = part::load_bytes("edited.step", bytes).unwrap();
    assert_eq!(resume::answers_for(&reopened).unwrap(), Some(answers));
    let mut after = characteristics::read(&reopened).unwrap();
    for c in &mut after {
        c.entity = 0;
    }
    let mut numbers = numbers;
    for c in &mut numbers {
        c.entity = 0;
    }
    assert_eq!(after, numbers);
    let model = existing::part_settings(&reopened).unwrap();
    assert_eq!(model["material"], "steel");
    assert_eq!(model["notes"], json!(["Revised instruction"]));
    assert_eq!(
        api::analyse(&reopened).unwrap()["existing"],
        initial["existing"]
    );
}

#[test]
fn old_add_mode_without_saved_answers_still_opens_as_existing_pmi() {
    let case = common::case("nist/nist_ctc_01_asme1_ap242");
    let loaded = part::load(&case.input().unwrap(), None).unwrap();
    let mut intent = specify_core::rules::Intent::new(json!(loaded.binding));
    intent
        .part
        .insert("coating".into(), json!("Existing coating"));
    let bytes = writer::write_bytes(&loaded, &intent, None).unwrap().0;
    let reopened = part::load_bytes("legacy-add.step", bytes).unwrap();
    assert!(resume::answers_for(&reopened).unwrap().is_none());
    let analysis = api::analyse(&reopened).unwrap();
    assert!(analysis.get("resume").is_none());
    assert_eq!(
        analysis["existing"],
        serde_json::to_value(existing::read_existing(&reopened).unwrap()).unwrap()
    );
    assert_eq!(
        analysis["part"],
        json!(existing::part_settings(&reopened).unwrap())
    );
}

#[test]
fn coincident_numbered_foreign_control_cannot_lend_its_number_to_specify() {
    let loaded = part::load(&common::fixtures().join("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let face = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "plane")
        .unwrap()["id"]
        .clone();
    let answers = json!({"extra.1":{"kind":"flatness","faces":[face],"value":"0.031"}})
        .as_object()
        .unwrap()
        .clone();
    let intent = api::preview(&analysis, &answers).unwrap();
    let bytes = writer::write_bytes(&loaded, &intent, None).unwrap().0;
    // Simulate an independent producer with a numbered control and its own history.
    // Keep all semantic values and numbers while removing Specify authorship.
    let bytes = String::from_utf8(bytes)
        .unwrap()
        .replace(writer::PMI_PROVENANCE, "independent producer")
        .into_bytes();
    round_trip(part::load_bytes("foreign-numbered.step", bytes).unwrap());
}

#[test]
fn stale_unmarked_legacy_answers_cannot_mask_scoped_add_saves() {
    let case = common::case("nist/nist_ctc_01_asme1_ap242");
    let loaded = part::load(&case.input().unwrap(), None).unwrap();
    let old_answers = json!({"part.material":"obsolete"})
        .as_object()
        .unwrap()
        .clone();
    let mut payload = resume::payload(&loaded, &old_answers).unwrap();
    payload["schema"] = json!(2);
    payload["face_count"] = json!(loaded.binding.face_count);
    payload.as_object_mut().unwrap().remove("face_names");
    payload["faces"] = json!("0".repeat(64));
    let doc = Document::parse(loaded.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&loaded.bytes).unwrap();
    // This unmarked UDA belongs to the input, so neither its payload nor its helpers may
    // be rewritten or removed. The original foreign NIST PMI keeps this in Add mode.
    let pmi = haecceity::pmi::PartPmi {
        attributes: vec![resume::attribute_set(&payload)],
        ..Default::default()
    };
    let (edit, _) = haecceity::pmi::write(
        &doc,
        &defs,
        &[(haecceity::pmi::PartId(0), pmi)],
        haecceity::pmi::write::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let loaded = part::load_bytes("stale-legacy.step", doc.apply(&edit).unwrap().bytes).unwrap();
    assert!(resume::answers_for(&loaded).unwrap().is_none());
    round_trip(loaded);
}

#[test]
fn datum_on_foreign_carrier_is_refused_before_specializing_it() {
    let bare = part::load(&common::fixtures().join("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&bare).unwrap();
    let face = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "plane")
        .unwrap()["id"]
        .as_u64()
        .unwrap() as usize;
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&bare.binding).unwrap());
    intent.requirements.push(
        json!({"kind":"flatness", "faces":[face], "tolerance":0.031})
            .as_object()
            .unwrap()
            .clone(),
    );
    let bytes = writer::write_bytes(&bare, &intent, None).unwrap().0;
    let bytes = String::from_utf8(bytes)
        .unwrap()
        .replace(writer::PMI_PROVENANCE, "independent producer")
        .into_bytes();
    let foreign = part::load_bytes("foreign-carrier.step", bytes.clone()).unwrap();
    let doc = Document::parse(bytes.clone()).unwrap();
    let defs = haecceity::step::read_part_definitions(&bytes).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let mut pmi = haecceity::pmi::PartPmi::default();
    pmi.features.push(read.parts[0].features[0].clone());
    pmi.datums.push(
        haecceity::pmi::Datum::new(
            haecceity::pmi::DatumLabel::new("A").unwrap(),
            Some(haecceity::pmi::FeatureId(0)),
            Vec::new(),
        )
        .unwrap(),
    );
    let (_, report) = haecceity::pmi::write(
        &doc,
        &defs,
        &[(haecceity::pmi::PartId(0), pmi)],
        haecceity::pmi::write::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    assert!(
        !report.rewritten.is_empty(),
        "0.4.5 specializes the kept carrier"
    );
    let mut intent =
        specify_core::rules::Intent::new(serde_json::to_value(&foreign.binding).unwrap());
    intent.datums.push(
        json!({"letter":"A", "faces":[face]})
            .as_object()
            .unwrap()
            .clone(),
    );
    let err = writer::write_bytes(&foreign, &intent, None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot preserve entity bytes"), "{err}");
    for id in report.rewritten {
        assert!(err.contains(&id.to_string()), "{err}");
    }
    assert_eq!(foreign.bytes.as_slice(), bytes);
}

#[test]
fn removing_owned_control_keeps_its_carrier_when_foreign_control_uses_it() {
    use haecceity::pmi::{PartId, PartPmi};
    let bare = part::load(&common::fixtures().join("parts/plate.step"), None).unwrap();
    // Establish genuine Add-mode scope before authoring the shared carrier. Legacy
    // Replace-mode saves intentionally replace the complete part's consumed PMI.
    let mut seed = specify_core::rules::Intent::new(serde_json::to_value(&bare.binding).unwrap());
    seed.requirements.push(
        json!({"kind":"flatness", "faces":[1], "tolerance":0.021})
            .as_object()
            .unwrap()
            .clone(),
    );
    let seed = writer::write_bytes(&bare, &seed, None).unwrap().0;
    let seed = String::from_utf8(seed)
        .unwrap()
        .replace(writer::PMI_PROVENANCE, "foreign seed")
        .into_bytes();
    let bare = part::load_bytes("foreign-seed.step", seed).unwrap();
    let analysis = api::analyse(&bare).unwrap();
    let face = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "plane" && f["id"] != 1)
        .unwrap()["id"]
        .clone();
    let answers = json!({"extra.1":{"kind":"flatness","faces":[face],"value":"0.031"}})
        .as_object()
        .unwrap()
        .clone();
    let intent = api::preview(&analysis, &answers).unwrap();
    let bytes = writer::write_bytes(&bare, &intent, Some(&answers))
        .unwrap()
        .0;
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    assert!(
        specify_core::resume::answers_for(
            &part::load_bytes("scope.step", doc.bytes().to_vec()).unwrap()
        )
        .unwrap()
        .is_some()
    );
    let mut tolerance = read.parts[0]
        .tolerances
        .iter()
        .find(|t| t.description.as_deref() == Some(writer::PMI_PROVENANCE))
        .unwrap()
        .clone();
    tolerance.description = Some("independent producer".into());
    let pmi = PartPmi {
        features: read.parts[0].features.clone(),
        tolerances: vec![tolerance],
        ..Default::default()
    };
    let (edit, _) = haecceity::pmi::write(
        &doc,
        &defs,
        &[(PartId(0), pmi)],
        haecceity::pmi::write::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let mixed = doc.apply(&edit).unwrap().bytes;
    let doc = Document::parse(mixed.clone()).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let controls: Vec<_> = doc
        .ids()
        .filter_map(|id| match doc.get(id) {
            Some(haecceity::p21::RawEntity::Simple {
                name, attributes, ..
            }) if name == "FLATNESS_TOLERANCE"
                && attributes[1] != haecceity::p21::Attribute::String("foreign seed".into()) =>
            {
                Some((id, attributes[3].clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(controls.len(), 2);
    assert_eq!(
        controls[0].1, controls[1].1,
        "both controls share the owned face carrier"
    );
    let haecceity::p21::Attribute::EntityRef(shared) = controls[0].1 else {
        panic!("carrier")
    };
    assert!(
        matches!(doc.get(shared), Some(haecceity::p21::RawEntity::Simple { attributes, .. })
        if attributes[1] == haecceity::p21::Attribute::String(writer::PMI_PROVENANCE.into())),
        "the carrier was authored by Specify"
    );
    let foreign = read.parts[0]
        .tolerances
        .iter()
        .position(|t| t.description.as_deref() == Some("independent producer"))
        .unwrap();
    let protected = &read.provenance.parts[0].tolerances[foreign];
    let loaded = part::load_bytes("shared-carrier.step", mixed).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let empty = Map::new();
    let intent = api::preview(&analysis, &empty).unwrap();
    let bytes = writer::write_bytes(&loaded, &intent, Some(&empty))
        .unwrap()
        .0;
    let after = Document::parse(bytes.clone()).unwrap();
    for &id in protected {
        assert_eq!(
            &doc.bytes()[doc.span(id).unwrap()],
            &after.bytes()[after
                .span(id)
                .unwrap_or_else(|| panic!("protected #{id} missing: {:?}", doc.get(id)))],
            "protected #{id}"
        );
    }
    let haecceity::p21::Attribute::EntityRef(carrier) = controls[0].1 else {
        panic!("carrier")
    };
    assert_eq!(
        &doc.bytes()[doc.span(carrier).unwrap()],
        &after.bytes()[after.span(carrier).unwrap()]
    );
    let reopened = part::load_bytes("removed.step", bytes).unwrap();
    let remaining = existing::read_existing(&reopened).unwrap();
    assert_eq!(
        remaining
            .iter()
            .filter(|e| e.kind == "geometric_tolerance")
            .count(),
        2
    );
    assert!(
        remaining
            .iter()
            .all(|e| e.provenance.as_deref() != Some(writer::PMI_PROVENANCE))
    );
}

#[test]
fn ctc01_intent_leaves_the_files_stated_sizes_to_the_file() {
    let case = common::case("nist/nist_ctc_01_asme1_ap242");
    let loaded = part::load(&case.input().unwrap(), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = json!({"part.material":"steel"})
        .as_object()
        .unwrap()
        .clone();
    let intent = api::apply(&analysis, &answers, true).unwrap();
    // The file states these diameters with their own tolerances; they are planner
    // authorities, never extra Specify requirements beside the file's dimensions.
    assert!(
        !intent
            .requirements
            .iter()
            .any(|r| r.get("stored") == Some(&json!(true))),
        "stored dimensions leaked into the intent"
    );
    let stated: Vec<Vec<i64>> = analysis["existing"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["type"] == "Size_Diameter" && d["value"].is_number())
        .map(|d| serde_json::from_value(d["faces"].clone()).unwrap())
        .collect();
    assert_eq!(stated.len(), 7);
    for r in &intent.requirements {
        if r.get("kind") != Some(&json!("size")) || r.contains_key("not_written_reason") {
            continue;
        }
        let faces: Vec<i64> = serde_json::from_value(r["faces"].clone()).unwrap();
        assert!(
            !stated.iter().any(|s| s.iter().all(|f| faces.contains(f))),
            "a size requirement repeats the file's diameter on {faces:?}"
        );
    }
    // Planning still sees them: the file's sizes keep General guesses off these holes.
    assert!(intent.requirements.iter().any(|r| {
        r.get("not_written_reason")
            .and_then(Value::as_str)
            .is_some_and(|w| w.contains("conflicts with an answered requirement"))
    }));
}
