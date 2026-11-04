//! A sealed interior void is stated in review as an `info` fact, with its faces and size, and
//! asks no question. `tests/fixtures/voids/sealed_cavities.step.gz` (made by
//! `sealed_cavities.py`) is a block with four closed ⌀4 × 12 cylindrical cavities
//! (BREP_WITH_VOIDS).
//!
//! The pinned kernel may also report the cavities as holes; nothing here depends on that.

use std::path::Path;

use serde_json::{Map, Value, json};
use specify_core::api;

fn sealed_cavities() -> Value {
    let step =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/voids/sealed_cavities.step.gz");
    api::analyse_file(&step, None).unwrap()
}

fn void_facts(problems: &Value) -> Vec<Value> {
    let mut facts: Vec<Value> = problems
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["code"] == "interior_void")
        .cloned()
        .collect();
    facts.sort_by_key(|p| p["faces"][0].as_i64());
    facts
}

#[test]
fn each_sealed_void_is_an_info_fact_with_its_faces_and_size() {
    let analysis = sealed_cavities();
    let review = api::review(&analysis, &Map::new()).unwrap();
    let facts = void_facts(&review["problems"]);
    let faces: Vec<Value> = facts.iter().map(|p| p["faces"].clone()).collect();
    assert_eq!(
        faces,
        [
            json!([6, 7, 8]),
            json!([9, 10, 11]),
            json!([12, 13, 14]),
            json!([15, 16, 17])
        ],
        "{facts:#?}"
    );
    for fact in &facts {
        assert_eq!(fact["severity"], "info");
        assert_eq!(fact["diameter"], 4.0);
        assert_eq!(fact["length"], 12.0);
        let ids: Vec<String> = fact["faces"]
            .as_array()
            .unwrap()
            .iter()
            .map(Value::to_string)
            .collect();
        assert_eq!(fact["feature"], format!("interior_voids:{}", ids.join(".")));
        assert_eq!(
            fact["message"],
            "Closed internal cavity ⌀4 × 12 long: no opening to the outside; can't be \
             drilled, tapped or moulded as modelled."
        );
    }
    // A fact, not a question: nothing is asked about a void.
    for q in review["questions"].as_array().unwrap() {
        assert!(
            !q["id"].as_str().unwrap().contains("interior_voids"),
            "{}",
            q["id"]
        );
    }
}

#[test]
fn serve_returns_the_same_void_facts() {
    let analysis = sealed_cavities();
    let review = api::review(&analysis, &Map::new()).unwrap();
    let request = json!({"op": "questions", "analysis": analysis, "answers": {}}).to_string();
    let mut out = Vec::new();
    specify_core::serve::serve(format!("{request}\n").as_bytes(), &mut out).unwrap();
    let reply: Value = serde_json::from_slice(&out).unwrap();
    let served = &reply["result"]["problems"];
    assert_eq!(void_facts(served), void_facts(&review["problems"]));
    assert_eq!(void_facts(served).len(), 4);
}
