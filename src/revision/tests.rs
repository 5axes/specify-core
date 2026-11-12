use super::*;
use serde_json::json;

fn report() -> Revised {
    let size = Size {
        volume: None,
        area: None,
        min: [0.0; 3],
        max: [1000.0; 3],
        faces: 0,
    };
    Revised {
        verdict: "ok".into(),
        summary: String::new(),
        answers: Map::new(),
        migrated: vec![],
        unresolved: vec![],
        check: vec![],
        features: vec![],
        measures: Measures {
            old: size.clone(),
            new: size,
        },
        pmi: Pmi::default(),
        pmi_graphics: vec![],
        check_lines: vec![],
        questions: vec![],
        problems: Problems::default(),
        faces: BTreeMap::new(),
        gone: vec![],
        correspondence: Some(Correspondence {
            fingerprint_version: String::new(),
            alignment: qc::Alignment {
                found: true,
                rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                translation: [0.; 3],
                anchors: 0,
                inliers: 0,
                symmetric: false,
                axisymmetric: false,
                axis: None,
                fold: None,
                rms: 0.,
            },
            features: vec![],
            new_features: vec![],
            faces: vec![],
            new_faces: vec![],
            questions: vec![],
        }),
        still: true,
    }
}

fn motion(rotation: [[f64; 3]; 3], translation: [f64; 3]) -> Option<String> {
    let mut r = report();
    let a = &mut r.correspondence.as_mut().unwrap().alignment;
    a.rotation = rotation;
    a.translation = translation;
    r.motion()
}

#[test]
fn quarter_turn_about_z() {
    assert_eq!(
        motion([[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]], [0.; 3]).as_deref(),
        Some("turned 90° about Z")
    );
}
#[test]
fn half_turn_about_z() {
    assert_eq!(
        motion([[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]], [0.; 3]).as_deref(),
        Some("turned 180° about Z")
    );
}
#[test]
fn half_turn_about_x() {
    assert_eq!(
        motion([[1., 0., 0.], [0., -1., 0.], [0., 0., -1.]], [0.; 3]).as_deref(),
        Some("turned 180° about X")
    );
}
#[test]
fn half_turn_about_a_skew_axis() {
    // n = (1,1,0)/sqrt(2): R = 2nnᵀ - I.
    assert_eq!(
        motion([[0., 1., 0.], [1., 0., 0.], [0., 0., -1.]], [0.; 3]).as_deref(),
        Some("turned 180° about a skew axis")
    );
}
#[test]
fn near_half_turn_about_y() {
    let angle = std::f64::consts::PI - 1e-8;
    let (s, c) = angle.sin_cos();
    assert_eq!(
        motion([[c, 0., s], [0., 1., 0.], [-s, 0., c]], [0.; 3]).as_deref(),
        Some("turned 180° about Y")
    );
}
#[test]
fn pure_translation() {
    assert_eq!(
        motion([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]], [8., 15., 3.]).as_deref(),
        Some("moved X +8, Y +15, Z +3")
    );
}

fn feature(family: &str, id: &str, record: Value, faces: Vec<usize>) -> Feature {
    Feature {
        family: family.into(),
        id: id.into(),
        record: record.as_object().unwrap().clone(),
        faces,
        members: vec![],
        notes: vec![],
    }
}

