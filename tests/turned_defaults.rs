//! Turned part datums and coaxial hole location, using captured geometry plus live corpus.
mod common;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::problems::problems;
use specify_core::rules::records::Ctx;
use specify_core::rules::{DatumKind, Question, datum_kind, preview, questions, with_all_defaults};

// These tests verify the proposed datum scheme and its dependent geometry after acceptance.
// #20 keeps suggestions out of preview; tests/default_suggestions.rs guards that distinction.
fn accepted_preview(a: &Value, answers: &Map<String, Value>) -> specify_core::rules::Intent {
    preview(a, &with_all_defaults(a, answers).unwrap()).unwrap()
}

fn captured(name: &str) -> Value {
    common::case(&format!("corpus/{name}"))
        .artifact("analysis")
        .unwrap()
}

fn q<'a>(qs: &'a [Question], id: &str) -> &'a Question {
    qs.iter()
        .find(|q| q.id == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}

fn check(a: &Value, axis: &[i64], shoulder: &[i64], hole: Option<&str>) {
    let ctx = Ctx::new(a).unwrap();
    let defaults = ctx.default_datums().unwrap();
    assert_eq!(defaults, &(axis.to_vec(), shoulder.to_vec(), vec![]));
    assert_eq!(datum_kind(axis, &ctx.faces).unwrap(), DatumKind::Axis);
    assert_eq!(datum_kind(shoulder, &ctx.faces).unwrap(), DatumKind::Plane);
    // The same seats are measurement origins in ± mode, including parts with no hole.
    let plus_minus = json!({"part.locating": specify_core::rules::LOCATING[1]})
        .as_object()
        .unwrap()
        .clone();
    let qs = questions(a, &plus_minus).unwrap();
    assert_eq!(q(&qs, "datum.A").default, Some(json!(axis)));
    assert_eq!(q(&qs, "datum.B").default, Some(json!(shoulder)));
    assert_eq!(q(&qs, "datum.C").default, Some(json!([])));
    assert!(q(&qs, "datum.A").basis.contains("plain turned seat"));
    assert!(preview(a, &plus_minus).unwrap().datums.is_empty());
    let intent = accepted_preview(a, &plus_minus);
    assert_eq!(intent.datums.len(), 2);
    assert!(intent.requirements.iter().all(|r| !matches!(
        r["kind"].as_str(),
        Some("position" | "flatness" | "perpendicularity" | "runout")
    )));
    if let Some(hole) = hole {
        let qs = questions(a, &Map::new()).unwrap();
        assert_eq!(
            q(&qs, &format!("hole.position:{hole}")).frame.as_deref(),
            Some("A")
        );
        assert_eq!(q(&qs, "datum.A").default, Some(json!(axis)));
        assert_eq!(q(&qs, "datum.B").default, Some(json!(shoulder)));
        assert!(q(&qs, "datum.A").basis.contains("plain turned seat"));
        assert!(q(&qs, "datum.B").basis.contains("perpendicular to A"));
        assert!(!qs.iter().any(|q| q.id == "datum.A.flatness"));
        let intent = accepted_preview(a, &Map::new());
        let pos = intent
            .requirements
            .iter()
            .find(|r| {
                r.get("kind") == Some(&json!("position")) && r.get("feature") == Some(&json!(hole))
            })
            .unwrap();
        assert_eq!(pos["datums"], json!(["A"]));
        assert_eq!(pos["tolerance"], 0.2);
        assert!(
            !problems(a, &Map::new())
                .unwrap()
                .iter()
                .any(|p| p["message"].as_str().unwrap().contains("nothing locates"))
        );
    }
}

#[test]
fn real_turned_analyses_choose_an_axis_and_perpendicular_face() {
    check(
        &captured("gramel/string_post"),
        &[0],
        &[8],
        Some("holes:17.19"),
    );
    check(
        &captured("gramel/GRM-03_thumbwheel_drive_screw"),
        &[6],
        &[0],
        Some("holes:12.13"),
    );
    check(&captured("gramel/GRM-05_depth_lock_bolt"), &[0], &[2], None);
}

