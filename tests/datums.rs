//! Datums as face sets or holes, datum controls, and a frame per tolerance: specify-core's
//! `tests/test_datums.py`.
//!
//! CTC-03 is answered as NIST's drawing specifies it (nist_ctc_03_asme1_rc.pdf): A is two
//! coplanar flange faces, B and C are holes, D is the web, and the switch-mounting holes are
//! located from D|B|C. Values are NIST's in inches; sizes use H11. Python's analyses of CTC-03,
//! the plate, the spool and the slotted block are the oracle's captured ones.
//!
//! Not here: `test_nist_datum_scheme_round_trips` writes the file and reads it back (stage 3,
//! the writer); its rules side, the tolerances the intent asks for, is
//! `nist_datum_scheme_round_trips`. `test_the_cli_reports_a_bad_datum_answer` is the command
//! line's (stage 4); the refusal it reports is `a_bad_datum_answer_is_a_clear_error`.

mod common;

use serde_json::{Map, Value, json};
use specify_core::pyfmt::g;
use specify_core::rules::{
    Control, Datum, DatumKind, Datums, Error, Faces, Question, apply_with_defaults, datum_control,
    datum_kind, default_datums, questions, questions_proposing,
};

const INCH: f64 = 25.4;
const CTC03: &str = "corpus/nist/nist_ctc_03_asme1_rc";

fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

fn ids(qs: &[Question]) -> Vec<&str> {
    qs.iter().map(|q| q.id.as_str()).collect()
}

fn by_id<'q>(qs: &'q [Question], id: &str) -> &'q Question {
    qs.iter()
        .find(|q| q.id == id)
        .unwrap_or_else(|| panic!("no question {id}"))
}

fn invalid(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(Error::Invalid(message)) => message,
        other => panic!("not refused as invalid: {other:?}"),
    }
}

/// `_bore`: a hole's bore faces (its cylinders at its diameter).
fn bore(analysis: &Value, hole_id: &str) -> Vec<i64> {
    let hole = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == hole_id)
        .unwrap();
    let diameter = hole["record"]["diameter"].as_f64().unwrap();
    let faces = analysis["faces"].as_array().unwrap();
    hole["faces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_i64().unwrap())
        .filter(|&i| {
            let f = faces.iter().find(|f| f["id"] == i).unwrap();
            f["kind"] == "cylinder"
                && (2.0 * f["radius"].as_f64().unwrap_or(0.0) - diameter).abs() < 1e-6
        })
        .collect()
}

/// The `nist_answers` fixture.
fn nist_answers(a: &Value) -> Map<String, Value> {
    let pair = "hole_patterns.rectangle:58.59.60.61";
    let switches = "hole_patterns.linear:72.73.74.75";
    answers(json!({
        "part.material": "6061-T6",
        "datum.A": [18, 28],
        "datum.B": bore(a, "holes:58"),
        "datum.C": bore(a, "holes:60"),
        "datum.D": [7],
        "datum.B.control": g(0.01 * INCH),
        "datum.C.control": g(0.02 * INCH),
        format!("hole.function:{pair}"): "fit:H11",
        format!("hole.position:{pair}"): g(0.05 * INCH),
        format!("hole.function:{switches}"): "fit:H11",
        format!("hole.frame:{switches}"): "D|B|C",
        format!("hole.position:{switches}"): g(0.05 * INCH),
        "hole.function:holes:70": "fit:H11",
        "hole.frame:holes:70": "D|B|C",
        "hole.position:holes:70": g(0.08 * INCH),
        // Untoleranced on NIST's drawing. Metric clearance matching would otherwise take them
        // for M24 and M4 holes: an inch part is not metric.
        "hole.function:holes:76": "general",
        "hole.function:hole_patterns.rectangle:77.78.79.80": "general",
    }))
}

