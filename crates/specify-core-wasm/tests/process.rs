use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use haecceity::pmi::{self, PartId, PartPmi};
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

fn marked_process(replacement: Option<(&str, &str)>) -> Document {
    let plate = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let bytes = write(&open(plate), &json!({process::QUESTION: process::MACHINED}));
    let original = String::from_utf8(bytes).unwrap();
    let before = item(process::ITEM, process::MACHINED);
    assert_eq!(original.matches(&before).count(), 1);
    let text = replacement.map_or_else(
        || original.clone(),
        |(name, value)| original.replace(&before, &item(name, value)),
    );
    Document::parse(text.into_bytes()).unwrap()
}

/// Write only the process, so its removal exercises files with no other marked requirement.
fn edited_process(replacement: Option<(&str, &str)>) -> Vec<u8> {
    let doc = marked_process(replacement);
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

fn add_foreign_process(bytes: Vec<u8>, set_name: &str, value: &str) -> Vec<u8> {
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let mut set = process::attribute_set(value, false);
    set.name = set_name.into();
    let foreign = PartPmi {
        attributes: vec![set],
        ..PartPmi::default()
    };
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(PartId(0), foreign)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

fn keep_marked_cast() -> Vec<u8> {
    let edited = marked_process(Some((process::ITEM, process::CAST)));
    let opened = open(edited.bytes());
    let snapshot = opened.snapshot();
    let analysis = &snapshot["analysis"];
    assert_eq!(conflict(analysis)["file"], process::CAST);
    assert!(conflict(analysis).get("foreign").is_none());
    let kept = write(&opened, &json!({"_specify": {"keep": [process::QUESTION]}}));
    assert_only_process(&kept, process::ITEM, process::CAST);
    kept
}

#[test]
fn a_supplier_process_with_the_kept_value_is_foreign() {
    let doc = Document::parse(keep_marked_cast()).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let kept = read.parts[0]
        .attributes
        .iter()
        .position(|set| set.name == process::PROPERTY)
        .unwrap();
    let mut edit = Edit::new();
    for &id in &read.provenance.parts[0].attributes[kept] {
        edit.remove(id);
    }
    let stripped = doc.apply(&edit).unwrap().bytes;
    let supplier = add_foreign_process(stripped, "supplier process", process::CAST);
    let session = open(&supplier);
    assert_eq!(
        session.snapshot()["analysis"]["part"]["process"],
        process::CAST
    );
    let saved = write(&session, &json!({process::QUESTION: process::MOULDED}));
    let doc = Document::parse(saved).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let sets: Vec<_> = read.parts[0]
        .attributes
        .iter()
        .filter(|s| s.items.iter().any(|(name, _)| name == process::ITEM))
        .map(|s| (s.name.as_str(), s.items.as_slice()))
        .collect();
    let expected = [(
        process::ITEM.into(),
        pmi::AttributeValue::Text(process::CAST.into()),
    )];
    assert_eq!(sets, [("supplier process", expected.as_slice())]);
    assert_eq!(
        open(doc.bytes()).snapshot()["analysis"]["part"]["process"],
        process::CAST
    );
}

#[test]
fn an_unmarked_process_with_only_a_legacy_keep_value_is_foreign() {
    let kept = keep_marked_cast();
    let mut payload = Value::Object(resume::stored(&kept)[&0].clone());
    let current = resume::encode(&payload);
    payload["kept_process"] = json!(process::CAST);
    let text = String::from_utf8(kept).unwrap();
    assert_eq!(text.matches(&current).count(), 1);
    let legacy = text.replace(&current, &resume::encode(&payload));
    let saved = write(
        &open(legacy.as_bytes()),
        &json!({process::QUESTION: process::MOULDED}),
    );
    assert_only_process(&saved, process::ITEM, process::CAST);
}

#[test]
fn the_same_kept_process_remains_replaceable_after_another_save() {
    let kept = keep_marked_cast();
    let reopened = open(&kept);
    let snapshot = reopened.snapshot();
    let kept_again = write(&reopened, &snapshot["analysis"]["resume"]["answers"]);
    assert_only_process(&kept_again, process::ITEM, process::CAST);
    let saved = write(
        &open(&kept_again),
        &json!({process::QUESTION: process::MOULDED}),
    );
    assert_only_process(&saved, process::ITEM, process::MOULDED);
}

#[test]
fn keeping_a_removal_never_makes_a_later_foreign_process_replaceable() {
    let removed = edited_process(None);
    let kept = write(
        &open(&removed),
        &json!({"_specify": {"keep": [process::QUESTION]}}),
    );
    assert!(
        resume::stored(&kept)[&0]["kept"]
            .as_array()
            .unwrap()
            .contains(&json!(process::QUESTION))
    );
    let added = add_foreign_process(kept, process::PROPERTY, process::CAST);
    assert_only_process(&added, process::ITEM, process::CAST);
    let reopened = open(&added);
    let saved = write(&reopened, &json!({process::QUESTION: process::MOULDED}));
    assert_only_process(&saved, process::ITEM, process::CAST);
    let snapshot = reopened.snapshot();
    let analysis = &snapshot["analysis"];
    assert_eq!(analysis["part"]["process"], process::CAST);
    assert!(
        !analysis["resume"]["file"]
            .as_array()
            .is_some_and(|ids| ids.contains(&json!(process::QUESTION)))
    );
}

/// The process edit has already lost its marker. Strip Specify's remaining description
/// markers, including the one on the saved answers, without changing the payload.
fn strip_description_markers(bytes: Vec<u8>) -> Vec<u8> {
    let stored = resume::stored(&bytes);
    assert!(
        stored[&0].get("pmi_scope").is_none(),
        "replace-written fixture"
    );
    let doc = Document::parse(bytes).unwrap();
    let mut edit = Edit::new();
    let mut removed = 0;
    for (&id, record) in &doc.graph().entities {
        if matches!(record, RawEntity::Simple { name, attributes, .. }
            if name == "DESCRIPTION_ATTRIBUTE"
                && matches!(attributes.first(), Some(Attribute::String(s))
                    if s == specify_core::writer::PMI_PROVENANCE))
        {
            edit.remove(id);
            removed += 1;
        }
    }
    assert!(removed > 0, "the answers payload had a description marker");
    let stripped = doc.apply(&edit).unwrap().bytes;
    assert_eq!(resume::stored(&stripped), stored);
    stripped
}

#[test]
fn a_replace_written_file_keeps_the_process_when_its_markers_are_stripped() {
    let edited = strip_description_markers(edited_process(Some((process::ITEM, process::CAST))));
    let reopened = open(&edited);
    let rewritten = write(&reopened, &json!({}));
    assert_only_process(&rewritten, process::ITEM, process::CAST);
    assert_eq!(conflict(&reopened.snapshot()["analysis"])["foreign"], true);
    let answered = write(
        &open(&rewritten),
        &json!({process::QUESTION: process::MOULDED}),
    );
    assert_only_process(&answered, process::ITEM, process::CAST);
}

#[test]
fn a_replace_written_file_does_not_restore_a_process_removed_along_with_its_markers() {
    let edited = strip_description_markers(edited_process(None));
    let reopened = open(&edited);
    let snapshot = reopened.snapshot();
    let analysis = &snapshot["analysis"];
    let rewritten = write(&reopened, &analysis["resume"]["answers"]);
    assert!(
        open(&rewritten).snapshot()["analysis"]
            .get("process")
            .is_none()
    );
    assert_eq!(conflict(analysis)["stored"], process::MACHINED);
    assert_eq!(conflict(analysis)["file"], Value::Null);
}

#[test]
fn process_evidence_also_preserves_edited_notes_and_tolerances_after_marker_loss() {
    let plate = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let bytes = write(
        &open(plate),
        &json!({
            process::QUESTION: process::MACHINED,
            "part.notes": ["original note"],
            "part.general_tolerance": "ISO 2768-m",
            "datum.A": [2],
            "datum.A.flatness": "0.03",
        }),
    );
    let text = String::from_utf8(strip_description_markers(bytes)).unwrap();
    let edits = [
        ("original note", "edited note"),
        ("ISO 2768-m", "ISO 2768-f"),
        ("LENGTH_MEASURE(0.03)", "LENGTH_MEASURE(0.058)"),
    ];
    let mut text = text.replace("'specify-core:written:v1'", "''");
    for (old, new) in edits {
        assert!(text.contains(old), "fixture has {old}");
        text = text.replace(old, new);
    }
    let reopened = open(text.as_bytes());
    let analysed = reopened.snapshot();
    let written = write(&reopened, &analysed["analysis"]["resume"]["answers"]);
    let written = String::from_utf8(written).unwrap();
    for (old, new) in edits {
        assert!(written.contains(new), "rewrite keeps {new}");
        assert!(!written.contains(old), "rewrite doesn't restore {old}");
    }
}