#[test]
fn hole_finishes_include_their_sizes_in_holes_and_patterns() {
    for (finish, words) in [
        (
            json!({"cbore":{"diameter":12.,"depth":3.}}),
            "counterbored Ø12 × 3 deep",
        ),
        (
            json!({"spotface":{"diameter":10.,"depth":1.}}),
            "spotfaced Ø10 × 1 deep",
        ),
        (
            json!({"csink":{"major_diameter":12.,"included_angle":90.,"depth":3.}}),
            "countersunk Ø12 × 90°, 3 deep",
        ),
        (
            json!({"cbore":{"diameter":12.,"depth":3.},"outer_steps":[{"diameter":18.,"depth":2.},{"diameter":15.,"depth":1.}]}),
            "counterbored Ø12 × 3 deep, counterbored Ø18 × 2 deep, counterbored Ø15 × 1 deep",
        ),
    ] {
        let mut h = json!({"diameter":6.,"depth":7.,"bottom":"through"});
        h.as_object_mut()
            .unwrap()
            .extend(finish.as_object().unwrap().clone());
        assert_eq!(
            describe(&feature("holes", "holes:1", h.clone(), vec![1])).0,
            format!("Ø6 through hole, {words}")
        );
        let pattern = feature(
            "hole_patterns",
            "hole_patterns.bolt_circle:1.2",
            json!({"diameter":60.,"holes":[h.clone(),h]}),
            vec![1, 2],
        );
        assert_eq!(
            describe(&pattern).0,
            format!("pattern of 2 × Ø6 through holes, {words} on a Ø60 circle")
        );
    }
}

fn change(f: &Feature, class: &str) -> FeatureChange {
    let (what, at) = describe(f);
    FeatureChange {
        class: class.into(),
        family: f.family.clone(),
        old: (class == "orphaned").then(|| f.id.clone()),
        new: (class == "new").then(|| f.id.clone()),
        candidates: vec![],
        changes: BTreeMap::new(),
        what,
        became: String::new(),
        at,
        place: Place::default(),
        axis: along(&f.record).map(|(k, _, _)| k),
        there: vec![],
        new_faces: vec![],
        old_faces: vec![],
    }
}

#[test]
fn small_holes_on_a_large_part_only_fold_in_the_same_place() {
    // Positions on a metre-scale part, 10 mm apart: the former box-scaled tolerance
    // folded these Ø6 holes. Folding no longer receives the part's bounds at all.
    let old = feature(
        "holes",
        "holes:1",
        json!({"diameter":6.,"depth":10.,"bottom":"through", "axis":[0.,0.,1.],"location":[520.,520.,0.]}),
        vec![1],
    );
    let mut new = feature(
        "holes",
        "holes:2",
        json!({"diameter":6.,"depth":7.,"bottom":"through", "cbore":{"diameter":12.,"depth":3.}, "axis":[0.,0.,1.],"location":[530.,520.,3.]}),
        vec![2],
    );
    let mut changes = vec![change(&old, "orphaned"), change(&new, "new")];
    fold(
        &mut changes,
        std::slice::from_ref(&old),
        std::slice::from_ref(&new),
        &Part::new(vec![], vec![], vec![]),
        &Part::new(vec![], vec![], vec![]),
    );
    assert_eq!(
        changes.iter().map(|c| c.class.as_str()).collect::<Vec<_>>(),
        ["orphaned", "new"]
    );
    // The mouth can move along the bore axis; rounding by 0.2 mm across it still pairs.
    new.record
        .insert("location".into(), json!([520.2, 520., 3.]));
    let mut changes = vec![change(&old, "orphaned"), change(&new, "new")];
    fold(
        &mut changes,
        &[old],
        &[new],
        &Part::new(vec![], vec![], vec![]),
        &Part::new(vec![], vec![], vec![]),
    );
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].class, "replaced");
}

#[test]
fn distant_small_patterns_on_a_large_part_do_not_fold() {
    let old = feature(
        "hole_patterns",
        "hole_patterns.bolt_circle:1.2",
        json!({"diameter":12.,"center":[520.,520.,0.]}),
        vec![1, 2],
    );
    let new = feature(
        "hole_patterns",
        "hole_patterns.bolt_circle:3.4",
        json!({"diameter":12.,"center":[530.,520.,0.]}),
        vec![3, 4],
    );
    let mut changes = vec![change(&old, "orphaned"), change(&new, "new")];
    fold(
        &mut changes,
        &[old],
        &[new],
        &Part::new(vec![], vec![], vec![]),
        &Part::new(vec![], vec![], vec![]),
    );
    assert_eq!(
        changes.iter().map(|c| c.class.as_str()).collect::<Vec<_>>(),
        ["orphaned", "new"]
    );
}