// test_datums.py::test_nist_datum_scheme_round_trips (the intent; writing it is stage 3)
#[test]
fn nist_datum_scheme_round_trips() {
    let a = analysis(CTC03);
    let intent = apply_with_defaults(&a, &nist_answers(&a), true).unwrap();
    let mut found: Vec<(String, Vec<String>, f64)> = intent
        .requirements
        .iter()
        .filter(|r| r.contains_key("tolerance") && r["kind"] != "flatness")
        .map(|r| {
            let datums = r["datums"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_string())
                .collect();
            let value = (r["tolerance"].as_f64().unwrap() / INCH * 1e4).round() / 1e4;
            (r["kind"].as_str().unwrap().to_string(), datums, value)
        })
        .collect();
    found.sort_by(|a, b| a.partial_cmp(b).unwrap());
    found.dedup();
    let expect = |kind: &str, datums: &[&str], value: f64| {
        (
            kind.to_string(),
            datums.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
            value,
        )
    };
    let mut wanted = vec![
        expect("perpendicularity", &["A"], 0.01),   // datum B
        expect("position", &["A", "B"], 0.02),      // datum C
        expect("position", &["A", "B", "C"], 0.05), // the 2x pair left after B and C
        expect("position", &["D", "B", "C"], 0.05), // switch mounting holes
        expect("position", &["D", "B", "C"], 0.08), // Ø2.00
    ];
    wanted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(found, wanted);
}

// test_datums.py::test_a_datum_hole_keeps_its_size_but_leaves_its_set
#[test]
fn a_datum_hole_keeps_its_size_but_leaves_its_set() {
    let a = analysis(CTC03);
    let qs = questions(&a, &nist_answers(&a)).unwrap();
    let asked = ids(&qs);
    assert!(asked.contains(&"hole.function:holes:58"));
    assert!(!asked.contains(&"hole.position:holes:58"));
    let pair = by_id(&qs, "hole.function:hole_patterns.rectangle:58.59.60.61");
    assert_eq!(pair.prompt, "What are the 2× Ø11.125 holes for?");
}

