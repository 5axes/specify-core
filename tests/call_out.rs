//! Calling out a dimension: a size or a distance a person tolerances themselves, in place of
//! what the general tolerance would have said of it. The rules side of specify-core's
//! `tests/test_call_out.py`, on the oracle's captured analysis of the plate.
//!
//! Each test's write and read-back is the writer's (stage 3). `test_a_distance_leaves_the_
//! general_list_by_place_not_by_the_faces_picked` tests `general._between`, an internal, and
//! is ported beside it in `src/general.rs`.

mod common;

use serde_json::{Map, Value, json};
use specify_core::problems::problems;
use specify_core::rules::{Error, Intent, apply_with_defaults, questions};

fn plate() -> Value {
    common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis")
}

/// The answers with a material, applied with every default.
fn applied(analysis: &Value, extra: Value) -> Intent {
    let mut given = extra.as_object().cloned().unwrap_or_default();
    given.insert("part.material".into(), json!("steel"));
    apply_with_defaults(analysis, &given, true).unwrap()
}

/// `_general`: the general dimensions with no call-outs.
fn general(analysis: &Value) -> Vec<Map<String, Value>> {
    applied(analysis, json!({})).general
}

/// `_added`: the requirement a call-out made.
fn added(intent: &Intent, key: &str) -> Map<String, Value> {
    intent
        .requirements
        .iter()
        .find(|r| r.get("question") == Some(&json!(key)))
        .unwrap_or_else(|| panic!("no requirement from {key}"))
        .clone()
}

fn faces(analysis: &Value) -> &Vec<Value> {
    analysis["faces"].as_array().unwrap()
}

/// `_planes`: the top, bottom and a side face.
fn planes(analysis: &Value) -> (i64, i64, i64) {
    let planes: Vec<&Value> = faces(analysis)
        .iter()
        .filter(|f| f["kind"] == "plane")
        .collect();
    let num = |v: &Value| v.as_f64().unwrap();
    let z: Vec<&Value> = planes
        .iter()
        .copied()
        .filter(|f| (num(&f["direction"][2]).abs() - 1.0).abs() < 1e-6)
        .collect();
    let (mut top, mut bottom) = (z[0], z[0]);
    for f in &z {
        if num(&f["centroid"][2]) > num(&top["centroid"][2]) {
            top = f;
        }
        if num(&f["centroid"][2]) < num(&bottom["centroid"][2]) {
            bottom = f;
        }
    }
    let side = planes
        .iter()
        .find(|f| num(&f["direction"][2]).abs() < 1e-6)
        .unwrap();
    let id = |f: &Value| f["id"].as_i64().unwrap();
    (id(top), id(bottom), id(side))
}

fn first_cylinder(analysis: &Value) -> &Value {
    faces(analysis)
        .iter()
        .find(|f| f["kind"] == "cylinder")
        .unwrap()
}

fn refusal(analysis: &Value, answers: Value) -> String {
    match questions(analysis, answers.as_object().unwrap()) {
        Err(Error::Invalid(message)) => message,
        other => panic!("not refused: {other:?}"),
    }
}

// test_call_out.py::test_a_called_out_size_is_written_and_leaves_the_general_list
#[test]
fn a_called_out_size_is_written_and_leaves_the_general_list() {
    let plate = plate();
    let size = general(&plate)
        .into_iter()
        .find(|g| g["kind"] == "size")
        .unwrap();
    let extra = json!({"faces": size["faces"], "kind": "size", "value": "+0.1/-0"});
    let intent = applied(&plate, json!({"extra.1": extra}));
    let req = added(&intent, "extra.1");
    assert_eq!(req["kind"], "size");
    assert_eq!(req["nominal"], size["nominal"]);
    assert_eq!(req["upper"].as_f64(), Some(0.1));
    assert_eq!(req["lower"].as_f64(), Some(0.0));
    assert_eq!(req["fit"], "");
    assert!(
        !intent
            .general
            .iter()
            .any(|g| g["kind"] == "size" && g["faces"] == size["faces"])
    );
}

