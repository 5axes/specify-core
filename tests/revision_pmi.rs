//! Requirement-level regression pairs; every public identity comes from pmiScene.
use serde_json::{Map, Value, json};
use specify_core::{api, part, pmi_scene, revision, writer};
use std::path::Path;

fn load(name: &str) -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        None,
    )
    .unwrap()
}
fn answers(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}
fn write(part: &part::LoadedPart, answers: &Map<String, Value>) -> part::LoadedPart {
    let analysis = api::analyse(part).unwrap();
    let bytes = writer::write_bytes(
        part,
        &api::preview(&analysis, answers).unwrap(),
        Some(answers),
    )
    .unwrap()
    .0;
    part::load_bytes("revision.step", bytes).unwrap()
}
/// The report's numbered items are the new file's: each number is one the file's pmiScene
/// states, with the same review status. (Diff reads the file's PMI only, so its ids are the
/// *part* written with every default, as `write --accept-defaults` writes a bare file.
fn with_defaults(part: &part::LoadedPart) -> part::LoadedPart {
    let analysis = api::analyse(part).unwrap();
    write(
        part,
        &specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap(),
    )
}

/// file's own, not the ids a resumed interview gives: ADR P11.)
fn assert_scene_ids(report: &revision::Revised, part: &part::LoadedPart) {
    let scene = pmi_scene::pmi_scene(&part.bytes, None).unwrap();
    for item in &report.pmi.items {
        if item.class == "removed" {
            continue;
        }
        let Some(number) = item.number else {
            continue;
        };
        assert!(
            scene["specify"]
                .as_object()
                .unwrap()
                .values()
                .any(|m| m["number"].as_u64() == Some(number)),
            "#{number} ({}) is not in the file",
            item.id
        );
    }
}

#[test]
fn a_fit_answer_changes_with_its_characteristic_number_and_scene_id() {
    let bare = load("parts/plate.step");
    let before = answers(json!({"hole.function:holes:9":"fit:H7"}));
    let old = write(&bare, &before);
    let after = answers(json!({"hole.function:holes:9":"fit:H8"}));
    let new = write(&old, &after);
    let report = revision::diff(&old, &new).unwrap();
    assert_scene_ids(&report, &new);
    let change = report
        .pmi
        .items
        .iter()
        .find(|c| c.kind == "dimension" && c.class == "changed")
        .unwrap();
    assert!(change.number.is_some());
    // A fit with stated limits prints the limits; its class is still compared semantically.
    assert!(change.old != change.new || change.reason.is_some());
    assert!(!report.pmi.items.iter().any(|c| c.class == "renumbered"));
    println!("FIT SAMPLE\n{}", report.markdown("Fit", None));
}

#[test]
fn a_moved_holes_general_location_changes_without_a_stored_location_answer() {
    let before = load("revisions/hole_moved.old.step.gz");
    let after = load("revisions/hole_moved.new.step.gz");
    let answers = answers(
        json!({"part.general_tolerance":"ISO 2768-m", "part.locating":"position tolerances"}),
    );
    // Each revision written with the same answers: diff compares the two files' PMI.
    let (old, new) = (write(&before, &answers), write(&after, &answers));
    let report = revision::diff(&old, &new).unwrap();
    let changes: Vec<_> = report
        .pmi
        .items
        .iter()
        .filter(|c| c.class == "changed" && c.kind == "dimension")
        .collect();
    assert!(!changes.is_empty(), "{:?}", report.pmi.items);
    assert!(changes.iter().any(|c| c.status == "general"), "{changes:?}");
    println!("MOVE SAMPLE\n{}", report.markdown("Moved hole", None));
}

#[test]
fn added_and_removed_requirements_keep_inspection_history() {
    let bare = load("parts/plate.step");
    let before = answers(json!({"extra.1":{"kind":"flatness", "faces":[4], "value":"0.05"}}));
    let old = write(&bare, &before);
    let after = answers(json!({"extra.2":{"kind":"flatness", "faces":[5], "value":"0.02"}}));
    let new = write(&old, &after);
    let report = revision::diff(&old, &new).unwrap();
    assert_scene_ids(&report, &new);
    assert!(
        report.pmi.items.iter().any(|c| c.class == "removed"
            && c.number.is_some()
            && c.reason.as_deref() == Some("retired")),
        "{:?}",
        report.pmi.items
    );
    assert!(
        report
            .pmi
            .items
            .iter()
            .any(|c| c.class == "added" && c.number.is_some())
    );
    assert!(report.markdown("Controls", None).contains(" retired:"));
}

