//! Free-text manufacturing notes are part PMI (draftwright-rust #123).

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use haecceity::p21::{Attribute, Document};
use haecceity::pmi::{self, AttributeSet, AttributeValue};
use serde_json::{Map, Value, json};
use specify_core::part::{self, LoadedPart};
use specify_core::rules::{self, Intent};
use specify_core::{api, existing, resume, writer};

fn fixture(stem: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(format!("{stem}.step"))
}

fn plate() -> LoadedPart {
    part::load(&fixture("plate"), None).unwrap()
}

fn given(notes: Value) -> Map<String, Value> {
    json!({"part.material": "steel", "part.notes": notes})
        .as_object()
        .unwrap()
        .clone()
}

fn note(text: &str, index: Option<i64>) -> AttributeSet {
    let mut items = vec![("general note".into(), AttributeValue::Text(text.into()))];
    if let Some(index) = index {
        items.push((writer::NOTE_INDEX.into(), AttributeValue::Integer(index)));
    }
    AttributeSet {
        name: writer::SEMANTIC_TEXT.into(),
        on: None,
        items,
        description: None,
        general_property: None,
    }
}

#[test]
fn notes_are_asked_as_an_ordered_list_and_default_to_empty() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let qs = api::questions(&analysis, &Map::new()).unwrap();
    let q = qs.iter().find(|q| q.id == "part.notes").unwrap();
    assert_eq!(q.kind, "text_list");
    assert_eq!(q.default, Some(json!([])));
    let answers = rules::with_all_defaults(&analysis, &given(json!([]))).unwrap();
    let intent = api::apply(&analysis, &answers, false).unwrap();
    assert_eq!(intent.rules_version, 3);
    assert!(!intent.part.contains_key("notes"));
    let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("'general note'"));
}

#[test]
fn invalid_notes_are_refused_by_rules_and_direct_intent_writes() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    for value in [
        json!(null),
        json!("one note"),
        json!({}),
        json!([1]),
        json!(["ok", false]),
    ] {
        let err = api::questions(&analysis, &given(value.clone())).unwrap_err();
        assert!(err.to_string().contains("part.notes"), "{err}");
        let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
        intent.part.insert("notes".into(), value);
        let err = writer::write_bytes(&loaded, &intent, None).unwrap_err();
        assert!(matches!(err, writer::WriteError::Invalid(_)), "{err}");
        assert!(err.to_string().contains("ordered list of strings"), "{err}");
    }
}