#[test]
fn turned_detection_rejects_different_principal_axes_and_prismatic_bodies() {
    let a = captured("gramel/string_post");
    for alteration in ["offset", "direction", "block", "no_steps"] {
        let mut a = a.clone();
        if alteration == "no_steps" {
            a["features"]
                .as_array_mut()
                .unwrap()
                .retain(|f| f["family"] != "turned_steps");
        } else {
            for f in a["faces"].as_array_mut().unwrap() {
                if f["id"] == 9 && alteration == "offset" {
                    f["origin"] = json!([1, 0, 0]);
                }
                if f["id"] == 9 && alteration == "direction" {
                    f["direction"] = json!([1, 0, 0]);
                }
                if f["id"] == 11 && alteration == "block" {
                    f["area"] = json!(10000);
                }
            }
        }
        let ctx = Ctx::new(&a).unwrap();
        assert_eq!(
            datum_kind(&ctx.default_datums().unwrap().0, &ctx.faces).unwrap(),
            DatumKind::Plane,
            "{alteration}"
        );
    }
}

#[test]
fn contact_selects_the_locating_shoulder_and_datum_answers_still_override() {
    let mut a = captured("gramel/string_post");
    for f in a["faces"].as_array_mut().unwrap() {
        if f["id"] == 3 {
            f["contact"] = json!({"area": 5, "part": "mount"});
        }
        // A much larger side-flat contact must not replace a turned axis or its shoulder.
        if f["id"] == 11 {
            f["contact"] = json!({"area": 100, "part": "clamp"});
        }
    }
    let ctx = Ctx::new(&a).unwrap();
    assert_eq!(ctx.default_datums().unwrap().1, [3]);
    let answers = json!({"datum.A": [0], "datum.B": [2]})
        .as_object()
        .unwrap()
        .clone();
    let intent = preview(&a, &answers).unwrap();
    assert_eq!(intent.datums[0]["faces"], json!([0]));
    assert_eq!(intent.datums[1]["faces"], json!([2]));
    let removed = json!({"datum.A": []}).as_object().unwrap().clone();
    assert!(
        problems(&a, &removed)
            .unwrap()
            .iter()
            .any(|p| p["about"] == "hole.function:holes:17.19"
                && p["message"].as_str().unwrap().contains("nothing locates"))
    );
}

#[test]
fn offset_parallel_and_cross_holes_are_not_treated_as_coaxial() {
    let a = captured("gramel/string_post");
    for direction in [false, true] {
        let mut a = a.clone();
        for face in a["faces"].as_array_mut().unwrap() {
            if face["id"] == 17 {
                if direction {
                    face["direction"] = json!([0, 1, 0]);
                } else {
                    face["origin"] = json!([1, 0, 0]);
                }
            }
        }
        let qs = questions(&a, &Map::new()).unwrap();
        assert_eq!(
            q(&qs, "hole.position:holes:17.19").frame.as_deref(),
            Some("A|B")
        );
    }
    let qs = questions(&a, &Map::new()).unwrap();
    assert_eq!(
        q(&qs, "hole.position:holes:5").frame.as_deref(),
        Some("A|B")
    );
}

#[test]
fn runout_uses_primary_axis_even_before_datums_are_proposed() {
    let a = captured("gramel/GRM-05_depth_lock_bolt");
    let qs = questions(&a, &Map::new()).unwrap();
    assert!(!qs.iter().any(|q| q.id == "diameter.runout:turned_steps:0"));
    assert_eq!(
        q(&qs, "diameter.runout:turned_steps:6").frame.as_deref(),
        Some("A")
    );
    let mut given = json!({"diameter.runout:turned_steps:6": "0.02"})
        .as_object()
        .unwrap()
        .clone();
    let pending = preview(&a, &given).unwrap();
    assert!(pending.datums.is_empty());
    assert!(pending.requirements.is_empty());
    let found = problems(&a, &given).unwrap();
    let awaiting = found
        .iter()
        .find(|p| p["code"] == "awaits_datum_frame")
        .unwrap();
    assert_eq!(awaiting["about"], "diameter.runout:turned_steps:6");
    assert_eq!(awaiting["awaiting"], json!(["A"]));
    given.insert("datum.A".into(), json!([0]));
    assert!(
        problems(&a, &given)
            .unwrap()
            .iter()
            .all(|p| p["code"] != "awaits_datum_frame")
    );
    let intent = preview(&a, &given).unwrap();
    assert_eq!(intent.datums[0]["faces"], json!([0]));
    assert_eq!(
        intent
            .requirements
            .iter()
            .find(|r| r["kind"] == "runout")
            .unwrap()["datums"],
        json!(["A"])
    );
}