#[test]
fn a_note_change_and_material_modifier_change_are_visible() {
    let bare = load("parts/plate.step");
    let supplied = answers(
        json!({"part.notes":["Keep dry"], "part.locating":"position tolerances", "hole.function:holes:9":"clearance:M6"}),
    );
    let supplied =
        specify_core::rules::with_all_defaults(&api::analyse(&bare).unwrap(), &supplied).unwrap();
    let old = write(&bare, &supplied);
    let text = String::from_utf8(old.bytes.to_vec()).unwrap();
    assert!(text.contains(".MAXIMUM_MATERIAL_REQUIREMENT."));
    let changed = text
        .replace(
            ".MAXIMUM_MATERIAL_REQUIREMENT.",
            ".LEAST_MATERIAL_REQUIREMENT.",
        )
        .replace("Keep dry", "Keep clean");
    let new = part::load_bytes("changed.step", changed.into_bytes()).unwrap();
    let report = revision::diff(&old, &new).unwrap();
    assert_scene_ids(&report, &new);
    assert!(
        report.pmi.items.iter().any(|c| c.class == "changed"
            && c.old.as_ref().is_some_and(|t| t.contains('Ⓜ'))
            && c.new.as_ref().is_some_and(|t| t.contains('Ⓛ'))),
        "{:?}",
        report.pmi.items
    );
    let controls: Vec<_> = report
        .pmi
        .items
        .iter()
        .filter(|c| c.kind == "geometric_tolerance")
        .collect();
    assert_eq!(controls.len(), 2, "{controls:?}");
    assert!(
        controls
            .iter()
            .all(|c| c.class == "changed" && c.number.is_some())
    );
    assert!(!report.pmi.items.iter().any(|c| c.class == "renumbered"));
    assert_eq!(
        specify_core::characteristics::read(&old).unwrap(),
        specify_core::characteristics::read(&new).unwrap()
    );
    let numbers: Vec<u64> = specify_core::characteristics::read(&old)
        .unwrap()
        .iter()
        .map(|c| c.number)
        .collect();
    for control in &controls {
        assert!(numbers.contains(&control.number.unwrap()), "{control:?}");
    }
    // The file's note, read as the file states it (ADR P11: not by a stored answer's id).
    let note = report
        .pmi
        .items
        .iter()
        .find(|c| c.kind == "note" && c.old.as_deref() == Some("Keep dry"))
        .unwrap();
    assert_eq!(note.class, "changed");
    assert_eq!(note.old.as_deref(), Some("Keep dry"));
    assert_eq!(note.new.as_deref(), Some("Keep clean"));
    assert!(
        report
            .markdown("Notes and modifiers", None)
            .contains("Keep dry → Keep clean")
    );
    // Nothing else changed: not the finish, the edges or the general tolerance.
    assert_eq!(
        report.pmi.items.len(),
        controls.len() + 1,
        "{:?}",
        report.pmi.items
    );
}

#[test]
fn changing_one_shared_note_leaves_every_other_requirement_unchanged() {
    let bare = load("parts/plate.step");
    let supplied = answers(json!({"part.notes":["Keep dry", "Do not stack"]}));
    let supplied =
        specify_core::rules::with_all_defaults(&api::analyse(&bare).unwrap(), &supplied).unwrap();
    let old = write(&bare, &supplied);
    let changed = String::from_utf8(old.bytes.to_vec())
        .unwrap()
        .replace("Keep dry", "Keep clean");
    let new = part::load_bytes("note-changed.step", changed.into_bytes()).unwrap();
    let report = revision::diff(&old, &new).unwrap();
    assert_scene_ids(&report, &new);
    assert_eq!(report.pmi.items.len(), 1, "{:?}", report.pmi.items);
    let note = &report.pmi.items[0];
    assert_eq!(note.kind, "note");
    assert_eq!(note.class, "changed");
    assert_eq!(note.old.as_deref(), Some("Keep dry"));
    assert_eq!(note.new.as_deref(), Some("Keep clean"));
    let markdown = report.markdown("One note", None);
    assert!(
        markdown
            .lines()
            .any(|line| line.starts_with("- ") && line.contains("Keep dry → Keep clean"))
    );
    assert!(!markdown.contains("General notes → General notes"));
    assert_eq!(
        specify_core::characteristics::read(&old).unwrap(),
        specify_core::characteristics::read(&new).unwrap()
    );
}

#[test]
fn foreign_pmi_changes_even_in_a_file_with_saved_specify_answers() {
    let bare = load("parts/plate.step");
    let own = write(&bare, &answers(json!({"part.notes":["Keep dry"]})));
    let old = foreign_flatness(&own, &[4], "0.2");
    let scene = pmi_scene::pmi_scene(&old.bytes, None).unwrap();
    assert!(
        scene["specify"]
            .as_object()
            .unwrap()
            .values()
            .any(|m| m["status"] == "existing")
    );
    let analysis = api::analyse(&old).unwrap();
    assert!(analysis["resume"].is_object());
    assert_eq!(
        analysis["existing"],
        json!([]),
        "legacy analysis stays unchanged"
    );
    let text = String::from_utf8(old.bytes.to_vec()).unwrap();
    let changed = text.replace("LENGTH_MEASURE(0.2)", "LENGTH_MEASURE(0.07)");
    assert_ne!(text, changed);
    let new = part::load_bytes("foreign-changed.step", changed.into_bytes()).unwrap();
    let report = revision::diff(&old, &new).unwrap();
    assert!(
        report
            .pmi
            .items
            .iter()
            .any(|c| c.class == "changed" && c.status == "existing"),
        "{:?}",
        report.pmi.items
    );
    assert_scene_ids(&report, &new);
}

