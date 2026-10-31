//! For #79: STEP is the assembly BOM authority; no entity-number identities.
use haecceity::p21::{self, Attribute as A, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use specify_core::{api, bom, part, resume, revision, rules, writer};
use std::path::{Path, PathBuf};

fn fixture() -> part::File {
    part::File::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/assembly.step"),
    )
    .unwrap()
}
fn write(file: &part::File, answers: &[Map<String, Value>]) -> Vec<u8> {
    let parts = file.load_all().unwrap();
    let intents: Vec<_> = parts
        .iter()
        .zip(answers)
        .map(|(p, a)| {
            let mut intent = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
            let fields: Map<_, _> = a
                .iter()
                .filter_map(|(k, v)| k.strip_prefix("bom.").map(|k| (k.into(), v.clone())))
                .collect();
            if !fields.is_empty() {
                intent.part.insert("bom".into(), json!(fields));
            }
            if let Some(material) = a.get("part.material") {
                intent.part.insert("material".into(), material.clone());
            }
            intent
        })
        .collect();
    let jobs: Vec<_> = parts
        .iter()
        .zip(&intents)
        .zip(answers)
        .map(|((p, i), a)| (p, i, Some(a)))
        .collect();
    writer::write_parts_bytes(&jobs).unwrap().0
}
fn file(bytes: Vec<u8>) -> part::File {
    part::File::of_bytes(Path::new("bom.step"), bytes).unwrap()
}
fn answers() -> Vec<Map<String, Value>> {
    vec![json!({"bom.make_buy":"made","bom.description":"Mounting plate","bom.optional":"yes","bom.stock":"to be made","bom.part_number":"PL-01","bom.revision":"A","part.material":"brass"}).as_object().unwrap().clone(),
    json!({"bom.make_buy":"bought","bom.description":"Locating pin","bom.stock":"from stock","bom.optional":"no"}).as_object().unwrap().clone()]
}
#[test]
fn round_trip_lossless_resumed_and_noop_with_sources_and_standard_material() {
    let source = fixture();
    let bytes = write(&source, &answers());
    let doc = Document::parse(bytes.clone()).unwrap();
    let before = Document::parse(source.bytes().as_ref().clone()).unwrap();
    for id in before.ids() {
        assert_eq!(
            before.span(id).map(|r| &source.bytes()[r]),
            doc.span(id).map(|r| &bytes[r]),
            "#{id}"
        );
    }
    assert!(
        haecceity::express::validate_document(doc.graph()).is_empty(),
        "{:?}",
        haecceity::express::validate_document(doc.graph())
    );
    assert!(
        haecceity::express_rules::check_all(&doc).is_empty(),
        "{:?}",
        haecceity::express_rules::check_all(&doc)
    );
    let rows = bom::read_bytes(bytes.clone()).unwrap();
    assert_eq!(rows[0].item_number.value, 1);
    assert_eq!(rows[1].item_number.value, 2);
    assert_eq!(rows[1].quantity.value, 2);
    assert_eq!(rows[0].optional.value, true);
    assert_eq!(rows[0].description.source, "answered");
    assert_eq!(rows[0].material.value, "brass");
    assert_eq!(rows[0].material.source, "answered");
    let back = file(bytes.clone());
    for (p, a) in back.load_all().unwrap().iter().zip(answers()) {
        assert_eq!(resume::answers_for(p).unwrap(), Some(a));
    }
    assert_eq!(write(&back, &answers()), bytes);
    let p = back.load(Some(0)).unwrap();
    let analysis = api::analyse(&p).unwrap();
    assert_eq!(analysis["bom"]["item_number"]["value"], 1);
    assert!(
        api::questions(&analysis, &Map::new())
            .unwrap()
            .iter()
            .any(|q| q.id == "bom.make_buy" && !q.reason.is_empty())
    );
}