#[test]
fn chamfer_continuation_requires_carried_faces_matching_sizes_and_tangent_fillet_connections() {
    let bevel = |id: &str, face, leg| {
        feature(
            "chamfers",
            id,
            json!({"leg1":leg,"leg2":leg,"angle":45.}),
            vec![face],
        )
    };
    let old = bevel("chamfers:1", 1, 1.);
    let anchor = bevel("chamfers:10", 10, 1.);
    let corner1 = bevel("chamfers:11", 11, 1.);
    let bridge = bevel("chamfers:12", 12, 1.);
    let corner2 = bevel("chamfers:13", 13, 1.);
    let separate = bevel("chamfers:30", 30, 1.);
    let resized = bevel("chamfers:31", 31, 2.);
    let sharp_join = bevel("chamfers:32", 32, 1.);
    let fillet = feature("fillets", "fillets:20", json!({"radius":5.}), vec![20]);
    let new = vec![
        anchor, corner1, bridge, corner2, separate, resized, sharp_join, fillet,
    ];
    let mut corr = report().correspondence.unwrap();
    // quiddity's correspondence records are non-exhaustive: build them as stored.
    corr.faces.push(
        serde_json::from_value(json!({"old": 1, "class": Class::Carried, "new": 10})).unwrap(),
    );
    let face = |index, ns: &[(usize, &str)]| -> qc::FaceFingerprint {
        serde_json::from_value(json!({
            "index": index,
            "surface": "plane",
            "parameters": {},
            "axis": null,
            "support": null,
            "area": null,
            "centroid": [0., 0., 0.],
            "normal": null,
            "adjacency": [],
            "neighbours": ns,
        }))
        .unwrap()
    };
    let mut prints = qc::Fingerprints {
        fingerprint_version: String::new(),
        scale: 1000.,
        features: vec![],
        faces: vec![
            face(10, &[(11, "smooth"), (31, "smooth"), (32, "convex")]),
            face(11, &[(10, "smooth"), (12, "smooth"), (20, "convex")]),
            face(12, &[(11, "smooth"), (13, "smooth")]),
            face(13, &[(12, "smooth"), (20, "convex")]),
            face(30, &[(20, "convex")]),
            face(31, &[(10, "smooth"), (20, "convex")]),
            face(32, &[(10, "convex"), (20, "convex")]),
        ],
    };
    let original: Vec<_> = new.iter().skip(1).map(|f| change(f, "new")).collect();
    let mut changes = original.clone();
    continue_chamfers(
        &mut changes,
        std::slice::from_ref(&old),
        &new,
        &corr,
        &prints,
    );
    let carried: Vec<_> = changes.iter().filter(|c| c.class == "continued").collect();
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].old.as_deref(), Some("chamfers:1"));
    assert_eq!(
        carried[0].candidates,
        ["chamfers:10", "chamfers:11", "chamfers:12", "chamfers:13"]
    );
    for id in ["chamfers:30", "chamfers:31", "chamfers:32"] {
        assert!(
            changes
                .iter()
                .any(|c| c.class == "new" && c.new.as_deref() == Some(id)),
            "{id}"
        );
    }
    // Mere proximity to old chamfers, without fillet borders, is not continuation.
    let with_fillet_borders = prints.clone();
    for f in &mut prints.faces {
        f.neighbours.retain(|(i, _)| *i != 20);
    }
    let mut changes = original.clone();
    continue_chamfers(
        &mut changes,
        std::slice::from_ref(&old),
        &new,
        &corr,
        &prints,
    );
    assert_eq!(
        changes
            .iter()
            .filter(|c| c.class == "new" && c.family == "chamfers")
            .count(),
        6
    );
    assert!(changes.iter().all(|c| c.class != "continued"));
    // No definite old-to-new face correspondence means no claim about continuation.
    prints = with_fillet_borders;
    corr.faces[0].class = Class::Ambiguous;
    let mut changes = original;
    continue_chamfers(&mut changes, &[old], &new, &corr, &prints);
    assert!(changes.iter().all(|c| c.class != "continued"));
}

