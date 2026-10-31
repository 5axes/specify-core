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
    let a = vec![Map::new(); 2];
    let bytes = write(&f, &a);
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
                ("bom.description".into(), json!(p.name.replace('_', " "))),
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
            a.insert(
                "part.material".into(),
                json!(if name.contains("blade retainer") {
                    "Copper"
                } else if name.contains("blade") && !name.contains("spacer") {
                    "O1 tool steel"
                } else if name.contains("push rod")
                    || name.contains("screw") && !name.contains("drive")
                {
                    "Steel"
                } else {
                    "Brass"
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
