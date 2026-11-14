//! Straight axial depths use two physical endpoint aspects (CAx-IF PMI RP 4.1).
use haecceity::{geom::Surface, p21::Document, pmi};
use serde_json::{Value, json};
use specify_core::{api, part, rules, writer};
use std::{collections::BTreeSet, path::Path};

fn anchors(p: &pmi::PartPmi, id: pmi::FeatureId) -> Vec<pmi::Anchor> {
    match &p.features[id.0] {
        pmi::Feature::Items(items) => items.clone(),
        pmi::Feature::Group { members, .. } => {
            members.iter().flat_map(|&m| anchors(p, m)).collect()
        }
        other => panic!("physical endpoint expected: {other:?}"),
    }
}

fn write_depths(name: &str) -> (part::LoadedPart, pmi::PartPmi, BTreeSet<String>) {
    write_depths_with_legacy_ids(name, false)
}

fn write_depths_with_legacy_ids(
    name: &str,
    legacy: bool,
) -> (part::LoadedPart, pmi::PartPmi, BTreeSet<String>) {
    let source = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/parts")
            .join(name),
        None,
    )
    .unwrap();
    let analysis = api::analyse(&source).unwrap();
    let mut answers = json!({"part.material":"steel"})
        .as_object()
        .unwrap()
        .clone();
    // Request plain bores so the pin exercises its drilling depth independently of
    // the thread manufacturing attributes, which already carry straight measures.
    for q in rules::questions(&analysis, &answers).unwrap() {
        if q.id.starts_with("hole.function:") {
            answers.insert(q.id, json!("fit:H11"));
        }
    }
    let mut intent = rules::apply_with_defaults(&analysis, &answers, true).unwrap();
    // Reproduce legacy analyses whose hole identities/supports contain only the
    // cylindrical faces, before recognition began including their flat floors.
    if legacy {
        for row in intent.requirements.iter_mut().chain(&mut intent.general) {
            let Some(planned) = row.get_mut("planner") else {
                continue;
            };
            if planned["required"]["kind"]["size"]["role"] != "bore.depth" {
                continue;
            }
            let mut renames = std::collections::BTreeMap::new();
            for support in planned["supports"].as_array_mut().unwrap() {
                let old = support["feature"].as_str().unwrap().to_owned();
                if !old.starts_with("hole:") {
                    continue;
                }
                let faces = support["faces"].as_array_mut().unwrap();
                faces.retain(|f| {
                    !matches!(
                        source.part.faces[f.as_u64().unwrap() as usize].surface,
                        Surface::Plane { .. }
                    )
                });
                let id = format!(
                    "hole:{}",
                    faces
                        .iter()
                        .map(|f| f.to_string())
                        .collect::<Vec<_>>()
                        .join(".")
                );
                support["feature"] = json!(id);
                renames.insert(old, id);
            }
            for id in planned["required"]["covers"].as_array_mut().unwrap() {
                if let Some(new) = renames.get(id.as_str().unwrap()) {
                    *id = json!(new);
                }
            }
            for members in planned["groups"].as_object_mut().unwrap().values_mut() {
                for id in members.as_array_mut().unwrap() {
                    if let Some(new) = renames.get(id.as_str().unwrap()) {
                        *id = json!(new);
                    }
                }
            }
            let feature = &mut planned["required"]["kind"]["size"]["feature"];
            if let Some(new) = renames.get(feature.as_str().unwrap()) {
                *feature = json!(new);
            }
        }
    }
    let (bytes, report) = writer::write_bytes(&source, &intent, Some(&answers)).unwrap();
    assert!(report.not_written.is_empty(), "{:?}", report.not_written);
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(!text.contains("DIMENSIONAL_SIZE('curve length'"));
    let doc = Document::parse(bytes).unwrap();
    let read = pmi::read(&doc, std::slice::from_ref(&source.definition)).unwrap();
    let p = read.parts.into_iter().next().unwrap();
    let mut roles = BTreeSet::new();
    for attribute in &p.attributes {
        if attribute.name != "specify dimension plan" {
            continue;
        }
        let pmi::AttributeValue::Text(metadata) = &attribute.items[0].1 else {
            panic!("plan metadata");
        };
        let metadata: Value = serde_json::from_str(metadata).unwrap();
        let Some(role) = metadata["required"]["kind"]["size"]["role"].as_str() else {
            continue;
        };
        if !role.ends_with(".depth") {
            continue;
        }
        roles.insert(role.to_owned());
        let Some(pmi::NoteOwner::Dimension(id)) = attribute.on else {
            panic!("dimension owner");
        };
        let dimension = &p.dimensions[id.0];
        let pmi::DimensionKind::Location {
            from,
            to,
            kind: pmi::LocationKind::LinearDistance,
            path: None,
            angle: None,
            ..
        } = dimension.kind
        else {
            panic!("straight depth: {dimension:?}");
        };
        assert_ne!(from, to);
        for end in [from, to] {
            let a = anchors(&p, end);
            assert!(!a.is_empty());
            assert!(
                a.iter()
                    .all(|a| matches!(a, pmi::Anchor::Face(_) | pmi::Anchor::Edge(_)))
            );
        }
    }
    assert!(!roles.is_empty(), "fixture must exercise a depth");
    (source, p, roles)
}