#[test]
fn live_corpus_turned_datums() {
    let Some(root) = common::corpus_dir() else {
        return;
    };
    for name in [
        "gramel/string_post",
        "gramel/GRM-03_thumbwheel_drive_screw",
        "gramel/GRM-05_depth_lock_bolt",
    ] {
        let a = api::analyse_file(&root.join(format!("{name}.step")), None).unwrap();
        // The same assertions run against actual recognition. Quiddity #216 adds the
        // planar floors (19 and 13) to these hole IDs; datum expectations stay exact.
        match name {
            "gramel/string_post" => check(&a, &[0], &[8], Some("holes:17.19")),
            "gramel/GRM-03_thumbwheel_drive_screw" => check(&a, &[6], &[0], Some("holes:12.13")),
            _ => check(&a, &[0], &[2], None),
        }
    }
}

#[test]
fn an_axial_hole_needs_no_shoulder_but_removing_a_still_leaves_holes_unlocated() {
    let a = captured("gramel/string_post");
    let no_b = json!({"datum.B": []}).as_object().unwrap().clone();
    let qs = questions(&a, &no_b).unwrap();
    assert_eq!(
        q(&qs, "hole.position:holes:17.19").frame.as_deref(),
        Some("A")
    );
    assert!(
        !problems(&a, &no_b)
            .unwrap()
            .iter()
            .any(|p| p["message"].as_str().unwrap().contains("nothing locates"))
    );
    let no_a = json!({"datum.A": []}).as_object().unwrap().clone();
    let found = problems(&a, &no_a).unwrap();
    for hole in ["holes:17.19", "holes:5"] {
        assert!(
            found
                .iter()
                .any(|p| p["about"] == format!("hole.function:{hole}")
                    && p["message"].as_str().unwrap().contains("nothing locates"))
        );
    }
}

#[test]
fn cylinder_ties_use_total_seat_area_and_a_needs_no_plane() {
    use specify_core::rules::{Faces, default_datums};
    let faces = Faces::from_values(&[
        json!({"id": 0, "kind": "cylinder", "direction": [0,0,1], "origin": [0,0,0], "radius": 5, "area": 10}),
        json!({"id": 1, "kind": "cylinder", "direction": [0,0,-1], "origin": [0,0,5], "radius": 5, "area": 10}),
        json!({"id": 2, "kind": "cylinder", "direction": [0,0,1], "origin": [20,0,0], "radius": 5, "area": 15}),
    ]).unwrap();
    assert_eq!(
        default_datums(&faces, true).unwrap(),
        (vec![0, 1], vec![], vec![])
    );
}

fn thumbwheel_answers() -> Map<String, Value> {
    // Keep the reviewer's supplied map verbatim. The pinned kernel names the M2 bore
    // holes:12.13, rather than holes:14; its default is already tapped:M2.
    json!({
        "hole.function:holes:14": "tapped:M2",
        "diameter.fit:turned_steps:6": "thread:M3",
        "diameter.fit:turned_steps:3": "knurl:straight"
    })
    .as_object()
    .unwrap()
    .clone()
}

