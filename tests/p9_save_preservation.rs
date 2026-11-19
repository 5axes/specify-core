//! Reopened answers must leave the file's requirements intact on save.

use std::path::{Path, PathBuf};

use haecceity::{p21::Document, pmi, step};
use serde_json::{Map, Value, json};
use specify_core::{api, existing, part::LoadedPart, process, rules};

fn plate() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step")
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    specify_core::part::load_bytes("plate.step", bytes).unwrap()
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn defaults() -> Map<String, Value> {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    rules::with_all_defaults(&analysis, &answers(json!({"part.material": "6082-T6"}))).unwrap()
}

fn write(loaded: &LoadedPart, given: &Map<String, Value>, saved: Map<String, Value>) -> LoadedPart {
    let intent = api::apply(&api::analyse(loaded).unwrap(), given, true).unwrap();
    load(
        api::write_bytes(loaded.bytes.to_vec(), &[(intent, Some(saved))])
            .unwrap()
            .0,
    )
}

fn save(loaded: &LoadedPart, analysis: &Value, given: Map<String, Value>) -> LoadedPart {
    let intent = api::preview(analysis, &given).unwrap();
    load(
        api::write_bytes(loaded.bytes.to_vec(), &[(intent, Some(given))])
            .unwrap()
            .0,
    )
}

fn add(loaded: &LoadedPart, pmi: pmi::PartPmi) -> LoadedPart {
    let doc = Document::parse(loaded.bytes.to_vec()).unwrap();
    let defs = step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), pmi)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    load(doc.apply(&edit).unwrap().bytes)
}

#[test]
fn edited_printed_general_tolerance_survives_two_saves() {
    let source = load(std::fs::read(plate()).unwrap());
    let setting = "part.general_tolerance";
    let stored = answers(json!({
        "part.material": "PLA (FDM)",
        "part.process": process::FDM,
        (setting): "±0.3 mm or ±0.3 % (FDM)"
    }));
    let edited = json!("±0.4 mm or ±0.3 % (FDM)");
    let authored = write(&source, &stored, stored.clone());
    let mut text = String::from_utf8(authored.bytes.to_vec()).unwrap();
    let start = text
        .find("DESCRIPTIVE_REPRESENTATION_ITEM('general tolerance',")
        .unwrap();
    let value = start + text[start..].find("0.3 mm").unwrap();
    text.replace_range(value..value + 3, "0.4");
    let mut part = load(text.into_bytes());
    let tolerance = |part: &LoadedPart| {
        existing::part_settings(part)
            .unwrap()
            .get("general_tolerance")
            .cloned()
    };
    let edited = Some(edited);
    assert_eq!(tolerance(&part), edited);
    assert!(existing::part_settings_provenance(&part).unwrap()["general_tolerance"].is_string());
    for i in 1..=2 {
        let analysis = api::analyse(&part).unwrap();
        part = save(
            &part,
            &analysis,
            answers(analysis["resume"]["answers"].clone()),
        );
        assert_eq!(tolerance(&part), edited, "save {i}: {}", analysis["resume"]);
    }
}

#[test]
fn competing_foreign_notes_do_not_hide_a_position_tolerance() {
    let source = load(std::fs::read(plate()).unwrap());
    let source = add(
        &source,
        pmi::PartPmi {
            attributes: vec![pmi::AttributeSet {
                name: "semantic text".into(),
                on: None,
                items: vec![
                    (
                        "general note".into(),
                        pmi::AttributeValue::Text("KEEP DRY".into()),
                    ),
                    ("note index".into(), pmi::AttributeValue::Integer(1)),
                ],
                description: None,
                general_property: None,
            }],
            ..Default::default()
        },
    );
    let position = "hole.position:holes:12";
    let mut given = defaults();
    given.remove("part.notes");
    given.insert(position.into(), json!("0.2"));
    let mut stored = given.clone();
    stored.insert(position.into(), json!("none"));
    stored.insert("part.notes".into(), json!(["KEEP DRY", "DEBURR"]));
    let mut part = write(&source, &given, stored);
    let positioned = |part: &LoadedPart| {
        existing::read_existing(part)
            .unwrap()
            .iter()
            .any(|item| item.r#type == "Position" && item.faces == [12] && item.value == Some(0.2))
    };
    assert!(positioned(&part));
    for i in 1..=2 {
        let analysis = api::analyse(&part).unwrap();
        let mut given = answers(analysis["resume"]["answers"].clone());
        given.remove("part.notes");
        part = save(&part, &analysis, given);
        assert!(positioned(&part), "save {i}: {}", analysis["resume"]);
        assert_eq!(
            existing::part_settings(&part).unwrap()["notes"],
            json!(["KEEP DRY"])
        );
        assert!(
            analysis["resume"]["answers"].get(position).is_none(),
            "{}",
            analysis["resume"]
        );
    }
}

#[test]
fn a_foreign_flatness_does_not_erase_the_marked_flatness() {
    let source = load(std::fs::read(plate()).unwrap());
    let mut given = defaults();
    given.insert("datum.A.flatness".into(), json!("0.05"));
    let marked = write(&source, &given, given.clone());
    given.insert("datum.A.flatness".into(), json!("0.08"));
    let other = write(&source, &given, given.clone());
    let defs = step::read_part_definitions(&other.bytes).unwrap();
    let read = pmi::read(&Document::parse(other.bytes.to_vec()).unwrap(), &defs).unwrap();
    let mut tolerance = read.parts[0]
        .tolerances
        .iter()
        .find(|t| t.kind == pmi::ToleranceKind::Flatness)
        .unwrap()
        .clone();
    tolerance.description = None;
    let mut part = add(
        &marked,
        pmi::PartPmi {
            features: read.parts[0].features.clone(),
            tolerances: vec![tolerance],
            ..Default::default()
        },
    );
    let flatness = |part: &LoadedPart| {
        let mut values: Vec<_> = existing::read_existing(part)
            .unwrap()
            .into_iter()
            .filter(|item| item.r#type == "Flatness" && item.faces == [2])
            .map(|item| (item.provenance.is_some(), item.value.unwrap()))
            .collect();
        values.sort_by_key(|(marked, _)| !marked);
        values
    };
    let both = [(true, 0.05), (false, 0.08)];
    assert_eq!(flatness(&part), both);
    for i in 1..=2 {
        let analysis = api::analyse(&part).unwrap();
        part = save(
            &part,
            &analysis,
            answers(analysis["resume"]["answers"].clone()),
        );
        assert_eq!(flatness(&part), both, "save {i}: {}", analysis["resume"]);
    }
}
