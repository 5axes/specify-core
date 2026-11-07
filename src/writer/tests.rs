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
fn lm10uu_semantics_match_the_v042_workaround_build() {
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
    let expected: Json = serde_json::from_str(include_str!(
        "../../tests/fixtures/freecad/lm10uu-v042-semantics.json"
    ))
    .unwrap();
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
