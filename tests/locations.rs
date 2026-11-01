//! Location dimensions, basic or ± from the selected datum planes:
//! specify-core's `tests/test_locations.py`, on the oracle's captured analysis of the plate.
//!
//! Each test's write and read-back is the writer's (stage 3); the intent each asserts is here.
//! `test_only_untoleranced_locations_are_marked_basic` is not ported: it tests `mark_basic`, an
//! edit of OpenCascade's output that this port does not make (haecceity writes a basic
//! dimension as basic).

use serde_json::{Map, Value, json};
use specify_core::choices::interpret;
use specify_core::locations::parse;
use specify_core::rules::{Intent, LOCATING, apply_with_defaults, questions};

fn plate() -> Value {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step");
    specify_core::api::analyse(&specify_core::part::load(&path, None).unwrap()).unwrap()
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

fn locations(intent: &Intent) -> Vec<&Map<String, Value>> {
    intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "location")
        .collect()
}

// test_locations.py::test_positioned_holes_get_basic_dimensions_from_their_datum_planes
#[test]
fn positioned_holes_get_basic_dimensions_from_their_datum_planes() {
    let intent =
        apply_with_defaults(&plate(), &answers(json!({"part.material": "steel"})), true).unwrap();
    let basic = locations(&intent);
    assert!(!basic.is_empty());
    assert!(
        basic
            .iter()
            .all(|r| r["basic"] == json!(true) && !r.contains_key("upper"))
    );
    let datum_faces: Vec<&Value> = intent
        .datums
        .iter()
        .filter(|d| d["letter"] == "B" || d["letter"] == "C")
        .map(|d| &d["faces"])
        .collect();
    let pitch =
        |r: &&Map<String, Value>| r["planner"]["required"]["pattern"].get("pitch").is_some();
    assert_eq!(basic.iter().filter(|r| pitch(r)).count(), 2); // the two counted grid pitches (#75)
    assert_eq!(basic.len(), 5); // shared origin Y, two origin Xs, and two pitches
    assert!(
        basic
            .iter()
            .filter(|r| !pitch(r))
            .all(|r| datum_faces.contains(&&r["reference"]))
    );
    // The M5 hole at x=45, y=0 on a 100 x 60 plate: 95 and 30 from its edges.
    let mut m5: Vec<f64> = basic
        .iter()
        .filter(|r| r["feature"] == "holes:12")
        .map(|r| r["distance"].as_f64().unwrap())
        .collect();
    m5.sort_by(f64::total_cmp);
    assert_eq!(m5.len(), 2);
    assert!(m5.iter().zip([30., 95.]).all(|(a, b)| (a - b).abs() < 1e-6));
}

// test_locations.py::test_holes_located_by_plus_minus_can_be_given_a_tolerance
#[test]
fn holes_located_by_plus_minus_can_be_given_a_tolerance() {
    let plate = plate();
    let qs = questions(&plate, &answers(json!({"part.locating": LOCATING[1]}))).unwrap();
    let asked: Vec<_> = qs
        .iter()
        .filter(|q| q.id.starts_with("hole.location"))
        .collect();
    assert!(!asked.is_empty());
    assert!(
        asked
            .iter()
            .all(|q| q.default == Some(json!("general")) && q.options.iter().any(|o| o == "±0.05"))
    );
    let grid = asked
        .iter()
        .find(|q| q.id.contains("grid"))
        .unwrap()
        .id
        .clone();
    let given = answers(json!({
        "part.locating": LOCATING[1],
        "part.material": "steel",
        grid: "±0.05",
    }));
    let intent = apply_with_defaults(&plate, &given, true).unwrap();
    let located = locations(&intent);
    // Shared planner: two pattern-origin offsets and two counted pitches replace twelve member offsets (#75).
    assert_eq!(located.len(), 4);
    assert!(
        located
            .iter()
            .all(|r| r["upper"].as_f64() == Some(0.05) && r["lower"].as_f64() == Some(-0.05))
    );
    assert!(!located.iter().any(|r| r.contains_key("basic")));
}