#[test]
fn schema6_additions_and_inside_rounds_have_plain_report_descriptions() {
    for (family, record, expected) in [
        (
            "face_grooves",
            json!({"outer_diameter":18.5,"inner_diameter":13.,"depth":0.25,"outer_wall_angle":45.,"inner_wall_angle":45.}),
            "face groove Ø18.5/Ø13 × 0.25 deep, 45° walls",
        ),
        (
            "face_grooves",
            json!({"outer_diameter":18.5,"inner_diameter":13.,"depth":0.25,"outer_wall_angle":45.,"inner_wall_angle":90.}),
            "face groove Ø18.5/Ø13 × 0.25 deep, 45° outer / 90° inner walls",
        ),
        (
            "cylindrical_profiles",
            json!({"radius":203.2,"side":"convex"}),
            "R203.2 convex profile surface",
        ),
        (
            "cylindrical_profiles",
            json!({"radius":203.2,"side":"concave"}),
            "R203.2 concave profile surface",
        ),
        (
            "fillets",
            json!({"radius":6.35,"side":"concave"}),
            "R6.35 concave fillet",
        ),
        (
            "fillets",
            json!({"radius":6.35,"side":"concave","turned":true}),
            "R6.35 concave fillet",
        ),
        (
            "fillets",
            json!({"radius":6.35,"side":"convex"}),
            "R6.35 fillet",
        ),
        (
            "turned_bodies",
            json!({"od":22.,"length":7.}),
            "Ø22 × 7 turned body",
        ),
    ] {
        let f = feature(family, &format!("{family}:1"), record, vec![1]);
        assert_eq!(describe(&f).0, expected);
        let mut r = report();
        let mut c = change(&f, "new");
        c.what = describe(&f).0;
        r.features.push(c);
        let md = r.markdown("test", None);
        assert!(md.contains(expected), "{md}");
    }
    assert_eq!(plural_of(2, &singular("turned_bodies")), "2 turned bodies");
}

#[test]
fn radius_pmi_on_inside_rounds_keeps_radius_and_material_side() {
    let f = feature(
        "fillets",
        "fillets:1.2",
        json!({"radius":6.35,"side":"concave"}),
        vec![1, 2],
    );
    // A fillet may include both its cylindrical wall and a spherical corner cap (#127).
    for support in [vec![1], vec![2], vec![1, 2]] {
        assert_eq!(
            pmi_on(&support, std::slice::from_ref(&f)),
            "R6.35 concave fillet"
        );
    }
    assert_eq!(
        pmi_words(
            &json!({"kind":"dimension","type":"Size_Radius","value":6.35,"upper":0.05,"lower":-0.05})
        ),
        "R6.35 ±0.05"
    );
    assert_eq!(
        pmi_words(&json!({"kind":"dimension","type":"Size_SphericalRadius","value":6.35})),
        "SR6.35"
    );
}

#[test]
fn coowning_body_fingerprints_never_alias_individual_features() {
    // Identical support must not turn a body into a groove (or duplicate its change).
    let fs = vec![
        feature("face_grooves", "face_grooves:1.2", json!({}), vec![1, 2]),
        feature("turned_bodies", "turned_bodies:1.2", json!({}), vec![1, 2]),
    ];
    let prints = qc::Fingerprints {
        fingerprint_version: String::new(),
        scale: 1.,
        faces: vec![],
        features: ["face_grooves", "turned_bodies"]
            .into_iter()
            .map(|family| {
                serde_json::from_value(json!({
                    "id": format!("{family}/0"),
                    "family": family,
                    "traits": {},
                    "sizes": {},
                    "axis": null,
                    "position": null,
                    "faces": [1, 2],
                    "neighbourhood": [],
                }))
                .unwrap()
            })
            .collect(),
    };
    let mapped = records(&fs, &prints);
    assert_eq!(mapped.len(), 2);
    assert_eq!(mapped["face_grooves/0"], "face_grooves:1.2");
    assert_eq!(mapped["turned_bodies/0"], "turned_bodies:1.2");
}