#[test]
fn unchanged_complex_and_resume_pairs_have_no_pmi_changes() {
    for name in [
        "revisions/complex_remodelled.old.step.gz",
        "resume-alpha6/plate.step.gz",
        "resume_alpha7/golden_oriented_slots.step.gz",
    ] {
        let part = load(name);
        let report = revision::diff(&part, &part).unwrap();
        assert!(
            report.pmi.items.is_empty(),
            "{name}: {:?}",
            report.pmi.items
        );
    }
}

fn foreign_flatness(part: &part::LoadedPart, faces: &[usize], value: &str) -> part::LoadedPart {
    use haecceity::pmi::*;
    let model = PartPmi {
        features: vec![Feature::Items(
            faces
                .iter()
                .copied()
                .map(|n| Anchor::Face(FaceIndex(n)))
                .collect(),
        )],
        tolerances: vec![GeometricTolerance {
            kind: ToleranceKind::Flatness,
            target: ToleranceTarget::Feature(FeatureId(0)),
            magnitude: Some(Value::length(
                Decimal::parse(value).unwrap(),
                LengthUnit::Millimetre,
            )),
            zone: None,
            modifiers: vec![],
            unit_basis: None,
            maximum: None,
            unequal: None,
            datums: None,
            auxiliary: vec![],
            description: None,
        }],
        ..PartPmi::default()
    };
    foreign_model(part, model)
}

