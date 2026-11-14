//! #75: the shared planner is the selection authority; STEP keeps its complete quantities.
use haecceity::{p21::Document, pmi};
use quiddity::dimensioning::{Input, PatternRole, Required, Tolerance};
use serde_json::{Value, json};
use specify_core::{api, characteristics, part, rules, writer};
use std::{collections::BTreeSet, path::Path};

fn plate() -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step"),
        None,
    )
    .unwrap()
}
fn required(i: &rules::Intent) -> Vec<Required> {
    let mut rs: Vec<Required> = i
        .requirements
        .iter()
        .chain(&i.general)
        .filter_map(|r| r.get("planner"))
        .map(|p| serde_json::from_value(p["required"].clone()).unwrap())
        .collect();
    rs.sort_by(|a, b| a.id.cmp(&b.id));
    rs
}
fn model(p: &part::LoadedPart) -> pmi::PmiRead {
    pmi::read(
        &Document::parse(p.bytes.to_vec()).unwrap(),
        std::slice::from_ref(&p.definition),
    )
    .unwrap()
}
#[test]
fn plate_writes_every_required_quantity_and_basics_use_the_pattern_origin() {
    let p = plate();
    let a = api::analyse(&p).unwrap();
    let given = json!({"part.material":"steel"});
    let i = rules::apply_with_defaults(&a, given.as_object().unwrap(), true).unwrap();
    let rs = required(&i);
    // Eleven raw quantities, minus the tapping drill carried by the thread,
    // plus two General offsets that cannot merge with Basic origins.
    assert_eq!(rs.len(), 12);
    let mut input: Input = serde_json::from_value(a["dimensioning"].clone()).unwrap();
    input.datums = i
        .datums
        .iter()
        .map(|d| quiddity::dimensioning::Datum {
            label: d["letter"].as_str().unwrap().into(),
            faces: serde_json::from_value(d["faces"].clone()).unwrap(),
        })
        .collect();
    assert_eq!(quiddity::dimensioning::plan(&input).0.len(), 11);
    input.specified = rs
        .iter()
        .filter(|r| r.id.starts_with("specified:") || r.id.starts_with("answered:"))
        .cloned()
        .collect();
    // The pattern's answered Y origin remains an authority even when its Basic
    // claim coincides with the M5's origin. Reconstruct that source claim using
    // the shared planner on the isolated pattern, before final consolidation.
    let pattern = input
        .patterns
        .iter()
        .find(|p| p.members.len() == 6)
        .unwrap()
        .clone();
    let mut isolated = input.clone();
    isolated.specified.clear();
    isolated
        .holes
        .retain(|h| pattern.members.contains(&h.support.feature));
    isolated.patterns = vec![pattern.clone()];
    isolated.steps = Default::default();
    isolated.cartesian_envelope = false;
    let mut origin = quiddity::dimensioning::plan(&isolated).0.into_iter().find(|r|
        matches!(&r.kind,quiddity::dimensioning::Kind::Location {to,axis:1,..} if to.feature == pattern.id())).unwrap();
    origin.id = format!("answered:{}", origin.id);
    origin.tolerance = Tolerance::Basic;
    input.specified.push(origin);
    let mut planned = quiddity::dimensioning::plan(&input).0;
    // M5's thread manufacturing attribute already states the 4.2 tapping drill.
    planned.retain(|r| r.id != "dim:hole:12/bore.diameter");
    assert_eq!(planned, rs);
    let centres: Vec<_> = rs
        .iter()
        .filter(|r| r.tolerance == Tolerance::Basic && r.pattern == Some(PatternRole::Centre))
        .collect();
    assert!(centres.iter().any(|r| (r.nominal - 50.).abs() < 1e-6));
    // The shared 30 also covers the pattern centre; every pitch is basic for the FCF.
    assert!(rs.iter().any(|r| (r.nominal - 30.).abs() < 1e-6
        && r.tolerance == Tolerance::Basic
        && r.covers.len() >= 6));
    let pitches: Vec<_> = rs
        .iter()
        .filter(|r| matches!(r.pattern, Some(PatternRole::Pitch { .. })))
        .collect();
    assert_eq!(pitches.len(), 2);
    assert!(pitches.iter().all(|r| r.tolerance == Tolerance::Basic));
    let (bytes, _) = writer::write_bytes(&p, &i, Some(given.as_object().unwrap())).unwrap();
    let written = part::load_bytes("planned.step", bytes).unwrap();
    let read = model(&written);
    assert_eq!(read.parts[0].dimensions.len(), rs.len());
    assert!(read.parts[0].dimensions.iter().any(|d| {
        if d.tolerance != pmi::DimTolerance::Basic { return false; }
        let pmi::DimensionKind::Location {to,..} = &d.kind else { return false; };
        let pmi::Feature::Group {kind:pmi::GroupKind::MultipleElements,members} = &read.parts[0].features[to.0] else { return false; };
        members.iter().any(|m| matches!(&read.parts[0].features[m.0],
            pmi::Feature::Group {kind:pmi::GroupKind::PatternOfFeatures,members} if members.len()==6))
    }), "the shared Basic origin must reach the pattern's semantic aspect");
    let mut stored: Vec<Required> = read.parts[0]
        .attributes
        .iter()
        .filter(|a| a.name == "specify dimension plan")
        .map(|a| match &a.items[0].1 {
            pmi::AttributeValue::Text(s) => serde_json::from_value(
                serde_json::from_str::<Value>(s).unwrap()["required"].clone(),
            )
            .unwrap(),
            _ => panic!("typed requirement"),
        })
        .collect();
    stored.sort_by(|a, b| a.id.cmp(&b.id));
    // Persistent JSON text rounds frame noise to nine places for native/WASM parity.
    // Requirement identities, grouping and tolerance classes remain exact.
    let mut expected = serde_json::to_value(&rs).unwrap();
    fn canonical(v: &mut Value) {
        match v {
            Value::Number(n) if n.is_f64() => {
                *v = json!((n.as_f64().unwrap() * 1e9).round() / 1e9 + 0.0);
            }
            Value::Array(a) => a.iter_mut().for_each(canonical),
            Value::Object(m) => m.values_mut().for_each(canonical),
            _ => {}
        }
    }
    canonical(&mut expected);
    assert_eq!(serde_json::to_value(stored).unwrap(), expected);
    assert!(read.parts[0].features.iter().any(|f| matches!(
        f,
        pmi::Feature::Group {
            kind: pmi::GroupKind::PatternOfFeatures,
            ..
        }
    )));
    assert_eq!(characteristics::read(&written).unwrap().len(), 19);
}