#[test]
fn no_bom_answers_add_no_bom_records() {
    let f = fixture();
    let mut jobs = Vec::new();
    for p in f.load_all().unwrap() {
        let analysis = api::analyse_reporting_in(&f, &p).unwrap().0;
        let given = json!({"part.material":"6061-T6"})
            .as_object()
            .unwrap()
            .clone();
        let complete = rules::with_all_defaults(&analysis, &given).unwrap();
        assert!(complete.keys().all(|key| !key.starts_with("bom.")));
        assert!(
            rules::open_questions(&api::questions(&analysis, &complete).unwrap(), &complete)
                .is_empty()
        );
        for defaults in [false, true] {
            let intent = api::apply(&analysis, &complete, defaults).unwrap();
            assert!(!intent.part.contains_key("bom"));
        }
        jobs.push((
            api::apply(&analysis, &complete, false).unwrap(),
            Some(complete),
        ));
    }
    let bytes = api::write_bytes(f.bytes().as_ref().clone(), &jobs)
        .unwrap()
        .0;
    assert!(!String::from_utf8_lossy(&bytes).contains(bom::PROPERTY));
    assert!(
        bom::read_bytes(bytes)
            .unwrap()
            .iter()
            .all(|r| r.item_number.value.is_null())
    );
}

fn attrs(doc: &Document, id: u64, name: &str) -> Option<Vec<A>> {
    match doc.get(id)? {
        RawEntity::Simple {
            name: n,
            attributes,
            ..
        } if n == name => Some(attributes.clone()),
        _ => None,
    }
}
fn added(source: &part::File) -> part::File {
    let doc = Document::parse(source.bytes().as_ref().clone()).unwrap();
    let def = &source.definitions()[0];
    let pd = attrs(&doc, def.product_definition, "PRODUCT_DEFINITION").unwrap();
    let mut edit = Edit::new();
    let context = match doc
        .get(match pd[2] {
            A::EntityRef(id) => id,
            _ => panic!(),
        })
        .unwrap()
    {
        RawEntity::Simple { attributes, .. } => attributes[2].clone(),
        _ => panic!(),
    };
    let product_attrs = match context {
        A::EntityRef(id) => attrs(&doc, id, "PRODUCT").unwrap(),
        _ => panic!(),
    };
    let product = edit.add(p21::simple(
        "PRODUCT",
        vec![
            A::String("extra".into()),
            A::String("extra".into()),
            A::String("".into()),
            product_attrs[3].clone(),
        ],
    ));
    let form = edit.add(p21::simple(
        "PRODUCT_DEFINITION_FORMATION",
        vec![A::String("".into()), A::String("".into()), product.to_ref()],
    ));
    let pd_new = edit.add(p21::simple(
        "PRODUCT_DEFINITION",
        vec![pd[0].clone(), pd[1].clone(), form.to_ref(), pd[3].clone()],
    ));
    let shape = edit.add(p21::simple(
        "PRODUCT_DEFINITION_SHAPE",
        vec![A::String("".into()), A::Unset, pd_new.to_ref()],
    ));
    let rep = doc
        .ids()
        .find_map(|id| {
            attrs(&doc, id, "SHAPE_DEFINITION_REPRESENTATION")
                .filter(|a| a[0] == A::EntityRef(def.shape))
        })
        .unwrap()[1]
        .clone();
    edit.add(p21::simple(
        "SHAPE_DEFINITION_REPRESENTATION",
        vec![shape.to_ref(), rep],
    ));
    let parent = doc
        .ids()
        .find_map(|id| attrs(&doc, id, "NEXT_ASSEMBLY_USAGE_OCCURRENCE"))
        .unwrap()[3]
        .clone();
    edit.add(p21::simple(
        "NEXT_ASSEMBLY_USAGE_OCCURRENCE",
        vec![
            A::String("extra".into()),
            A::String("".into()),
            A::String("".into()),
            parent,
            pd_new.to_ref(),
            A::Unset,
        ],
    ));
    file(doc.apply(&edit).unwrap().bytes)
}
fn removed(source: &part::File, part: usize) -> part::File {
    let doc = Document::parse(source.bytes().as_ref().clone()).unwrap();
    let shape = source.definitions()[part].shape;
    let mut edit = Edit::new();
    for id in doc.ids() {
        if attrs(&doc, id, "SHAPE_DEFINITION_REPRESENTATION")
            .is_some_and(|a| a[0] == A::EntityRef(shape))
        {
            edit.remove(id);
        }
    }
    file(doc.apply(&edit).unwrap().bytes)
}
#[test]
fn adding_removing_and_readding_retires_numbers_with_geometric_identity() {
    let first = file(write(&fixture(), &answers()));
    let fresh = fixture();
    let expanded = added(&fresh);
    assert_eq!(expanded.definitions().len(), 3);
    let mut given = answers();
    given[0].extend(
        revision::diff(
            &first.load(Some(0)).unwrap(),
            &expanded.load(Some(0)).unwrap(),
            &Map::new(),
        )
        .unwrap()
        .answers,
    );
    given.push(json!({"bom.make_buy":"made"}).as_object().unwrap().clone());
    let grown = file(write(&expanded, &given));
    let rows = bom::read(&grown).unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r.item_number.value.clone())
            .collect::<Vec<_>>(),
        vec![json!(1), json!(2), json!(3)]
    );
    let shrunk = removed(&expanded, 0);
    assert_eq!(shrunk.definitions().len(), 2);
    // Stripping an old leaf's shape changes which indexes are loaded, not identities.
    let mut surviving = given[1..].to_vec();
    surviving[0].extend(
        revision::diff(
            &grown.load(Some(1)).unwrap(),
            &shrunk.load(Some(0)).unwrap(),
            &Map::new(),
        )
        .unwrap()
        .answers,
    );
    let shrunk_written = file(write(&shrunk, &surviving));
    assert_eq!(
        bom::read(&shrunk_written)
            .unwrap()
            .iter()
            .map(|r| r.item_number.value.clone())
            .collect::<Vec<_>>(),
        vec![json!(2), json!(3)]
    );
    let restored = added(&shrunk);
    let mut a = surviving;
    a[0].extend(
        revision::diff(
            &shrunk_written.load(Some(0)).unwrap(),
            &restored.load(Some(0)).unwrap(),
            &Map::new(),
        )
        .unwrap()
        .answers,
    );
    let mut restored_answers = given[0].clone();
    restored_answers.remove(bom::CARRY);
    a.push(restored_answers);
    let restored = file(write(&restored, &a));
    assert_eq!(
        bom::read(&restored)
            .unwrap()
            .iter()
            .map(|r| r.item_number.value.clone())
            .collect::<Vec<_>>(),
        vec![json!(2), json!(3), json!(4)]
    );
}

