use super::*;

fn lm10uu() -> (LoadedPart, Intent, Map<String, Json>) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/freecad/bearing-lm10uu.step.gz");
    let loaded = crate::part::load(&path, None).unwrap();
    let analysis = crate::api::analyse(&loaded).unwrap();
    let answers = serde_json::json!({"part.material":"6061-T6"});
    let answers = answers.as_object().unwrap().clone();
    let intent = crate::rules::apply_with_defaults(&analysis, &answers, true).unwrap();
    (loaded, intent, answers)
}

#[test]
fn upstream_reader_keeps_three_independent_lands_on_a_multi_face_datum() {
    let (loaded, intent, _) = lm10uu();
    let source = Source::of(&loaded.bytes).unwrap();
    let (model, _) = part_pmi(&loaded, &intent, &source.read.parts[0], Mode::Replace).unwrap();
    // Exercise haecceity directly, without Specify's write/annotation pipeline.
    let (edit, _) = pmi::write(
        &source.doc,
        &source.defs,
        &[(PartId(0), model)],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let raw = Document::parse(source.doc.apply(&edit).unwrap().bytes).unwrap();
    let read = pmi::read(&raw, &source.defs).unwrap();
    let p = &read.parts[0];
    let faces = |id: FeatureId| {
        let Feature::Items(items) = &p.features[id.0] else {
            panic!("expected face-supported feature");
        };
        items
            .iter()
            .map(|a| match a {
                Anchor::Face(f) => f.0,
                _ => panic!("expected face anchor"),
            })
            .collect::<Vec<_>>()
    };
    let lands: Vec<_> = p
        .dimensions
        .iter()
        .filter_map(|d| {
            let DimensionKind::Size {
                feature,
                kind: SizeKind::Diameter,
                ..
            } = d.kind
            else {
                return None;
            };
            matches!(&d.nominal.as_ref()?.quantity, pmi::Quantity::Length(l) if l.mm() == 19.0)
                .then(|| faces(feature))
        })
        .collect();
    assert_eq!(lands, [vec![8], vec![4], vec![0]]);
    let a = p
        .datums
        .iter()
        .find(|d| d.label().to_string() == "A")
        .unwrap();
    assert_eq!(
        faces(a.feature().expect("datum A has a feature")),
        [0, 4, 8],
        "datum A still collects every land"
    );
}

#[test]
fn lm10uu_semantics_match_the_v042_workaround_build_except_straight_depth() {
    let (loaded, intent, answers) = lm10uu();
    let (bytes, _) = write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let back = crate::part::load_bytes("lm10uu.step", bytes).unwrap();
    let source = Source::of(&back.bytes).unwrap();
    let p = &source.read.parts[0];
    let model = serde_json::to_value(p).unwrap();
    // Compare semantic supports, not run-local feature indices or STEP record IDs.
    fn expand(value: &Json, model: &Json) -> Json {
        match value {
            Json::Array(items) => Json::Array(items.iter().map(|v| expand(v, model)).collect()),
            Json::Object(fields) => Json::Object(
                fields
                    .iter()
                    .map(|(key, value)| {
                        let value = if let Some(id) = value.as_u64() {
                            match key.as_str() {
                                "feature" | "from" | "to" => {
                                    let mut items = model["features"][id as usize]["items"]
                                        .as_array()
                                        .expect("face-supported LM10UU feature")
                                        .clone();
                                    items.sort_by_key(Json::to_string);
                                    Json::Array(items)
                                }
                                "datum" => model["datums"][id as usize]["label"].clone(),
                                _ => value.clone(),
                            }
                        } else {
                            expand(value, model)
                        };
                        (key.clone(), value)
                    })
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
    let mut actual = serde_json::json!({
        "characteristic_count": crate::characteristics::read(&back).unwrap().len()
    });
    for area in ["dimensions", "tolerances", "datums"] {
        let mut rows = expand(&model[area], &model).as_array().unwrap().clone();
        rows.sort_by_key(Json::to_string);
        actual[area] = Json::Array(rows);
    }
    let mut expected: Json = serde_json::from_str(include_str!(
        "../../tests/fixtures/freecad/lm10uu-v042-semantics.json"
    ))
    .unwrap();
    // Keep the historical baseline intact. The sole intentional semantic change
    // is the 1.4 mm recess depth: its actual entry/floor planes replace a curve
    // length on three cylinders. Every other value and support still compares.
    let depths: Vec<_> = expected["dimensions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|d| d["kind"]["size"]["kind"] == "curve length")
        .collect();
    assert_eq!(depths.len(), 1);
    for depth in depths {
        assert_eq!(
            depth["kind"]["size"]["feature"],
            json!([{"face":10},{"face":14},{"face":18}])
        );
        assert_eq!(depth["nominal"]["quantity"]["length"]["value"], "1.4");
        depth["kind"] = json!({"location": {
            "from":[{"face":19}], "to":[{"face":17}], "kind":"linear distance",
            "path":null, "directed":false, "angle":null
        }});
    }
    expected["dimensions"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(Json::to_string);
    assert_eq!(
        actual, expected,
        "semantic content must survive workaround removal"
    );
}

#[test]
fn write_report_exemptions_are_additive_and_use_written_entity_ids() {
    let mut report = WriteReport {
        output: "out.step".into(),
        written: vec![],
        not_written: vec![],
        warnings: vec![],
        geometry_carrier_exemptions: vec![],
        presentation_warnings: vec![],
        preserved_normalizations: vec![],
        retained_shared: vec![],
        saved_view_findings: vec![],
    };
    assert!(
        report
            .to_dict()
            .get("geometry_carrier_exemptions")
            .is_none()
    );
    assert!(report.to_dict().get("presentation_warnings").is_none());
    assert!(report.to_dict().get("preserved_normalizations").is_none());
    assert!(report.to_dict().get("retained_shared").is_none());
    report.retained_shared.push(301);
    assert_eq!(report.to_dict()["retained_shared"], json!([301]));
    assert!(report.to_dict().get("saved_view_findings").is_none());
    report.saved_view_findings.push(SavedViewFinding {
        kind: "fact".into(),
        part: Some(0),
        ids: vec![],
        entity: "draughting_model".into(),
        detail: "Notes saved view was not written".into(),
    });
    assert_eq!(
        report.to_dict()["saved_view_findings"],
        json!([{"kind":"fact","part":0,"ids":[],"entity":"draughting_model","detail":"Notes saved view was not written"}])
    );
    report
        .presentation_warnings
        .push("feature note glyphs could not be drawn".into());
    report
        .preserved_normalizations
        .push(PreservedNormalization {
            id: 77,
            normalization: "legacy-endpoint-retirement".into(),
        });
    assert_eq!(
        report.to_dict()["presentation_warnings"],
        json!(["feature note glyphs could not be drawn"])
    );
    assert_eq!(
        report.to_dict()["preserved_normalizations"],
        json!([{"id":77,"normalization":"legacy-endpoint-retirement"}])
    );
    report
        .geometry_carrier_exemptions
        .push(GeometryCarrierExemption {
            part: 2,
            condition: "shared-carrier-controlled-pair".into(),
            aspect: 123,
            feature: 45,
        });
    assert_eq!(
        report.to_dict()["geometry_carrier_exemptions"],
        json!([{
            "part":2,"condition":"shared-carrier-controlled-pair","aspect":123,"feature":45
        }])
    );
}

#[test]
fn authored_standalone_composition_members_remain_features_on_read() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step");
    let loaded = crate::part::load(&path, None).unwrap();
    let source = Source::of(&loaded.bytes).unwrap();
    let face = |f| Anchor::Face(FaceIndex(f));
    let model = PartPmi {
        features: vec![
            Feature::Items(vec![face(0)]),
            Feature::Items(vec![face(1)]),
            Feature::Items(vec![face(0), face(1)]),
        ],
        ..Default::default()
    };
    let (edit, _) = pmi::write(
        &source.doc,
        &source.defs,
        &[(PartId(0), model)],
        Mode::Add,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let bytes = source.doc.apply(&edit).unwrap().bytes;
    let doc = Document::parse(bytes.clone()).unwrap();
    assert!(doc.ids().any(|id| matches!(doc.get(id), Some(haecceity::p21::RawEntity::Simple { name, attributes, .. })
        if name == "SHAPE_ASPECT" && attributes.first() == Some(&haecceity::p21::Attribute::String("haecceity.feature/1".into())))));
    let read = pmi::read(&doc, &source.defs).unwrap();
    for faces in [vec![0], vec![1], vec![0, 1]] {
        assert!(read.parts[0].features.iter().any(|f| match f {
            Feature::Items(items) => items == &faces.iter().map(|&f| face(f)).collect::<Vec<_>>(),
            _ => false,
        }));
    }
    let loaded = crate::part::load_bytes("standalone.step", bytes).unwrap();
    let reading = crate::existing::read(&loaded).unwrap();
    assert!(
        reading.existing(&loaded).unwrap().is_empty(),
        "standalone supports do not invent requirements"
    );
    assert!(reading.findings().is_empty());
}

#[test]
fn add_upgrade_refusal_reaches_specify_without_retrying_replace() {
    let source = Source::of(include_bytes!("../../tests/fixtures/parts/plate.step")).unwrap();
    // A legacy AP214 producer using PRODUCT_TYPE, with foreign semantic PMI.
    let mut foreign = PartPmi::default();
    foreign
        .features
        .push(Feature::Items(vec![Anchor::Face(FaceIndex(0))]));
    foreign
        .datums
        .push(Datum::new(DatumLabel::new("A").unwrap(), Some(FeatureId(0)), vec![]).unwrap());
    let (edit, _) = pmi::write(
        &source.doc,
        &source.defs,
        &[(PartId(0), foreign)],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let doc = Document::parse(source.doc.apply(&edit).unwrap().bytes).unwrap();
    let mut legacy = p21::Edit::new();
    legacy.set_file_schema(vec!["AUTOMOTIVE_DESIGN".into()]);
    let product = doc
        .ids()
        .find(
            |&id| matches!(doc.get(id), Some(RawEntity::Simple { name, .. }) if name == "PRODUCT"),
        )
        .unwrap();
    legacy.add(p21::simple(
        "PRODUCT_TYPE",
        vec![
            Attribute::String("legacy".into()),
            Attribute::String("".into()),
            Attribute::List(vec![Attribute::EntityRef(product)]),
        ],
    ));
    let bytes = doc.apply(&legacy).unwrap().bytes;
    let loaded = part::load_bytes("legacy-foreign.step", bytes.clone()).unwrap();
    let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.part.insert("material".into(), json!("steel"));
    let error = write_bytes(&loaded, &intent, None).unwrap_err();
    assert!(matches!(&error, WriteError::Refused(_)), "{error}");
    let message = error.to_string();
    assert!(
        message.contains("Add would change original entities"),
        "{message}"
    );
    assert!(
        message.contains(
            "AP214 keyword rewrite PRODUCT_TYPE -> PRODUCT_RELATED_PRODUCT_CATEGORY on entity #"
        ),
        "{message}"
    );
    assert!(message.contains("Replace re-authors that PMI"), "{message}");
    assert_eq!(loaded.bytes.as_slice(), bytes);
}

#[test]
fn drill_point_depth_ends_at_the_cylindrical_shoulder_not_the_cone_length() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz");
    let loaded = crate::part::load(&path, None).unwrap();
    let input = crate::dimensioning::input(&loaded.part).unwrap();
    let hole = input
        .holes
        .iter()
        .find(|h| h.bottom == quiddity::features::holes::Bottom::DrillPoint)
        .unwrap();
    let required = quiddity::dimensioning::Required {
        id: "test:drill-point.depth".into(),
        kind: quiddity::dimensioning::Kind::Size {
            feature: hole.support.feature.clone(),
            role: "bore.depth".into(),
            kind: quiddity::dimensioning::SizeKind::Length,
        },
        nominal: hole.depth.unwrap(),
        count: 1,
        pattern: None,
        tolerance: quiddity::dimensioning::Tolerance::General,
        reference: None,
        covers: vec![hole.support.feature.clone()],
    };
    let mut planned = crate::dimensioning::Planned {
        required,
        supports: vec![hole.support.clone()],
        groups: Default::default(),
        frame: input.frame,
        witnesses: None,
    };
    let mut features = Vec::new();
    let kind = planned_depth_kind(
        &mut planned,
        &input,
        &loaded.part,
        &mut Features(&mut features),
        &mut PartPmi::default(),
        &LengthUnit::Millimetre,
    )
    .unwrap();
    let DimensionKind::Location {
        from,
        to,
        kind: LocationKind::LinearDistance,
        ..
    } = kind
    else {
        panic!("straight depth expected");
    };
    assert!(
        matches!(&features[from.0], Feature::Items(a) if a.iter().all(|a| matches!(a, Anchor::Face(_))))
    );
    assert!(
        matches!(&features[to.0], Feature::Items(a) if a.iter().all(|a| matches!(a, Anchor::Edge(_))))
    );
    let model = PartPmi {
        features: features.clone(),
        dimensions: vec![Dimension {
            kind,
            nominal: Some(mm(planned.required.nominal).unwrap()),
            tolerance: DimTolerance::None,
            qualifier: None,
            modifiers: vec![],
            principle: None,
        }],
        ..PartPmi::default()
    };
    let source = Source::of(&loaded.bytes).unwrap();
    let (edit, _) = pmi::write(
        &source.doc,
        &source.defs,
        &[(PartId(0), model)],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let bytes = source.doc.apply(&edit).unwrap().bytes;
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("DIMENSIONAL_LOCATION('linear distance'")
    );
    let read = pmi::read(&Document::parse(bytes).unwrap(), &source.defs).unwrap();
    assert!(matches!(
        read.parts[0].dimensions[0].kind,
        DimensionKind::Location {
            kind: LocationKind::LinearDistance,
            ..
        }
    ));

    // A genuine curve length still follows the existing single-feature path.
    let quiddity::dimensioning::Kind::Size { role, .. } = &mut planned.required.kind else {
        unreachable!()
    };
    *role = "curve.length".into();
    let kind = planned_kind(
        &planned,
        &mut Features(&mut features),
        &mut Vec::new(),
        &LengthUnit::Millimetre,
    )
    .unwrap();
    assert!(matches!(
        kind,
        DimensionKind::Size {
            kind: SizeKind::CurveLength,
            ..
        }
    ));
}

#[test]
fn enlargement_depth_roles_measure_between_their_own_parallel_planes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/revisions/complex_counterbore.new.step.gz");
    let loaded = crate::part::load(&path, None).unwrap();
    let original = crate::dimensioning::input(&loaded.part).unwrap();
    // Exercise the lowering contract for each cylindrical enlargement role on
    // a real 12 mm x 3 mm recess; their independent dimension semantics agree.
    for prefix in ["counterbore", "spotface", "outer_steps.0"] {
        let mut input = original.clone();
        let hole = &mut input.holes[0];
        for detail in &mut hole.details {
            detail.role = detail.role.replace("counterbore", prefix);
        }
        let required = quiddity::dimensioning::Required {
            id: format!("test:{prefix}.depth"),
            kind: quiddity::dimensioning::Kind::Size {
                feature: hole.support.feature.clone(),
                role: format!("{prefix}.depth"),
                kind: quiddity::dimensioning::SizeKind::Length,
            },
            nominal: 3.,
            count: 1,
            pattern: None,
            tolerance: quiddity::dimensioning::Tolerance::General,
            reference: None,
            covers: vec![hole.support.feature.clone()],
        };
        let mut planned = crate::dimensioning::Planned {
            required,
            supports: vec![hole.support.clone()],
            groups: Default::default(),
            frame: input.frame,
            witnesses: None,
        };
        let mut features = Vec::new();
        let kind = planned_depth_kind(
            &mut planned,
            &input,
            &loaded.part,
            &mut Features(&mut features),
            &mut PartPmi::default(),
            &LengthUnit::Millimetre,
        )
        .unwrap();
        let DimensionKind::Location {
            from,
            to,
            kind: LocationKind::LinearDistance,
            path: None,
            ..
        } = kind
        else {
            panic!("{prefix}: straight location required");
        };
        let plane = |id: FeatureId| {
            let Feature::Items(anchors) = &features[id.0] else {
                panic!("plane support");
            };
            assert_eq!(anchors.len(), 1);
            let Anchor::Face(f) = anchors[0] else {
                panic!("real planar face");
            };
            let haecceity::geom::Surface::Plane { frame } = &loaded.part.faces[f.0].surface else {
                panic!("plane");
            };
            frame
        };
        let (a, b) = (plane(from), plane(to));
        assert!(haecceity::geom::dot(a.z, b.z).abs() > 0.999999);
        let depth = haecceity::geom::dot(haecceity::geom::sub(a.origin, b.origin), a.z).abs();
        assert!((depth - 3.).abs() < 1e-8, "{prefix}: {depth}");
    }
}

#[test]
fn an_oblique_counterbore_entry_uses_a_derived_axial_point() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_02_asme1_ap242.stp.gz");
    let loaded = crate::part::load(&path, None).unwrap();
    let input = crate::dimensioning::input(&loaded.part).unwrap();
    let hole = input
        .holes
        .iter()
        .find(|h| h.support.feature == "hole:98.99.101.102")
        .unwrap();
    let mut planned = crate::dimensioning::Planned {
        required: quiddity::dimensioning::Required {
            id: "test:oblique-counterbore.depth".into(),
            kind: quiddity::dimensioning::Kind::Size {
                feature: hole.support.feature.clone(),
                role: "counterbore.depth".into(),
                kind: quiddity::dimensioning::SizeKind::Length,
            },
            nominal: 23.49,
            count: 1,
            pattern: None,
            tolerance: quiddity::dimensioning::Tolerance::General,
            reference: None,
            covers: vec![hole.support.feature.clone()],
        },
        supports: vec![hole.support.clone()],
        groups: Default::default(),
        frame: input.frame,
        witnesses: None,
    };
    let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.general.push(
        json!({"kind":"size", "feature":hole.support.feature,
        "faces":hole.support.faces, "nominal":23.49, "general":true, "planner":planned})
        .as_object()
        .unwrap()
        .clone(),
    );
    let (written, report) = write_bytes(&loaded, &intent, None).unwrap();
    assert!(
        !std::str::from_utf8(&written)
            .unwrap()
            .contains("specify-core:depth axis point")
    );
    assert!(report.not_written.is_empty(), "{:?}", report.not_written);
    assert!(
        std::str::from_utf8(&written)
            .unwrap()
            .contains("DERIVED_SHAPE_ASPECT('axis point'")
    );
    let mut features = Vec::new();
    let mut model = PartPmi::default();
    let kind = planned_depth_kind(
        &mut planned,
        &input,
        &loaded.part,
        &mut Features(&mut features),
        &mut model,
        &LengthUnit::Millimetre,
    )
    .unwrap();
    let DimensionKind::Location {
        from,
        to,
        kind: LocationKind::LinearDistance,
        ..
    } = kind
    else {
        panic!("straight depth");
    };
    let Feature::Derived {
        geometry: Some(g), ..
    } = features[from.0]
    else {
        panic!("derived entry point");
    };
    let geometry = &model.geometry[model.derived_geometry[g.0].supplemental[0].0];
    let pmi::GeometryValue::List(coords) = &geometry.item.leaves[0].1[1] else {
        panic!("Cartesian point");
    };
    let coords: Vec<_> = coords
        .iter()
        .map(|v| match v {
            pmi::GeometryValue::Real(n) => n.to_f64(),
            _ => panic!("coordinate"),
        })
        .collect();
    // The actual axis/entry-plane intersection, not the tilted ellipse's extreme.
    assert!((coords[0] + 203.515275872).abs() < 1e-7);
    assert!((planned.required.nominal - 21.484724).abs() < 1e-9);
    let haecceity::geom::Surface::Plane { frame } = loaded.part.faces[619].surface else {
        panic!("entry plane");
    };
    assert!(
        haecceity::geom::dot(
            haecceity::geom::sub([coords[0], coords[1], coords[2]], frame.origin),
            frame.z
        )
        .abs()
            < 1e-7
    );
    assert!((coords[1] - 200.).abs() < 1e-7);
    assert!((coords[2] + 185.).abs() < 1e-7);
    let mut repeated_features = Vec::new();
    let mut repeated_model = PartPmi::default();
    let repeated = planned_depth_kind(
        &mut planned,
        &input,
        &loaded.part,
        &mut Features(&mut repeated_features),
        &mut repeated_model,
        &LengthUnit::Millimetre,
    )
    .unwrap();
    assert_eq!(repeated, kind);
    assert_eq!(repeated_features, features);
    assert_eq!(repeated_model, model);
    let Feature::Items(anchors) = &features[to.0] else {
        panic!("counterbore shoulder plane");
    };
    assert!(anchors.iter().all(|a| matches!(a, Anchor::Face(_))));
    model.features = features;
    model.dimensions.push(Dimension {
        kind,
        nominal: Some(mm(planned.required.nominal).unwrap()),
        tolerance: DimTolerance::None,
        qualifier: None,
        modifiers: vec![],
        principle: None,
    });
    let source = Source::of(&loaded.bytes).unwrap();
    let (edit, _) = pmi::write(
        &source.doc,
        &source.defs,
        &[(PartId(0), model)],
        Mode::Replace,
        PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let bytes = source.doc.apply(&edit).unwrap().bytes;
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains("DIMENSIONAL_LOCATION('linear distance'"));
    assert!(text.contains("DERIVED_SHAPE_ASPECT("));
    assert!(text.contains("CONSTRUCTIVE_GEOMETRY_REPRESENTATION("));
    let read = pmi::read(&Document::parse(bytes).unwrap(), &source.defs).unwrap();
    let d = read.parts[0]
        .dimensions
        .iter()
        .find(|d| {
            matches!(&d.nominal.as_ref().unwrap().quantity,
        pmi::Quantity::Length(n) if (n.mm()-21.484724).abs()<1e-8)
        })
        .unwrap();
    let DimensionKind::Location { from, .. } = d.kind else {
        panic!("linear location on readback");
    };
    assert!(matches!(
        read.parts[0].features[from.0],
        Feature::Derived {
            geometry: Some(_),
            ..
        }
    ));
}

#[test]
fn unresolved_pattern_depth_rolls_back_its_partial_endpoint_geometry() {
    let loaded = crate::part::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/parts/electronics-enclosure-base.step.gz"),
        None,
    )
    .unwrap();
    let input = crate::dimensioning::input(&loaded.part).unwrap();
    let hole = input.holes.iter().find(|h| h.depth == Some(8.)).unwrap();
    let mut planned = crate::dimensioning::Planned {
        required: quiddity::dimensioning::Required {
            id: "test:unresolved-pattern.depth".into(),
            kind: quiddity::dimensioning::Kind::Size {
                feature: hole.support.feature.clone(),
                role: "bore.depth".into(),
                kind: quiddity::dimensioning::SizeKind::Length,
            },
            nominal: 8.,
            count: 2,
            pattern: None,
            tolerance: quiddity::dimensioning::Tolerance::General,
            reference: None,
            covers: vec![hole.support.feature.clone(), "missing hole".into()],
        },
        supports: vec![hole.support.clone()],
        groups: Default::default(),
        frame: input.frame,
        witnesses: None,
    };
    planned.bore_supports(&loaded.part);
    let mut features = vec![Feature::Items(vec![Anchor::Face(FaceIndex(0))])];
    let prior = features.clone();
    let mut model = PartPmi::default();
    let error = planned_depth_kind(
        &mut planned,
        &input,
        &loaded.part,
        &mut Features(&mut features),
        &mut model,
        &LengthUnit::Millimetre,
    )
    .unwrap_err();
    assert!(error.to_string().contains("missing hole"));
    assert_eq!(features, prior);
    assert_eq!(model, PartPmi::default());
}

#[test]
fn a_circular_entry_wins_over_a_conservative_cylinder_trim_bound() {
    use haecceity::geom::{Curve, Surface, dot, sub};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/resume_alpha7/cgb217.step.gz");
    let loaded = crate::part::load(&path, None).unwrap();
    let input = crate::dimensioning::input(&loaded.part).unwrap();
    let hole = input
        .holes
        .iter()
        .find(|h| h.support.feature == "hole:105.106")
        .unwrap();
    let mut planned = crate::dimensioning::Planned {
        required: quiddity::dimensioning::Required {
            id: "test:circular-entry.depth".into(),
            kind: quiddity::dimensioning::Kind::Size {
                feature: hole.support.feature.clone(),
                role: "bore.depth".into(),
                kind: quiddity::dimensioning::SizeKind::Length,
            },
            nominal: 24.5,
            count: 1,
            pattern: None,
            tolerance: quiddity::dimensioning::Tolerance::General,
            reference: None,
            covers: vec![hole.support.feature.clone()],
        },
        supports: vec![hole.support.clone()],
        groups: Default::default(),
        frame: input.frame,
        witnesses: None,
    };
    let mut features = Vec::new();
    let mut model = PartPmi::default();
    let kind = planned_depth_kind(
        &mut planned,
        &input,
        &loaded.part,
        &mut Features(&mut features),
        &mut model,
        &LengthUnit::Millimetre,
    )
    .unwrap();
    let DimensionKind::Location {
        from,
        to,
        kind: LocationKind::LinearDistance,
        ..
    } = kind
    else {
        panic!("straight depth");
    };
    let origin = input.frame.to_world(hole.support.at);
    let direction = sub(
        input.frame.to_world(std::array::from_fn(|k| {
            hole.support.at[k] + hole.direction[k]
        })),
        origin,
    );
    for (id, station) in [(from, 0.), (to, 24.5)] {
        let Feature::Items(anchors) = &features[id.0] else {
            panic!("physical endpoint");
        };
        assert!(!anchors.is_empty());
        for anchor in anchors {
            let point = match anchor {
                Anchor::Face(f) => match &loaded.part.faces[f.0].surface {
                    Surface::Plane { frame } => frame.origin,
                    _ => panic!("plane"),
                },
                Anchor::Edge(e) => match &loaded.part.edges[e.0].curve {
                    Curve::Circle { frame, .. } => frame.origin,
                    _ => panic!("circle"),
                },
                _ => panic!("actual geometry required"),
            };
            assert!((dot(sub(point, origin), direction) - station).abs() < 1e-7);
        }
    }
    assert!(model.geometry.is_empty());
}

#[test]
fn disjoint_coplanar_features_do_not_convey_each_others_dimensions() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/revisions/complex_counterbore.new.step.gz");
    let loaded = crate::part::load(&path, None).unwrap();
    let a = DepthPlane::of(&loaded.part, &[21]).unwrap();
    let b = DepthPlane::of(&loaded.part, &[22]).unwrap();
    assert!(haecceity::geom::dot(haecceity::geom::sub(a.at, b.at), a.normal).abs() < 1e-8);
    assert!(
        !a.same(&b),
        "different hole floors remain different physical carriers"
    );
    let group = DepthPlane::of(&loaded.part, &[21, 22]).unwrap();
    assert!(
        a.same(&group),
        "a witness within the depth's group is the same carrier"
    );
}

#[test]
fn equal_counterbores_at_opposite_ends_have_distinct_mouth_to_floor_depths() {
    use haecceity::{
        Part,
        brep::{Edge, Face, Loop, Solid},
        geom::{Curve, Frame, Surface},
    };
    let rings = [
        (20., 0.),
        (20., 40.),
        (13., 40.),
        (13., 33.),
        (5., 33.),
        (5., 7.),
        (13., 7.),
        (13., 0.),
    ];
    let frame = |z| Frame {
        origin: [0., 0., z],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
        z: [0., 0., 1.],
    };
    let mut edges: Vec<_> = rings
        .iter()
        .enumerate()
        .map(|(i, &(radius, z))| {
            let curve = Curve::Circle {
                frame: frame(z),
                radius,
            };
            Edge {
                start: curve.value(0.),
                end: curve.value(0.),
                samples: (0..=32)
                    .map(|j| curve.value(std::f64::consts::TAU * j as f64 / 32.))
                    .collect(),
                curve,
                vertices: (i, i),
                same_sense: true,
            }
        })
        .collect();
    let mut faces = Vec::new();
    for (i, &(radius, z)) in rings.iter().enumerate() {
        let j = (i + 1) % rings.len();
        let (next_radius, next_z) = rings[j];
        let (surface, reversed, boundaries) = if radius == next_radius {
            let (bottom, top) = if z < next_z { (i, j) } else { (j, i) };
            let seam = edges.len();
            let (start, end) = (edges[bottom].start, edges[top].start);
            edges.push(Edge {
                curve: Curve::Line {
                    origin: start,
                    dir: [0., 0., 1.],
                },
                start,
                end,
                vertices: (bottom, top),
                same_sense: true,
                samples: vec![start, end],
            });
            (
                Surface::Cylinder {
                    frame: frame(0.),
                    radius,
                },
                z > next_z,
                vec![vec![
                    (bottom, z < next_z),
                    (seam, true),
                    (top, z > next_z),
                    (seam, false),
                ]],
            )
        } else {
            assert_eq!(z, next_z);
            (
                Surface::Plane { frame: frame(z) },
                radius < next_radius,
                vec![vec![(i, true)], vec![(j, false)]],
            )
        };
        faces.push(Face {
            surface,
            reversed,
            solid: Some(0),
            pcurves: vec![],
            loops: boundaries
                .into_iter()
                .map(|edges| Loop {
                    edges,
                    vertex: None,
                })
                .collect(),
        });
    }
    let part = Part::new(
        faces,
        edges,
        vec![Solid {
            faces: (0..rings.len()).collect(),
        }],
    );
    let mut input = crate::dimensioning::input(&part).unwrap();
    let h = input
        .holes
        .iter_mut()
        .find(|h| (h.diameter - 10.).abs() < 1e-8)
        .unwrap();
    // #169's published contract has equal diameter/depth quantities at both
    // mouths. Preserve the actual geometry and exercise those two distinct roles.
    use quiddity::dimensioning::{Kind, Required, Size, SizeKind, Tolerance};
    h.details = vec![
        Size {
            role: "counterbore.diameter".into(),
            nominal: 26.,
            kind: SizeKind::Diameter,
        },
        Size {
            role: "counterbore.depth".into(),
            nominal: 7.,
            kind: SizeKind::Length,
        },
        Size {
            role: "outer_steps.0.diameter".into(),
            nominal: 26.,
            kind: SizeKind::Diameter,
        },
        Size {
            role: "outer_steps.0.depth".into(),
            nominal: 7.,
            kind: SizeKind::Length,
        },
    ];
    let h = h.clone();
    let origin = input.frame.to_world(h.support.at);
    let direction = haecceity::geom::sub(
        input
            .frame
            .to_world(std::array::from_fn(|k| h.support.at[k] + h.direction[k])),
        origin,
    );
    let expected = if direction[2] < 0. {
        [[40., 33.], [0., 7.]]
    } else {
        [[0., 7.], [40., 33.]]
    };
    let mut all_faces = Vec::new();
    for (i, role) in ["counterbore.depth", "outer_steps.0.depth"]
        .into_iter()
        .enumerate()
    {
        let mut plan = crate::dimensioning::Planned {
            required: Required {
                id: format!("test:{role}"),
                kind: Kind::Size {
                    feature: h.support.feature.clone(),
                    role: role.into(),
                    kind: SizeKind::Length,
                },
                nominal: 7.,
                count: 1,
                pattern: None,
                tolerance: Tolerance::General,
                reference: None,
                covers: vec![h.support.feature.clone()],
            },
            supports: vec![h.support.clone()],
            groups: Default::default(),
            frame: input.frame,
            witnesses: None,
        };
        let mut features = Vec::new();
        let kind = planned_depth_kind(
            &mut plan,
            &input,
            &part,
            &mut Features(&mut features),
            &mut PartPmi::default(),
            &LengthUnit::Millimetre,
        )
        .unwrap();
        let DimensionKind::Location {
            from,
            to,
            kind: LocationKind::LinearDistance,
            ..
        } = kind
        else {
            panic!("linear depth")
        };
        let mut supports = Vec::new();
        for (end, z) in [from, to].into_iter().zip(expected[i]) {
            let Feature::Items(items) = &features[end.0] else {
                panic!("physical endpoint")
            };
            assert_eq!(items.len(), 1);
            let Anchor::Face(f) = items[0] else {
                panic!("plane endpoint")
            };
            let Surface::Plane { frame } = &part.faces[f.0].surface else {
                panic!("plane")
            };
            assert!(
                (frame.origin[2] - z).abs() < 1e-8,
                "{role}: {} != {z}",
                frame.origin[2]
            );
            supports.push(f.0);
        }
        assert_eq!(plan.required.nominal, 7.);
        all_faces.push(supports);
    }
    assert!(all_faces[0].iter().all(|f| !all_faces[1].contains(f)));
}