// test_datums.py::test_frames_are_offered_only_with_extra_datums
#[test]
fn frames_are_offered_only_with_extra_datums() {
    let a = analysis(CTC03);
    let all = nist_answers(&a);
    let without_d: Map<String, Value> = all
        .iter()
        .filter(|(k, _)| *k != "datum.D" && !k.contains("frame"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let qs = questions(&a, &without_d).unwrap();
    assert!(!qs.iter().any(|q| q.id.starts_with("hole.frame")));
    let qs = questions(&a, &all).unwrap();
    let frame = qs.iter().find(|q| q.id.starts_with("hole.frame")).unwrap();
    assert_eq!(frame.options, ["A|B|C", "D|B|C"]);
}

// test_datums.py::test_a_frame_that_is_not_offered_is_refused
#[test]
fn a_frame_that_is_not_offered_is_refused() {
    let a = analysis(CTC03);
    let mut bad = nist_answers(&a);
    bad.insert("hole.frame:holes:70".into(), json!("C|B|A"));
    let message = invalid(apply_with_defaults(&a, &bad, true));
    assert!(message.contains("is not one of"), "{message}");
    assert_eq!(
        message,
        "hole.frame:holes:70: 'C|B|A' is not one of ['A|B|C', 'D|B|C']"
    );
}

// test_datums.py::test_frame_answers_lapse_when_the_extra_datum_goes (both parameters)
#[test]
fn frame_answers_lapse_when_the_extra_datum_goes() {
    // Removing D must not leave its D|B|C answers breaking apply.
    let a = analysis(CTC03);
    let removed = |answers: &mut Map<String, Value>| {
        answers.remove("datum.D");
    };
    let emptied = |answers: &mut Map<String, Value>| {
        answers.insert("datum.D".into(), json!([]));
    };
    for remove in [&removed as &dyn Fn(&mut Map<String, Value>), &emptied] {
        let mut answers = nist_answers(&a);
        remove(&mut answers);
        let intent = apply_with_defaults(&a, &answers, true).unwrap();
        let mut frames: Vec<Value> = intent
            .requirements
            .iter()
            .filter(|r| r["kind"] == "position")
            .map(|r| r["datums"].clone())
            .collect();
        frames.sort_by_key(|f| f.as_array().map_or(0, Vec::len));
        frames.dedup();
        assert_eq!(frames, [json!(["A", "B"]), json!(["A", "B", "C"])]);
    }
}

// test_datums.py::test_without_a_there_are_no_positions_or_controls
#[test]
fn without_a_there_are_no_positions_or_controls() {
    let a = analysis(CTC03);
    let mut answers = nist_answers(&a);
    answers.insert("datum.A".into(), json!([]));
    let qs = questions(&a, &answers).unwrap();
    assert!(!qs.iter().any(|q| q.id.starts_with("hole.position")
        || q.id.starts_with("hole.frame")
        || q.id.contains("control")));
}

// test_datums.py::test_without_b_a_hole_c_has_no_control_but_keeps_a_position
#[test]
fn without_b_a_hole_c_has_no_control_but_keeps_a_position() {
    let a = analysis(CTC03);
    let mut answers: Map<String, Value> = nist_answers(&a)
        .into_iter()
        .filter(|(k, _)| !k.contains("frame"))
        .collect();
    answers.insert("datum.B".into(), json!([]));
    answers.insert("hole.function:holes:60".into(), json!("fit:H11"));
    let qs = questions(&a, &answers).unwrap();
    assert!(!ids(&qs).contains(&"datum.C.control"));
    // Its own letter dropped from A|C.
    assert_eq!(
        by_id(&qs, "hole.position:holes:60").frame.as_deref(),
        Some("A")
    );
}

// test_datums.py::test_a_datum_hole_without_a_control_keeps_its_position
#[test]
fn a_datum_hole_without_a_control_keeps_its_position() {
    // D gets no control in v1, so a hole chosen as D must still be located.
    let a = analysis(CTC03);
    let mut answers = nist_answers(&a);
    answers.insert("datum.D".into(), json!(bore(&a, "holes:70")));
    let answers: Map<String, Value> = answers
        .into_iter()
        .filter(|(k, _)| !k.contains("frame"))
        .collect();
    let qs = questions(&a, &answers).unwrap();
    assert_eq!(
        by_id(&qs, "hole.position:holes:70").frame.as_deref(),
        Some("A|B|C")
    );
    assert_eq!(by_id(&qs, "hole.frame:holes:70").options, ["A|B|C", "B|C"]);
}

// test_datums.py::test_a_bad_datum_answer_is_a_clear_error
#[test]
fn a_bad_datum_answer_is_a_clear_error() {
    let a = analysis(CTC03);
    let message = invalid(questions(&a, &answers(json!({"datum.A": [99999]}))));
    assert_eq!(message, "datum A: face 99999 does not exist");
    let message = invalid(questions(&a, &answers(json!({"datum.A": [18, 7]}))));
    assert!(
        message.starts_with("datum A: ") && message.contains("cannot form one datum"),
        "{message}"
    );
}

// test_datums.py::test_datum_faces_must_be_coplanar_or_one_hole
#[test]
fn datum_faces_must_be_coplanar_or_one_hole() {
    let a = analysis(CTC03);
    let faces = Faces::from_analysis(&a).unwrap();
    assert_eq!(datum_kind(&[18, 28], &faces).unwrap(), DatumKind::Plane);
    assert_eq!(
        datum_kind(&bore(&a, "holes:58"), &faces).unwrap(),
        DatumKind::Axis
    );
    // Parallel planes, not coplanar.
    let message = invalid(datum_kind(&[18, 7], &faces));
    assert!(message.contains("cannot form one datum"), "{message}");
    let mut mixed = vec![18];
    mixed.extend(bore(&a, "holes:58"));
    let message = invalid(datum_kind(&mixed, &faces));
    assert!(message.contains("cannot form one datum"), "{message}");
}

// test_datums.py::test_default_datum_a_takes_every_coplanar_face
#[test]
fn default_datum_a_takes_every_coplanar_face() {
    // The slotted block has nothing to position, so A is proposed only on request.
    let slotted = analysis("parts/default_datum_a_takes_every_coplanar_face");
    let qs = questions_proposing(&slotted, &Map::new(), "ABC").unwrap();
    let a = by_id(&qs, "datum.A");
    let bottoms: Vec<i64> = slotted["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "plane" && f["centroid"][2] == json!(-5.0))
        .map(|f| f["id"].as_i64().unwrap())
        .collect();
    assert_eq!(bottoms.len(), 2);
    let mut default: Vec<i64> = a
        .default
        .as_ref()
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_i64().unwrap())
        .collect();
    default.sort_unstable();
    assert_eq!(default, bottoms);
}

// test_datums.py::test_a_plane_c_is_held_square_to_a_and_b
#[test]
fn a_plane_c_is_held_square_to_a_and_b() {
    let plate = analysis("parts/plate");
    let qs = questions(&plate, &Map::new()).unwrap();
    assert_eq!(
        by_id(&qs, "datum.C.control").prompt,
        "Perpendicularity required on datum C, relative to A|B?"
    );
    let answers = answers(json!({"part.material": "steel", "datum.C.control": "0.05"}));
    let intent = apply_with_defaults(&plate, &answers, true).unwrap();
    let c: Vec<_> = intent
        .requirements
        .iter()
        .filter(|r| r.get("feature") == Some(&json!("datum C")))
        .collect();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0]["kind"], "perpendicularity");
    assert_eq!(c[0]["datums"], json!(["A", "B"]));
    assert_eq!(c[0]["diametral"], json!(false));
}

