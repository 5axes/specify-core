//! Dimensions under the general tolerance, found and listed (written since #63): specify-core's
//! `tests/test_general.py`, on the oracle's captured analyses of the pin and the plate.
//!
//! `test_listed_but_not_written`'s write is the writer's (stage 3); its rules side (listed in
//! the intent, not among the requirements, kept by an intent's JSON round trip) is here.

use std::collections::HashSet;

use serde_json::{Map, Value, json};
use specify_core::general::deviation;
use specify_core::rules::{Intent, apply_with_defaults};

fn analysis(case: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{case}.step"));
    specify_core::api::analyse(&specify_core::part::load(&path, None).unwrap()).unwrap()
}

/// `_general`: the part applied with a material and every default.
fn general(case: &str) -> (Value, Intent) {
    let analysis = analysis(case);
    let material = json!({"part.material": "Aluminium 6082-T6"});
    let intent = apply_with_defaults(&analysis, material.as_object().unwrap(), true).unwrap();
    (analysis, intent)
}

fn ids(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_i64().unwrap())
        .collect()
}

// test_general.py::test_iso_2768_linear (every parameter)
#[test]
fn iso_2768_linear() {
    for (class, nominal, expected) in [
        ("ISO 2768-m", 4.0, Some(0.1)),
        ("ISO 2768-m", 24.0, Some(0.2)),
        ("ISO 2768-m", 52.0, Some(0.3)),
        ("ISO 2768-f", 30.0, Some(0.1)),
        ("ISO 2768-c", 120.0, Some(0.8)),
        ("ISO 2768-v", 2.0, None),
        ("ISO 2768-m", 0.3, None),
        ("ISO 2768-f", 3000.0, None),
    ] {
        assert_eq!(deviation(class, nominal), expected, "{class} {nominal}");
    }
}

// test_general.py::test_pin_lengths_are_baseline_from_datum_a
#[test]
fn pin_lengths_are_baseline_from_the_shoulder_datum_b() {
    let (analysis, intent) = general("parts/pin");
    let faces = analysis["faces"].as_array().unwrap();
    let a = intent.datums.iter().find(|d| d["letter"] == "A").unwrap();
    // Axial lengths use the same locating plane, now the secondary shoulder B.
    let b = intent.datums.iter().find(|d| d["letter"] == "B").unwrap();
    assert_eq!(faces[ids(&a["faces"])[0] as usize]["kind"], "cylinder");
    let lengths: Vec<&Map<String, Value>> = intent
        .general
        .iter()
        .filter(|g| g["kind"] == "location" && g["feature"] == "")
        .collect();
    assert!(!lengths.is_empty(), "{:?}", intent.general);
    assert!(lengths.iter().all(|g| g["reference"] == b["faces"]));
    // The tap drill's floor is the hole's, not a length of the part.
    let mut distances: Vec<f64> = lengths
        .iter()
        .map(|g| g["distance"].as_f64().unwrap())
        .collect();
    distances.sort_by(f64::total_cmp);
    assert_eq!(distances, [20.0, 32.0]);
    for g in lengths {
        let upper = g["upper"].as_f64().unwrap();
        assert_eq!(g["general"], json!(true));
        assert_eq!(upper, -g["lower"].as_f64().unwrap());
        assert_eq!(
            Some(upper),
            deviation("ISO 2768-m", g["distance"].as_f64().unwrap())
        );
        for i in ids(&g["faces"]) {
            let face = faces.iter().find(|f| f["id"] == i).unwrap();
            assert_eq!(face["kind"], "plane");
        }
    }
}

// test_general.py::test_a_diameter_sized_otherwise_is_not_listed_again
#[test]
fn a_diameter_sized_otherwise_is_not_listed_again() {
    let (_, intent) = general("parts/plate");
    let faces_of = |items: &[Map<String, Value>], kind: &str| -> HashSet<i64> {
        items
            .iter()
            .filter(|r| r["kind"] == kind)
            .flat_map(|r| ids(&r["faces"]))
            .collect()
    };
    let sized = faces_of(&intent.requirements, "size");
    let listed = faces_of(&intent.general, "size");
    assert!(!sized.is_empty() && sized.is_disjoint(&listed));
    // Holes positioned from datums have basic locations; none are added.
    let positioned: HashSet<&Value> = intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "location")
        .map(|r| &r["feature"])
        .collect();
    let added: HashSet<&Value> = intent
        .general
        .iter()
        .filter(|g| g["kind"] == "location" && g["feature"] != "")
        .map(|g| &g["feature"])
        .collect();
    assert!(!positioned.is_empty() && positioned.is_disjoint(&added));
    let mut outline: Vec<f64> = intent
        .general
        .iter()
        .filter(|g| g["kind"] == "location" && g["feature"] == "")
        .map(|g| g["distance"].as_f64().unwrap())
        .collect();
    outline.sort_by(f64::total_cmp);
    assert_eq!(outline, [10.0, 60.0, 100.0]);
}

// test_general.py::test_listed_but_not_written (the rules side)
#[test]
fn listed_but_not_written() {
    let (_, intent) = general("parts/pin");
    assert!(!intent.general.is_empty());
    assert!(
        !intent
            .requirements
            .iter()
            .any(|r| r.get("general").is_some_and(|g| g == &json!(true)))
    );
    let again = Intent::from_dict(&intent.to_dict()).unwrap();
    assert_eq!(again.general, intent.general);
}