#[test]
fn exported_description_and_revision_are_file_data_and_overrides_append() {
    let f = fixture();
    let doc = Document::parse(f.bytes().as_ref().clone()).unwrap();
    let mut edit = Edit::new();
    let product = doc
        .ids()
        .find(|&id| attrs(&doc, id, "PRODUCT").is_some_and(|a| a[1] == A::String("plate".into())))
        .unwrap();
    let mut a = attrs(&doc, product, "PRODUCT").unwrap();
    a[2] = A::String("Exported description".into());
    edit.replace(product, p21::simple("PRODUCT", a));
    let source = file(doc.apply(&edit).unwrap().bytes);
    let rows = bom::read(&source).unwrap();
    assert_eq!(rows[0].description.value, "Exported description");
    assert_eq!(rows[0].description.source, "file");
    let mut a = vec![Map::new(); 2];
    a[1].insert("bom.make_buy".into(), json!("bought"));
    let back = file(write(&source, &a));
    assert_eq!(
        bom::read(&back).unwrap()[0].description,
        rows[0].description
    );
}

#[test]
fn revision_transport_retains_history_without_entity_ids() {
    let first = file(write(&fixture(), &answers()));
    let old = first.load(Some(0)).unwrap();
    let new = fixture().load(Some(0)).unwrap();
    let carried = revision::diff(&old, &new, &Map::new()).unwrap().answers;
    assert!(carried.contains_key(bom::CARRY));
    let mut a = answers();
    a[0].extend(carried);
    assert_eq!(
        bom::read_bytes(write(&fixture(), &a)).unwrap()[0]
            .item_number
            .value,
        1
    );
}