#[test]
fn notes_round_trip_as_semantic_text_and_answers_with_a_byte_stable_rewrite() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    // Literal text: duplicates, sentinel-like words, whitespace, Unicode and STEP escaping.
    let notes = json!([
        "Inspect O'Brien's \\ bore ⌀8\nAfter coating",
        "None",
        "  Keep dry  ",
        "",
        "None",
        "   ",
        "(blank)",
        "  \nVisible note\n  "
    ]);
    let answers = given(notes.clone());
    let intent = api::apply(&analysis, &answers, true).unwrap();
    assert_eq!(intent.part["notes"], notes);
    let (bytes, report) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    assert!(report.not_written.is_empty());
    let defs = haecceity::step::read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes.clone()).unwrap();
    // quiddity-rust#137 must not produce an empty coordinates/curve list for the blank row.
    assert!(haecceity::express::validate_document(doc.graph()).is_empty());
    assert!(haecceity::express_rules::check_all(&doc).is_empty());
    // haecceity 0.4.8 (#193) names curve sets by the PMI RP 4.1 Table 18 (`note`) instead of
    // the drawn text, so the drawn block is checked through the kernel's own notes layout of
    // the read-back PMI (the rows and block it draws from), and the drawn set by its name.
    assert!(doc.ids().any(|id| matches!(doc.get(id),
        Some(haecceity::p21::RawEntity::Simple { name, attributes, .. })
            if name == "TESSELLATED_CURVE_SET" && matches!(attributes.first(),
                Some(Attribute::String(text)) if text == "note"))));
    let read = pmi::read(&doc, &defs).unwrap();
    let drawn = haecceity::presentation::notes::notes_block(
        &haecceity::presentation::notes::general_notes(&read.parts[0]),
    )
    .cells
    .join("\n");
    assert!(drawn.starts_with("GENERAL NOTES\n") && drawn.matches("(blank)").count() == 1);
    let sets: Vec<_> = read.parts[0]
        .attributes
        .iter()
        .filter(|a| a.items.iter().any(|(k, _)| k == "general note"))
        .collect();
    assert_eq!(sets.len(), notes.as_array().unwrap().len());
    for (i, set) in sets.iter().enumerate() {
        assert_eq!(set.name, writer::SEMANTIC_TEXT);
        assert!(set.on.is_none());
        assert_eq!(
            set.items,
            note(notes[i].as_str().unwrap(), Some((i + 1) as i64)).items
        );
    }
    let reopened = part::load_bytes("notes.step", bytes.clone()).unwrap();
    assert_eq!(existing::part_settings(&reopened).unwrap()["notes"], notes);
    assert_eq!(
        resume::answers_for(&reopened).unwrap(),
        Some(answers.clone())
    );
    let analysis = api::analyse(&reopened).unwrap();
    assert_eq!(analysis["resume"]["answers"], json!(answers));
    assert!(
        api::questions(&analysis, &answers)
            .unwrap()
            .iter()
            .any(|q| q.id == "part.notes")
    );
    let intent = api::apply(&analysis, &answers, true).unwrap();
    let (rewritten, _) = writer::write_bytes(&reopened, &intent, Some(&answers)).unwrap();
    assert!(
        rewritten == bytes,
        "an unchanged resumed rewrite must preserve every byte"
    );

    let edited = given(json!(["New first", "New second"]));
    let intent = api::apply(&analysis, &edited, true).unwrap();
    let (bytes, _) = writer::write_bytes(&reopened, &intent, Some(&edited)).unwrap();
    let edited_part = part::load_bytes("edited.step", bytes).unwrap();
    assert_eq!(
        existing::part_settings(&edited_part).unwrap()["notes"],
        edited["part.notes"]
    );
    // Explicitly clearing the list removes all previously written free-text notes.
    let analysis = api::analyse(&edited_part).unwrap();
    let cleared = given(json!([]));
    let intent = api::apply(&analysis, &cleared, true).unwrap();
    let (bytes, _) = writer::write_bytes(&edited_part, &intent, Some(&cleared)).unwrap();
    let cleared_part = part::load_bytes("cleared.step", bytes).unwrap();
    assert!(
        !existing::part_settings(&cleared_part)
            .unwrap()
            .contains_key("notes")
    );
}

#[test]
fn foreign_semantic_notes_are_read_by_index_without_stored_answers_or_text_migration() {
    let loaded = plate();
    let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.datums.push(
        json!({"letter": "A", "faces": [0]})
            .as_object()
            .unwrap()
            .clone(),
    );
    // These are caller-supplied attribute sets, not the Specify notes writer. Their order
    // deliberately disagrees with their indices. No Specify answers are embedded.
    let attributes = [
        note("last, unindexed", None),
        note("second", Some(2)),
        note("first", Some(1)),
        note("also second", Some(2)),
        note("invalid index, appended", Some(0)),
        AttributeSet {
            on: Some(pmi::NoteOwner::Feature(pmi::FeatureId(0))),
            ..note("feature only", Some(1))
        },
        AttributeSet {
            name: "drawing notes".into(),
            ..note("drawing only", Some(1))
        },
        AttributeSet {
            items: vec![(
                "edge condition".into(),
                AttributeValue::Text("Deburr".into()),
            )],
            ..note("unused", None)
        },
    ];
    let (bytes, _) = writer::write_bytes_with(&loaded, &intent, None, &attributes).unwrap();
    let reopened = part::load_bytes("foreign.step", bytes).unwrap();
    assert_eq!(resume::answers_for(&reopened).unwrap(), None);
    let analysis = api::analyse(&reopened).unwrap();
    assert_eq!(
        analysis["part"]["notes"],
        json!([
            "first",
            "second",
            "also second",
            "last, unindexed",
            "invalid index, appended"
        ])
    );
    assert_eq!(analysis["part"]["edges"], "Deburr");
    // Match the other imported part notes: already-stated requirements are retained, not
    // asked again or copied into add-mode intent.
    assert!(
        !api::questions(&analysis, &Map::new())
            .unwrap()
            .iter()
            .any(|q| q.id == "part.notes")
    );
}