/// `_faces`: hand-made faces, numbered in order.
fn faces(specs: &[Value]) -> Faces {
    let records: Vec<Value> = specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let mut record = spec.clone();
            record["id"] = json!(i);
            record
        })
        .collect();
    Faces::from_values(&records).unwrap()
}

// test_datums.py::test_a_bore_parallel_to_datum_a_gets_no_control
#[test]
fn a_bore_parallel_to_datum_a_gets_no_control() {
    // A cross-bore runs along A: it would need parallelism, which v1 cannot write.
    let faces = faces(&[
        json!({"kind": "plane", "direction": [0, 0, 1]}),
        json!({"kind": "cylinder", "direction": [1, 0, 0]}),
        json!({"kind": "cylinder", "direction": [0, 0, 1]}),
    ]);
    let datum = |faces: Vec<i64>, kind| Datum {
        faces,
        kind,
        existing: false,
    };
    let plane = datum(vec![0], DatumKind::Plane);
    let across = Datums(vec![
        ('A', plane.clone()),
        ('B', datum(vec![1], DatumKind::Axis)),
    ]);
    assert_eq!(datum_control('B', &across, &faces).unwrap(), None);
    let square = Datums(vec![('A', plane), ('B', datum(vec![2], DatumKind::Axis))]);
    assert_eq!(
        datum_control('B', &square, &faces).unwrap(),
        Some(Control {
            kind: "perpendicularity",
            datums: vec!["A"],
            diametral: true,
        })
    );
}

