//! For #65: the STEP is the inspection number authority; no feature-ID or drawing match.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::{api, characteristics, existing, part, resume, revision, rules, writer};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
fn load(name: &str) -> part::LoadedPart {
    part::load(&fixture(name), None).unwrap()
}
fn back(bytes: Vec<u8>) -> part::LoadedPart {
    part::load_bytes("numbered.step", bytes).unwrap()
}
fn given() -> Map<String, Value> {
    Map::new()
}
fn intent(p: &part::LoadedPart) -> rules::Intent {
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.datums.push(
        json!({"letter":"A", "faces":[4]})
            .as_object()
            .unwrap()
            .clone(),
    );
    for r in [
        json!({"kind":"size", "faces":[9], "feature":"holes:9", "nominal":10., "upper":0.1,"lower":-0.1}),
        json!({"kind":"location", "faces":[9], "reference":[4], "distance":20., "basic":true}),
        json!({"kind":"flatness", "faces":[4], "tolerance":0.05}),
        json!({"kind":"finish", "faces":[9], "value":"Ra 3.2"}),
    ] {
        i.requirements.push(r.as_object().unwrap().clone());
    }
    i.part.insert("surface_finish".into(), json!("Ra 6.3"));
    i
}
fn write(p: &part::LoadedPart, i: &rules::Intent) -> part::LoadedPart {
    back(writer::write_bytes(p, i, Some(&given())).unwrap().0)
}
fn numbers(p: &part::LoadedPart) -> Vec<characteristics::Characteristic> {
    characteristics::read(p).unwrap()
}
fn rebind(i: &rules::Intent, p: &part::LoadedPart) -> rules::Intent {
    let mut i = i.clone();
    i.binding = serde_json::to_value(&p.binding).unwrap();
    i
}