// test_locations.py::test_general_writes_no_location
#[test]
fn general_writes_no_location() {
    let given = answers(json!({"part.locating": LOCATING[1], "part.material": "steel"}));
    let intent = apply_with_defaults(&plate(), &given, true).unwrap();
    assert!(locations(&intent).is_empty());
}

// test_locations.py::test_a_typed_location_tolerance (every parameter)
#[test]
fn a_typed_location_tolerance() {
    for (text, value) in [
        ("0.03", "±0.03"),
        ("±0.1", "±0.1"),
        ("+-0.2", "±0.2"),
        ("none", "general"),
    ] {
        let none: [&str; 0] = [];
        assert_eq!(
            interpret("hole.location:x", text, &none, None, None).unwrap(),
            value
        );
        let expected = (value != "general").then(|| value[2..].parse::<f64>().unwrap());
        assert_eq!(parse(&json!(value)).unwrap(), expected, "{value}");
    }
}

// test_locations.py::test_a_part_whose_only_dimensions_are_locations_keeps_its_tolerance_values
// (the intent; the value read back is the writer's)
#[test]
fn a_part_whose_only_dimensions_are_locations_keeps_its_tolerance_values() {
    let plate = plate();
    let mut given = answers(json!({"part.material": "steel"}));
    for q in questions(&plate, &Map::new()).unwrap() {
        let tapped = q
            .default
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|d| d.starts_with("tapped"));
        if q.id.starts_with("hole.function") && !tapped {
            given.insert(q.id.clone(), json!("general"));
        }
    }
    let intent = apply_with_defaults(&plate, &given, true).unwrap();
    assert!(!intent.requirements.iter().any(|r| r["kind"] == "size"));
    assert!(!locations(&intent).is_empty());
    let positions: Vec<_> = intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "position")
        .collect();
    assert_eq!(positions.len(), 1);
}

#[test]
fn plus_minus_locations_follow_changed_and_removed_datums() {
    let plate = plate();
    let mut given = answers(json!({"part.locating": LOCATING[1], "part.material": "steel"}));
    for q in questions(&plate, &given).unwrap() {
        if q.id.starts_with("hole.location:") {
            given.insert(q.id, json!("±0.05"));
        }
    }
    let original = apply_with_defaults(&plate, &given, true).unwrap();
    let old_b = &original.datums.iter().find(|d| d["letter"] == "B").unwrap()["faces"];
    // Select the opposite parallel side: distances must now come from that side.
    let faces = plate["faces"].as_array().unwrap();
    let b_face = faces.iter().find(|f| f["id"] == old_b[0]).unwrap();
    let opposite = faces
        .iter()
        .find(|f| {
            f["kind"] == "plane"
                && (0..3)
                    .map(|i| {
                        f["direction"][i].as_f64().unwrap()
                            * b_face["direction"][i].as_f64().unwrap()
                    })
                    .sum::<f64>()
                    < -0.999
        })
        .unwrap();
    given.insert("datum.B".into(), json!([opposite["id"]]));
    given.insert("datum.C".into(), json!([]));
    let changed = apply_with_defaults(&plate, &given, true).unwrap();
    let located = locations(&changed);
    assert!(!located.is_empty());
    assert!(
        located
            .iter()
            .filter(|r| r["planner"]["required"]["pattern"].get("pitch").is_none())
            .all(|r| r["reference"] == json!([opposite["id"]]))
    );
    // The grid, centre hole and M5 are symmetric at 30 from either Y side.
    // Shared planner offsets therefore keep their value while changing their origin (#75).
    let origins: Vec<_> = located
        .iter()
        .filter(|r| r["planner"]["required"]["pattern"].get("pitch").is_none())
        .collect();
    assert_eq!(origins.len(), 1);
    assert!((origins[0]["distance"].as_f64().unwrap() - 30.).abs() < 1e-6);
    assert_ne!(origins[0]["reference"], *old_b);
    assert!(
        located
            .iter()
            .all(|r| r["upper"] == 0.05 && r["lower"] == -0.05 && !r.contains_key("basic"))
    );
    given.insert("datum.A".into(), json!([]));
    given.insert("datum.B".into(), json!([]));
    let removed = apply_with_defaults(&plate, &given, true).unwrap();
    assert!(removed.datums.is_empty());
    assert!(locations(&removed).is_empty());
}
