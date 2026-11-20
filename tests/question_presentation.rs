mod common;

use serde_json::{Map, json};
use specify_core::rules::{Characteristic, questions};

#[test]
fn hole_titles_and_characteristics_are_data_not_prompt_readings() {
    let analysis = common::case("parts/plate").artifact("analysis").unwrap();
    let qs = questions(&analysis, &Map::new()).unwrap();
    let mut q = qs
        .iter()
        .find(|q| q.id.starts_with("hole.function:") && q.diameter == Some(6.6))
        .unwrap()
        .clone();
    assert_eq!(q.title.as_deref(), Some("6× Ø6.6 holes"));
    q.prompt = "A translated question".into();
    let mut expected = q.to_dict();
    expected["source"] = q.source_json().unwrap();
    expected["title"] = json!("6× Ø6.6 holes");
    expected["label"] = json!("6× Ø6.6 holes");
    assert_eq!(q.to_presented_dict(), expected);
    for (id, characteristic, wire) in [
        ("hole.position:", Characteristic::Position, "position"),
        ("datum.A.flatness", Characteristic::Flatness, "flatness"),
        (
            "datum.B.control",
            Characteristic::Perpendicularity,
            "perpendicularity",
        ),
    ] {
        let mut q = qs.iter().find(|q| q.id.starts_with(id)).unwrap().clone();
        assert_eq!(q.characteristic, Some(characteristic));
        q.prompt = "Unrelated wording".into();
        assert_eq!(q.to_presented_dict()["characteristic"], json!(wire));
    }
}

#[test]
fn turned_titles_and_runout_characteristics_are_data_not_prompt_readings() {
    let analysis = common::case("corpus/cadgenbench/flanged_spool_132")
        .artifact("analysis")
        .unwrap();
    let qs = questions(&analysis, &Map::new()).unwrap();
    let fits: Vec<_> = qs
        .iter()
        .filter(|q| q.id.starts_with("diameter.fit:"))
        .collect();
    assert_eq!(
        fits.iter().map(|q| q.title.as_deref()).collect::<Vec<_>>(),
        vec![Some("Turned 2× Ø130"), Some("Turned Ø70")]
    );
    for q in fits {
        assert!(q.title.as_ref().unwrap().starts_with("Turned "));
        let mut q = q.clone();
        q.prompt.clear();
        assert_eq!(q.to_presented_dict()["title"], json!(q.title));
    }
    let runouts: Vec<_> = qs
        .iter()
        .filter(|q| q.id.starts_with("diameter.runout:"))
        .collect();
    assert!(!runouts.is_empty());
    for q in runouts {
        assert_eq!(q.characteristic, Some(Characteristic::Runout));
        assert_eq!(q.to_presented_dict()["characteristic"], json!("runout"));
        assert!(q.title.is_some());
    }
}

/// #217: every question has a short label for rows and messages, without the prompt's hint.
#[test]
fn every_question_has_a_short_label() {
    let analysis = common::case("parts/plate").artifact("analysis").unwrap();
    let qs = questions(&analysis, &Map::new()).unwrap();
    let label = |id: &str| qs.iter().find(|q| q.id == id).unwrap().label();
    assert_eq!(label("part.notes"), "Other notes");
    assert_eq!(label("part.coating"), "Coating");
    assert_eq!(label("part.heat_treatment"), "Heat treatment");
    assert_eq!(label("datum.C"), "Datum C");
    assert_eq!(label("datum.A.flatness"), "Datum A flatness");
    assert_eq!(label("datum.B.control"), "Datum B perpendicularity");
    for q in &qs {
        let label = q.label();
        assert!(!label.is_empty(), "{}", q.id);
        assert!(
            !label.contains('?') && !label.contains("(empty"),
            "{}: {label}",
            q.id
        );
        assert_eq!(q.to_presented_dict()["label"], json!(label));
    }
    let position = qs
        .iter()
        .find(|q| q.id.starts_with("hole.position:"))
        .unwrap();
    assert_eq!(
        position.label(),
        format!("{} position", position.title.as_ref().unwrap())
    );
}