#[test]
fn repeated_face_grooves_pluralise_the_feature_not_its_measurements() {
    assert_eq!(
        several("face groove Ø18.5/Ø13 × 0.25 deep, 45° walls"),
        "face grooves Ø18.5/Ø13 × 0.25 deep, 45° walls"
    );
}

#[test]
fn profile_surfaces_are_not_located_as_rounds_on_a_boss() {
    let profile = feature(
        "cylindrical_profiles",
        "cylindrical_profiles:1",
        json!({"radius":203.2,"axis":"x","axis_point":[0.,5.,0.],"side":"convex"}),
        vec![1],
    );
    let boss = feature(
        "bosses",
        "bosses:2",
        json!({"diameter":10.,"height":4.,"axis":[1.,0.,0.],"location":[0.,0.,0.]}),
        vec![2],
    );
    let size = report().measures.old;
    assert!(
        locate(&profile, std::slice::from_ref(&boss), &size)
            .on
            .is_empty()
    );
    let round = feature(
        "fillets",
        "fillets:1",
        json!({"radius":1.,"axis":"x","at":[0.,5.,0.],"side":"concave"}),
        vec![1],
    );
    assert_eq!(locate(&round, &[boss], &size).on, "round the Ø10 boss");
}

/// `refuse_unfit` with a review that refuses by these rules, on *carried*.
fn refusing(
    carried: Value,
    rule: impl Fn(&Map<String, Value>) -> std::result::Result<(), crate::rules::Error>,
) -> (
    std::result::Result<Value, String>,
    Map<String, Value>,
    Vec<String>,
) {
    let mut carried = carried.as_object().unwrap().clone();
    let mut unresolved = Vec::new();
    let outcome = refuse_unfit(&mut carried, &BTreeMap::new(), &mut unresolved, |a| {
        Ok(rule(a).map(|()| json!({"problems": []})))
    })
    .unwrap();
    let ids = unresolved.into_iter().map(|u| u.id).collect();
    (outcome, carried, ids)
}

fn invalid(message: &str) -> std::result::Result<(), crate::rules::Error> {
    Err(crate::rules::Error::Invalid(message.into()))
}

// A refusal that names no answer: removing a sound answer changes it too (a required answer
// missing, another refusal), which is no evidence against that answer.
#[test]
fn an_unnamed_refusal_is_pinned_only_on_the_answer_whose_removal_clears_it() {
    let (outcome, carried, refused) = refusing(
        json!({"datum.A": [4], "extra.bad": {"faces": [7]}, "part.material": "6061-T6"}),
        |a| {
            if !a.contains_key("part.material") {
                Err(crate::rules::Error::Incomplete(
                    crate::rules::IncompleteError::new(vec!["part.material".into()], None),
                ))
            } else if !a.contains_key("datum.A") {
                invalid("the frame has no primary datum")
            } else if a.contains_key("extra.bad") {
                invalid("could not convert string to float: 'X'")
            } else {
                Ok(())
            }
        },
    );
    assert!(outcome.is_ok());
    assert_eq!(refused, ["extra.bad"]);
    assert_eq!(carried.len(), 2);
}

