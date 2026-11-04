//! A sealed interior void is stated in review as an `info` fact, with its faces and size, and
//! asks no question. `tests/fixtures/voids/sealed_cavities.step.gz` (made by
//! `sealed_cavities.py`) is a block with four closed ⌀2.5 × 12 cylindrical cavities
//! (BREP_WITH_VOIDS). quiddity 0.4.7 proves each one sealed from its closed shell
//! (`volume_method: "closed_shell"`), whatever the air-sampling grid resolves, and reports it
//! only as `interior_voids`, never also as a hole.

use std::path::Path;

use serde_json::{Map, Value, json};
use specify_core::api;

fn sealed_cavities() -> Value {
    let step =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/voids/sealed_cavities.step.gz");
    api::analyse_file(&step, None).unwrap()
}

const CAVITY_FACES: [[i64; 3]; 4] = [[6, 7, 8], [9, 10, 11], [12, 13, 14], [15, 16, 17]];

#[test]
fn the_kernel_reports_each_cavity_as_a_closed_shell_void_and_not_as_a_hole() {
    let analysis = sealed_cavities();
    let features = analysis["features"].as_array().unwrap();
    let voids: Vec<&Value> = features
        .iter()
        .filter(|f| f["family"] == "interior_voids")
        .collect();
    let faces: Vec<Value> = voids.iter().map(|f| f["faces"].clone()).collect();
    assert_eq!(faces, CAVITY_FACES.map(|f| json!(f)), "{voids:#?}");
    // pi x 1.25² x 12, from the shell's boundary integral, not grid samples.
    let exact = std::f64::consts::PI * 1.25 * 1.25 * 12.0;
    for void in &voids {
        let record = &void["record"];
        assert_eq!(record["volume_method"], "closed_shell", "{void:#}");
        assert_eq!(record["openings"], json!([]), "{void:#}");
        assert_eq!(record["void_faces"], void["faces"], "{void:#}");
        let volume = record["estimated_volume"].as_f64().unwrap();
        assert!((volume - exact).abs() < 1e-9, "{volume}");
    }
    // No cavity face is claimed by any other feature: no holes, hole patterns or profiles.
    let cavity: Vec<i64> = CAVITY_FACES.concat();
    for f in features.iter().filter(|f| f["family"] != "interior_voids") {
        let claimed: Vec<i64> = f["faces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_i64)
            .collect();
        assert!(
            !claimed.iter().any(|id| cavity.contains(id)),
            "{} claims a cavity face",
            f["id"]
        );
    }
    assert!(
        !features
            .iter()
            .any(|f| f["family"] == "holes" || f["family"] == "hole_patterns"),
        "{features:#?}"
    );
}

#[test]
fn an_unsealed_grid_chamber_is_not_stated_as_a_sealed_void() {
    // An interior_voids chamber the air-sampling grid found behind openings, as quiddity
    // still reports (e.g. corpus 11512), has no closed shell: it is not "closed".
    let mut analysis = sealed_cavities();
    for f in analysis["features"].as_array_mut().unwrap() {
        if f["id"] == "interior_voids:6.7.8" {
            f["record"]["volume_method"] = json!("six_axis_grid");
            f["record"]["openings"] = json!([[6]]);
        }
    }
    let review = api::review(&analysis, &Map::new()).unwrap();
    let faces: Vec<Value> = void_facts(&review["problems"])
        .iter()
        .map(|p| p["faces"].clone())
        .collect();
    assert_eq!(
        faces,
        CAVITY_FACES[1..]
            .iter()
            .map(|f| json!(f))
            .collect::<Vec<_>>()
    );
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
    assert_eq!(faces, CAVITY_FACES.map(|f| json!(f)), "{facts:#?}");
    let features = analysis["features"].as_array().unwrap();
    for fact in &facts {
        assert_eq!(fact["severity"], "info");
        assert_eq!(fact["diameter"], 2.5);
        assert_eq!(fact["length"], 12.0);
        // The kernel's closed-shell volume, carried as given.
        let void = features
            .iter()
            .find(|f| f["id"] == fact["feature"])
            .unwrap();
        assert_eq!(fact["estimated_volume"], void["record"]["estimated_volume"]);
        let ids: Vec<String> = fact["faces"]
            .as_array()
            .unwrap()
            .iter()
            .map(Value::to_string)
            .collect();
        assert_eq!(fact["feature"], format!("interior_voids:{}", ids.join(".")));
        assert_eq!(
            fact["message"],
            "Closed internal cavity ⌀2.5 × 12 long: no opening to the outside; can't be \
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