#[test]
fn gramil_acceptance() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bom/gramil.step.gz");
    let source = part::File::read(&path).unwrap();
    let a: Vec<_> = source
        .parts()
        .iter()
        .map(|p| {
            let name = p.name.to_lowercase();
            let mut a = Map::from_iter([
                (
                    "bom.make_buy".into(),
                    json!(if !name.starts_with("grm-") && name != "push rod" {
                        "bought"
                    } else {
                        "made"
                    }),
                ),
                (
                    "bom.description".into(),
                    json!(if name.starts_with("grm-06") {
                        "Bone-shaped copper blade retainer".to_owned()
                    } else if name == "push rod" {
                        "Steel push rod, cut to length".to_owned()
                    } else {
                        p.name.replace('_', " ")
                    }),
                ),
                (
                    "bom.stock".into(),
                    json!(if name.contains("push rod") {
                        "from stock"
                    } else {
                        "to be made"
                    }),
                ),
                (
                    "bom.optional".into(),
                    json!(if name.contains("spacer") { "yes" } else { "no" }),
                ),
            ]);
            if name.starts_with("grm-") {
                a.insert(
                    "bom.part_number".into(),
                    json!(p.name.split_whitespace().next().unwrap()),
                );
            }
            a.insert(
                "part.material".into(),
                json!(if name.contains("blade retainer") {
                    "Copper sheet (annealed)"
                } else if name.contains("blade") && !name.contains("spacer") {
                    "O1 tool steel, hardened ~60 HRC"
                } else if name.contains("push rod") {
                    "Mild steel, bright-drawn"
                } else if name.contains("pan-head") || name.contains("washer") {
                    "Stainless steel"
                } else if name.contains("screw") && !name.contains("drive") {
                    "Steel"
                } else {
                    "Brass H62 (CuZn40)"
                }),
            );
            a
        })
        .collect();
    let bytes = write(&source, &a);
    let original = Document::parse(source.bytes().as_ref().clone()).unwrap();
    let written = Document::parse(bytes.clone()).unwrap();
    for id in original.ids() {
        assert_eq!(
            original.span(id).map(|r| &source.bytes()[r]),
            written.span(id).map(|r| &bytes[r])
        );
    }
    let rows = bom::read_bytes(bytes.clone()).unwrap();
    assert!(
        rows.iter()
            .all(|r| r.make_buy.source == "answered" && r.description.source == "answered")
    );
    // This supplied export has a zero-thickness spacer, hence no spacer occurrence.
    assert!(rows.iter().all(|r| r.optional.value == false));
    assert!(rows.iter().any(|r| r.stock.value == "from stock"));
    assert_eq!(write(&file(bytes.clone()), &a), bytes);
    let dir = std::env::temp_dir().join("specify-bom-79");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("gramil.step"), bytes).unwrap();
    std::fs::write(
        dir.join("gramil.json"),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
    // A real 1 x 3.6 x 23 mm channel shim, matching gramil's Box construction.
    let variant = part::File::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bom/gramil-spacer.step.gz"),
    )
    .unwrap();
    let mut variant_answers = a.clone();
    variant_answers[0].extend(
        revision::diff(
            &file(std::fs::read(dir.join("gramil.step")).unwrap())
                .load(Some(0))
                .unwrap(),
            &variant.load(Some(0)).unwrap(),
            &Map::new(),
        )
        .unwrap()
        .answers,
    );
    variant_answers.push(json!({"bom.make_buy":"made","bom.description":"Channel-width shim, 1 mm","bom.optional":"yes","bom.stock":"from stock","part.material":"Brass"}).as_object().unwrap().clone());
    let variant_bytes = write(&variant, &variant_answers);
    let variant_rows = bom::read_bytes(variant_bytes.clone()).unwrap();
    assert_eq!(variant_rows.len(), rows.len() + 1);
    assert_eq!(
        variant_rows[..rows.len()]
            .iter()
            .map(|r| r.item_number.value.clone())
            .collect::<Vec<_>>(),
        rows.iter()
            .map(|r| r.item_number.value.clone())
            .collect::<Vec<_>>()
    );
    let spacer = variant_rows.last().unwrap();
    assert_eq!(spacer.name.value, "channel spacer");
    assert_eq!(spacer.item_number.value, 13);
    assert_eq!(spacer.optional.value, true);
    assert_eq!(spacer.quantity.value, 1);
    std::fs::write(dir.join("gramil-spacer.step"), variant_bytes).unwrap();
    std::fs::write(
        dir.join("gramil-spacer.json"),
        serde_json::to_vec_pretty(&variant_rows).unwrap(),
    )
    .unwrap();
    println!("gramil artifacts: {}", dir.display());
}

