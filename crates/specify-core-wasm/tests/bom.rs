//! The browser session shares the opt-in and provenance rules of the native writer.
use serde_json::{Map, json};
use specify_core::{bom, resume};
use specify_core_wasm::session::Session;
#[test]
fn write_pmi_bom_is_explicit_and_suggestions_do_not_become_answers() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/assembly.step");
    let session = Session::open_part("assembly", bytes, 0).unwrap();
    let ordinary = session.write_pmi(&Map::new(), None, None).unwrap();
    let ordinary: Vec<u8> = serde_json::from_value(ordinary["bytes"].clone()).unwrap();
    assert!(
        bom::read_bytes(ordinary)
            .unwrap()
            .iter()
            .all(|r| r.item_number.value.is_null())
    );
    assert!(
        session
            .accept_defaults(&Map::new())
            .unwrap()
            .keys()
            .all(|k| !k.starts_with("bom."))
    );
    let written = session
        .write_pmi_with_bom(&Map::new(), None, None, true)
        .unwrap();
    let written: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    let rows = bom::read_bytes(written.clone()).unwrap();
    assert_eq!(rows[0].item_number.value, 1);
    assert_eq!(rows[0].description.source, "default");
    assert!(
        resume::stored(&written)
            .values()
            .all(|p| p["answers"].as_object().unwrap().is_empty())
    );
    let session = Session::open_part("written", &written, 0).unwrap();
    let explicit = json!({"bom.stock":"from stock","bom.description":"Chosen description"});
    let edited = session
        .write_pmi(explicit.as_object().unwrap(), None, None)
        .unwrap();
    let edited: Vec<u8> = serde_json::from_value(edited["bytes"].clone()).unwrap();
    let snapshot = Session::open_part("edited", &edited, 0).unwrap().snapshot();
    assert_eq!(snapshot["analysis"]["resume"]["answers"], explicit);
    let reopened = Session::open_part("edited", &edited, 0).unwrap();
    let requested = reopened
        .write_pmi_with_bom(&Map::new(), None, None, true)
        .unwrap();
    let requested: Vec<u8> = serde_json::from_value(requested["bytes"].clone()).unwrap();
    assert_eq!(
        bom::read_bytes(requested).unwrap()[0].stock.source,
        "answered"
    );
    assert_eq!(bom::read_bytes(edited).unwrap()[0].stock.source, "answered");
}