// Removing one answer leaves a refusal naming another carried answer: both are refused.
#[test]
fn an_unnamed_refusal_narrowed_to_a_named_one_refuses_both() {
    let (outcome, carried, refused) = refusing(
        json!({"a.x": 1, "extra.y": {"faces": [7]}, "part.material": "6061-T6"}),
        |a| {
            if a.contains_key("a.x") {
                invalid("could not convert string to float: 'X'")
            } else if a.contains_key("extra.y") {
                invalid("extra.y: a face picked does not exist")
            } else {
                Ok(())
            }
        },
    );
    assert!(outcome.is_ok());
    assert_eq!(refused, ["a.x", "extra.y"]);
    assert_eq!(carried.keys().collect::<Vec<_>>(), ["part.material"]);
}

// No single answer explains the refusal: nothing is guessed or moved; it is returned.
#[test]
fn an_unnamed_refusal_no_answer_explains_is_returned_not_guessed() {
    let (outcome, carried, refused) = refusing(
        json!({"datum.A": [4], "extra.one": {"faces": [7]}, "extra.two": {"faces": [8]}}),
        |a| {
            if a.contains_key("extra.one") || a.contains_key("extra.two") {
                invalid("the pair cannot both apply")
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(outcome.unwrap_err(), "the pair cannot both apply");
    assert!(refused.is_empty());
    assert_eq!(carried.len(), 3);
}

#[test]
fn pocket_section_fold_requires_exact_walls_and_one_adjacent_planar_floor() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/revisions/complex_many.old.step.gz");
    let loaded = crate::part::load(&path, None).unwrap();
    let features = crate::features::recognise(&loaded).unwrap();
    let mut pocket = features
        .iter()
        .find(|f| f.family == "pockets")
        .unwrap()
        .clone();
    // Model both pre- and post-#216 supports without depending on the kernel pin.
    let walls: Vec<_> = pocket
        .faces
        .iter()
        .copied()
        .filter(|f| {
            loaded
                .part
                .neighbours(*f)
                .iter()
                .filter(|n| pocket.faces.contains(n))
                .count()
                < pocket.faces.len() - 1
        })
        .collect();
    let walls = if walls.is_empty() {
        pocket.faces.clone()
    } else {
        walls
    };
    let floor = (0..loaded.part.faces.len())
        .find(|f| {
            !walls.contains(f)
                && matches!(
                    loaded.part.faces[*f].surface,
                    haecceity::geom::Surface::Plane { .. }
                )
                && walls.iter().all(|w| loaded.part.neighbours(*f).contains(w))
        })
        .unwrap();
    pocket.faces = walls.clone();
    pocket.faces.push(floor);
    let mut section = pocket.clone();
    section.family = "section_recesses".into();
    section.faces = walls;
    assert!(pocket_section_twins(&pocket, &section, &loaded.part));
    assert!(pocket_section_twins(&section, &pocket, &loaded.part));
    pocket.faces.sort_unstable();
    pocket.id = format!(
        "pockets:{}",
        pocket
            .faces
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(".")
    );
    section.id = format!(
        "section_recesses:{}",
        section
            .faces
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(".")
    );
    let fs = [pocket.clone(), section.clone()];
    let mut changes: Vec<_> = fs
        .iter()
        .map(|f| {
            let mut c = change(f, "adapted");
            c.old = Some(f.id.clone());
            c.new = Some(f.id.clone());
            c
        })
        .collect();
    changes[0].what = "pocket".into();
    changes[1].what = "section recess".into();
    fold(&mut changes, &fs, &fs, &loaded.part, &loaded.part);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].what, "section recess");
    assert_eq!(changes[0].old.as_deref(), Some(pocket.id.as_str()));
    assert_eq!(changes[0].new.as_deref(), Some(pocket.id.as_str()));

    let mut other = section.clone();
    other.family = "slots".into();
    assert!(!pocket_section_twins(&pocket, &other, &loaded.part));
    other = section.clone();
    other.faces.pop();
    assert!(!pocket_section_twins(&pocket, &other, &loaded.part));
    other = section.clone();
    other.faces.push(floor);
    assert!(!pocket_section_twins(&pocket, &other, &loaded.part));
}