// test_datums.py::test_turned_datum_b_takes_every_face_of_a_split_bore
#[test]
fn turned_datum_a_takes_every_face_of_the_greatest_area_seat() {
    let common = json!({"kind": "cylinder", "direction": [0, 0, 1], "origin": [0, 0, 0],
                        "radius": 5.0, "centroid": [0, 0, 0]});
    let with = |extra: Value| {
        let mut face = common.clone();
        for (k, v) in extra.as_object().unwrap() {
            face[k] = v.clone();
        }
        face
    };
    let faces = faces(&[
        json!({"kind": "plane", "direction": [0, 0, 1], "centroid": [0, 0, 0], "area": 100.0}),
        with(json!({"area": 20.0})),
        with(json!({"area": 20.0})),
        with(json!({"radius": 9.0, "area": 10.0})),
    ]);
    let (a, b, c) = default_datums(&faces, true).unwrap();
    assert_eq!(a, [1, 2]);
    assert_eq!(b, [0]);
    assert!(c.is_empty());
    // Equal diameters are one feature even when the seam splits their faces.
    let split = Faces::from_values(
        &faces
            .iter()
            .map(|f| {
                json!({
                    "id": f.id, "kind": f.kind, "direction": f.direction, "centroid": f.centroid,
                    "origin": f.origin, "radius": if f.id == 3 { Some(4.0) } else { f.radius },
                    "area": f.area
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(default_datums(&split, true).unwrap().0, [1, 2]);
}

// test_datums.py::test_a_datum_bore_on_a_turned_part_keeps_its_fit
#[test]
fn a_datum_bore_on_a_turned_part_keeps_its_fit() {
    // The spool now locates on its largest external diameter. The bore keeps its fit
    // and gets a coaxial position to A; B is the shoulder.
    let qs = questions(
        &analysis("corpus/cadgenbench/flanged_spool_132"),
        &answers(json!({"hole.function:holes:34.55": "fit:H7"})),
    )
    .unwrap();
    let asked = ids(&qs);
    assert!(asked.contains(&"hole.function:holes:34.55"));
    assert_eq!(
        by_id(&qs, "hole.position:holes:34.55").frame.as_deref(),
        Some("A")
    );
    assert!(asked.contains(&"datum.B.control"));
}

// test_datums.py::test_datums_are_proposed_only_when_something_is_measured_from_them
#[test]
fn datums_are_proposed_even_when_all_holes_use_general_tolerance() {
    let plate = analysis("parts/plate");
    let qs = questions(&plate, &Map::new()).unwrap();
    let asked = ids(&qs);
    for x in ["datum.A", "datum.B", "datum.C"] {
        assert!(asked.contains(&x));
    }
    // Asked after the features they serve.
    let a = asked.iter().position(|x| *x == "datum.A").unwrap();
    let last_hole = asked.iter().rposition(|x| x.starts_with("hole.")).unwrap();
    assert!(a > last_hole);
    let plain = plain_holes(&qs);
    let qs = questions(&plate, &plain).unwrap();
    for x in ["datum.A", "datum.B", "datum.C"] {
        assert!(ids(&qs).contains(&x));
    }
}

/// Every hole answered `general`.
fn plain_holes(qs: &[Question]) -> Map<String, Value> {
    qs.iter()
        .filter(|q| q.id.starts_with("hole.function"))
        .map(|q| (q.id.clone(), json!("general")))
        .collect()
}

// test_datums.py::test_a_datum_given_or_removed_is_always_asked
#[test]
fn a_datum_given_or_removed_is_always_asked() {
    let plate = analysis("parts/plate");
    let mut plain = plain_holes(&questions(&plate, &Map::new()).unwrap());
    let top = plate["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "plane")
        .max_by(|a, b| {
            a["centroid"][2]
                .as_f64()
                .partial_cmp(&b["centroid"][2].as_f64())
                .unwrap()
        })
        .unwrap()["id"]
        .clone();
    plain.insert("datum.A".into(), json!([top]));
    let added = questions(&plate, &plain).unwrap();
    assert_eq!(
        by_id(&added, "datum.A").default,
        by_id(&questions(&plate, &Map::new()).unwrap(), "datum.A").default
    );
    assert!(ids(&added).contains(&"datum.B"));
    let removed = questions(&plate, &answers(json!({"datum.C": []}))).unwrap();
    assert!(ids(&removed).contains(&"datum.C"));
    let positions: Vec<&Question> = removed
        .iter()
        .filter(|q| q.id.starts_with("hole.position"))
        .collect();
    assert!(!positions.is_empty());
    assert!(positions.iter().all(|q| q.frame.as_deref() == Some("A|B")));
}

// #217: a datum without the one before it makes no sense, so C is not asked once B is
// decided none, nor B (or C) once A is. A letter the person gave stays asked.
#[test]
fn a_later_datum_is_not_asked_once_the_one_before_is_none() {
    let plate = analysis("parts/plate");
    let fresh = questions(&plate, &Map::new()).unwrap();
    assert!(ids(&fresh).contains(&"datum.B"));
    assert!(ids(&fresh).contains(&"datum.C"));
    let no_b = questions(&plate, &answers(json!({"datum.B": []}))).unwrap();
    assert!(ids(&no_b).contains(&"datum.B"));
    assert!(!ids(&no_b).contains(&"datum.C"));
    let no_a = questions(&plate, &answers(json!({"datum.A": []}))).unwrap();
    assert!(ids(&no_a).contains(&"datum.A"));
    assert!(
        !ids(&no_a)
            .iter()
            .any(|id| *id == "datum.B" || *id == "datum.C")
    );
    let given = questions(&plate, &answers(json!({"datum.B": [], "datum.C": []}))).unwrap();
    assert!(ids(&given).contains(&"datum.C"));
}