fn check_thumbwheel(a: &Value) {
    let given = thumbwheel_answers();
    let ctx = Ctx::new(a).unwrap();
    assert_eq!(
        ctx.default_datums_with(&given).unwrap(),
        (vec![2], vec![0], vec![])
    );
    assert_eq!(ctx.faces.at(2).unwrap().radius().unwrap(), 2.5);
    // The Ø5 × 3 seat has ~47 mm², versus ~9 mm² for the Ø6 × 0.5 collar.
    assert!(ctx.faces.at(2).unwrap().area().unwrap() > ctx.faces.at(9).unwrap().area().unwrap());
    let qs = questions(a, &given).unwrap();
    assert_eq!(q(&qs, "datum.A").default, Some(json!([2])));
    assert_eq!(q(&qs, "datum.B").default, Some(json!([0])));
    assert!(!qs.iter().any(|q| q.id == "diameter.runout:turned_steps:2"));
    assert_eq!(
        q(&qs, "diameter.runout:turned_steps:3").frame.as_deref(),
        Some("A")
    );
    let intent = accepted_preview(a, &given);
    assert_eq!(intent.datums[0]["faces"], json!([2]));
    assert_eq!(intent.datums[1]["faces"], json!([0]));
    assert!(
        intent
            .requirements
            .iter()
            .any(|r| r["kind"] == "thread" && r["faces"] == json!([6]))
    );
    assert!(
        intent
            .requirements
            .iter()
            .any(|r| r["kind"] == "knurl" && r["faces"] == json!([3]))
    );
    assert!(
        !problems(a, &given)
            .unwrap()
            .iter()
            .any(|p| p["message"].as_str().unwrap().contains("nothing locates"))
    );

    // Typed specifications have the same effect, and changing/removing an answer cannot
    // reuse stale defaults from this same context's no-answer cache.
    ctx.default_datums().unwrap();
    let typed = json!({"diameter.fit:turned_steps:6": "M3", "diameter.fit:turned_steps:3": "straight knurl"})
        .as_object().unwrap().clone();
    assert_eq!(ctx.default_datums_with(&typed).unwrap().0, [2]);
    assert_eq!(ctx.default_datums().unwrap().0, [6]);
    let overridden = json!({"datum.A": [9], "datum.B": []})
        .as_object()
        .unwrap()
        .clone();
    let mut given = given;
    given.extend(overridden);
    assert_eq!(preview(a, &given).unwrap().datums[0]["faces"], json!([9]));
}

#[test]
fn thumbwheel_uses_the_plain_five_mm_seat_and_its_shoulder() {
    check_thumbwheel(&captured("gramel/GRM-03_thumbwheel_drive_screw"));
    if let Some(root) = common::corpus_dir() {
        let loaded = specify_core::part::load(
            &root.join("gramel/GRM-03_thumbwheel_drive_screw.step"),
            None,
        )
        .unwrap();
        let a = api::analyse(&loaded).unwrap();
        check_thumbwheel(&a);
        let intent = preview(&a, &thumbwheel_answers()).unwrap();
        let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, None).unwrap();
        let reread = specify_core::part::load_bytes("thumbwheel.step", bytes).unwrap();
        let mut a = api::analyse(&reread).unwrap();
        for kind in ["external thread", "knurl"] {
            assert!(
                a["existing"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["kind"] == "note" && e["type"] == kind)
            );
        }
        // Force default selection independently of the file datums: the semantic thread
        // and knurl alone must keep the original seat after writing and reading AP242.
        a["existing"]
            .as_array_mut()
            .unwrap()
            .retain(|e| e["kind"] != "datum");
        assert_eq!(
            Ctx::new(&a).unwrap().default_datums().unwrap(),
            &(vec![2], vec![0], vec![])
        );
    }
}