fn foreign_model(part: &part::LoadedPart, model: haecceity::pmi::PartPmi) -> part::LoadedPart {
    let doc = haecceity::p21::Document::parse(part.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&part.bytes).unwrap();
    let (edit, _) = haecceity::pmi::write::write(
        &doc,
        &defs,
        &[(haecceity::pmi::PartId(0), model)],
        haecceity::pmi::write::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    part::load_bytes("foreign.step", doc.apply(&edit).unwrap().bytes).unwrap()
}

#[test]
fn a_face_split_matches_the_foreign_control_through_correspondence() {
    use quiddity::correspondence as qc;
    let before = load("revisions/pmi_split.old.step.gz");
    let after = load("revisions/pmi_split.new.step.gz");
    let old_q = qc::recognise(&before.part);
    let new_q = qc::recognise(&after.part);
    let correspondence = qc::correspond(&old_q.fingerprints, &new_q.fingerprints).unwrap();
    assert!(after.part.faces.len() > before.part.faces.len());
    // This quiddity version leaves these true split faces orphaned. The unique
    // remaining kind fallback must still pair the control; no number/id can help.
    let split = correspondence
        .faces
        .iter()
        .find(|f| f.class == qc::Class::Orphaned)
        .unwrap();
    let old_normal = old_q.fingerprints.faces[split.old].normal.unwrap();
    let candidates: Vec<_> = new_q
        .fingerprints
        .faces
        .iter()
        .filter(|f| {
            f.normal
                .is_some_and(|n| haecceity::geom::dist(n, old_normal) < 1e-6)
        })
        .map(|f| f.index)
        .collect();
    assert!(candidates.len() > 1);
    let old = foreign_flatness(&before, &[split.old], "0.2");
    let new = foreign_flatness(&after, &candidates, "0.1");
    let report = revision::diff(&old, &new).unwrap();
    assert_eq!(report.pmi.items.len(), 1, "{:?}", report.pmi.items);
    assert_eq!(report.pmi.items[0].class, "changed");
    assert_eq!(report.pmi.items[0].status, "existing");
    assert_scene_ids(&report, &new);
}

#[test]
fn a_fit_class_hidden_by_equal_stated_limits_is_still_changed() {
    use haecceity::pmi::*;
    let bare = load("parts/plate.step");
    let length = |s| Value::length(Decimal::parse(s).unwrap(), LengthUnit::Millimetre);
    let model = |grade| PartPmi {
        features: vec![Feature::Items(vec![Anchor::Face(FaceIndex(9))])],
        dimensions: vec![Dimension {
            kind: DimensionKind::Size {
                feature: FeatureId(0),
                kind: SizeKind::Diameter,
                path: None,
                angle: None,
            },
            nominal: Some(length("8.0")),
            tolerance: DimTolerance::Fit {
                class: Iso286Class {
                    deviation: FundamentalDeviation::parse("H").unwrap(),
                    grade: ToleranceGrade::parse(grade).unwrap(),
                },
                limits: Some(Bounds::new(length("8.02"), length("8.0")).unwrap()),
            },
            qualifier: None,
            modifiers: vec![],
            principle: None,
        }],
        ..PartPmi::default()
    };
    let old = foreign_model(&bare, model("7"));
    let new = foreign_model(&bare, model("8"));
    let report = revision::diff(&old, &new).unwrap();
    assert_eq!(report.pmi.items.len(), 1, "{:?}", report.pmi.items);
    let change = &report.pmi.items[0];
    assert_eq!(change.class, "changed");
    assert_eq!(
        change.old, change.new,
        "equal limits deliberately hide the fit class"
    );
    assert_eq!(change.reason.as_deref(), Some("semantic fields changed"));
}

#[test]
fn moving_a_hole_recomputes_default_basic_locations() {
    let before = load("revisions/hole_moved.old.step.gz");
    let after = load("revisions/hole_moved.new.step.gz");
    let given = answers(
        json!({"part.general_tolerance":"ISO 2768-m", "part.locating":"position tolerances", "hole.function:holes:8":"clearance"}),
    );
    let all = |p: &part::LoadedPart| {
        specify_core::rules::with_all_defaults(&api::analyse(p).unwrap(), &given).unwrap()
    };
    // Each revision written with the same answers and its defaults.
    let old = write(&before, &all(&before));
    let new = write(&after, &all(&after));
    let scene = pmi_scene::pmi_scene(&old.bytes, None).unwrap();
    let basic: Vec<_> = scene["pmi"]["pmi"]["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["value"]["tolerance"] == "basic")
        .collect();
    assert!(!basic.is_empty());
    let report = revision::diff(&old, &new).unwrap();
    assert_scene_ids(&report, &new);
    let edits: Vec<_> = report
        .pmi
        .items
        .iter()
        .filter(|c| c.class != "renumbered")
        .collect();
    assert!(edits.iter().all(|c| c.kind == "dimension"), "{edits:?}");
    assert_eq!(
        edits.len(),
        1,
        "only the moved hole changes; planner frame metadata and rounding noise are not inspection changes (#75): {edits:?}"
    );
    assert!(
        edits[0].class == "changed" && edits[0].number.is_some(),
        "{edits:?}"
    );
}

// Captured from unmodified main d6f65a8, with schema-1 value-sensitive signatures.
// Do not regenerate with this branch: it exercises reopening real pre-fix history.
#[test]
fn main_history_survives_direct_modifier_and_value_edits() {
    let old = load("characteristic-numbers-main.step.gz");
    let mut supplied = api::analyse(&old).unwrap()["resume"]["answers"]
        .as_object()
        .unwrap()
        .clone();
    // Force a rewrite. #75 retires old generated member dimensions; every retained
    // requirement must still keep its number while history migrates.
    supplied.insert("part.notes".into(), json!(["Keep dry", "Migration test"]));
    let migrated = write(&old, &supplied);
    let numbers = |p: &part::LoadedPart| {
        specify_core::characteristics::read(p)
            .unwrap()
            .into_iter()
            .map(|c| (c.number, c.kind, c.r#type, c.faces, c.reference))
            .collect::<Vec<_>>()
    };
    let prior = numbers(&old);
    let current = numbers(&migrated);
    let mut retained = 0;
    for (number, kind, ty, faces, reference) in &prior {
        if let Some(now) = current
            .iter()
            .find(|(_, k, t, f, r)| k == kind && t == ty && f == faces && r == reference)
        {
            assert_eq!(*number, now.0);
            retained += 1;
        }
    }
    assert!(
        retained >= 12,
        "real retained main requirements: {retained}"
    );
    let doc = haecceity::p21::Document::parse(migrated.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&migrated.bytes).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let history = read.parts[0]
        .attributes
        .iter()
        .find(|a| a.name == specify_core::characteristics::PROPERTY)
        .unwrap();
    let haecceity::pmi::AttributeValue::Text(text) = &history.items[0].1 else {
        panic!("history text")
    };
    let history: Value = serde_json::from_str(text).unwrap();
    assert_eq!(history["schema"], 2);
    assert!(
        history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["key"].get("specification").is_none() && e["key"]["role"].is_string())
    );
    for old in [&old, &migrated] {
        let text = String::from_utf8(old.bytes.to_vec()).unwrap();
        for (from, to, count) in [
            (
                ".MAXIMUM_MATERIAL_REQUIREMENT.",
                ".LEAST_MATERIAL_REQUIREMENT.",
                2,
            ),
            ("LENGTH_MEASURE(0.2)", "LENGTH_MEASURE(0.3)", 1),
        ] {
            assert_eq!(text.matches(from).count(), count);
            let new = part::load_bytes("edited.step", text.replace(from, to).into_bytes()).unwrap();
            let report = revision::diff(old, &new).unwrap();
            assert_scene_ids(&report, &new);
            assert_eq!(report.pmi.items.len(), count, "{:?}", report.pmi.items);
            let numbers: Vec<u64> = specify_core::characteristics::read(old)
                .unwrap()
                .iter()
                .map(|c| c.number)
                .collect();
            for item in &report.pmi.items {
                assert_eq!(item.class, "changed");
                assert!(numbers.contains(&item.number.unwrap()), "{item:?}");
            }
            assert_eq!(
                specify_core::characteristics::read(old).unwrap(),
                specify_core::characteristics::read(&new).unwrap()
            );
        }
    }
}

#[test]
fn lm10uu_independent_lands_keep_numbers_through_compositions_resaves_and_a_value_edit() {
    use specify_core::characteristics;
    use std::collections::{BTreeMap, BTreeSet};
    let bare = load("freecad/bearing-lm10uu.step.gz");
    let supplied = specify_core::rules::with_all_defaults(
        &api::analyse(&bare).unwrap(),
        &answers(json!({"part.material":"6061-T6"})),
    )
    .unwrap();
    let old = write(&bare, &supplied);
    let lands = |p: &part::LoadedPart| -> BTreeMap<usize, u64> {
        let numbers = characteristics::read(p).unwrap();
        let datum = numbers
            .iter()
            .find(|c| c.kind == "datum" && c.r#type == "A")
            .unwrap();
        assert_eq!(datum.faces, [0, 4, 8]);
        assert_eq!(datum.number, 1);
        let result: BTreeMap<_, _> = numbers
            .iter()
            .filter(|c| {
                c.kind == "dimension"
                    && c.r#type == "Size_Diameter"
                    && [vec![0], vec![4], vec![8]].contains(&c.faces)
            })
            .map(|c| (c.faces[0], c.number))
            .collect();
        assert_eq!(result.len(), 3);
        assert_eq!(result.values().copied().collect::<BTreeSet<_>>().len(), 3);
        assert!(result.values().all(|n| *n != datum.number));
        let scene = pmi_scene::pmi_scene(&p.bytes, None).unwrap();
        for number in result.values() {
            assert_eq!(
                scene["specify"]
                    .as_object()
                    .unwrap()
                    .values()
                    .filter(|m| m["number"].as_u64() == Some(*number))
                    .count(),
                1
            );
        }
        result
    };
    let before = lands(&old);
    assert_eq!(before, BTreeMap::from([(0, 8), (4, 6), (8, 4)]));
    let repeated = write(&bare, &supplied);
    let rewritten = write(&old, &supplied);
    assert_eq!(lands(&repeated), before);
    assert_eq!(lands(&rewritten), before);
    assert_eq!(old.bytes, repeated.bytes);
    assert_eq!(old.bytes, rewritten.bytes);

    let doc = haecceity::p21::Document::parse(old.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&old.bytes).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let pmi = &read.parts[0];
    let history = pmi
        .attributes
        .iter()
        .find(|a| a.name == characteristics::PROPERTY)
        .unwrap();
    let haecceity::pmi::AttributeValue::Text(text) = &history.items[0].1 else {
        panic!("history text")
    };
    let history: Value = serde_json::from_str(text).unwrap();
    assert_eq!(history["schema"], 2);
    for (face, number) in &before {
        let entry = history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["number"] == *number)
            .unwrap();
        let characteristic = characteristics::read(&old)
            .unwrap()
            .into_iter()
            .find(|c| c.number == *number)
            .unwrap();
        let dimension = read.provenance.parts[0]
            .dimensions
            .iter()
            .position(|ids| ids.contains(&characteristic.entity))
            .unwrap();
        assert!(
            matches!(&pmi.dimensions[dimension].nominal.as_ref().unwrap().quantity,
            haecceity::pmi::Quantity::Length(length) if length.mm() == 19.0)
        );
        // This is also the 0.4.3 removal contract: native compositions must read to
        // the same leaf faces/role. Shape-aspect ids and wrapper depth are not identity.
        assert_eq!(
            entry["key"],
            json!({"kind":"dimension", "type":"Size_Diameter",
            "role":"size", "faces":[face], "reference":[]})
        );
    }
    let first = characteristics::read(&old)
        .unwrap()
        .into_iter()
        .find(|c| c.number == before[&8])
        .unwrap();
    let index = read.provenance.parts[0]
        .dimensions
        .iter()
        .position(|ids| ids.contains(&first.entity))
        .unwrap();
    assert!(
        matches!(&pmi.dimensions[index].nominal.as_ref().unwrap().quantity,
        haecceity::pmi::Quantity::Length(length) if length.mm() == 19.0)
    );
    let nominal = regex::Regex::new(r"LENGTH_MEASURE\(19(?:\.0*)?\)").unwrap();
    let measures: Vec<_> = read.provenance.parts[0].dimensions[index]
        .iter()
        .filter_map(|id| {
            let span = doc.span(*id).unwrap();
            let text = std::str::from_utf8(&doc.bytes()[span.clone()]).unwrap();
            nominal.is_match(text).then_some((*id, span, text))
        })
        .collect();
    assert_eq!(measures.len(), 1);
    let (id, span, text) = &measures[0];
    assert!(
        read.provenance.parts[0]
            .dimensions
            .iter()
            .enumerate()
            .all(|(i, ids)| i == index || !ids.contains(id)),
        "edit exactly one size"
    );
    let mut bytes = old.bytes.to_vec();
    bytes.splice(
        span.clone(),
        nominal.replace(text, "LENGTH_MEASURE(19.1)").bytes(),
    );
    let edited = part::load_bytes("edited-lm10uu.step", bytes).unwrap();
    assert_eq!(lands(&edited), before);
    let report = revision::diff(&old, &edited).unwrap();
    assert_scene_ids(&report, &edited);
    assert_eq!(report.pmi.items.len(), 1, "{:?}", report.pmi.items);
    let change = &report.pmi.items[0];
    assert_eq!(change.class, "changed");
    assert_eq!(change.kind, "dimension");
    assert_eq!(change.number, Some(before[&8]));
    assert_eq!(
        characteristics::read(&old).unwrap(),
        characteristics::read(&edited).unwrap()
    );
    println!(
        "LM10UU: before={before:?}; rerun={:?}; rewrite={:?}; 19→19.1={:?}; changed={}",
        lands(&repeated),
        lands(&rewritten),
        lands(&edited),
        change.number.unwrap()
    );
}

#[test]
fn compared_scenes_are_opt_in_and_are_each_file_s_own_pmi() {
    let old = write(
        &load("revisions/hole_moved.old.step.gz"),
        &answers(json!({"part.notes":["Keep dry"]})),
    );
    let new = load("revisions/hole_moved.new.step.gz");
    let default = revision::diff(&old, &new).unwrap();
    let included =
        revision::diff_with_options(&old, &new, revision::DiffOptions { scenes: true }).unwrap();
    let mut value = serde_json::to_value(&included).unwrap();
    value["pmi"].as_object_mut().unwrap().remove("scenes");
    assert_eq!(
        serde_json::to_vec(&default).unwrap(),
        serde_json::to_vec(&value).unwrap(),
        "opting in changes only scenes"
    );
    assert_eq!(
        revision::picture(&old, &new, &default, 300),
        revision::picture(&old, &new, &included, 300)
    );
    let notes = |scene: &Value| -> Vec<String> {
        scene["specify"]
            .as_object()
            .unwrap()
            .values()
            .filter_map(|m| m["notation"].as_str().map(str::to_owned))
            .filter(|n| n.starts_with("Keep"))
            .collect()
    };
    // The bare new file states nothing; the old file's note is its own.
    let scenes = included.pmi.scenes.as_ref().unwrap();
    assert_eq!(notes(&scenes.old), ["Keep dry"]);
    assert!(notes(&scenes.new).is_empty());
    // A written destination's scene is its own PMI.
    let stored = write(&new, &answers(json!({"part.notes":["Keep clean"]})));
    let included =
        revision::diff_with_options(&old, &stored, revision::DiffOptions { scenes: true })
            .unwrap();
    let scenes = included.pmi.scenes.unwrap();
    assert_eq!(notes(&scenes.new), ["Keep clean"]);
    assert_eq!(notes(&scenes.old), ["Keep dry"]);
}

#[test]
fn semantic_only_attributes_have_both_sides_mesh_face_anchors() {
    use haecceity::pmi::*;
    let bare = load("parts/plate.step");
    let model = |text: &str| PartPmi {
        features: vec![Feature::Items(vec![Anchor::Face(FaceIndex(9))])],
        attributes: vec![AttributeSet {
            name: "inspection method".into(),
            on: Some(NoteOwner::Feature(FeatureId(0))),
            items: vec![("method".into(), AttributeValue::Text(text.into()))],
            description: None,
            general_property: None,
        }],
        ..PartPmi::default()
    };
    let old = foreign_model(&bare, model("CMM"));
    let new = foreign_model(&bare, model("gauge"));
    let report = revision::diff_with_options(
        &old,
        &new,
        revision::DiffOptions { scenes: true },
    )
    .unwrap();
    let change = report
        .pmi
        .items
        .iter()
        .find(|c| c.class == "changed")
        .unwrap();
    assert_eq!(change.old.as_deref(), Some("method: CMM"));
    assert_eq!(change.new.as_deref(), Some("method: gauge"));
    let scenes = report.pmi.scenes.as_ref().unwrap();
    assert!(
        scenes.new["presentation"]["items"]
            .get(&change.id)
            .is_none()
    );
    assert_eq!(
        serde_json::to_value(&change.faces).unwrap(),
        json!({"old":[{"face":9}],"new":[{"face":9}]})
    );
}

#[test]
fn picture_draws_changed_added_and_removed_presentation_inside_the_frame() {
    let bare = load("parts/plate.step");
    let old = write(
        &bare,
        &answers(json!({
            "extra.1":{"kind":"flatness","faces":[4],"value":"0.05"},
            "extra.2":{"kind":"flatness","faces":[5],"value":"0.02"}
        })),
    );
    let new = write(
        &old,
        &answers(json!({
            "extra.1":{"kind":"flatness","faces":[4],"value":"0.1"},
            "extra.3":{"kind":"flatness","faces":[0],"value":"0.03"}
        })),
    );
    let report = revision::diff(&old, &new).unwrap();
    for class in ["changed", "added", "removed"] {
        assert!(report.pmi.items.iter().any(|c| c.class == class), "{class}");
    }
    assert!(report.faces.is_empty() && report.gone.is_empty());
    let png = revision::picture(&old, &new, &report, 900);
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    let (mut orange, mut green, mut red) = (0, 0, 0);
    for (i, pixel) in pixels[..info.buffer_size()]
        .as_chunks::<3>()
        .0
        .iter()
        .enumerate()
    {
        let (r, g, b) = (pixel[0] as i32, pixel[1] as i32, pixel[2] as i32);
        let is_orange = r > g + 30 && g > b + 30;
        let is_green = g > r + 30 && g > b + 30;
        let is_red = r > g + 30 && (g - b).abs() < 15;
        orange += usize::from(is_orange);
        green += usize::from(is_green);
        red += usize::from(is_red);
        if is_orange || is_green || is_red {
            let (x, y) = (i % info.width as usize, i / info.width as usize);
            assert!(
                x > 1 && y > 1 && x + 2 < info.width as usize && y + 2 < info.height as usize,
                "clipped at {x},{y}"
            );
        }
    }
    assert!(
        orange > 20 && green > 20 && red > 20,
        "orange={orange}, green={green}, red={red}"
    );
}

#[test]
fn thread_manufacturing_changes_show_drawing_wording_on_both_sides() {
    let bare = load("revisions/gramil_shaft.old.step.gz");
    let supplied = answers(json!({"hole.function:holes:13":"tapped:M4"}));
    let old = write(&bare, &supplied);
    let text = String::from_utf8(old.bytes.to_vec()).unwrap();
    assert!(text.contains("6H"));
    let new = part::load_bytes("thread-class.step", text.replace("6H", "6G").into_bytes()).unwrap();
    let report = revision::diff(&old, &new).unwrap();
    assert_eq!(report.pmi.items.len(), 1, "{:?}", report.pmi.items);
    assert_eq!(report.pmi.items[0].kind, "thread");
    assert!(
        report
            .pmi
            .items
            .iter()
            .any(|c| c.old.as_deref() == Some("M4×0.7-6H THRU")
                && c.new.as_deref() == Some("M4×0.7-6G THRU")),
        "{:?}",
        report.pmi.items
    );
    // #216 adds the floor to this hole ID; keep the one-change wording assertion.
    let old = write(
        &bare,
        &answers(
            json!({"hole.function:holes:14.15":"tapped:M2", "hole.full_thread:holes:14.15":"2"}),
        ),
    );
    let new = write(
        &old,
        &answers(
            json!({"hole.function:holes:14.15":"tapped:M2", "hole.full_thread:holes:14.15":"1.8"}),
        ),
    );
    let report = revision::diff(&old, &new).unwrap();
    // Depth lives in the manufacturing attribute, so the folded thread must still
    // report it even though its AP242 thread record did not change.
    assert_eq!(report.pmi.items.len(), 1, "{:?}", report.pmi.items);
    let thread = report
        .pmi
        .items
        .iter()
        .find(|c| c.kind == "thread")
        .unwrap();
    assert_eq!(thread.old.as_deref(), Some("M2×0.4-6H ↧ 2"));
    assert_eq!(thread.new.as_deref(), Some("M2×0.4-6H ↧ 1.8"));
}

#[test]
fn gramil_end_chamfer_does_not_swap_fresh_dimensions() {
    // Each revision written with its own defaults, numbered independently.
    let old = with_defaults(&load("revisions/gramil_shaft.old.step.gz"));
    let new = with_defaults(&load("revisions/gramil_shaft.new.step.gz"));
    let report =
        revision::diff_with_options(&old, &new, revision::DiffOptions { scenes: true }).unwrap();
    // No dimension is paired with another of a different value.
    assert!(
        !report
            .pmi
            .items
            .iter()
            .any(|c| c.class == "changed" && c.old != c.new),
        "{:?}",
        report.pmi.items
    );
    let scenes = report.pmi.scenes.as_ref().unwrap();
    // The overall length keeps its own value on both sides.
    for text in ["37.6"] {
        let notation = |scene: &Value| {
            scene["presentation"]["items"]
                .as_object()
                .unwrap()
                .values()
                .flat_map(|g| g.as_array().unwrap())
                .any(|g| g["name"] == text)
        };
        assert!(notation(&scenes.old) && notation(&scenes.new), "{text}");
    }
}

#[test]
fn fresh_revision_fixture_pairs_never_cross_requirement_types() {
    // Fresh inspection-order numbers shift when geometry/planner rows change.
    // Tuple: changed, renumbered, added, removed.
    // Recorded from two files each written with its own defaults (ADR P11: diff compares the
    // files' PMI, paired by faces and kind, never two plans). They keep every addition, removal
    // and real renumbering rather than treating unrelated occupants of a number as edits.
    let cases = [
        ("complex_counterbore", [5, 6, 2, 0]), // Five surviving pattern requirements grow support; two counterbore sizes are new.
        ("complex_fillet_all", [0, 2, 2, 2]),
        ("complex_many", [4, 2, 4, 4]),
        ("complex_moved", [5, 12, 1, 0]),
        ("complex_pattern", [0, 6, 2, 3]), // Changed pattern structure replaces requirements.
        ("complex_remodelled", [1, 0, 3, 3]),
        ("gramil_shaft", [1, 1, 3, 3]),
        ("hole_added", [0, 9, 4, 0]),
        ("hole_removed", [0, 9, 0, 4]), // Removed hole requirements go.
        ("named_plate", [1, 0, 7, 7]),
        ("plate_removed", [0, 3, 4, 6]),
        ("plate_thickened", [1, 0, 2, 2]),
        ("pmi_split", [2, 2, 0, 0]),
    ];
    let signature = |scene: &Value, id: &str| {
        scene["pmi"]["pmi"]
            .as_object()
            .unwrap()
            .iter()
            .find_map(|(collection, rows)| {
                rows.as_array()?
                    .iter()
                    .find(|row| row["id"] == id)
                    .map(|row| {
                        let subtype = if collection == "dimensions" {
                            row.pointer("/value/kind/size/kind")
                                .or_else(|| row.pointer("/value/kind/location/kind"))
                                .cloned()
                                .unwrap_or(Value::Null)
                        } else {
                            Value::Null
                        };
                        (collection.clone(), subtype)
                    })
            })
            .unwrap()
    };
    for (case, expected) in cases {
        // Each revision written with its own defaults: two files, numbered independently,
        // paired by faces and kind (ADR P11).
        let old = with_defaults(&load(&format!("revisions/{case}.old.step.gz")));
        let new = with_defaults(&load(&format!("revisions/{case}.new.step.gz")));
        let report =
            revision::diff_with_options(&old, &new, revision::DiffOptions { scenes: true })
                .unwrap();
        let counts = ["changed", "renumbered", "added", "removed"]
            .map(|class| report.pmi.items.iter().filter(|i| i.class == class).count());
        assert_eq!(counts, expected, "{case}: {:?}", report.pmi.items);
        let scenes = report.pmi.scenes.unwrap();
        for item in report
            .pmi
            .items
            .iter()
            .filter(|i| i.class == "changed" || i.class == "renumbered")
        {
            let old_id = item.old_id.as_deref().unwrap_or(&item.id);
            assert_eq!(
                signature(&scenes.old, old_id),
                signature(&scenes.new, &item.id),
                "{case}: {old_id} → {}",
                item.id
            );
            // Datums pair by their faces (ADR P11): a letter that changed on the same faces is
            // reported as a change, never hidden as a renumbering.
            if item.kind == "datum" && item.old != item.new {
                assert!(
                    report
                        .pmi
                        .items
                        .iter()
                        .any(|c| c.class == "changed" && c.id == item.id),
                    "{case}: {item:?}"
                );
            }
        }
    }
}

#[test]
fn a_semantic_only_knurl_change_has_its_own_wording() {
    let bare = load("parts/pin.step");
    let analysis = api::analyse(&bare).unwrap();
    let question = api::questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| {
            q.id.starts_with("diameter.fit:") && q.options.iter().any(|v| v.starts_with("knurl:"))
        })
        .unwrap();
    let old = write(
        &bare,
        &answers(json!({question.id.clone():"knurl:straight"})),
    );
    let new = write(&old, &answers(json!({question.id:"knurl:diamond"})));
    let report = revision::diff(&old, &new).unwrap();
    let knurl = report
        .pmi
        .items
        .iter()
        .find(|i| i.kind == "knurl" && i.class == "changed")
        .unwrap();
    assert!(
        knurl
            .old
            .as_ref()
            .unwrap()
            .starts_with("straight knurl, pitch "),
        "{knurl:?}"
    );
    assert!(
        knurl
            .new
            .as_ref()
            .unwrap()
            .starts_with("diamond knurl, pitch "),
        "{knurl:?}"
    );
}

#[test]
fn counterbores_grow_the_existing_pattern_requirements_instead_of_replacing_them() {
    // Each revision written with its defaults: the through holes counterbored.
    let old = with_defaults(&load("revisions/complex_counterbore.old.step.gz"));
    let new = with_defaults(&load("revisions/complex_counterbore.new.step.gz"));
    let report = revision::diff(&old, &new).unwrap();
    // The pattern's pitches, bore diameter and locations keep their requirement: each pairs
    // with its grown support instead of being removed and added.
    for text in ["42.426407", "⌀6", "50", "40"] {
        let paired: Vec<_> = report
            .pmi
            .items
            .iter()
            .filter(|item| {
                item.class == "changed"
                    && item.old.as_deref() == Some(text)
                    && item.new.as_deref() == Some(text)
            })
            .collect();
        assert!(!paired.is_empty(), "missing {text}: {:?}", report.pmi.items);
        assert!(
            paired
                .iter()
                .all(|p| p.reason.as_deref() == Some("support changed")),
            "{paired:?}"
        );
        assert!(
            !report.pmi.items.iter().any(|item| (item.class == "removed"
                && item.old.as_deref() == Some(text))
                || (item.class == "added" && item.new.as_deref() == Some(text))),
            "{text}: {:?}",
            report.pmi.items
        );
    }
    let added: Vec<_> = report
        .pmi
        .items
        .iter()
        .filter(|item| item.class == "added")
        .collect();
    assert_eq!(added.len(), 2, "{added:?}");
    for text in ["⌀12", "3"] {
        assert!(
            added.iter().any(|item| item.new.as_deref() == Some(text)),
            "{added:?}"
        );
    }
}

#[test]
fn spindle_thread_number_cannot_pair_with_a_new_location_dimension() {
    let old = load("revisions/spindle_pmi.old.step.gz");
    let new = load("revisions/spindle_pmi.new.step.gz");
    let report = revision::diff(&old, &new).unwrap();
    assert!(
        report.pmi.items.iter().any(|item| {
            item.class == "added"
                && item.kind == "dimension"
                && item.number == Some(16)
                && item.new.as_deref() == Some("1.5")
                && item.old.is_none()
        }),
        "{:?}",
        report.pmi.items
    );
    assert!(!report.pmi.items.iter().any(|item| {
        item.old.as_deref().is_some_and(|text| text.starts_with("M4"))
            && item
                .new
                .as_deref()
                .is_some_and(|text| !text.starts_with("M4"))
    }));
}