#[test]
fn assembly_notes_belong_to_their_part() {
    let file = part::File::read(&fixture("assembly")).unwrap();
    let parts: Vec<_> = (0..2).map(|k| file.load(Some(k)).unwrap()).collect();
    let answers: Vec<_> = (0..2)
        .map(|k| given(json!([format!("part {k} note")])))
        .collect();
    let intents: Vec<_> = parts
        .iter()
        .zip(&answers)
        .map(|(p, a)| api::apply(&api::analyse(p).unwrap(), a, true).unwrap())
        .collect();
    let jobs: Vec<_> = parts
        .iter()
        .zip(&intents)
        .zip(&answers)
        .map(|((p, i), a)| (p, i, Some(a)))
        .collect();
    let (bytes, _) = writer::write_parts_bytes(&jobs).unwrap();
    let file = part::File::of_bytes(Path::new("assembly-notes.step"), bytes).unwrap();
    for (k, answers) in answers.iter().enumerate() {
        let loaded = file.load(Some(k)).unwrap();
        assert_eq!(
            existing::part_settings(&loaded).unwrap()["notes"],
            answers["part.notes"]
        );
        assert_eq!(
            api::analyse(&loaded).unwrap()["resume"]["answers"],
            json!(answers)
        );
    }
}

#[test]
fn opencascade_can_open_a_part_with_ordered_notes() {
    let Some(python) = common::live_python() else {
        eprintln!("skipped: set SPECIFY_TEST_PYTHON for live OpenCascade checks");
        return;
    };
    let loaded = plate();
    let answers = given(json!(["Inspect bore", "Deburr after coating"]));
    let intent = api::apply(&api::analyse(&loaded).unwrap(), &answers, true).unwrap();
    let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("ordered-notes-occt.step");
    std::fs::write(&path, bytes).unwrap();
    let output = Command::new(python)
        .args([
            "-c",
            r#"
import sys
from OCP.STEPCAFControl import STEPCAFControl_Reader
from OCP.IFSelect import IFSelect_RetDone
from OCP.TDocStd import TDocStd_Document
from OCP.TCollection import TCollection_ExtendedString
reader = STEPCAFControl_Reader()
assert reader.ReadFile(sys.argv[1]) == IFSelect_RetDone
doc = TDocStd_Document(TCollection_ExtendedString('notes'))
assert reader.Transfer(doc)
"#,
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn semantic_readback_without_answers_rewrites_without_duplicate_notes() {
    let loaded = plate();
    let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent
        .part
        .insert("notes".into(), json!(["First", "Second", "First"]));
    let (bytes, _) = writer::write_bytes(&loaded, &intent, None).unwrap();
    let reopened = part::load_bytes("semantic-notes.step", bytes.clone()).unwrap();
    assert!(resume::answers_for(&reopened).unwrap().is_none());
    let mut read_intent = Intent::new(serde_json::to_value(&reopened.binding).unwrap());
    read_intent.part = existing::part_settings(&reopened).unwrap();
    assert_eq!(read_intent.part, intent.part);
    let (rewritten, report) = writer::write_bytes(&reopened, &read_intent, None).unwrap();
    assert!(
        rewritten == bytes,
        "semantic readback must not duplicate foreign notes"
    );
    assert!(report.written.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("not written again"))
    );
    read_intent.part.insert("notes".into(), json!(["Changed"]));
    let err = writer::write_bytes(&reopened, &read_intent, None).unwrap_err();
    assert!(matches!(err, writer::WriteError::Invalid(_)), "{err}");
    assert!(
        err.to_string().contains("already states general notes"),
        "{err}"
    );
}
