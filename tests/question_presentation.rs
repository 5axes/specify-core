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
