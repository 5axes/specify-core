use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use specify_core::{process, resume};
use specify_core_wasm::session::Session;

fn write(session: &Session, answers: &Value) -> Vec<u8> {
    let written = session
        .write_pmi(answers.as_object().unwrap(), None, None)
        .unwrap();
    serde_json::from_value(written["bytes"].clone()).unwrap()
}

fn open(bytes: &[u8]) -> Session {
    Session::open("plate.step", bytes).unwrap()
}

fn item(name: &str, value: &str) -> String {
    format!("DESCRIPTIVE_REPRESENTATION_ITEM('{name}','{value}')")
}

/// Write only the process, so its removal exercises files with no other marked requirement.
fn edited_process(replacement: Option<(&str, &str)>) -> Vec<u8> {
    let plate = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let bytes = write(&open(plate), &json!({process::QUESTION: process::MACHINED}));
    let original = String::from_utf8(bytes).unwrap();
    let before = item(process::ITEM, process::MACHINED);
    assert_eq!(original.matches(&before).count(), 1);
    let text = replacement.map_or_else(
        || original.clone(),
        |(name, value)| original.replace(&before, &item(name, value)),
    );
    let doc = Document::parse(text.into_bytes()).unwrap();
    let property = doc
        .graph()
        .entities
        .iter()
        .find_map(|(&id, entity)| {
            matches!(entity, RawEntity::Simple {name, attributes, ..}
            if name == "PROPERTY_DEFINITION"
                && attributes.first() == Some(&Attribute::String(process::PROPERTY.into())))
            .then_some(id)
        })
        .unwrap();
    let link = *doc
        .referrers(property)
        .iter()
        .find(|&&id| {
            matches!(doc.get(id), Some(RawEntity::Simple {name, ..})
            if name == "PROPERTY_DEFINITION_REPRESENTATION")
        })
        .unwrap();
    let marker = *doc
        .referrers(link)
        .iter()
        .find(|&&id| {
            matches!(doc.get(id), Some(RawEntity::Simple {name, attributes, ..})
            if name == "DESCRIPTION_ATTRIBUTE"
                && matches!(attributes.first(), Some(Attribute::String(s))
                    if s == specify_core::writer::PMI_PROVENANCE))
        })
        .unwrap();
    let mut edit = Edit::new();
    edit.remove(marker);
    if replacement.is_none() {
        edit.remove(link);
    }
    doc.apply(&edit).unwrap().bytes
}

fn conflict(analysis: &Value) -> &Value {
    analysis["resume"]["conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == process::QUESTION)
        .unwrap()
}

fn assert_only_process(bytes: &[u8], name: &str, value: &str) {
    let doc = Document::parse(bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(bytes).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let values: Vec<_> = read.parts[0]
        .attributes
        .iter()
        .filter(|s| s.name == process::PROPERTY)
        .flat_map(|s| s.items.iter())
        .collect();
    assert_eq!(
        values,
        [&(
            name.into(),
            haecceity::pmi::AttributeValue::Text(value.into())
        )]
    );
}

#[test]
fn keeping_a_foreign_process_never_makes_it_replaceable() {
    let edited = edited_process(Some((process::ITEM, process::CAST)));
    let opened = open(&edited);
    assert_eq!(conflict(&opened.snapshot()["analysis"])["foreign"], true);
    let kept = write(&opened, &json!({"_specify": {"keep": [process::QUESTION]}}));
    let answered = write(&open(&kept), &json!({process::QUESTION: process::MOULDED}));
    assert_only_process(&answered, process::ITEM, process::CAST);
    assert_eq!(
        open(&answered).snapshot()["analysis"]["part"]["process"],
        process::CAST
    );
}

fn stale_answer_after_the_first_save(name: &str) {
    let edited = edited_process(Some((name, process::CAST)));
    let opened = open(&edited);
    assert_eq!(conflict(&opened.snapshot()["analysis"])["foreign"], true);
    let stale = json!({process::QUESTION: process::MACHINED});
    let first = write(&opened, &stale);
    let reopened = open(&first);
    assert!(
        reopened.snapshot()["analysis"]["resume"]
            .get("conflicts")
            .is_none()
    );
    let second = write(&reopened, &stale);
    assert_only_process(&second, name, process::CAST);
    assert!(
        resume::stored(&second)[&0]["answers"]
            .get(process::QUESTION)
            .is_none()
    );
    assert_eq!(
        open(&second).snapshot()["analysis"]["part"]["process"],
        process::CAST
    );
}

#[test]
fn a_stale_answer_is_filtered_on_every_save_with_the_canonical_item_name() {
    stale_answer_after_the_first_save(process::ITEM);
}

#[test]
fn a_stale_answer_is_filtered_on_every_save_with_a_normalised_item_name() {
    stale_answer_after_the_first_save("Manufacturing Process");
}

#[test]
fn a_foreign_process_outside_the_options_survives_rewrite() {
    let edited = edited_process(Some((process::ITEM, "laser cut")));
    let opened = open(&edited);
    let rewritten = write(&opened, &json!({}));
    assert_only_process(&rewritten, process::ITEM, "laser cut");
    let stale = write(
        &open(&rewritten),
        &json!({process::QUESTION: process::MACHINED}),
    );
    assert_only_process(&stale, process::ITEM, "laser cut");
}

#[test]
fn removing_the_only_marked_requirement_is_a_conflict() {
    let edited = edited_process(None);
    let opened = open(&edited);
    let snapshot = opened.snapshot();
    let analysis = &snapshot["analysis"];
    assert_eq!(conflict(analysis)["stored"], process::MACHINED);
    assert_eq!(conflict(analysis)["file"], Value::Null);
    let resumed = analysis["resume"]["answers"].as_object().unwrap();
    assert_eq!(resumed, &Map::new());
    let rewritten = write(&opened, &Value::Object(resumed.clone()));
    assert!(
        open(&rewritten).snapshot()["analysis"]
            .get("process")
            .is_none()
    );
}