fn check_enclosure(legacy: bool) {
    let (source, p, roles) =
        write_depths_with_legacy_ids("electronics-enclosure-base.step.gz", legacy);
    assert_eq!(roles, BTreeSet::from(["bore.depth".into()]));
    let depths: Vec<_> = p.dimensions.iter().filter(|d|
        matches!(d.kind, pmi::DimensionKind::Location { .. })
        && matches!(&d.nominal.as_ref().unwrap().quantity, pmi::Quantity::Length(l) if l.mm() == 8.)).collect();
    assert_eq!(depths.len(), 1);
    let pmi::DimensionKind::Location { from, to, .. } = depths[0].kind else {
        unreachable!()
    };
    for (end, z) in [(from, 15.), (to, 7.)] {
        let faces: BTreeSet<_> = anchors(&p, end)
            .into_iter()
            .map(|a| {
                let pmi::Anchor::Face(f) = a else {
                    panic!("actual parallel planes required");
                };
                let Surface::Plane { frame } = &source.part.faces[f.0].surface else {
                    panic!("plane");
                };
                assert!((frame.origin[2] - z).abs() < 1e-8);
                assert!(frame.z[2].abs() > 0.999999);
                f.0
            })
            .collect();
        assert_eq!(faces.len(), 4);
    }
}

#[test]
fn enclosure_blind_pattern_depth_uses_boss_tops_and_flat_floors() {
    check_enclosure(false);
}

#[test]
fn legacy_cylinder_only_hole_ids_resolve_the_same_depth_planes() {
    check_enclosure(true);
}

#[test]
fn pin_blind_depth_uses_a_linear_location() {
    let (_, _, roles) = write_depths("pin.step");
    assert!(roles.contains("bore.depth"));
}

#[test]
fn counterbore_depths_use_linear_locations() {
    let (_, _, roles) = write_depths("../revisions/complex_counterbore.new.step.gz");
    assert!(roles.contains("counterbore.depth"), "{roles:?}");
}

#[test]
fn a_depth_conveys_only_the_duplicate_legacy_general_plane_span() {
    let source = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/revisions/complex_counterbore.new.step.gz"),
        None,
    )
    .unwrap();
    let analysis = api::analyse(&source).unwrap();
    let given = json!({"part.material":"steel"});
    let intent = rules::apply_with_defaults(&analysis, given.as_object().unwrap(), true).unwrap();
    let (bytes, _) = writer::write_bytes(&source, &intent, None).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let read = pmi::read(&doc, std::slice::from_ref(&source.definition)).unwrap();
    let p = &read.parts[0];
    let dim = p
        .attributes
        .iter()
        .find_map(|a| {
            if a.name != "specify dimension plan" {
                return None;
            }
            let pmi::AttributeValue::Text(text) = &a.items[0].1 else {
                return None;
            };
            let plan: Value = serde_json::from_str(text).unwrap();
            if plan["required"]["kind"]["size"]["role"] != "counterbore.depth" {
                return None;
            }
            let Some(pmi::NoteOwner::Dimension(id)) = a.on else {
                return None;
            };
            Some(&p.dimensions[id.0])
        })
        .unwrap();
    let pmi::DimensionKind::Location { from, to, .. } = dim.kind else {
        panic!("depth location");
    };
    let face_ids = |f| {
        anchors(p, f)
            .iter()
            .map(|a| match a {
                pmi::Anchor::Face(f) => f.0,
                _ => panic!("plane"),
            })
            .collect::<Vec<_>>()
    };
    let row = json!({"kind":"location","feature":"legacy depth span","faces":face_ids(to),"reference":face_ids(from),"distance":3.000001,"general":true});
    let mut duplicate = intent.clone();
    duplicate.general.push(row.as_object().unwrap().clone());
    let (bytes, _) = writer::write_bytes(&source, &duplicate, None).unwrap();
    let read = pmi::read(
        &Document::parse(bytes).unwrap(),
        std::slice::from_ref(&source.definition),
    )
    .unwrap();
    assert_eq!(read.parts[0].dimensions.len(), p.dimensions.len());
    // A separately specified tolerance remains authoritative, even at the same span.
    let mut stated = row.as_object().unwrap().clone();
    stated.remove("general");
    stated.insert("upper".into(), json!(0.03));
    stated.insert("lower".into(), json!(-0.03));
    duplicate.requirements.push(stated);
    let (bytes, _) = writer::write_bytes(&source, &duplicate, None).unwrap();
    let read = pmi::read(
        &Document::parse(bytes).unwrap(),
        std::slice::from_ref(&source.definition),
    )
    .unwrap();
    assert!(read.parts[0].dimensions.iter().any(|d|matches!(d.tolerance,pmi::DimTolerance::Deviations(_))
        && matches!(&d.nominal.as_ref().unwrap().quantity,pmi::Quantity::Length(n) if (n.mm()-3.000001).abs()<1e-9)));
}