#[test]
fn existing_source_material_category_and_numeric_find_numbers_survive() {
    let source = fixture();
    let doc = Document::parse(source.bytes().as_ref().clone()).unwrap();
    let mut edit = Edit::new();
    for (i, def) in source.definitions().iter().enumerate() {
        for id in doc.ids() {
            if let Some(mut a) = attrs(&doc, id, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")
                && a[4] == A::EntityRef(def.product_definition)
            {
                a[1] = A::String(if i == 0 { "7" } else { "9" }.into());
                edit.replace(id, p21::simple("NEXT_ASSEMBLY_USAGE_OCCURRENCE", a));
            }
        }
    }
    let pd = attrs(
        &doc,
        source.definitions()[0].product_definition,
        "PRODUCT_DEFINITION",
    )
    .unwrap();
    let A::EntityRef(formation) = pd[2] else {
        panic!()
    };
    let mut f = attrs(&doc, formation, "PRODUCT_DEFINITION_FORMATION").unwrap();
    f[0] = A::String("R3".into());
    f.push(A::Enum("BOUGHT".into()));
    edit.replace(
        formation,
        p21::simple("PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE", f),
    );
    let bytes = doc.apply(&edit).unwrap().bytes;
    let doc = Document::parse(bytes.clone()).unwrap();
    let pmi = haecceity::pmi::PartPmi {
        material: Some(haecceity::pmi::Material {
            id: "Aluminium 6061".into(),
            name: None,
            density: None,
        }),
        ..Default::default()
    };
    let (edit, _) = haecceity::pmi::write(
        &doc,
        source.definitions(),
        &[(haecceity::pmi::PartId(0), pmi)],
        haecceity::pmi::write::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::default(),
    )
    .unwrap();
    let source = file(doc.apply(&edit).unwrap().bytes);
    let rows = bom::read(&source).unwrap();
    assert_eq!(rows[0].item_number.value, 7);
    assert_eq!(rows[1].item_number.value, 9);
    assert_eq!(
        rows[0].revision,
        bom::Field {
            value: json!("R3"),
            source: "file".into()
        }
    );
    assert_eq!(
        rows[0].make_buy,
        bom::Field {
            value: json!("bought"),
            source: "file".into()
        }
    );
    assert_eq!(
        rows[0].material,
        bom::Field {
            value: json!("Aluminium 6061"),
            source: "file".into()
        }
    );
    let a = vec![
        json!({"bom.description":"Only this changed"})
            .as_object()
            .unwrap()
            .clone(),
        Map::new(),
    ];
    let bytes = write(&source, &a);
    let after = bom::read_bytes(bytes.clone()).unwrap();
    for key in ["item_number", "revision", "make_buy", "material"] {
        assert_eq!(
            serde_json::to_value(&rows[0]).unwrap()[key],
            serde_json::to_value(&after[0]).unwrap()[key],
            "{key}"
        );
    }
    let original = Document::parse(source.bytes().as_ref().clone()).unwrap();
    let back = Document::parse(bytes.clone()).unwrap();
    for id in original.ids() {
        assert_eq!(
            original.span(id).map(|r| &source.bytes()[r]),
            back.span(id).map(|r| &bytes[r])
        );
    }
}

#[test]
fn invalid_optional_and_transport_are_refused_and_destination_history_wins() {
    let f = fixture();
    let p = f.load(Some(1)).unwrap();
    let mut intent = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    intent.part.insert("bom".into(), json!({"optional":"yes"}));
    assert!(
        writer::write_bytes(&p, &intent, Some(&Map::new()))
            .unwrap_err()
            .to_string()
            .contains("exactly one occurrence")
    );
    intent
        .part
        .insert("bom".into(), json!({"make_buy":"sometimes"}));
    assert!(writer::write_bytes(&p, &intent, Some(&Map::new())).is_err());
    intent.part.insert("bom".into(), json!({"make_buy":"made"}));
    let invalid = json!({bom::CARRY:{"schema":99}})
        .as_object()
        .unwrap()
        .clone();
    assert!(writer::write_bytes(&p, &intent, Some(&invalid)).is_err());
    let written = file(write(&f, &answers()));
    let mut a = answers();
    a[0].extend(invalid);
    assert_eq!(
        bom::read_bytes(write(&written, &a)).unwrap()[0]
            .item_number
            .value,
        1
    );
}

#[test]
fn entity_renumbering_and_part_reordering_do_not_establish_identity() {
    let original = fixture();
    let numbered = file(write(&original, &answers()));
    let doc = Document::parse(original.bytes().as_ref().clone()).unwrap();
    let source = String::from_utf8(original.bytes().as_ref().clone()).unwrap();
    let renumbered = regex::Regex::new(r"#(\d+)")
        .unwrap()
        .replace_all(&source, |c: &regex::Captures| {
            format!("#{}", doc.max_id() + 1 - c[1].parse::<u64>().unwrap())
        });
    let reordered = file(renumbered.as_bytes().to_vec());
    let new_parts = reordered.load_all().unwrap();
    let mut a: Vec<_> = new_parts
        .iter()
        .map(|p| {
            let k = if p.name == "plate" { 0 } else { 1 };
            answers()[k].clone()
        })
        .collect();
    let old = numbered.load(Some(0)).unwrap();
    let new = new_parts.iter().find(|p| p.name == "plate").unwrap();
    a[0].extend(
        revision::diff(&old, new, &Map::new())
            .unwrap()
            .answers
            .into_iter()
            .filter(|(k, _)| k == bom::CARRY),
    );
    for row in bom::read_bytes(write(&reordered, &a)).unwrap() {
        assert_eq!(
            row.item_number.value,
            if row.name.value == "plate" { 1 } else { 2 }
        );
    }
}

#[test]
fn pmi_and_bom_share_one_verified_write_and_partial_edits_keep_other_rows() {
    let source = fixture();
    let loaded = source.load(Some(0)).unwrap();
    let a = json!({"bom.description":"Plate with datum A","part.material":"Brass"})
        .as_object()
        .unwrap()
        .clone();
    let mut intent = rules::Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent
        .part
        .insert("bom".into(), json!({"description":"Plate with datum A"}));
    intent.part.insert("material".into(), json!("Brass"));
    intent.datums.push(
        json!({"letter":"A","faces":[4]})
            .as_object()
            .unwrap()
            .clone(),
    );
    let bytes = writer::write_bytes(&loaded, &intent, Some(&a)).unwrap().0;
    let back = file(bytes.clone());
    let part = back.load(Some(0)).unwrap();
    assert_eq!(specify_core::characteristics::read(&part).unwrap().len(), 1);
    assert_eq!(
        bom::read(&back).unwrap()[0].description.value,
        "Plate with datum A"
    );
    intent.binding = serde_json::to_value(&part.binding).unwrap();
    assert_eq!(
        writer::write_bytes(&part, &intent, Some(&a)).unwrap().0,
        bytes
    );
    let other = back.load(Some(1)).unwrap();
    let mut intent = rules::Intent::new(serde_json::to_value(&other.binding).unwrap());
    intent
        .part
        .insert("bom".into(), json!({"stock":"from stock"}));
    let a = json!({"bom.stock":"from stock"})
        .as_object()
        .unwrap()
        .clone();
    let bytes = writer::write_bytes(&other, &intent, Some(&a)).unwrap().0;
    let rows = bom::read_bytes(bytes).unwrap();
    assert_eq!(rows[0].description.value, "Plate with datum A");
    assert_eq!(rows[1].stock.value, "from stock");
}

#[test]
fn an_assembly_with_every_part_renamed_keeps_bom_answers_and_numbers() {
    let source = fixture();
    let old = file(write(&source, &answers()));
    let doc = Document::parse(source.bytes().as_ref().clone()).unwrap();
    let mut edit = Edit::new();
    for id in doc.ids() {
        if let Some(mut a) = attrs(&doc, id, "PRODUCT")
            && matches!(a.get(1),Some(A::String(s)) if s=="plate" || s=="pin")
        {
            let A::String(name) = &a[1] else { panic!() };
            a[1] = A::String(format!("renamed {name}"));
            edit.replace(id, p21::simple("PRODUCT", a));
        }
        if let Some(mut a) = attrs(&doc, id, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")
            && matches!(a.get(1),Some(A::String(s)) if s=="plate" || s=="pin")
        {
            let A::String(name) = &a[1] else { panic!() };
            a[1] = A::String(format!("renamed {name}"));
            edit.replace(id, p21::simple("NEXT_ASSEMBLY_USAGE_OCCURRENCE", a));
        }
    }
    let new = file(doc.apply(&edit).unwrap().bytes);
    let compared = revision::assembly::diff(
        &old.load_all().unwrap(),
        &new.load_all().unwrap(),
        &Map::new(),
    )
    .unwrap();
    let transport = compared.answers();
    assert_eq!(transport.len(), 2);
    let a: Vec<_> = new
        .parts()
        .iter()
        .map(|p| {
            let mut a = transport[&p.name].as_object().unwrap().clone();
            a.insert("bom.stock".into(), json!("from stock"));
            a
        })
        .collect();
    let rows = bom::read_bytes(write(&new, &a)).unwrap();
    assert_eq!(rows[0].item_number.value, 1);
    assert_eq!(rows[1].item_number.value, 2);
    assert_eq!(rows[0].description.value, "Mounting plate");
    assert_eq!(rows[1].description.value, "Locating pin");
    assert_eq!(rows[0].material.value, "brass");
}

#[test]
fn indistinguishable_parts_keep_their_explicit_numbers_on_a_rerun() {
    let source = added(&fixture());
    let doc = Document::parse(source.bytes().as_ref().clone()).unwrap();
    let mut edit = Edit::new();
    let id = doc
        .ids()
        .find(|&id| attrs(&doc, id, "PRODUCT").is_some_and(|a| a[1] == A::String("extra".into())))
        .unwrap();
    let mut a = attrs(&doc, id, "PRODUCT").unwrap();
    a[1] = A::String("plate".into());
    edit.replace(id, p21::simple("PRODUCT", a));
    let source = file(doc.apply(&edit).unwrap().bytes);
    let mut a = answers();
    a.push(
        json!({"bom.description":"Second identical plate"})
            .as_object()
            .unwrap()
            .clone(),
    );
    let first = write(&source, &a);
    let second = write(&file(first.clone()), &a);
    assert_eq!(first, second);
    assert_eq!(
        bom::read_bytes(second)
            .unwrap()
            .iter()
            .map(|r| r.item_number.value.clone())
            .collect::<Vec<_>>(),
        vec![json!(1), json!(2), json!(3)]
    );
}

#[test]
fn explicit_bom_suggestions_are_not_answers_and_current_file_data_wins() {
    let f = fixture();
    let p = f.load(Some(0)).unwrap();
    let analysis = api::analyse_reporting_in(&f, &p).unwrap().0;
    let empty = Map::new();
    let mut intent = api::preview(&analysis, &empty).unwrap();
    bom::accept_suggestions(&analysis, &empty, &mut intent).unwrap();
    assert!(
        !intent.part["bom"]
            .as_object()
            .unwrap()
            .values()
            .any(|v| v == "")
    );
    let bytes = writer::write_bytes(&p, &intent, Some(&empty)).unwrap().0;
    let rows = bom::read_bytes(bytes.clone()).unwrap();
    assert_eq!(rows[0].description.source, "default");
    assert_eq!(rows[0].make_buy.source, "default");
    let back = file(bytes.clone());
    assert!(
        resume::answers_for(&back.load(Some(0)).unwrap())
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert!(resume::stored(&bytes).values().all(|p| {
        p["answers"]
            .as_object()
            .unwrap()
            .keys()
            .all(|key| !key.starts_with("bom."))
    }));
    // Simulate a later exporter populating native description/revision while keeping history.
    let doc = Document::parse(bytes).unwrap();
    let pd = attrs(
        &doc,
        back.definitions()[0].product_definition,
        "PRODUCT_DEFINITION",
    )
    .unwrap();
    let formation = match pd[2] {
        A::EntityRef(id) => id,
        _ => panic!(),
    };
    let mut fattrs = attrs(&doc, formation, "PRODUCT_DEFINITION_FORMATION").unwrap();
    fattrs[0] = A::String("B".into());
    let product = match fattrs[2] {
        A::EntityRef(id) => id,
        _ => panic!(),
    };
    let mut pattrs = attrs(&doc, product, "PRODUCT").unwrap();
    pattrs[2] = A::String("New CAD description".into());
    let mut edit = Edit::new();
    edit.replace(
        formation,
        p21::simple("PRODUCT_DEFINITION_FORMATION", fattrs),
    );
    edit.replace(product, p21::simple("PRODUCT", pattrs));
    let rows = bom::read_bytes(doc.apply(&edit).unwrap().bytes).unwrap();
    assert_eq!(rows[0].description.value, "New CAD description");
    assert_eq!(rows[0].description.source, "file");
    assert_eq!(rows[0].revision.value, "B");
    assert_eq!(rows[0].revision.source, "file");
}
