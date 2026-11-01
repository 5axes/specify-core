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