#[test]
fn release_016_debug_history_upgrades_every_number_to_schema_2() {
    // Captured by the unmodified v0.1.6 tag (d6f65a8), using this exact intent
    // on parts/pin.step. Never regenerate this fixture with the current writer.
    let old = load("characteristic-numbers-016.step.gz");
    let mut i = rules::Intent::new(serde_json::to_value(&old.binding).unwrap());
    i.datums.push(
        json!({"letter":"A","faces":[6]})
            .as_object()
            .unwrap()
            .clone(),
    );
    for (face, nominal, fit) in [(0, 10., "h6"), (3, 4.2, "H7")] {
        let (upper, lower) = specify_core::standards::fit_limits(fit, nominal).unwrap();
        requirement(
            &mut i,
            json!({"kind":"size","faces":[face],"nominal":nominal,"fit":fit,"upper":upper,"lower":lower}),
        );
    }
    for r in [
        json!({"kind":"flatness","faces":[6],"tolerance":0.2}),
        json!({"kind":"thread","faces":[4],"side":"external","spec":"M6x1","class":"6g","hand":"RH","pitch":1.,"nominal":6.,"length":12.}),
        json!({"kind":"knurl","faces":[0],"pattern":"diamond","pitch":1.,"diameter":10.}),
    ] {
        requirement(&mut i, r);
    }
    i.part
        .insert("general_tolerance".into(), json!("ISO 2768-m"));
    let history = |p: &part::LoadedPart| {
        let doc = Document::parse(p.bytes.to_vec()).unwrap();
        let read = haecceity::pmi::read(&doc, std::slice::from_ref(&p.definition)).unwrap();
        let a = read.parts[0]
            .attributes
            .iter()
            .find(|a| a.name == characteristics::PROPERTY)
            .unwrap();
        let haecceity::pmi::AttributeValue::Text(s) = &a.items[0].1 else {
            panic!("history")
        };
        serde_json::from_str::<Value>(s).unwrap()
    };
    let prior = history(&old);
    assert_eq!(prior["schema"], 1);
    assert!(prior.to_string().contains("Iso286Class {"));
    let upgraded = write(&old, &i);
    let current = history(&upgraded);
    assert_eq!(current["schema"], 2);
    assert_eq!(current["high_water"], prior["high_water"]);
    assert_eq!(current["entries"].as_array().unwrap().len(), 6);
    let identity_numbers = |p: &part::LoadedPart| {
        numbers(p)
            .into_iter()
            .map(|c| (c.number, c.kind, c.r#type, c.faces, c.reference))
            .collect::<Vec<_>>()
    };
    assert_eq!(identity_numbers(&old), identity_numbers(&upgraded));
    assert!(!current.to_string().contains("specification"));
    assert!(!current.to_string().contains("Iso286Class"));
    assert_eq!(
        upgraded.bytes,
        write(&upgraded, &rebind(&i, &upgraded)).bytes
    );
    let report = revision::diff(&old, &upgraded, &Map::new()).unwrap();
    assert!(
        report.pmi.items.iter().all(|c| c.class != "renumbered"),
        "{:?}",
        report.pmi.items
    );
}

#[test]
fn every_kind_round_trips_standard_ids_additively_and_is_exposed_in_analysis_and_pmi() {
    let p = load("parts/plate.step");
    let i = intent(&p);
    let written = write(&p, &i);
    let ns = numbers(&written);
    assert_eq!(ns.len(), 6); // datum, size, location, FCF, face and part textures
    assert_eq!(
        ns.iter().map(|c| c.number).collect::<BTreeSet<_>>(),
        (1..=6).collect()
    );
    let doc = Document::parse(written.bytes.to_vec()).unwrap();
    assert!(haecceity::express::validate_document(doc.graph()).is_empty());
    assert!(haecceity::express_rules::check_all(&doc).is_empty());
    for n in &ns {
        assert!(doc.referrers(n.entity).iter().any(|id| matches!(doc.get(*id),
            Some(RawEntity::Simple {name,attributes,..}) if name == "ID_ATTRIBUTE"
            && attributes == &vec![Attribute::String(n.number.to_string()), Attribute::EntityRef(n.entity)])));
    }
    // haecceity's append preserves every source record exactly (the AP214 schema header
    // alone may become AP242); numbers never cause geometry or original PMI rewrites.
    let original = Document::parse(p.bytes.to_vec()).unwrap();
    for id in original.ids() {
        assert_eq!(
            original.span(id).map(|s| &original.bytes()[s]),
            doc.span(id).map(|s| &doc.bytes()[s]),
            "#{id}"
        );
    }
    let analysis = api::analyse(&written).unwrap();
    assert_eq!(
        analysis["characteristics"],
        serde_json::to_value(&ns).unwrap()
    );
    assert!(
        analysis["existing"].as_array().unwrap().is_empty(),
        "resumed interview stays geometry-only"
    );
    let existing = existing::read_existing(&written).unwrap();
    assert!(existing.iter().all(|e| e.number.is_some()));
    assert_eq!(
        existing.len(),
        5,
        "part finish is in the authoritative characteristics list"
    );
    // Independent fresh runs and a resumed run allocate identical numbers; an unchanged
    // resumed run also retains every byte, including original entity IDs and presentation.
    assert_eq!(written.bytes, write(&p, &i).bytes);
    assert_eq!(written.bytes, write(&written, &rebind(&i, &written)).bytes);
}

#[test]
fn additions_do_not_renumber_removals_retire_even_after_an_empty_write_and_values_can_change() {
    let p = load("parts/plate.step");
    let mut i = intent(&p);
    let first = write(&p, &i);
    let old = numbers(&first);
    i = rebind(&i, &first);
    i.requirements.insert(
        0,
        json!({"kind":"flatness", "faces":[5], "tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let added = write(&first, &i);
    let ns = numbers(&added);
    for n in &old {
        let now = ns
            .iter()
            .find(|c| {
                c.kind == n.kind
                    && c.r#type == n.r#type
                    && c.faces == n.faces
                    && c.reference == n.reference
            })
            .unwrap();
        assert_eq!(now.number, n.number);
    }
    assert_eq!(ns.iter().find(|c| c.faces == [5]).unwrap().number, 7);
    i = rebind(&i, &added);
    i.requirements.remove(0);
    i.requirements[0].insert("upper".into(), json!(0.2));
    let removed = write(&added, &i);
    assert_eq!(
        numbers(&removed)
            .iter()
            .map(|c| c.number)
            .collect::<Vec<_>>(),
        old.iter().map(|c| c.number).collect::<Vec<_>>()
    );
    i = rebind(&i, &removed);
    i.requirements.insert(
        0,
        json!({"kind":"flatness", "faces":[5], "tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let readded = write(&removed, &i);
    assert_eq!(
        numbers(&readded)
            .iter()
            .find(|c| c.faces == [5])
            .unwrap()
            .number,
        8
    );
    let empty = write(
        &readded,
        &rules::Intent::new(serde_json::to_value(&readded.binding).unwrap()),
    );
    assert!(numbers(&empty).is_empty());
    let mut i = rules::Intent::new(serde_json::to_value(&empty.binding).unwrap());
    i.requirements.push(
        json!({"kind":"flatness", "faces":[5], "tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    assert_eq!(numbers(&write(&empty, &i))[0].number, 9);
}

#[test]
fn written_general_dimensions_are_numbered_and_entityless_rows_are_not() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.part
        .insert("general_tolerance".into(), json!("ISO 2768-m"));
    for r in [
        json!({"kind":"size","faces":[9],"nominal":10.,"general":true}),
        json!({"kind":"location","faces":[5],"reference":[4],"distance":10.,"general":true}),
        json!({"kind":"size","faces":[],"nominal":2.,"general":true}),
    ] {
        i.general.push(r.as_object().unwrap().clone());
    }
    let result = write(&p, &i);
    let ns = numbers(&result);
    assert_eq!(ns.len(), 2);
    assert!(
        ns.iter()
            .all(|n| n.kind == "dimension" && !n.faces.is_empty())
    );
    assert_eq!(api::analyse(&result).unwrap()["characteristics"], json!(ns));
    // Nominal-only authored dimensions still settle no interview questions (#63).
    assert!(existing::read_existing(&result).unwrap().is_empty());
    assert_eq!(result.bytes, write(&result, &rebind(&i, &result)).bytes);
    i = rebind(&i, &result);
    i.general.remove(0);
    let removed = write(&result, &i);
    assert_eq!(numbers(&removed)[0].number, ns[1].number);
    i = rebind(&i, &removed);
    i.general.push(
        json!({"kind":"size","faces":[9],"nominal":10.,"general":true})
            .as_object()
            .unwrap()
            .clone(),
    );
    let added = write(&removed, &i);
    assert!(numbers(&added).iter().any(|n| n.number == 3));
    assert!(numbers(&added).iter().any(|n| n.number == ns[1].number));
    // With only a row that cannot produce PMI, there is no identity or history to invent.
    i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.general.push(
        json!({"kind":"size","faces":[],"nominal":2.,"general":true})
            .as_object()
            .unwrap()
            .clone(),
    );
    let empty = write(&p, &i);
    assert!(numbers(&empty).is_empty());
    assert!(!String::from_utf8_lossy(&empty.bytes).contains(characteristics::PROPERTY));
}

fn legacy_cases() -> Map<String, Value> {
    serde_json::from_str::<Value>(include_str!("fixtures/characteristic-numbers-legacy.json"))
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn legacy_numberless_files_expose_no_number_fields_on_every_platform() {
    for name in legacy_cases().keys() {
        let p = load(name);
        assert!(numbers(&p).is_empty());
        assert!(api::analyse(&p).unwrap().get("characteristics").is_none());
        let pmi = serde_json::to_value(existing::read_existing(&p).unwrap()).unwrap();
        assert!(
            pmi.as_array()
                .unwrap()
                .iter()
                .all(|row| row.get("number").is_none())
        );
    }
}

#[test]
fn legacy_numberless_files_have_byte_identical_analysis_and_pmi_serialization() {
    // Captured on macOS by unmodified main a49dc3d in a separate checkout and target dir.
    // quiddity-rust#134: only shaft analysis was recaptured (boss 36.1 → 37.6,
    // retained plate and its six-decimal published centre). Analysis and read hashes stay
    // unchanged for quiddity-rust#137; settings-only writes gain its General notes callout.
    // #77 leaves these settings-only hashes unchanged: no size/location UDA is written.
    // v0.2.7 moves the General notes block; only settings-only writer hashes change.
    // v0.4.1 regenerates the numbered GENERAL NOTES block and its plane view;
    // only settings-only writer hashes change, never analysis or semantic-read hashes.
    // Basic dimensions gain `"basic":true` in the existing read: only the alpha6 plate's
    // PMI read hash changes (its 14 basics); stripped of that key it is the old hash.
    // Float serialization varies by platform; portable absence checks run separately above.
    if !cfg!(target_os = "macos") {
        return;
    }
    let hash = |bytes: Vec<u8>| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    for (name, baseline) in legacy_cases() {
        let p = load(&name);
        let mut a = api::analyse(&p).unwrap();
        // #75 adds typed planner input; the pre-existing analysis fields still match exactly.
        let input = a
            .as_object_mut()
            .unwrap()
            .shift_remove("dimensioning")
            .unwrap();
        serde_json::from_value::<quiddity::dimensioning::Input>(input).unwrap();
        a.as_object_mut()
            .unwrap()
            .shift_remove("dimensioning_stated");
        let pmi = existing::read_existing(&p).unwrap();
        assert_eq!(
            hash(serde_json::to_vec(&a).unwrap()),
            baseline["analysis_sha256"],
            "{name}: analysis bytes"
        );
        assert_eq!(
            hash(serde_json::to_vec(&pmi).unwrap()),
            baseline["pmi_sha256"],
            "{name}: PMI read bytes"
        );
        let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
        i.part.insert("material".into(), json!("steel"));
        let answers = json!({"part.material":"steel"})
            .as_object()
            .unwrap()
            .clone();
        let output = writer::write_bytes(&p, &i, Some(&answers)).unwrap().0;
        assert_eq!(
            hash(output),
            baseline["settings_only_writer_sha256"],
            "{name}: settings-only writer with v0.4.1 GENERAL NOTES layout"
        );
    }
}

#[test]
fn revised_part_correspondence_carries_numbers_and_retirement_with_the_answers() {
    let old = load("revisions/hole_removed.old.step.gz");
    let new = load("revisions/hole_removed.new.step.gz");
    let a = api::analyse(&old).unwrap();
    let answers = json!({"hole.function:holes:6":"fit:H7", "hole.function:holes:7":"fit:H7", "hole.function:holes:8":"fit:H7", "hole.function:holes:9":"fit:H7"}).as_object().unwrap().clone();
    let i = api::preview(&a, &answers).unwrap();
    let old = back(writer::write_bytes(&old, &i, Some(&answers)).unwrap().0);
    let ns = numbers(&old);
    assert_eq!(ns.len(), 4);
    let revised = revision::diff(&old, &new, &Map::new()).unwrap();
    assert!(revised.answers.contains_key(characteristics::CARRY));
    let i = api::preview(&api::analyse(&new).unwrap(), &revised.answers).unwrap();
    let written = back(
        writer::write_bytes(&new, &i, Some(&revised.answers))
            .unwrap()
            .0,
    );
    let now = numbers(&written);
    assert_eq!(now.len(), 3);
    for n in ns.iter().filter(|n| n.faces != [7]) {
        let mapped = if n.faces[0] > 7 {
            n.faces[0] - 1
        } else {
            n.faces[0]
        };
        assert_eq!(
            now.iter().find(|c| c.faces == [mapped]).unwrap().number,
            n.number
        );
    }
    assert!(
        !resume::answers_for(&written)
            .unwrap()
            .unwrap()
            .contains_key(characteristics::CARRY),
        "transport is consumed, never reapplied on resume"
    );
    let mut i = rebind(&i, &written);
    i.requirements.push(
        json!({"kind":"flatness", "faces":[4],"tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let final_part = write(&written, &i);
    assert_eq!(
        numbers(&final_part)
            .iter()
            .find(|c| c.kind == "geometric_tolerance")
            .unwrap()
            .number,
        ns.iter().map(|c| c.number).max().unwrap() + 1
    );
}

#[test]
fn older_quiddity_written_pmi_keeps_numbers_on_upgrade_to_024() {
    let p = load("resume-alpha6/ftc10-reid.step.gz");
    // Add standard numeric ids to the committed OLD writer's PMI without regenerating any
    // old semantic records. This simulates numbering in a producer predating this kernel.
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&p.bytes).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let mut edit = Edit::new();
    let mut count = 0;
    for ids in &read.provenance.parts[0].dimensions {
        for &id in ids {
            if matches!(doc.get(id), Some(RawEntity::Simple {name,..}) if name == "DIMENSIONAL_SIZE" || name == "DIMENSIONAL_LOCATION")
            {
                count += 1;
                edit.add(haecceity::p21::simple(
                    "ID_ATTRIBUTE",
                    vec![
                        Attribute::String((100 + count).to_string()),
                        Attribute::EntityRef(id),
                    ],
                ));
            }
        }
    }
    assert!(count > 0);
    let annotated = back(doc.apply(&edit).unwrap().bytes);
    let old_numbers = numbers(&annotated);
    let a = api::analyse(&annotated).unwrap();
    assert!(
        !a["resume"]["migrated"].as_array().unwrap().is_empty(),
        "fixture actually crosses changed recognition IDs"
    );
    let answers = a["resume"]["answers"].as_object().unwrap();
    let i = api::preview(&a, answers).unwrap();
    let written = back(
        writer::write_bytes(&annotated, &i, Some(answers))
            .unwrap()
            .0,
    );
    let now = numbers(&written);
    let old_max = old_numbers.iter().map(|n| n.number).max().unwrap();
    let old_ids: BTreeSet<_> = old_numbers.iter().map(|n| n.number).collect();
    let now_ids: BTreeSet<_> = now.iter().map(|n| n.number).collect();
    let retired: BTreeSet<_> = old_ids.difference(&now_ids).copied().collect();
    let mut retained = 0;
    for n in old_numbers {
        if let Some(c) = now.iter().find(|c| c.number == n.number) {
            assert_eq!(c.kind, n.kind);
            assert_eq!(c.r#type, n.r#type);
            assert!(n.faces.iter().all(|f| c.faces.contains(f)));
            assert!(n.reference.iter().all(|f| c.reference.contains(f)));
            retained += 1;
        } else {
            // Complete coincident groups and aligned pattern witnesses supersede
            // these old individual location rows; no surviving number is reused.
            if [103, 106, 108, 153, 155].contains(&n.number) {
                eprintln!("replaced old member location: {n:?}");
            }
            assert_eq!(
                n.r#type, "Location_LinearDistance",
                "unexpected retirement: {n:?}"
            );
        }
    }
    assert!(
        now.iter()
            .filter(|c| !old_ids.contains(&c.number))
            .all(|c| c.number > old_max)
    );
    // Coincident basics now carry their complete semantic feature groups, and
    // pitches use aligned member witnesses. Superseded per-member rows retire.
    eprintln!("retired old-kernel location numbers: {retired:?}");
    assert_eq!(retained, 15);
    assert_eq!(
        retired,
        [
            103, 106, 108, 110, 111, 112, 113, 114, 115, 117, 118, 119, 120, 121, 122, 123, 124,
            125, 126, 127, 128, 129, 130, 131, 132, 134, 135, 136, 137, 138, 141, 142, 143, 144,
            145, 149, 150, 151, 152, 153, 155, 157, 158, 159, 160
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn an_added_control_on_the_same_face_cannot_take_an_existing_controls_number() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.requirements.push(
        json!({"kind":"flatness","faces":[4],"tolerance":0.05})
            .as_object()
            .unwrap()
            .clone(),
    );
    let first = write(&p, &i);
    i = rebind(&i, &first);
    i.requirements.insert(
        0,
        json!({"kind":"flatness","faces":[4],"tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let added = write(&first, &i);
    assert_eq!(
        numbers(&added).iter().map(|c| c.number).collect::<Vec<_>>(),
        [2, 1]
    );
    i = rebind(&i, &added);
    i.requirements.remove(1);
    let removed = write(&added, &i);
    assert_eq!(numbers(&removed)[0].number, 2);
}

#[test]
fn foreign_pmi_append_keeps_original_bytes_and_numbers_even_for_equal_requirements() {
    let p = load("parts/plate.step");
    let i = intent(&p);
    // Without stored answers the second write uses lossless Add, including a shared datum.
    let first = back(writer::write_bytes(&p, &i, None).unwrap().0);
    let original = numbers(&first);
    let added = back(
        writer::write_bytes(&first, &rebind(&i, &first), None)
            .unwrap()
            .0,
    );
    let after = numbers(&added);
    assert_eq!(after.len(), 11); // the existing datum is reused, five new requirements
    assert_eq!(
        after.iter().map(|c| c.number).collect::<BTreeSet<_>>(),
        (1..=11).collect()
    );
    for n in original {
        assert!(after.contains(&n));
    }
    let doc = Document::parse(first.bytes.to_vec()).unwrap();
    let new = Document::parse(added.bytes.to_vec()).unwrap();
    for id in doc.ids() {
        assert_eq!(
            doc.span(id).map(|s| &doc.bytes()[s]),
            new.span(id).map(|s| &new.bytes()[s])
        );
    }
}

#[test]
fn stale_revision_transport_cannot_reuse_numbers_retired_in_the_destination_file() {
    let old = load("revisions/hole_removed.old.step.gz");
    let new = load("revisions/hole_removed.new.step.gz");
    let answers = json!({"hole.function:holes:6":"fit:H7"})
        .as_object()
        .unwrap()
        .clone();
    let i = api::preview(&api::analyse(&old).unwrap(), &answers).unwrap();
    let old = back(writer::write_bytes(&old, &i, Some(&answers)).unwrap().0);
    let r = revision::diff(&old, &new, &Map::new()).unwrap();
    let mut i = api::preview(&api::analyse(&new).unwrap(), &r.answers).unwrap();
    let new = back(writer::write_bytes(&new, &i, Some(&r.answers)).unwrap().0);
    i = rebind(&i, &new);
    let extra = json!({"kind":"flatness","faces":[4],"tolerance":0.02})
        .as_object()
        .unwrap()
        .clone();
    i.requirements.push(extra.clone());
    let added = back(writer::write_bytes(&new, &i, Some(&r.answers)).unwrap().0);
    let retired = numbers(&added)
        .into_iter()
        .find(|c| c.kind == "geometric_tolerance")
        .unwrap()
        .number;
    i = rebind(&i, &added);
    i.requirements.pop();
    let removed = back(writer::write_bytes(&added, &i, Some(&r.answers)).unwrap().0);
    i = rebind(&i, &removed);
    i.requirements.push(extra);
    let readded = back(
        writer::write_bytes(&removed, &i, Some(&r.answers))
            .unwrap()
            .0,
    );
    assert!(
        numbers(&readded)
            .iter()
            .find(|c| c.kind == "geometric_tolerance")
            .unwrap()
            .number
            > retired
    );
    // The transport is also bound to its intended target geometry, even on a direct writer
    // call that does not go through the browser's answer-shape checks.
    let unrelated = load("parts/pin.step");
    let empty = rules::Intent::new(serde_json::to_value(&unrelated.binding).unwrap());
    assert!(
        writer::write_bytes(&unrelated, &empty, Some(&r.answers))
            .unwrap_err()
            .to_string()
            .contains("different geometry")
    );
}

#[test]
fn imported_shared_features_numeric_ids_are_reserved_without_numbering_old_requirements() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.requirements.push(
        json!({"kind":"size","faces":[9],"nominal":10.,"upper":0.1,"lower":-0.1})
            .as_object()
            .unwrap()
            .clone(),
    );
    let numbered = write(&p, &i);
    let d = Document::parse(numbered.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&numbered.bytes).unwrap();
    let mut model = haecceity::pmi::read(&d, &defs).unwrap().parts.remove(0);
    model.attributes.clear();
    // Import the semantic requirement without inspection identity, using haecceity alone.
    let original = Document::parse(p.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&p.bytes).unwrap();
    let (edit, _) = haecceity::pmi::write(
        &original,
        &defs,
        &[(haecceity::pmi::PartId(0), model)],
        haecceity::pmi::write::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let imported = Document::parse(original.apply(&edit).unwrap().bytes).unwrap();
    let read = haecceity::pmi::read(&imported, &defs).unwrap();
    let feature = read.provenance.parts[0].features[0].iter().copied().find(|id|
        matches!(imported.get(*id), Some(RawEntity::Simple {name,..}) if name == "SHAPE_ASPECT")).unwrap();
    let mut edit = Edit::new();
    edit.add(haecceity::p21::simple(
        "ID_ATTRIBUTE",
        vec![
            Attribute::String("40".into()),
            Attribute::EntityRef(feature),
        ],
    ));
    let imported = back(imported.apply(&edit).unwrap().bytes);
    assert!(
        numbers(&imported).is_empty(),
        "a geometric feature id is not a requirement number"
    );
    let added = back(
        writer::write_bytes(&imported, &rebind(&i, &imported), None)
            .unwrap()
            .0,
    );
    assert_eq!(numbers(&added)[0].number, 41);
    assert!(
        existing::read_existing(&added).unwrap()[0].number.is_none(),
        "the imported requirement remains unnumbered"
    );
}

#[test]
fn a_datum_requirement_number_is_not_overridden_by_its_geometric_features_id() {
    let p = load("parts/plate.step");
    let p = write(&p, &intent(&p));
    let required = numbers(&p)
        .iter()
        .find(|c| c.kind == "datum")
        .unwrap()
        .number;
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let feature = doc
        .ids()
        .find(|id| {
            // quiddity-rust#137 can split datum/size and use complex datum features.
            match doc.get(*id) {
                Some(RawEntity::Simple { name, .. }) => {
                    haecceity::express::is_a(&name.to_ascii_lowercase(), "datum_feature")
                }
                Some(RawEntity::Complex { parts, .. }) => parts.iter().any(|p| {
                    haecceity::express::is_a(&p.name.to_ascii_lowercase(), "datum_feature")
                }),
                None => false,
            }
        })
        .unwrap();
    let mut edit = Edit::new();
    edit.add(haecceity::p21::simple(
        "ID_ATTRIBUTE",
        vec![
            Attribute::String("40".into()),
            Attribute::EntityRef(feature),
        ],
    ));
    let p = back(doc.apply(&edit).unwrap().bytes);
    assert_eq!(
        numbers(&p)
            .iter()
            .find(|c| c.kind == "datum")
            .unwrap()
            .number,
        required
    );
    assert_eq!(
        existing::read_existing(&p)
            .unwrap()
            .iter()
            .find(|c| c.kind == "datum")
            .unwrap()
            .number,
        Some(required)
    );
}

#[test]
fn surface_texture_pmi_reads_the_number_from_its_root_even_when_parameters_have_lower_ids() {
    let p = load("parts/plate.step");
    let p = write(&p, &intent(&p));
    let expected = numbers(&p)
        .into_iter()
        .find(|c| c.kind == "surface_texture" && !c.faces.is_empty())
        .unwrap();
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let parameter = doc.ids().find(|id| matches!(doc.get(*id), Some(RawEntity::Simple {name,attributes,..})
        if name == "PROPERTY_DEFINITION" && matches!(attributes.first(), Some(Attribute::String(s)) if s == "surface_condition"))).unwrap();
    assert!(parameter > expected.entity);
    let swap = |id| {
        if id == parameter {
            expected.entity
        } else if id == expected.entity {
            parameter
        } else {
            id
        }
    };
    fn refs(a: &mut Attribute, swap: &impl Fn(u64) -> u64) {
        match a {
            Attribute::EntityRef(id) => *id = swap(*id),
            Attribute::List(items) => {
                for a in items {
                    refs(a, swap);
                }
            }
            Attribute::Typed { value, .. } => refs(value, swap),
            _ => {}
        }
    }
    let mut edit = Edit::new();
    for id in doc.ids() {
        let mut e = doc.get(id).unwrap().clone();
        match &mut e {
            RawEntity::Simple { id, attributes, .. } => {
                *id = swap(*id);
                for a in attributes {
                    refs(a, &swap);
                }
            }
            RawEntity::Complex { id, parts, .. } => {
                *id = swap(*id);
                for part in parts {
                    for a in &mut part.attributes {
                        refs(a, &swap);
                    }
                }
            }
        }
        edit.replace(swap(id), e);
    }
    let p = back(doc.apply(&edit).unwrap().bytes);
    let public = existing::read_existing(&p).unwrap();
    assert_eq!(
        public
            .iter()
            .find(|c| c.kind == "note" && c.r#type == "surface finish")
            .unwrap()
            .number,
        Some(expected.number)
    );
    assert_eq!(
        numbers(&p)
            .iter()
            .find(|c| c.entity == parameter)
            .unwrap()
            .number,
        expected.number
    );
}

// Every entry pins both the stored number and its complete public geometric identity.
// The array is the inspector's walk, independent of the reader's semantic array order.
type WalkItem<'a> = (&'a str, &'a str, &'a [usize], &'a [usize]);
fn assert_walk(p: &part::LoadedPart, expected: &[WalkItem<'_>]) {
    let mut ns = numbers(p);
    ns.sort_by_key(|c| c.number);
    assert_eq!(ns.len(), expected.len());
    for (i, (n, &(kind, ty, faces, reference))) in ns.iter().zip(expected).enumerate() {
        assert_eq!(
            (
                n.number,
                n.kind.as_str(),
                n.r#type.as_str(),
                n.faces.as_slice(),
                n.reference.as_slice()
            ),
            ((i + 1) as u64, kind, ty, faces, reference),
            "inspection station {}",
            i + 1
        );
    }
}
fn requirement(i: &mut rules::Intent, r: Value) {
    i.requirements.push(r.as_object().unwrap().clone());
}

#[test]
fn first_plate_walk_is_datum_letters_then_front_top_right_and_rows() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    // Deliberately reverse letters and scramble the holes' semantic order.
    for d in [
        json!({"letter":"B","faces":[0]}),
        json!({"letter":"A","faces":[4]}),
    ] {
        i.datums.push(d.as_object().unwrap().clone());
    }
    for face in [8, 6, 13, 10, 9] {
        requirement(
            &mut i,
            json!({"kind":"size","faces":[face],"nominal":if face == 9 { 8. } else { 6.6 },"upper":0.1,"lower":-0.1}),
        );
    }
    for r in [
        json!({"kind":"location","faces":[9],"reference":[0],"distance":50.,"basic":true}),
        json!({"kind":"position","faces":[9],"tolerance":0.1,"diametral":true,"datums":["A"]}),
        json!({"kind":"finish","faces":[8],"value":"Ra 3.2"}),
        json!({"kind":"finish","faces":[10],"value":"Ra 3.2"}),
        json!({"kind":"location","faces":[5],"reference":[0],"distance":100.,"upper":0.1,"lower":-0.1}),
        json!({"kind":"location","faces":[3],"reference":[1],"distance":60.,"upper":0.1,"lower":-0.1}),
    ] {
        requirement(&mut i, r);
    }
    i.part.insert("surface_finish".into(), json!("Ra 6.3"));
    let written = write(&p, &i);
    assert_walk(
        &written,
        &[
            ("datum", "A", &[4], &[]), // 1: letters, despite B preceding A in the model
            ("datum", "B", &[0], &[]), // 2
            ("surface_texture", "Ra", &[10], &[]), // 3: front, (-30,0), edge-on Z cylinder
            ("surface_texture", "Ra", &[8], &[]), // 4: front, (30,0), same row to the right
            ("dimension", "Location_LinearDistance", &[5], &[0]), // 5: front, (50,0), X span
            ("dimension", "Size_Diameter", &[10], &[]), // 6: top, (-30,15), first end-on hole
            ("dimension", "Size_Diameter", &[13], &[]), // 7: top, (30,15), top row right
            ("dimension", "Size_Diameter", &[9], &[]), // 8: top, (0,0), size before location
            ("dimension", "Location_LinearDistance", &[9], &[0]), // 9: plane-to-bore in top
            ("geometric_tolerance", "Position", &[9], &[]), // 10: same site, after location
            ("dimension", "Size_Diameter", &[6], &[]), // 11: top, (-30,-15), bottom row left
            ("dimension", "Size_Diameter", &[8], &[]), // 12: top, (30,-15), bottom row right
            ("dimension", "Location_LinearDistance", &[3], &[1]), // 13: right, Y span
            ("surface_texture", "Ra", &[], &[]), // 14: default finish has no face position
        ],
    );
    assert_eq!(written.bytes, write(&p, &i).bytes);
}

#[test]
fn first_turned_walk_interleaves_axial_sizes_finishes_threads_and_knurls() {
    let p = load("parts/pin.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    for d in [
        json!({"letter":"B","faces":[6]}),
        json!({"letter":"A","faces":[4]}),
    ] {
        i.datums.push(d.as_object().unwrap().clone());
    }
    for r in [
        json!({"kind":"size","faces":[4],"nominal":6.,"upper":0.1,"lower":-0.1}),
        json!({"kind":"size","faces":[0],"nominal":10.,"upper":0.1,"lower":-0.1}),
        json!({"kind":"size","faces":[3],"nominal":4.2,"upper":0.1,"lower":-0.1}),
        json!({"kind":"location","faces":[1],"reference":[6],"distance":32.,"upper":0.1,"lower":-0.1}),
        json!({"kind":"finish","faces":[4],"value":"Ra 3.2"}),
        json!({"kind":"finish","faces":[0],"value":"Ra 3.2"}),
        json!({"kind":"thread","faces":[4],"side":"external","spec":"M6x1","class":"6g","hand":"RH","pitch":1.,"nominal":6.,"length":12.}),
        json!({"kind":"knurl","faces":[0],"pattern":"diamond","pitch":1.,"diameter":10.}),
        json!({"kind":"position","faces":[3],"tolerance":0.1,"diametral":true,"datums":["A"]}),
    ] {
        requirement(&mut i, r);
    }
    i.part.insert("surface_finish".into(), json!("Ra 6.3"));
    let written = write(&p, &i);
    assert_walk(
        &written,
        &[
            ("datum", "A", &[4], &[]), // 1: OD datum first, by letter
            ("datum", "B", &[6], &[]), // 2: bottom end datum
            ("dimension", "Location_LinearDistance", &[1], &[6]), // 3: front, Z=32, top end
            ("dimension", "Size_Diameter", &[0], &[]), // 4: front OD profile at Z=22
            ("surface_texture", "Ra", &[0], &[]), // 5: same station, after size
            ("knurl", "knurl", &[0], &[]), // 6: same station, semantic tie after finish
            ("dimension", "Size_Diameter", &[4], &[]), // 7: front lower OD, Z=6
            ("surface_texture", "Ra", &[4], &[]), // 8: lower OD finish
            ("thread", "External", &[4], &[]), // 9: external thread shares OD's front view
            ("dimension", "Size_Diameter", &[3], &[]), // 10: top, bore seen end-on at (0,0)
            ("geometric_tolerance", "Position", &[3], &[]), // 11: same site, size before control
            ("surface_texture", "Ra", &[], &[]), // 12: unpositioned default last
        ],
    );
    assert_eq!(written.bytes, write(&p, &i).bytes);
}

#[test]
fn a_later_top_left_hole_appends_even_though_its_fresh_allocation_would_be_first() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    for f in [8, 6] {
        requirement(
            &mut i,
            json!({"kind":"size","faces":[f],"nominal":6.6,"upper":0.1,"lower":-0.1}),
        );
    }
    let first = write(&p, &i);
    assert_walk(
        &first,
        &[
            ("dimension", "Size_Diameter", &[6], &[]),
            ("dimension", "Size_Diameter", &[8], &[]),
        ],
    );
    requirement(
        &mut i,
        json!({"kind":"size","faces":[10],"nominal":6.6,"upper":0.1,"lower":-0.1}),
    );
    let fresh = write(&p, &i);
    assert_walk(
        &fresh,
        &[
            ("dimension", "Size_Diameter", &[10], &[]), // top left would be 1 on first allocation
            ("dimension", "Size_Diameter", &[6], &[]),
            ("dimension", "Size_Diameter", &[8], &[]),
        ],
    );
    let added = write(&first, &rebind(&i, &first));
    assert_walk(
        &added,
        &[
            ("dimension", "Size_Diameter", &[6], &[]),
            ("dimension", "Size_Diameter", &[8], &[]),
            ("dimension", "Size_Diameter", &[10], &[]), // high_water + 1 despite position
        ],
    );
}

#[test]
fn category_order_fixture_from_the_013_era_keeps_numbers_on_noop_and_rewrite() {
    // Captured from unmodified 68f375c with intent(), before implementing inspection order.
    // Keep the old schema-1 history and the actual ID_ATTRIBUTE records, not a regenerated
    // new-order file whose numbers have merely been patched in this test.
    let old = load("characteristic-numbers-category.step.gz");
    let category = [
        ("datum", "A", &[4][..], &[][..]),
        ("dimension", "Size_Diameter", &[9][..], &[][..]),
        ("dimension", "Location_LinearDistance", &[9][..], &[4][..]),
        ("geometric_tolerance", "Flatness", &[4][..], &[][..]),
        ("surface_texture", "Ra", &[9][..], &[][..]),
        ("surface_texture", "Ra", &[][..], &[][..]),
    ];
    assert_walk(&old, &category);
    let i = intent(&old);
    // #77 upgrades written legacy dimensions with origin/source; their stored numbers
    // and original category walk still win. A subsequent unchanged write keeps its bytes.
    let upgraded = write(&old, &i);
    assert_walk(&upgraded, &category);
    assert!(numbers(&old).iter().all(|n| n.origin.is_none()));
    assert!(
        numbers(&upgraded)
            .iter()
            .filter(|n| n.kind == "dimension")
            .all(|n| n.origin.is_some())
    );
    assert_eq!(
        upgraded.bytes,
        write(&upgraded, &rebind(&i, &upgraded)).bytes
    );
    let mut i = i;
    i.requirements[0].insert("upper".into(), json!(0.2)); // force rewrite, carry unique support
    requirement(
        &mut i,
        json!({"kind":"flatness","faces":[5],"tolerance":0.02}),
    );
    let rewritten = write(&old, &i);
    let mut expected = category.to_vec();
    expected.push(("geometric_tolerance", "Flatness", &[5], &[]));
    assert_walk(&rewritten, &expected);
    let raw = load("parts/plate.step");
    let fresh = write(&raw, &intent(&raw));
    assert_ne!(
        numbers(&fresh).iter().map(|c| c.number).collect::<Vec<_>>(),
        (1..=6).collect::<Vec<_>>()
    );
}

#[test]
fn first_shaft_walk_uses_front_shoulders_then_end_on_holes_and_side_width() {
    let p = load("parts/GRM-02_shaft.step");
    let a = api::analyse(&p).unwrap();
    let answers = rules::with_all_defaults(&a, &Map::new()).unwrap();
    let i = api::preview(&a, &answers).unwrap();
    // The axis-based position requirements cannot be replaced by an unrelated
    // General offset from the flats; report that proposal instead (#75).
    let rejected: Vec<_> = i
        .requirements
        .iter()
        .filter_map(|r| r.get("not_written_reason"))
        .collect();
    assert_eq!(rejected.len(), 1);
    assert!(
        rejected[0]
            .as_str()
            .unwrap()
            .contains("General contract conflicts with an answered requirement")
    );
    let written = back(writer::write_bytes(&p, &i, Some(&answers)).unwrap().0);
    assert_walk(
        &written,
        &[
            // A thread carries its drill. Locations require real face supports;
            // the drill floor is not an independent part length (#75).
            ("datum", "A", &[0], &[]),
            ("datum", "B", &[7, 11], &[]),
            ("dimension", "Location_LinearDistance", &[5], &[7, 11]),
            ("dimension", "Location_LinearDistance", &[3], &[7, 11]),
            ("dimension", "Location_LinearDistance", &[9], &[7, 11]),
            ("dimension", "Location_LinearDistance", &[13], &[7, 11]),
            ("dimension", "Location_LinearDistance", &[8], &[10]),
            ("dimension", "Location_LinearDistance", &[2, 6], &[10]),
            ("geometric_tolerance", "Position", &[15], &[]),
            ("geometric_tolerance", "Position", &[14], &[]),
            ("thread", "Internal", &[15], &[]),
            ("thread", "Internal", &[14], &[]),
            ("dimension", "Location_LinearDistance", &[12], &[4]),
            ("surface_texture", "Ra", &[], &[]),
        ],
    );
    assert_eq!(
        written.bytes.as_slice(),
        writer::write_bytes(&p, &i, Some(&answers))
            .unwrap()
            .0
            .as_slice()
    );
    assert_eq!(
        written.bytes.as_slice(),
        writer::write_bytes(&written, &rebind(&i, &written), Some(&answers))
            .unwrap()
            .0
            .as_slice()
    );
}

#[test]
fn bore_to_bore_location_uses_the_end_on_view_even_when_both_axes_are_parallel() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    // Both cylinders run along Z; their axes' separation lies in XY, so the location
    // belongs in top, beside the bore size. A front-view finish must precede both.
    for r in [
        json!({"kind":"location","faces":[9],"reference":[12],"distance":45.,"upper":0.1,"lower":-0.1}),
        json!({"kind":"size","faces":[9],"nominal":8.,"upper":0.1,"lower":-0.1}),
        json!({"kind":"finish","faces":[8],"value":"Ra 3.2"}),
    ] {
        requirement(&mut i, r);
    }
    assert_walk(
        &write(&p, &i),
        &[
            ("surface_texture", "Ra", &[8], &[]), // front, X=30: before every top characteristic
            ("dimension", "Size_Diameter", &[9], &[]), // top, (0,0): size wins over location
            ("dimension", "Location_LinearDistance", &[9], &[12]), // top, same site
        ],
    );
}

#[test]
fn presentation_shapes_keep_requirement_ids_history_and_face_supports() {
    // quiddity-rust#137: a sized datum is split, multi-face supports are composite and
    // locations own non-datum aspects. #135 must report #65 ids on the semantic roots.
    let p = load("parts/GRM-02_shaft.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    for d in [
        json!({"letter":"A","faces":[0]}),
        json!({"letter":"B","faces":[7,11]}),
    ] {
        i.datums.push(d.as_object().unwrap().clone());
    }
    for r in [
        json!({"kind":"size","faces":[0],"nominal":8.,"upper":0.1,"lower":-0.1}),
        json!({"kind":"location","faces":[13],"reference":[7,11],"distance":37.6,"upper":0.1,"lower":-0.1}),
        json!({"kind":"flatness","faces":[7,11],"tolerance":0.05}),
        json!({"kind":"finish","faces":[0],"value":"Ra 3.2"}),
    ] {
        requirement(&mut i, r);
    }
    i.part.insert("material".into(), json!("steel"));
    let written = write(&p, &i);
    let ns = numbers(&written);
    assert_eq!(ns.len(), 6);
    let doc = Document::parse(written.bytes.to_vec()).unwrap();
    assert!(doc.ids().any(|id| match doc.get(id) {
        Some(RawEntity::Simple { name, .. }) => name == "COMPOSITE_SHAPE_ASPECT",
        Some(RawEntity::Complex { parts, .. }) =>
            parts.iter().any(|p| p.name == "COMPOSITE_SHAPE_ASPECT"),
        None => false,
    }));
    for id in doc.ids() {
        if let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
            && name == "DIMENSIONAL_LOCATION"
        {
            for a in &attributes[2..4] {
                let Attribute::EntityRef(target) = a else {
                    panic!("location aspect reference")
                };
                assert!(
                    !matches!(doc.get(*target), Some(RawEntity::Simple { name, .. }) if name == "DATUM_FEATURE")
                );
                assert!(
                    !matches!(doc.get(*target), Some(RawEntity::Complex { parts, .. }) if parts.iter().any(|p| p.name == "DATUM_FEATURE"))
                );
            }
        }
    }
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let ids = &read.identifiers.parts[0];
    let decoded: BTreeSet<_> = ids
        .datums
        .iter()
        .chain(&ids.dimensions)
        .chain(&ids.tolerances)
        .chain(&ids.surface_textures)
        .map(|id| id.as_ref().unwrap().parse::<u64>().unwrap())
        .collect();
    assert_eq!(decoded, ns.iter().map(|n| n.number).collect());
    assert!(
        read.parts[0]
            .attributes
            .iter()
            .any(|a| a.name == characteristics::PROPERTY)
    );
    assert!(haecceity::express::validate_document(doc.graph()).is_empty());
    assert!(haecceity::express_rules::check_all(&doc).is_empty());
    assert_eq!(written.bytes, write(&written, &rebind(&i, &written)).bytes);
    i.requirements[0].insert("upper".into(), json!(0.2));
    let rewritten = write(&written, &rebind(&i, &written));
    let signature = |p: &part::LoadedPart| {
        numbers(p)
            .into_iter()
            .map(|n| (n.number, n.kind, n.r#type, n.faces, n.reference))
            .collect::<Vec<_>>()
    };
    assert_eq!(signature(&written), signature(&rewritten));
    let existing = existing::read_existing(&rewritten).unwrap();
    assert!(
        existing
            .iter()
            .all(|e| e.number.is_some()
                && e.provenance.as_deref() == Some("specify-core:written:v1"))
    );
}