#[test]
fn an_existing_written_plate_retires_member_numbers_and_never_reuses_them() {
    let old = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/planner75/plate-v028.step.gz"),
        None,
    )
    .unwrap();
    let before = characteristics::read(&old).unwrap();
    assert_eq!(before.len(), 28);
    assert_eq!(before.iter().filter(|c| c.kind == "dimension").count(), 21);
    let a = api::analyse(&old).unwrap();
    let given = a["resume"]["answers"].as_object().unwrap();
    let i = rules::apply_with_defaults(&a, given, true).unwrap();
    let (bytes, report) = writer::write_bytes(&old, &i, Some(given)).unwrap();
    assert!(report.warnings.contains(
        &"retired characteristic numbers: 6, 7, 8, 9, 10, 11, 21, 22, 23, 24, 25, 26".into()
    ));
    let new = part::load_bytes("planned.step", bytes).unwrap();
    let after = characteristics::read(&new).unwrap();
    assert_eq!(after.len(), 19);
    let max = before.iter().map(|c| c.number).max().unwrap();
    for prior in &before {
        if let Some(now) = after.iter().find(|c| {
            c.kind == prior.kind
                && c.r#type == prior.r#type
                && c.faces == prior.faces
                && c.reference == prior.reference
        }) {
            assert_eq!(now.number, prior.number, "unchanged {prior:?}");
        }
    }
    let old_numbers: BTreeSet<_> = before.iter().map(|c| c.number).collect();
    let new_numbers: BTreeSet<_> = after.iter().map(|c| c.number).collect();
    let retired: BTreeSet<_> = old_numbers.difference(&new_numbers).copied().collect();
    assert_eq!(retired, (6..=11).chain(21..=26).collect());
    assert_eq!(
        new_numbers
            .difference(&old_numbers)
            .copied()
            .collect::<BTreeSet<_>>(),
        (29..=31).collect()
    );
    assert_eq!(
        after
            .iter()
            .find(|c| c.faces == [2] && c.reference == [4])
            .unwrap()
            .number,
        5,
        "the reversed 10 mm extent keeps its number"
    );
    assert!(
        after
            .iter()
            .filter(|c| !old_numbers.contains(&c.number))
            .all(|c| c.number > max)
    );
    assert_eq!(new_numbers.len(), after.len());
    let a = api::analyse(&new).unwrap();
    let given = a["resume"]["answers"].as_object().unwrap();
    let i = rules::apply_with_defaults(&a, given, true).unwrap();
    let (again, _) = writer::write_bytes(&new, &i, Some(given)).unwrap();
    assert!(again == *new.bytes, "unchanged resume bytes");
    // A new answered inspection requirement must allocate above the entire old history.
    let mut given = given.clone();
    given.insert(
        "extra.new".into(),
        json!({"kind":"flatness","faces":[5],"tolerance":0.02}),
    );
    let mut i = i;
    i.requirements.push(
        json!({"kind":"flatness","faces":[5],"tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let (bytes, _) = writer::write_bytes(&new, &i, Some(&given)).unwrap();
    let added = part::load_bytes("added.step", bytes).unwrap();
    let numbers = characteristics::read(&added).unwrap();
    assert!(numbers.iter().all(|c| !retired.contains(&c.number)));
    assert!(
        numbers
            .iter()
            .any(|c| c.number > new_numbers.iter().copied().max().unwrap())
    );
    eprintln!("retired plate numbers: {retired:?}");
}

#[test]
fn reference_flag_is_written_as_a_standard_auxiliary_dimension() {
    let p = plate();
    let a = api::analyse(&p).unwrap();
    let given = json!({"part.general_tolerance":"ISO 2768-m"});
    let mut i = api::preview(&a, given.as_object().unwrap()).unwrap();
    let r = i
        .general
        .iter_mut()
        .find(|r| r["kind"] == "location")
        .unwrap();
    r["planner"]["required"]["reference"] = json!(["determining-plane-path"]);
    r["planner"]["required"]["tolerance"] = json!("untoleranced");
    let (bytes, _) = writer::write_bytes(&p, &i, Some(given.as_object().unwrap())).unwrap();
    let p = part::load_bytes("reference.step", bytes).unwrap();
    let read = model(&p);
    assert_eq!(
        read.parts[0]
            .dimensions
            .iter()
            .filter(|d| d.modifiers.contains(&pmi::DimensionModifier::Reference))
            .count(),
        1
    );
}

#[test]
fn stored_dimensions_enter_the_planner_and_are_kept_once_in_add_mode() {
    let p = plate();
    let mut i = rules::Intent::new(json!(p.binding));
    i.requirements.push(json!({"kind":"size","faces":[9],"feature":"holes:9","nominal":8.,"upper":0.02,"lower":-0.02}).as_object().unwrap().clone());
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let imported = part::load_bytes("imported.step", bytes).unwrap();
    let a = api::analyse(&imported).unwrap();
    assert_eq!(a["dimensioning_stated"].as_array().unwrap().len(), 1);
    let given = json!({"part.material":"steel"});
    let i = rules::apply_with_defaults(&a, given.as_object().unwrap(), true).unwrap();
    let rs = required(&i);
    // The stored size is the planner's authority: no fresh proposal on the hole, and it
    // stays the file's own dimension rather than an intent requirement beside it.
    assert!(!rs.iter().any(|r| r.id.starts_with("stored:")));
    assert!(!rs.iter().any(|r| r.id == "dim:hole:9/bore.diameter"));
    let (bytes, _) = writer::write_bytes(&imported, &i, None).unwrap();
    let written = part::load_bytes("added.step", bytes).unwrap();
    let read = model(&written);
    assert_eq!(
        read.parts[0]
            .dimensions
            .iter()
            .filter(|d| d.nominal.as_ref().is_some_and(
                |v| matches!(&v.quantity,pmi::Quantity::Length(l) if l.value.to_f64()==8.)
            ))
            .count(),
        1
    );
}

#[test]
fn inch_parts_do_not_acquire_synthetic_location_supports() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_05_asme1_ap242.stp.gz");
    let p = part::load(&path, Some(0)).unwrap();
    let given = json!({"part.general_tolerance":"ISO 2768-m"});
    let i = api::preview(&api::analyse(&p).unwrap(), given.as_object().unwrap()).unwrap();
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let p = part::File::of_bytes(Path::new("inch.step"), bytes)
        .unwrap()
        .load(Some(0))
        .unwrap();
    let read = model(&p);
    let points: Vec<_> = read.parts[0]
        .geometry
        .iter()
        .filter(|g| {
            g.item.leaves.iter().any(|(name, attrs)| {
                name == "CARTESIAN_POINT"
                    && attrs.first()
                        == Some(&pmi::GeometryValue::Text(writer::PMI_PROVENANCE.into()))
            })
        })
        .collect();
    assert!(points.is_empty(), "location supports must name real faces");
}

#[test]
fn stored_pattern_origin_is_a_specified_authority_not_a_second_origin() {
    let p = plate();
    let mut i = rules::Intent::new(json!(p.binding));
    i.requirements.push(json!({"kind":"location", "faces":[6,7,8,10,11,13], "reference":[0], "distance":50., "upper":0.03, "lower":-0.03}).as_object().unwrap().clone());
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let imported = part::load_bytes("origin.step", bytes).unwrap();
    let a = api::analyse(&imported).unwrap();
    let given = json!({"part.general_tolerance":"ISO 2768-m"});
    let i = api::preview(&a, given.as_object().unwrap()).unwrap();
    let rs = required(&i);
    // The stored origin is the planner's authority, kept in the file and not repeated in
    // the intent: no proposal locates the pattern from the same plane again.
    assert!(!rs.iter().any(|r| r.id.starts_with("stored:")));
    assert!(!rs.iter().any(|r| matches!(&r.kind,
        quiddity::dimensioning::Kind::Location { from, to, .. }
            if to.feature.starts_with("pattern[") && from.faces == [0])));
    let (bytes, _) = writer::write_bytes(&imported, &i, None).unwrap();
    let written = part::load_bytes("origin-added.step", bytes).unwrap();
    let read = model(&written);
    assert_eq!(
        read.parts[0].dimensions.len(),
        rs.len() + 1,
        "stored origin is already present exactly once"
    );
}

#[test]
fn an_old_general_only_analysis_is_upgraded_at_the_writer_boundary() {
    let p = plate();
    let a = api::analyse(&p).unwrap();
    let given = json!({"part.general_tolerance":"ISO 2768-m"});
    let current = api::preview(&a, given.as_object().unwrap()).unwrap();
    let expected = required(&current).len();
    assert!(expected > 0);
    let mut old = a.clone();
    old.as_object_mut().unwrap().shift_remove("dimensioning");
    let i = api::preview(&old, given.as_object().unwrap()).unwrap();
    assert!(i.requirements.is_empty());
    assert!(i.datums.is_empty());
    assert!(required(&i).is_empty());
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let written = part::load_bytes("old-general.step", bytes).unwrap();
    let read = model(&written);
    assert_eq!(read.parts[0].dimensions.len(), expected);
    assert_eq!(
        read.parts[0]
            .attributes
            .iter()
            .filter(|a| a.name == "specify dimension plan")
            .count(),
        expected
    );
}

mod common;

fn faces(p: &pmi::PartPmi, id: pmi::FeatureId) -> BTreeSet<usize> {
    match &p.features[id.0] {
        pmi::Feature::Items(items) => items
            .iter()
            .filter_map(|a| match a {
                pmi::Anchor::Face(f) => Some(f.0),
                _ => None,
            })
            .collect(),
        pmi::Feature::Group { members, .. } => {
            members.iter().flat_map(|id| faces(p, *id)).collect()
        }
        pmi::Feature::Derived { from, .. } => from.iter().flat_map(|id| faces(p, *id)).collect(),
    }
}
struct DepthEnd {
    at: [f64; 3],
    normal: [f64; 3],
    faces: BTreeSet<usize>,
    edges: BTreeSet<usize>,
    plane: bool,
}
fn depth_ends(p: &pmi::PartPmi, part: &haecceity::Part, id: pmi::FeatureId) -> Vec<DepthEnd> {
    use haecceity::geom::{Curve, Surface};
    match &p.features[id.0] {
        pmi::Feature::Items(items) => {
            assert!(!items.is_empty(), "empty depth endpoint");
            items
                .iter()
                .map(|a| match a {
                    pmi::Anchor::Face(f) => {
                        let Surface::Plane { frame } = &part.faces[f.0].surface else {
                            panic!("depth face must be planar");
                        };
                        DepthEnd {
                            at: frame.origin,
                            normal: frame.z,
                            faces: BTreeSet::from([f.0]),
                            edges: BTreeSet::new(),
                            plane: true,
                        }
                    }
                    pmi::Anchor::Edge(e) => {
                        let Curve::Circle { frame, radius } = &part.edges[e.0].curve else {
                            panic!("depth edge must establish a circular centre");
                        };
                        assert!(*radius > 0.);
                        assert!(part.faces.iter().any(|f| {
                            f.loops
                                .iter()
                                .flat_map(|l| &l.edges)
                                .any(|(i, _)| *i == e.0)
                        }));
                        DepthEnd {
                            at: frame.origin,
                            normal: frame.z,
                            faces: BTreeSet::new(),
                            edges: BTreeSet::from([e.0]),
                            plane: false,
                        }
                    }
                    pmi::Anchor::Geometry(_) => {
                        panic!("a supplemental depth point needs a deriving aspect")
                    }
                })
                .collect()
        }
        pmi::Feature::Group { members, .. } => {
            assert!(!members.is_empty());
            members
                .iter()
                .flat_map(|&f| depth_ends(p, part, f))
                .collect()
        }
        pmi::Feature::Derived { geometry, .. } => {
            let g = &p.derived_geometry[geometry
                .expect("depth derivation must read back geometrically")
                .0];
            assert_eq!(g.kind, pmi::DerivedGeometryKind::Axis);
            assert!(!g.deriving_faces.is_empty());
            assert_eq!(faces(p, id), g.deriving_faces.iter().map(|f| f.0).collect());
            assert!(!g.supplemental.is_empty());
            g.supplemental
                .iter()
                .map(|g_id| {
                    let geometry = &p.geometry[g_id.0];
                    assert_eq!(geometry.item.leaves.len(), 1);
                    assert_eq!(geometry.item.leaves[0].0, "CARTESIAN_POINT");
                    let pmi::GeometryValue::List(coords) = &geometry.item.leaves[0].1[1] else {
                        panic!("point coordinates");
                    };
                    assert_eq!(coords.len(), 3);
                    let unit = geometry
                        .length_unit
                        .as_ref()
                        .expect("point units")
                        .millimetres();
                    let at = std::array::from_fn(|i| match &coords[i] {
                        pmi::GeometryValue::Real(n) => n.to_f64() * unit,
                        _ => panic!("real coordinate"),
                    });
                    DepthEnd {
                        at,
                        normal: g.direction.0.each_ref().map(pmi::Decimal::to_f64),
                        faces: g.deriving_faces.iter().map(|f| f.0).collect(),
                        edges: BTreeSet::new(),
                        plane: false,
                    }
                })
                .collect()
        }
    }
}

fn checked_depth_ends(
    pmi: &pmi::PartPmi,
    part: &haecceity::Part,
    input: &Input,
    from: pmi::FeatureId,
    to: pmi::FeatureId,
    r: &Required,
    path: &Path,
) -> (Vec<DepthEnd>, Vec<DepthEnd>) {
    use haecceity::geom::{cross, dot, norm, sub};
    let (from_ends, to_ends) = (depth_ends(pmi, part, from), depth_ends(pmi, part, to));
    assert!(!r.covers.is_empty());
    assert_eq!(
        input
            .holes
            .iter()
            .filter(|h| r.covers.contains(&h.support.feature))
            .count(),
        r.covers.len()
    );
    for h in input
        .holes
        .iter()
        .filter(|h| r.covers.contains(&h.support.feature))
    {
        let origin = input.frame.to_world(h.support.at);
        let direction = sub(
            input
                .frame
                .to_world(std::array::from_fn(|k| h.support.at[k] + h.direction[k])),
            origin,
        );
        let hole_edges: BTreeSet<_> = h
            .support
            .faces
            .iter()
            .flat_map(|&f| part.faces[f].loops.iter())
            .flat_map(|l| l.edges.iter().map(|&(e, _)| e))
            .collect();
        let stations = |ends: &[DepthEnd]| {
            ends.iter()
                .filter(|e| {
                    e.at.iter().chain(&e.normal).all(|v| v.is_finite())
                        && dot(e.normal, direction).abs() > 0.999999
                        && (e.plane || norm(cross(sub(e.at, origin), direction)) < 1e-6)
                        && (e.edges.iter().any(|i| hole_edges.contains(i))
                            || e.faces.iter().any(|f| {
                                h.support.faces.contains(f)
                                    || part.faces[*f]
                                        .loops
                                        .iter()
                                        .flat_map(|l| &l.edges)
                                        .any(|(i, _)| hole_edges.contains(i))
                            }))
                })
                .map(|e| dot(sub(e.at, origin), direction))
                .collect::<Vec<_>>()
        };
        let (from_stations, to_stations) = (stations(&from_ends), stations(&to_ends));
        assert!(
            from_stations.iter().any(|a| to_stations
                .iter()
                .any(|b| ((b - a).abs() - r.nominal).abs() < 0.0051)),
            "{} {}: depth {} is not established by its real endpoints: {from_stations:?} -> {to_stations:?}",
            path.display(),
            r.id,
            r.nominal
        );
    }
    (from_ends, to_ends)
}

/// Check the written PMI, including retained later-slice rows, rather than only
/// the shared planner's output. References do not add independent constraints.
#[test]
fn corpus_locations_have_faces_unique_pairs_and_no_closed_plane_chains() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let corpus = common::corpus_dir();
    let mut paths = vec![root.join("parts/plate.step"), root.join("parts/pin.step")];
    if let Some(corpus) = corpus {
        for name in common::corpus::SUBSET {
            paths.push(
                ["step", "stp", "step.gz", "stp.gz"]
                    .iter()
                    .map(|ext| corpus.join(format!("{name}.{ext}")))
                    .find(|p| p.exists())
                    .unwrap_or_else(|| panic!("missing {name}")),
            );
        }
    }
    let dump = std::env::var_os("SPECIFY_DIMENSION_COUNTS").map(std::path::PathBuf::from);
    if let Some(dir) = &dump {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut counts = Vec::new();
    let errors = std::cell::RefCell::new(Vec::new());
    for path in paths {
        eprintln!("dimension corpus: {}", path.display());
        let file = part::File::read(&path).unwrap();
        for k in 0..file.parts().len() {
            let p = file.load(Some(k)).unwrap();
            // Local count audits can reuse recognition from an identical source;
            // writes, PMI readback and every invariant always run afresh. The
            // default CI path still performs live analysis for every part.
            let name = path
                .strip_prefix(&root)
                .or_else(|_| path.strip_prefix(common::corpus_dir().unwrap_or_default()))
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let cached = std::env::var_os("SPECIFY_DIMENSION_ANALYSES")
                .map(std::path::PathBuf::from)
                .map(|dir| dir.join(format!("{}.analysis.json", name.replace('/', "_"))))
                .filter(|p| p.exists());
            let a = if let Some(cached) = cached {
                let a: Value = serde_json::from_slice(&std::fs::read(cached).unwrap()).unwrap();
                assert_eq!(
                    a["binding"],
                    json!(p.binding),
                    "cached recognition must bind this exact source part"
                );
                a
            } else {
                api::analyse(&p).unwrap()
            };
            let given = json!({"part.material":"steel"});
            let i = rules::apply_with_defaults(&a, given.as_object().unwrap(), true).unwrap();
            let (bytes, report) = writer::write_bytes(&p, &i, None).unwrap();
            let back = part::load_bytes("checked.step", bytes).unwrap();
            let read = model(&back);
            let pmi = &read.parts[0];
            let chars = characteristics::read(&back).unwrap();

            counts.push(json!({"part":name,"dimensions":pmi.dimensions.len(),"characteristics":chars.len()}));
            if let Some(dir) = &dump {
                let stem = name.replace('/', "_");
                std::fs::write(
                    dir.join(format!("{stem}.report.json")),
                    serde_json::to_vec_pretty(&report.to_dict()).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    dir.join(format!("{stem}.intent.json")),
                    serde_json::to_vec_pretty(&i.to_dict()).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    dir.join(format!("{stem}.analysis.json")),
                    serde_json::to_vec_pretty(&a).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    dir.join(format!("{stem}.chars.json")),
                    serde_json::to_vec_pretty(&chars).unwrap(),
                )
                .unwrap();
                std::fs::write(dir.join(format!("{stem}.step")), back.bytes.as_slice()).unwrap();
            }
            let claims: std::collections::BTreeMap<usize, Required> = pmi
                .attributes
                .iter()
                .filter(|a| a.name == "specify dimension plan")
                .map(|a| {
                    let Some(pmi::NoteOwner::Dimension(id)) = &a.on else {
                        panic!("dimension plan owner")
                    };
                    let pmi::AttributeValue::Text(text) = &a.items[0].1 else {
                        panic!("dimension plan text")
                    };
                    let value: Value = serde_json::from_str(text).unwrap();
                    (
                        id.0,
                        serde_json::from_value(value["required"].clone()).unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                claims
                    .values()
                    .map(|r| r.id.clone())
                    .collect::<BTreeSet<_>>(),
                required(&i)
                    .iter()
                    .map(|r| r.id.clone())
                    .collect::<BTreeSet<_>>(),
                "every eligible requirement must reach written PMI"
            );
            let mut pairs = BTreeSet::new();
            let input: Input = serde_json::from_value(a["dimensioning"].clone()).unwrap();
            let mut edges: Vec<(Option<usize>, usize, i64, i64)> = Vec::new();
            let mut edge = |body: Option<usize>, axis: usize, a: f64, b: f64, add: bool| {
                let (a, b) = ((a * 1e4).round() as i64, (b * 1e4).round() as i64);
                if a == b {
                    return;
                }
                let mut reached = BTreeSet::from([a]);
                loop {
                    let old = reached.len();
                    for &(owner, k, x, y) in &edges {
                        if owner == body
                            && k == axis
                            && (reached.contains(&x) || reached.contains(&y))
                        {
                            reached.extend([x, y]);
                        }
                    }
                    if reached.len() == old {
                        break;
                    }
                }
                if reached.contains(&b) {
                    errors.borrow_mut().push(format!(
                        "closed chain: {} axis {axis} {a}->{b}",
                        path.display()
                    ));
                    return;
                }
                if add {
                    edges.push((body, axis, a, b));
                }
            };
            for (dimension_index, d) in pmi.dimensions.iter().enumerate() {
                let pmi::DimensionKind::Location { from, to, .. } = &d.kind else {
                    continue;
                };
                let (mut a, mut b) = (faces(pmi, *from), faces(pmi, *to));
                let depth = claims.get(&dimension_index).filter(|r| matches!(&r.kind,
                    quiddity::dimensioning::Kind::Size {kind:quiddity::dimensioning::SizeKind::Length, role,..} if role.ends_with(".depth")));
                if depth.is_none() && (a.is_empty() || b.is_empty()) {
                    errors
                        .borrow_mut()
                        .push(format!("faceless location: {} {d:?}", path.display()));
                    continue;
                }
                // A one-face witness and its coplanar face group state the same support.
                let canonical = |f: BTreeSet<usize>| {
                    input
                        .planes
                        .iter()
                        .find(|p| f.iter().all(|id| p.support.faces.contains(id)))
                        .map_or_else(|| f.clone(), |p| p.support.faces.iter().copied().collect())
                };
                let depth_facts = depth
                    .map(|r| checked_depth_ends(pmi, &back.part, &input, *from, *to, r, &path));
                // Preserve face-only checks for every original location. Depths may
                // instead use RP circular centres or explicit derived axis points;
                // their topology, units, derivation and actual span are checked above.
                let endpoint_key = |f: BTreeSet<usize>, ends: Option<&[DepthEnd]>| {
                    if let Some(ends) = ends
                        && ends.iter().any(|e| !e.plane)
                    {
                        let mut keys = ends
                            .iter()
                            .map(|e| {
                                if e.plane {
                                    format!("faces:{:?}", canonical(e.faces.clone()))
                                } else {
                                    format!(
                                        "point:{:?}",
                                        e.at.map(|x| (x * 1e9).round() / 1e9 + 0.0)
                                    )
                                }
                            })
                            .collect::<Vec<_>>();
                        keys.sort();
                        keys.dedup();
                        format!("{keys:?}")
                    } else {
                        format!("faces:{:?}", canonical(f))
                    }
                };
                let (mut ka, mut kb) = (
                    endpoint_key(a.clone(), depth_facts.as_ref().map(|v| v.0.as_slice())),
                    endpoint_key(b.clone(), depth_facts.as_ref().map(|v| v.1.as_slice())),
                );
                if ka > kb {
                    std::mem::swap(&mut ka, &mut kb);
                }
                if !pairs.insert((ka.clone(), kb.clone())) {
                    errors
                        .borrow_mut()
                        .push(format!("duplicate pair: {} {ka} {kb}", path.display()));
                }
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                a = canonical(a);
                b = canonical(b);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                if d.modifiers.contains(&pmi::DimensionModifier::Reference) {
                    continue;
                }
                let plane = |f: &BTreeSet<usize>| {
                    input
                        .planes
                        .iter()
                        .find(|p| f.iter().all(|id| p.support.faces.contains(id)))
                };
                if let (Some(a), Some(b)) = (plane(&a), plane(&b))
                    && a.axis != b.axis
                {
                    errors.borrow_mut().push(format!(
                        "nonparallel plane supports: {} {a:?} {b:?}",
                        path.display()
                    ));
                }
                // An axis feature can include a planar hole floor among its
                // defining faces. Only an explicit covered plane claim makes
                // that floor a graph edge; locating an axis does not locate its floor.
                let interval = if let Some(r) = claims.get(&dimension_index) {
                    if let quiddity::dimensioning::Kind::Location { from, to, axis } = &r.kind {
                        let from_plane = input.planes.iter().find(|p| {
                            from.plane
                                && !from.faces.is_empty()
                                && from.faces.iter().all(|f| p.support.faces.contains(f))
                        });
                        let to_plane = input.planes.iter().find(|p| {
                            p.axis == *axis
                                && if to.plane {
                                    !to.faces.is_empty()
                                        && to.faces.iter().all(|f| p.support.faces.contains(f))
                                } else {
                                    (r.covers.contains(&p.support.feature)
                                        || input.steps.heights.iter().any(|s| {
                                            r.covers.contains(&s.feature)
                                                && s.faces
                                                    .iter()
                                                    .any(|f| p.support.faces.contains(f))
                                        }))
                                        && (p.support.at[*axis] - to.at[*axis]).abs() < 1e-4
                                }
                        });
                        from_plane.zip(to_plane)
                    } else {
                        None
                    }
                } else {
                    plane(&a).zip(plane(&b))
                };
                if let Some((a, b)) = interval {
                    edge(
                        a.support.body,
                        a.axis,
                        a.support.at[a.axis],
                        b.support.at[b.axis],
                        true,
                    );
                }
            }
            for r in required(&i) {
                if r.reference.is_some() {
                    continue;
                }
                if let quiddity::dimensioning::Kind::Size { role, .. } = &r.kind
                    && role.ends_with(".depth")
                {
                    for h in input
                        .holes
                        .iter()
                        .filter(|h| r.covers.contains(&h.support.feature))
                    {
                        for axis in 0..3 {
                            if h.direction[axis].abs() > 0.999 {
                                let floor = h.support.at[axis] + h.direction[axis] * r.nominal;
                                // Recognition publishes depths to 0.01 mm. Check the
                                // actual nearby floor plane, not an artificial station
                                // displaced by nominal rounding.
                                let floor = input
                                    .planes
                                    .iter()
                                    .filter(|p| {
                                        p.axis == axis
                                            && p.support.body == h.support.body
                                            && (p.support.at[axis] - floor).abs() < 0.0051
                                    })
                                    .min_by(|a, b| {
                                        (a.support.at[axis] - floor)
                                            .abs()
                                            .total_cmp(&(b.support.at[axis] - floor).abs())
                                    })
                                    .map_or(floor, |p| p.support.at[axis]);
                                edge(h.support.body, axis, h.support.at[axis], floor, false);
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(dir) = dump {
        std::fs::write(
            dir.join("counts.json"),
            serde_json::to_vec_pretty(&counts).unwrap(),
        )
        .unwrap();
    }
    let errors = errors.into_inner();
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

#[test]
fn answered_h11_bores_keep_their_fit_and_basic_locations() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let p = part::load(&corpus.join("mfcadpp/10224.step"), None).unwrap();
    let a = api::analyse(&p).unwrap();
    let mut given = json!({"part.material":"steel"})
        .as_object()
        .unwrap()
        .clone();
    // This test specifies clearance fits explicitly. An exact tap drill now
    // defaults to a thread, so accepting defaults would test a different intent.
    for q in rules::questions(&a, &given).unwrap() {
        if q.id.starts_with("hole.function:")
            && let Some(clearance) = q.options.iter().find(|v| v.starts_with("clearance"))
        {
            given.insert(q.id, json!(clearance));
        }
    }
    let i = rules::apply_with_defaults(&a, &given, true).unwrap();
    let sizes: Vec<_> = i
        .requirements
        .iter()
        .filter(|r| r.get("fit") == Some(&json!("H11")))
        .collect();
    assert_eq!(sizes.len(), 3);
    // The 3.52 blind depth ends at face 24's unrounded 3.5204636 floor.
    // It must not also become a baseline part length from datum A.
    assert!(
        !i.general
            .iter()
            .any(|r| r["kind"] == "location" && r["faces"] == json!([24]))
    );
    assert!(
        sizes
            .iter()
            .all(|r| r["faces"].as_array().is_some_and(|f| !f.is_empty()))
    );
    assert_eq!(
        i.requirements
            .iter()
            .filter(|r| r.get("basic") == Some(&json!(true)))
            .count(),
        6
    );
    assert!(!required(&i).iter().any(|r| r.tolerance == Tolerance::General
        && matches!(&r.kind, quiddity::dimensioning::Kind::Location { to,.. } if to.feature.starts_with("hole:"))));
    assert!(!required(&i).iter().any(|r| r.tolerance == Tolerance::General
        && matches!(&r.kind, quiddity::dimensioning::Kind::Size { role,.. } if role == "bore.diameter")));
}

#[test]
fn an_unknown_bottom_is_reported_without_inventing_a_blind_depth() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let p = part::load(&corpus.join("gramel/string_post.step"), None).unwrap();
    let a = api::analyse(&p).unwrap();
    let given = json!({"part.material":"steel"});
    let i = rules::apply_with_defaults(&a, given.as_object().unwrap(), true).unwrap();
    assert!(!required(&i).iter().any(|r| r.id == "dim:hole:5/bore.depth"));
    let (bytes, report) = writer::write_bytes(&p, &i, None).unwrap();
    assert!(
        report
            .not_written
            .iter()
            .any(|r| r.contains("dim:hole:5/bore.depth: bottom is unknown"))
    );
    let back = part::load_bytes("unknown.step", bytes).unwrap();
    assert!(!model(&back).parts[0].dimensions.iter().any(|d|
        matches!(d.nominal.as_ref().map(|v| &v.quantity),Some(pmi::Quantity::Length(n)) if n.value.to_f64()==5.4)));
}

#[test]
fn add_mode_references_never_depend_on_unwritten_legacy_baselines() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let p = part::load(&corpus.join("cadgenbench/flanged_spool_132.step"), None).unwrap();
    let mut foreign = rules::Intent::new(json!(p.binding));
    foreign.requirements.push(
        json!({"kind":"flatness","faces":[3],"tolerance":0.01})
            .as_object()
            .unwrap()
            .clone(),
    );
    let (bytes, _) = writer::write_bytes(&p, &foreign, None).unwrap();
    let p = part::load_bytes("foreign.step", bytes).unwrap();
    let given = json!({"part.material":"steel"});
    let i =
        rules::apply_with_defaults(&api::analyse(&p).unwrap(), given.as_object().unwrap(), true)
            .unwrap();
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let back = part::load_bytes("added.step", bytes).unwrap();
    let read = model(&back);
    let depths: Vec<_> = read.parts[0].dimensions.iter().filter(|d|
        matches!(d.kind,pmi::DimensionKind::Location {kind:pmi::LocationKind::LinearDistance,..})
        && matches!(d.nominal.as_ref().map(|v| &v.quantity),Some(pmi::Quantity::Length(n)) if n.value.to_f64()==10.)).collect();
    assert_eq!(depths.len(), 1);
    assert!(
        !depths[0]
            .modifiers
            .contains(&pmi::DimensionModifier::Reference)
    );
}

#[test]
fn coincident_basics_on_unpatterned_holes_share_a_real_feature_group() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let p = part::load(&corpus.join("gramel/string_post.step"), None).unwrap();
    // The frozen answered intent states 1.2 from face 11 to both hole axes.
    // This is a coincident span, not a hole pattern or an invented envelope.
    let captured = common::case("corpus/gramel/string_post")
        .artifact("apply")
        .unwrap();
    let i = rules::Intent::from_dict(&captured).unwrap();
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let back = part::load_bytes("coincident.step", bytes).unwrap();
    let read = model(&back);
    let basics: Vec<_> = read.parts[0].dimensions.iter().filter(|d|
        d.tolerance == pmi::DimTolerance::Basic
        && matches!(d.nominal.as_ref().map(|v| &v.quantity),Some(pmi::Quantity::Length(n)) if n.value.to_f64()==1.2)).collect();
    assert_eq!(basics.len(), 1);
    let pmi::DimensionKind::Location { to, .. } = &basics[0].kind else {
        panic!("basic location")
    };
    let pmi::Feature::Group {
        kind: pmi::GroupKind::MultipleElements,
        members,
    } = &read.parts[0].features[to.0]
    else {
        panic!("both hole axes must carry the shared basic")
    };
    let faces: BTreeSet<_> = members
        .iter()
        .flat_map(|m| match &read.parts[0].features[m.0] {
            pmi::Feature::Items(items) => items
                .iter()
                .filter_map(|a| match a {
                    pmi::Anchor::Face(f) => Some(f.0),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => panic!("individual hole support"),
        })
        .collect();
    assert_eq!(faces, BTreeSet::from([5, 17]));
}

#[test]
fn a_position_frames_bolt_circle_is_basic_even_without_cartesian_basics() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let p = part::load(&corpus.join("cadgenbench/flanged_spool_132.step"), None).unwrap();
    let a = api::analyse(&p).unwrap();
    let given = json!({"part.material":"steel"});
    let i = rules::apply_with_defaults(&a, given.as_object().unwrap(), true).unwrap();
    let rs = required(&i);
    let circles: Vec<_> = rs
        .iter()
        .filter(|r| {
            matches!(&r.kind,
        quiddity::dimensioning::Kind::Size {role,..} if role=="bolt_circle.diameter")
        })
        .collect();
    assert_eq!(circles.len(), 3);
    assert!(
        circles
            .iter()
            .any(|r| r.nominal == 90. && r.tolerance == Tolerance::Basic)
    );
    assert_eq!(
        circles
            .iter()
            .filter(|r| r.tolerance == Tolerance::General)
            .count(),
        2
    );
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let p = part::load_bytes("basic-circle.step", bytes).unwrap();
    let read = model(&p);
    assert_eq!(
        read.parts[0]
            .dimensions
            .iter()
            .filter(|d| d.tolerance == pmi::DimTolerance::Basic
                && d.nominal.as_ref().is_some_and(
                    |n| matches!(&n.quantity,pmi::Quantity::Length(l) if l.value.to_f64()==90.)
                ))
            .count(),
        1
    );
}

#[test]
fn blind_hole_diameters_and_axis_locations_stay_on_bores_but_depth_uses_planes() {
    // CTC-05 exercises the supported depth/location writer path from the
    // independent oracle intent, including the writer's legacy-input upgrade.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let nist = part::load(&root.join("nist/nist_ctc_05_asme1_ap242.stp.gz"), None).unwrap();
    let file = std::fs::File::open(root.join("oracle/nist/nist_ctc_05_asme1_ap242/apply.json.gz"))
        .unwrap();
    let captured: Value = serde_json::from_reader(flate2::read::GzDecoder::new(file)).unwrap();
    let intent = rules::Intent::from_dict(&captured).unwrap();
    let back = part::load_bytes(
        "nist.step",
        writer::write_bytes(&nist, &intent, None).unwrap().0,
    )
    .unwrap();
    // The existing-PMI interview deliberately omits owned General dimensions;
    // inspect the actual semantic dimension arena instead.
    let read = model(&back);
    let pmi = &read.parts[0];
    fn faces(p: &pmi::PartPmi, id: pmi::FeatureId) -> Vec<usize> {
        match &p.features[id.0] {
            pmi::Feature::Items(items) => items
                .iter()
                .filter_map(|a| match a {
                    pmi::Anchor::Face(f) => Some(f.0),
                    _ => None,
                })
                .collect(),
            pmi::Feature::Group { members, .. } => {
                members.iter().flat_map(|id| faces(p, *id)).collect()
            }
            pmi::Feature::Derived { from, .. } => {
                from.iter().flat_map(|id| faces(p, *id)).collect()
            }
        }
    }
    let mut depth = 0;
    let mut location = 0;
    for d in &pmi.dimensions {
        if d.tolerance == pmi::DimTolerance::None
            && matches!(d.nominal.as_ref().map(|n| &n.quantity),
                Some(pmi::Quantity::Length(length)) if (length.mm() - 92.08).abs() < 1e-8)
        {
            let pmi::DimensionKind::Location {
                from,
                to,
                kind: pmi::LocationKind::LinearDistance,
                ..
            } = d.kind
            else {
                panic!("blind depth must be a straight location: {d:?}");
            };
            assert_eq!(faces(pmi, to), [38]);
            for f in faces(pmi, from).into_iter().chain(faces(pmi, to)) {
                assert!(matches!(
                    back.part.faces[f].surface,
                    haecceity::geom::Surface::Plane { .. }
                ));
            }
            depth += 1;
            continue;
        }
        let id = match d.kind {
            pmi::DimensionKind::Size { feature, .. } => feature,
            pmi::DimensionKind::Location { to, .. } => to,
        };
        let support = faces(pmi, id);
        if !support.contains(&37) {
            continue;
        }
        // A location can target a consolidated group of holes. Each member is
        // bore-only; the group may legitimately contain other bore cylinders.
        assert!(!support.contains(&38), "{d:?}");
        assert!(
            support.iter().all(|f| matches!(
                back.part.faces[*f].surface,
                haecceity::geom::Surface::Cylinder { .. }
            )),
            "{d:?}"
        );
        if d.tolerance != pmi::DimTolerance::None {
            continue;
        }
        match d.kind {
            pmi::DimensionKind::Size { .. } => {
                assert_eq!(support, [37]);
                let pmi::Quantity::Length(length) = &d.nominal.as_ref().unwrap().quantity else {
                    panic!("length")
                };
                assert!(
                    (length.mm() - 92.08).abs() >= 1e-8,
                    "depth must not be a dimensional size"
                );
            }
            pmi::DimensionKind::Location { .. } => location += 1,
        }
    }
    assert_eq!(depth, 1);
    assert!(location > 0);
}

#[test]
fn ctc04_depths_have_measurable_endpoints_and_do_not_repeat_a_legacy_span() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let path = corpus.join("nist/nist_ctc_04_asme1_rd.stp");
    let loaded = part::load(&path, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let input: Input = serde_json::from_value(analysis["dimensioning"].clone()).unwrap();
    let given = json!({"part.material":"steel"});
    let intent = rules::apply_with_defaults(&analysis, given.as_object().unwrap(), true).unwrap();
    let (bytes, _) = writer::write_bytes(&loaded, &intent, None).unwrap();
    let back = part::load_bytes("ctc04.step", bytes).unwrap();
    let read = model(&back);
    let p = &read.parts[0];
    let mut depths = 0;
    for a in p
        .attributes
        .iter()
        .filter(|a| a.name == "specify dimension plan")
    {
        let pmi::AttributeValue::Text(text) = &a.items[0].1 else {
            panic!("plan");
        };
        let value: Value = serde_json::from_str(text).unwrap();
        let r: Required = serde_json::from_value(value["required"].clone()).unwrap();
        if !matches!(&r.kind,quiddity::dimensioning::Kind::Size {role,..} if role.ends_with(".depth"))
        {
            continue;
        }
        let Some(pmi::NoteOwner::Dimension(id)) = a.on else {
            panic!("dimension owner");
        };
        let pmi::DimensionKind::Location {
            from,
            to,
            kind: pmi::LocationKind::LinearDistance,
            ..
        } = p.dimensions[id.0].kind
        else {
            panic!("straight depth");
        };
        checked_depth_ends(p, &back.part, &input, from, to, &r, &path);
        depths += 1;
    }
    assert!(depths > 0);
    let pair = [BTreeSet::from([68, 73, 82, 87]), BTreeSet::from([393, 400])];
    let spans: Vec<_> = p
        .dimensions
        .iter()
        .filter(|d| match d.kind {
            pmi::DimensionKind::Location { from, to, .. } => {
                let mut actual = [faces(p, from), faces(p, to)];
                actual.sort();
                actual == pair
            }
            _ => false,
        })
        .collect();
    assert_eq!(
        spans.len(),
        1,
        "the new depth conveys the old General plane span"
    );
    assert!(
        matches!(&spans[0].nominal.as_ref().unwrap().quantity,pmi::Quantity::Length(n) if n.mm()==20.)
    );
}