#[test]
fn grouped_answers_and_single_face_pmi_exclude_every_split_face_of_a_rough_seat() {
    let mut a = captured("gramel/GRM-03_thumbwheel_drive_screw");
    let mut split_face = a["faces"][6].clone();
    split_face["id"] = json!(14);
    a["faces"].as_array_mut().unwrap().push(split_face);
    let mut split_feature = a["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "turned_steps:6")
        .unwrap()
        .clone();
    split_feature["id"] = json!("turned_steps:14");
    split_feature["faces"] = json!([14]);
    a["features"].as_array_mut().unwrap().push(split_feature);
    let given = thumbwheel_answers();
    assert_eq!(
        Ctx::new(&a).unwrap().default_datums_with(&given).unwrap().0,
        [2]
    );
    assert_eq!(
        q(
            &questions(&a, &given).unwrap(),
            "diameter.fit:turned_steps:6"
        )
        .faces,
        [6, 14]
    );
    a["existing"] = json!([{"kind": "note", "type": "surface finish", "faces": [6]}]);
    assert_eq!(
        Ctx::new(&a).unwrap().default_datums_with(&given).unwrap().0,
        [2]
    );
    a["existing"] = json!([
        {"kind": "note", "type": "external thread", "faces": [6]},
        {"kind": "note", "type": "knurl", "faces": [3]}
    ]);
    assert_eq!(Ctx::new(&a).unwrap().default_datums().unwrap().0, [2]);
    // With every seat rough, a planar fallback is preferable to silently using a thread.
    a["existing"].as_array_mut().unwrap().extend([
        json!({"kind": "note", "type": "knurl", "faces": [2]}),
        json!({"kind": "note", "type": "external thread", "faces": [9]}),
    ]);
    let ctx = Ctx::new(&a).unwrap();
    assert_eq!(
        datum_kind(&ctx.default_datums().unwrap().0, &ctx.faces).unwrap(),
        DatumKind::Plane
    );
}

#[test]
fn recognition_and_pmi_exclude_rough_datum_surfaces_even_with_plain_answers() {
    for source in ["pmi", "recognition", "modelled_threads"] {
        let mut a = captured("gramel/GRM-03_thumbwheel_drive_screw");
        if source == "pmi" {
            a["existing"] = json!([
                {"kind": "note", "type": "external thread", "faces": [6]},
                {"kind": "note", "type": "knurl", "faces": [3]}
            ]);
        } else {
            a["features"].as_array_mut().unwrap().extend([
                json!({"id": format!("{source}:6"), "family": if source == "modelled_threads" { source } else { "external_threads" }, "faces": [6], "record": {}}),
                json!({"id": "knurls:3", "family": "knurls", "faces": [3], "record": {}}),
            ]);
        }
        let ctx = Ctx::new(&a).unwrap();
        let plain = json!({"diameter.fit:turned_steps:6": "general", "diameter.fit:turned_steps:3": "general"}).as_object().unwrap().clone();
        assert_eq!(ctx.default_datums_with(&plain).unwrap().0, [2], "{source}");
        // File datum intent keeps precedence, even if it explicitly references a thread.
        a["existing"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind": "datum", "type": "A", "faces": [6]}));
        assert_eq!(
            preview(&a, &Map::new()).unwrap().datums[0]["faces"],
            json!([6])
        );
    }
}

#[test]
fn interrupted_shaft_uses_main_axis_and_end_seat_instead_of_window_walls() {
    let mut a =
        api::analyse_file(&common::fixtures().join("parts/GRM-02_shaft.step"), None).unwrap();
    let ctx = Ctx::new(&a).unwrap();
    assert!(!ctx.has_family("turned_steps"));
    assert_eq!(
        ctx.default_datums().unwrap(),
        &(vec![0], vec![7, 11], vec![])
    );
    assert_eq!(ctx.faces.at(0).unwrap().radius().unwrap(), 4.0);
    let qs = questions(&a, &Map::new()).unwrap();
    assert_eq!(q(&qs, "datum.A").default, Some(json!([0])));
    assert_eq!(q(&qs, "datum.B").default, Some(json!([7, 11])));
    assert_eq!(q(&qs, "datum.C").default, Some(json!([])));
    let intent = accepted_preview(&a, &Map::new());
    assert_eq!(intent.datums[0]["faces"], json!([0]));
    assert_eq!(intent.datums[1]["faces"], json!([7, 11]));
    assert!(
        intent
            .requirements
            .iter()
            .filter(|r| r["kind"] == "position")
            .all(|r| r["datums"] == json!(["A"]))
    );

    // A window's contact and area cannot displace a usable external axial stop.
    for f in a["faces"].as_array_mut().unwrap() {
        if f["id"] == 5 {
            f["contact"] = json!({"area": 1000, "part": "window insert"});
        }
    }
    assert_eq!(Ctx::new(&a).unwrap().default_datums().unwrap().1, [7, 11]);
    // If the only perpendicular face is a window end it can be used as a fallback.
    // Retain lateral faces and all feature references, changing unused axial planes to
    // unresolved surfaces to exercise the rule independently of recognition changes.
    for f in a["faces"].as_array_mut().unwrap() {
        if [3, 7, 9, 11, 13, 16].iter().any(|id| f["id"] == *id) {
            f["kind"] = json!("other");
        }
    }
    assert_eq!(Ctx::new(&a).unwrap().default_datums().unwrap().1, [5]);
    // Two recess walls alone do not identify a unique locating seat.
    a["faces"][3]["kind"] = json!("plane");
    assert!(Ctx::new(&a).unwrap().default_datums().unwrap().1.is_empty());
}