// test_call_out.py::test_a_called_out_size_may_be_plus_minus_or_a_fit
#[test]
fn a_called_out_size_may_be_plus_minus_or_a_fit() {
    let plate = plate();
    let size = general(&plate)
        .into_iter()
        .find(|g| g["kind"] == "size")
        .unwrap();
    for (value, fit, upper, lower) in [("±0.05", "", Some(0.05), -0.05), ("H7", "H7", None, 0.0)] {
        let extra = json!({"faces": size["faces"], "kind": "size", "value": value});
        let req = added(&applied(&plate, json!({"extra.1": extra})), "extra.1");
        assert_eq!(req["fit"], fit);
        assert_eq!(req["lower"].as_f64(), Some(lower));
        if let Some(upper) = upper {
            assert_eq!(req["upper"].as_f64(), Some(upper));
        }
    }
}

// test_call_out.py::test_a_called_out_length_is_written_and_leaves_the_general_list
#[test]
fn a_called_out_length_is_written_and_leaves_the_general_list() {
    let plate = plate();
    let length = general(&plate)
        .into_iter()
        .find(|g| g["kind"] == "location")
        .unwrap();
    let extra = json!({
        "faces": length["faces"],
        "reference": length["reference"],
        "kind": "distance",
        "value": "±0.05",
    });
    let intent = applied(&plate, json!({"extra.1": extra}));
    let req = added(&intent, "extra.1");
    assert_eq!(req["kind"], "location");
    let (d, want) = (
        req["distance"].as_f64().unwrap(),
        length["distance"].as_f64().unwrap(),
    );
    assert!((d - want).abs() <= 1e-6 * want.abs().max(1e-12));
    assert_eq!(req["upper"].as_f64(), Some(0.05));
    assert_eq!(req["lower"].as_f64(), Some(-0.05));
    let set = |v: &Value| {
        let mut ids: Vec<i64> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_i64().unwrap())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    assert!(
        !intent
            .general
            .iter()
            .any(|g| g["kind"] == "location" && set(&g["faces"]) == set(&length["faces"]))
    );
}

// test_call_out.py::test_a_distance_to_a_holes_axis_is_measured_from_a_plane_along_it
#[test]
fn a_distance_to_a_holes_axis_is_measured_from_a_plane_along_it() {
    let plate = plate();
    let bore = first_cylinder(&plate);
    let num = |v: &Value, k: usize| v[k].as_f64().unwrap();
    let side = faces(&plate)
        .iter()
        .find(|f| {
            f["kind"] == "plane"
                && (0..3)
                    .map(|k| num(&f["direction"], k) * num(&bore["direction"], k))
                    .sum::<f64>()
                    .abs()
                    < 1e-6
        })
        .unwrap();
    let extra = json!({"faces": [bore["id"]], "reference": [side["id"]], "kind": "distance",
                       "value": "±0.1"});
    let req = added(&applied(&plate, json!({"extra.1": extra})), "extra.1");
    let expected = (0..3)
        .map(|k| (num(&bore["origin"], k) - num(&side["centroid"], k)) * num(&side["direction"], k))
        .sum::<f64>()
        .abs();
    assert!((req["distance"].as_f64().unwrap() - expected).abs() <= 1e-5);
    assert_eq!(req["reference"], json!([side["id"]]));
}

// test_call_out.py::test_a_call_out_that_does_not_suit_its_faces_is_refused (every parameter)
#[test]
fn a_call_out_that_does_not_suit_its_faces_is_refused() {
    let plate = plate();
    let (top, bottom, side) = planes(&plate);
    let bore = first_cylinder(&plate)["id"].as_i64().unwrap();
    let (top, bottom, side, bore) = (json!([top]), json!([bottom]), json!([side]), json!([bore]));
    for (extra, reason) in [
        (
            json!({"faces": top, "kind": "size", "value": "±0.1"}),
            "one diameter",
        ),
        (
            json!({"faces": bore, "kind": "size", "value": "0.1 or so"}),
            "give ±0.05",
        ),
        (
            json!({"faces": bore, "kind": "size", "value": "+0/-0"}),
            "must differ",
        ),
        (
            json!({"faces": top, "kind": "distance", "value": "±0.1"}),
            "from flat faces",
        ),
        (
            json!({"faces": top, "reference": top, "kind": "distance", "value": "±0.1"}),
            "itself",
        ),
        (
            json!({"faces": side, "reference": bottom, "kind": "distance", "value": "±0.1"}),
            "parallel",
        ),
        (
            json!({"faces": top, "reference": bottom, "kind": "distance", "value": "H7"}),
            "give ±0.05",
        ),
        (
            json!({"faces": bore, "reference": top, "kind": "distance", "value": "±0.1"}),
            "along",
        ),
        // Slash bounds are now accepted and tested in limits_and_classes.rs.
        // Dash ranges remain refused: they are ambiguous with signed deviations.
        (
            json!({"faces": top, "reference": bottom, "kind": "distance", "value": "9.9-10.1"}),
            "deviations",
        ),
        (
            json!({"faces": top, "reference": bottom, "kind": "distance", "value": "0.1-0.05"}),
            "deviations",
        ),
        (
            json!({"faces": top, "reference": bottom, "kind": "distance", "value": "+3/-3"}),
            "too big",
        ),
        (
            json!({"faces": top, "reference": "12", "kind": "distance", "value": "±0.1"}),
            "from flat faces",
        ),
    ] {
        let message = refusal(&plate, json!({ "extra.1": extra }));
        assert!(
            message.contains(reason),
            "{extra}: {message} lacks {reason}"
        );
    }
}

