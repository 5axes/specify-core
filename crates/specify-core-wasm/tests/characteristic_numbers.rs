//! The browser uses exactly the native file numbers, including revision answer transport.
use serde_json::{Value, json};
use specify_core::part;
use specify_core_wasm::session::Session;
use std::path::Path;

fn fixture(name: &str) -> Vec<u8> {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}")),
        None,
    )
    .unwrap()
    .bytes
    .to_vec()
}
fn bytes(result: &Value) -> Vec<u8> {
    serde_json::from_value(result["bytes"].clone()).unwrap()
}

#[test]
fn snapshot_exposes_file_numbers_written_by_write_pmi() {
    let old = fixture("revisions/hole_removed.old.step.gz");
    let s = Session::open("old", &old).unwrap();
    assert!(s.snapshot()["analysis"].get("characteristics").is_none());
    let answers = json!({"hole.function:holes:6":"fit:H7", "hole.function:holes:7":"fit:H7", "hole.function:holes:8":"fit:H7", "hole.function:holes:9":"fit:H7"});
    let written = bytes(
        &s.write_pmi(answers.as_object().unwrap(), None, None)
            .unwrap(),
    );
    let reopened = Session::open("old", &written).unwrap();
    let numbers = reopened.snapshot()["analysis"]["characteristics"].clone();
    assert_eq!(numbers.as_array().unwrap().len(), 4);
    for row in numbers.as_array().unwrap() {
        assert_eq!(row["origin"], "requirement");
        assert_eq!(row["source"]["parameter"], "bore.diameter");
        assert_eq!(row["source"]["support"], row["faces"]);
        assert_eq!(row["source"]["schema"], 2);
    }
}
