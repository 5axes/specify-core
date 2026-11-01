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
fn assert_scene_ids(report: &revision::Revised, part: &part::LoadedPart) {
    let scene = pmi_scene::pmi_scene(&part.bytes, None).unwrap();
    for item in &report.pmi.items {
        if item.class == "removed" {
            continue;
        }
        let meta = &scene["specify"][&item.id];
        assert!(meta.is_object(), "{}", item.id);
        assert_eq!(meta["number"].as_u64(), item.number);
        assert_eq!(meta["status"], item.status);
    }
}

#[test]
fn a_fit_answer_changes_with_its_characteristic_number_and_scene_id() {
    let bare = load("parts/plate.step");
    let before = answers(json!({"hole.function:holes:9":"fit:H7"}));
    let old = write(&bare, &before);
    let after = answers(json!({"hole.function:holes:9":"fit:H8"}));
    let new = write(&old, &after);
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
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
    let new = load("revisions/hole_moved.new.step.gz");
    let answers = answers(
        json!({"part.general_tolerance":"ISO 2768-m", "part.locating":"position tolerances"}),
    );
    let old = write(&before, &answers);
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
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
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
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
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
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
    let old_scene = pmi_scene::pmi_scene(&old.bytes, None).unwrap();
    for control in controls {
        assert_eq!(
            old_scene["specify"][&control.id]["number"].as_u64(),
            control.number
        );
    }
    let note = report
        .pmi
        .items
        .iter()
        .find(|c| c.id == "part.notes:1")
        .unwrap();
    assert_eq!(note.class, "changed");
    assert_eq!(note.old.as_deref(), Some("Keep dry"));
    assert_eq!(note.new.as_deref(), Some("Keep clean"));
    assert!(
        report
            .markdown("Notes and modifiers", None)
            .contains("Keep dry → Keep clean")
    );
    for id in [
        "part.surface_finish",
        "part.edges",
        "part.general_tolerance",
    ] {
        assert!(
            !report.pmi.items.iter().any(|c| c.id == id),
            "{id}: {:?}",
            report.pmi.items
        );
    }
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
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
    assert_scene_ids(&report, &new);
    assert_eq!(report.pmi.items.len(), 1, "{:?}", report.pmi.items);
    let note = &report.pmi.items[0];
    assert_eq!(note.id, "part.notes:1");
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
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
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
    // Also keep foreign PMI when projecting carried answers onto a resumed source.
    let given = answers(json!({"part.notes":["Keep dry"]}));
    let projected = revision::diff(&old, &new, &given).unwrap();
    assert!(
        projected
            .pmi
            .items
            .iter()
            .any(|c| c.class == "changed" && c.status == "existing")
    );
}

#[test]
fn unchanged_complex_and_resume_pairs_have_no_pmi_changes() {
    for name in [
        "revisions/complex_remodelled.old.step.gz",
        "resume-alpha6/plate.step.gz",
        "resume_alpha7/golden_oriented_slots.step.gz",
    ] {
        let part = load(name);
        let report = revision::diff(&part, &part, &Map::new()).unwrap();
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
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
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
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
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
    let new = load("revisions/hole_moved.new.step.gz");
    let supplied = answers(
        json!({"part.general_tolerance":"ISO 2768-m", "part.locating":"position tolerances", "hole.function:holes:8":"clearance"}),
    );
    let supplied =
        specify_core::rules::with_all_defaults(&api::analyse(&before).unwrap(), &supplied).unwrap();
    let old = write(&before, &supplied);
    let scene = pmi_scene::pmi_scene(&old.bytes, None).unwrap();
    let basic: Vec<_> = scene["pmi"]["pmi"]["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["value"]["tolerance"] == "basic")
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert!(!basic.is_empty());
    let report = revision::diff(&old, &new, &Map::new()).unwrap();
    assert!(
        report.pmi.items.iter().all(|c| c.kind == "dimension"),
        "{:?}",
        report.pmi.items
    );
    let written_new = write(&new, &report.answers);
    let saved_report = revision::diff(&old, &written_new, &Map::new()).unwrap();
    assert_scene_ids(&saved_report, &written_new);
    assert!(
        saved_report
            .pmi
            .items
            .iter()
            .any(|c| c.class == "changed" && basic.contains(&c.id.as_str()))
    );
    assert!(
        report
            .pmi
            .items
            .iter()
            .any(|c| c.class == "changed" && basic.contains(&c.id.as_str()) && c.number.is_some()),
        "{:?}",
        report.pmi.items
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
    // Force a normal rewrite, migrating history while preserving every inspection number.
    supplied.insert("part.notes".into(), json!(["Keep dry", "Migration test"]));
    let migrated = write(&old, &supplied);
    let numbers = |p: &part::LoadedPart| {
        specify_core::characteristics::read(p)
            .unwrap()
            .into_iter()
            .map(|c| (c.number, c.kind, c.r#type, c.faces, c.reference))
            .collect::<Vec<_>>()
    };
    assert_eq!(numbers(&old), numbers(&migrated));
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
            let report = revision::diff(old, &new, &Map::new()).unwrap();
            assert_scene_ids(&report, &new);
            assert_eq!(report.pmi.items.len(), count, "{:?}", report.pmi.items);
            let old_scene = pmi_scene::pmi_scene(&old.bytes, None).unwrap();
            for item in &report.pmi.items {
                assert_eq!(item.class, "changed");
                assert!(item.number.is_some());
                assert_eq!(
                    old_scene["specify"][&item.id]["number"].as_u64(),
                    item.number
                );
            }
            assert_eq!(
                specify_core::characteristics::read(old).unwrap(),
                specify_core::characteristics::read(&new).unwrap()
            );
        }
    }
}