// test_call_out.py::test_a_distance_with_any_limits_is_written_and_verified (the intent; the
// write is stage 3's)
#[test]
fn a_distance_with_any_limits_is_written_and_verified() {
    let plate = plate();
    let (top, bottom, _) = planes(&plate);
    for (value, upper, lower) in [
        ("-0.05/-0.1", -0.05, -0.1),
        ("+0.1/+0.05", 0.1, 0.05),
        ("+0.2/-0.1", 0.2, -0.1),
        ("+0.1/0", 0.1, 0.0),
    ] {
        let extra = json!({"faces": [top], "reference": [bottom], "kind": "distance",
                           "value": value});
        let req = added(&applied(&plate, json!({"extra.1": extra})), "extra.1");
        assert_eq!(req["upper"].as_f64(), Some(upper), "{value}");
        assert_eq!(req["lower"].as_f64(), Some(lower), "{value}");
    }
}

// test_call_out.py::test_a_diameter_sized_twice_is_reported
#[test]
fn a_diameter_sized_twice_is_reported() {
    let plate = plate();
    let size = general(&plate)
        .into_iter()
        .find(|g| g["kind"] == "size")
        .unwrap();
    let given = json!({
        "extra.1": {"faces": size["faces"], "kind": "fit", "value": "H7"},
        "extra.2": {"faces": size["faces"], "kind": "size", "value": "±0.05"},
    });
    let found: Vec<Value> = problems(&plate, given.as_object().unwrap())
        .unwrap()
        .into_iter()
        .filter(|p| p["message"].as_str().unwrap().contains("sized twice"))
        .collect();
    let about: Vec<&Value> = found.iter().map(|p| &p["about"]).collect();
    assert_eq!(about, [&json!("extra.2")]);
    assert!(
        found[0]["message"]
            .as_str()
            .unwrap()
            .contains("H7 and ±0.05")
    );
}

// test_call_out.py::test_a_datum_a_call_out_is_measured_from_is_used
#[test]
fn a_datum_a_call_out_is_measured_from_is_used() {
    let plate = plate();
    let (top, bottom, _) = planes(&plate);
    let mut given: Map<String, Value> = questions(&plate, &Map::new())
        .unwrap()
        .into_iter()
        .filter(|q| q.id.starts_with("hole.function"))
        .map(|q| (q.id, json!("general")))
        .collect();
    given.insert("datum.A".into(), json!([bottom]));
    given.insert(
        "extra.1".into(),
        json!({"faces": [top], "reference": [bottom], "kind": "distance", "value": "±0.1"}),
    );
    let unused: Vec<Value> = problems(&plate, &given)
        .unwrap()
        .into_iter()
        .filter(|p| p["message"].as_str().unwrap().contains("not used"))
        .collect();
    assert!(!unused.iter().any(|p| p["about"] == "datum.A"));
}

// test_call_out.py::test_an_added_requirement_that_is_not_an_object_is_refused (every parameter)
#[test]
fn an_added_requirement_that_is_not_an_object_is_refused() {
    let plate = plate();
    for value in [json!("Ra 0.8"), json!(["finish"]), json!(null), json!(3)] {
        let message = refusal(&plate, json!({"extra.finish1": value.clone()}));
        assert!(message.contains("extra.finish1"), "{message}");
        let given = json!({"part.material": "steel", "extra.finish1": value});
        match apply_with_defaults(&plate, given.as_object().unwrap(), true) {
            Err(Error::Invalid(message)) => assert!(message.contains("extra.finish1")),
            other => panic!("not refused: {other:?}"),
        }
    }
}
