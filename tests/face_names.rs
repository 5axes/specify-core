//! For #87: only names authored into STEP are persistent face identities.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use specify_core::{api, features, part, resume, revision, writer};
use std::path::Path;

fn load(name: &str) -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/revisions")
            .join(name),
        None,
    )
    .unwrap()
}
fn pair() -> (part::LoadedPart, part::LoadedPart) {
    (
        load("named_plate.old.step.gz"),
        load("named_plate.new.step.gz"),
    )
}
fn object(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}
// Revision answer maps now include destination numbering evidence, as well as requirements.
fn diff_answers(p: &part::LoadedPart, mut expected: Map<String, Value>) -> Map<String, Value> {
    expected.insert(
        "_specify".into(),
        json!({
            "evidence": resume::evidence(p).unwrap(), "unresolved": {},
        }),
    );
    expected
}
fn face(p: &part::LoadedPart, feature: &str, number: usize) -> usize {
    let name = format!("f:named_plate.{feature}.{number};M:named_plate.body");
    p.face_names
        .iter()
        .position(|n| n.as_deref() == Some(&name))
        .unwrap()
}
fn answers(p: &part::LoadedPart) -> Map<String, Value> {
    let hole = face(p, "body_make_cylinder1", 1);
    let top = face(p, "body_make_box1", 6);
    let bottom = face(p, "body_make_box1", 5);
    object(json!({
        "part.material": "steel",
        format!("hole.function:holes:{hole}"): "fit:H7",
        "datum.A": [bottom],
        "datum.A.flatness": "0.03",
        "extra.1": {"kind": "parallelism", "faces": [top, bottom], "value": "0.1", "datums": ["A"]},
        "extra.2": {"kind": "distance", "faces": [top], "reference": [bottom], "value": "±0.05"}
    }))
}
fn rematch(p: &part::LoadedPart, a: &Map<String, Value>, evidence: &Value) -> resume::Resumed {
    resume::rematch_answers(p, &features::recognise(p).unwrap(), a, Some(evidence), None).unwrap()
}
fn renamed(p: &part::LoadedPart, replacements: &[(usize, &str)]) -> part::LoadedPart {
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let mut edit = Edit::new();
    for &(i, value) in replacements {
        let id = p.face_entity(i);
        let RawEntity::Simple {
            name, attributes, ..
        } = doc.get(id).unwrap()
        else {
            panic!("face entity")
        };
        let mut attributes = attributes.clone();
        attributes[0] = Attribute::String(value.into());
        edit.replace(id, haecceity::p21::simple(name, attributes));
    }
    part::load_bytes("renamed.step", doc.apply(&edit).unwrap().bytes).unwrap()
}
fn embedded(p: &part::LoadedPart, payload: &Value) -> part::LoadedPart {
    // Use the actual attribute-set writer to carry old evidence into the new revision.
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let model = haecceity::pmi::PartPmi {
        attributes: vec![resume::attribute_set(payload)],
        ..Default::default()
    };
    let (edit, _) = haecceity::pmi::write(
        &doc,
        std::slice::from_ref(&p.definition),
        &[(haecceity::pmi::PartId(0), model)],
        haecceity::pmi::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let bytes = doc.apply(&edit).unwrap().bytes;
    part::load_bytes("embedded.step", bytes).unwrap()
}

#[test]
fn haecceity_names_are_indexed_optional_and_flow_into_analysis() {
    let p = load("cadgen_pmi_plate.step.gz");
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let names = haecceity::json::Anchors::read(&doc, &p.definition).names;
    assert_eq!(names.len(), 7);
    assert_eq!(names.len(), p.binding.face_count);
    assert_eq!(names, p.face_names);
    assert_eq!(
        names
            .iter()
            .flatten()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        p.binding.face_count,
        "cadgen gives each face a unique name"
    );
    assert!(
        names
            .iter()
            .flatten()
            .all(|n| n.starts_with("f:pmi_plate."))
    );
    let a = api::analyse(&p).unwrap();
    for (i, name) in names.iter().enumerate() {
        assert_eq!(a["faces"][i]["name"], json!(name));
    }
    let p = load("hole_moved.old.step.gz");
    assert!(p.face_names.iter().all(Option::is_none));
    assert!(
        api::analyse(&p).unwrap()["faces"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f.get("name").is_none())
    );
    assert_eq!(resume::evidence(&p).unwrap()["schema"], 2);
}

#[test]
fn rebuilt_names_carry_fit_datum_multi_face_tolerance_and_extra() {
    let (old, new) = pair();
    let before = answers(&old);
    assert_ne!(old.face_names, new.face_names);
    assert_ne!(
        resume::fingerprint(&old).unwrap(),
        resume::fingerprint(&new).unwrap()
    );
    let evidence = resume::evidence(&old).unwrap();
    assert_eq!(evidence["schema"], 3);
    let out = rematch(&new, &before, &evidence);
    assert!(out.unresolved.is_empty(), "{out:?}");
    assert_eq!(out.answers, answers(&new));
    assert!(!out.migrated.is_empty());
    let report = revision::diff(&old, &new, &before).unwrap();
    assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
    assert_eq!(report.answers, diff_answers(&new, answers(&new)));
}

#[test]
fn missing_and_duplicate_names_refuse_even_when_geometry_is_identical() {
    let (old, _) = pair();
    let a = answers(&old);
    let evidence = resume::evidence(&old).unwrap();
    let hole = face(&old, "body_make_cylinder1", 1);
    let other = face(&old, "body_make_cylinder2", 1);
    for (changed, reason) in [
        (renamed(&old, &[(hole, "")]), "not in this revision"),
        (
            renamed(&old, &[(other, old.face_names[hole].as_deref().unwrap())]),
            "ambiguous face name",
        ),
    ] {
        assert_eq!(
            resume::fingerprint(&changed).unwrap(),
            resume::fingerprint(&old).unwrap()
        );
        let out = rematch(&changed, &a, &evidence);
        assert_eq!(out.unresolved.len(), 1, "{out:?}");
        assert!(out.unresolved[0].reason.contains(reason), "{out:?}");
        assert!(
            !out.answers
                .contains_key(&format!("hole.function:holes:{hole}"))
        );
        let report = revision::diff(&old, &changed, &a).unwrap();
        assert_eq!(report.unresolved.len(), 1);
        assert!(report.unresolved[0].reason.contains(reason));
        let back = embedded(&changed, &resume::payload(&old, &a).unwrap());
        let resumed = api::analyse(&back).unwrap();
        assert!(
            resumed["resume"]["unresolved"][0]["reason"]
                .as_str()
                .unwrap()
                .contains(reason)
        );
    }
    // Ambiguity in saved names is a refusal too, even if the new revision is unique.
    let mut evidence = evidence;
    evidence["face_names"][other] = evidence["face_names"][hole].clone();
    assert!(
        rematch(&old, &a, &evidence).unresolved[0]
            .reason
            .contains("saved revision")
    );
}

#[test]
fn mixed_support_uses_names_and_geometric_correspondence_per_face() {
    let (old, new) = pair();
    let unnamed_old = face(&old, "body_make_box1", 5);
    let unnamed_new = face(&new, "body_make_box1", 5);
    let old = renamed(&old, &[(unnamed_old, "")]);
    let new = renamed(&new, &[(unnamed_new, "")]);
    let top_old = face(&old, "body_make_box1", 6);
    let top_new = face(&new, "body_make_box1", 6);
    let a = object(
        json!({"datum.A": [unnamed_old], "extra.1": {"kind": "parallelism", "faces": [top_old, unnamed_old], "datums": ["A"], "value": "0.1"}}),
    );
    let expected = object(
        json!({"datum.A": [unnamed_new], "extra.1": {"kind": "parallelism", "faces": [top_new, unnamed_new], "datums": ["A"], "value": "0.1"}}),
    );
    let evidence = resume::evidence(&old).unwrap();
    assert!(evidence["geometry"].is_object());
    let out = rematch(&new, &a, &evidence);
    assert!(out.unresolved.is_empty(), "{out:?}");
    assert_eq!(out.answers, expected);
    assert_eq!(
        revision::diff(&old, &new, &a).unwrap().answers,
        diff_answers(&new, expected)
    );
}

#[test]
fn schema_three_stored_answers_round_trip_and_resume_on_a_rebuild() {
    let (old, new) = pair();
    let a = answers(&old);
    let payload = resume::payload(&old, &a).unwrap();
    assert_eq!(payload["face_names"], json!(old.face_names));
    let back = embedded(&new, &payload);
    assert_eq!(resume::stored(&back.bytes)[&0], object(payload));
    assert_eq!(
        api::analyse(&back).unwrap()["resume"]["answers"],
        json!(answers(&new))
    );
    // Exercise the normal writer, not just an attribute-only embed.
    let a = object(
        json!({"part.material":"steel", format!("hole.function:holes:{}", face(&old, "body_make_cylinder1", 1)): "fit:H7"}),
    );
    let bytes = writer::write_bytes(
        &old,
        &api::preview(&api::analyse(&old).unwrap(), &a).unwrap(),
        Some(&a),
    )
    .unwrap()
    .0;
    let back = part::load_bytes("written.step", bytes).unwrap();
    assert_eq!(resume::stored(&back.bytes)[&0]["schema"], 3);
    assert_eq!(api::analyse(&back).unwrap()["resume"]["answers"], json!(a));
}

#[test]
fn future_geometry_format_leaves_only_unnamed_supports_open() {
    let (old, new) = pair();
    let old_bottom = face(&old, "body_make_box1", 5);
    let new_bottom = face(&new, "body_make_box1", 5);
    let old = renamed(&old, &[(old_bottom, "")]);
    let new = renamed(&new, &[(new_bottom, "")]);
    let hole = face(&old, "body_make_cylinder1", 1);
    let a = object(
        json!({format!("hole.function:holes:{hole}"): "fit:H7", "datum.A":[old_bottom], "datum.A.flatness":"0.03"}),
    );
    let mut evidence = resume::evidence(&old).unwrap();
    evidence["geometry"]["fingerprint_version"] = json!("future/99");
    let out = rematch(&new, &a, &evidence);
    assert_eq!(out.answers.len(), 1);
    assert!(out.answers.contains_key(&format!(
        "hole.function:holes:{}",
        face(&new, "body_make_cylinder1", 1)
    )));
    assert_eq!(out.unresolved.len(), 2);
    assert!(
        out.unresolved
            .iter()
            .any(|u| u.id == "datum.A.flatness" && u.reason.contains("datum.A is unresolved"))
    );
}

#[test]
fn unnamed_revision_keeps_pre87_payload_and_current_presentation_bytes() {
    use sha2::{Digest, Sha256};
    let p = load("hole_moved.old.step.gz");
    let a = object(json!({"part.material":"steel"}));
    // Captured from the pre-#87 engine on this unchanged fixture. Schema 2 is
    // intentionally still written when there are no STEP face names.
    let expected = r#"{"schema":2,"answers":{"part.material":"steel"},"part":0,"face_count":11,"faces":"20524ed166f82e0a134620c6a45f95e2936d1893658acdf6f1e0e8d23a2600a5"}"#;
    assert_eq!(
        specify_core::pyjson::dumps_compact(&resume::payload(&p, &a).unwrap()),
        expected
    );
    let written = writer::write_bytes(
        &p,
        &api::preview(&api::analyse(&p).unwrap(), &a).unwrap(),
        Some(&a),
    )
    .unwrap()
    .0;
    let sha: String = Sha256::digest(&written)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // v0.4.1 regenerates the GENERAL NOTES graphic and generated plane views.
    // The schema-2 payload assertion above remains byte-identical to pre-#87.
    // v0.4.8 adds #198's Notes saved view and material description and #193's RP 4.1
    // FILE_DESCRIPTION reference and `note` curve-set name; nothing else changes.
    assert_eq!(
        sha,
        "f150377ff78ff19bd6bcd77cc61d9ff3b34fd59352806c1e86d91786b324f8cf"
    );
}

#[test]
fn shared_source_names_are_unnamed_and_new_unique_bindings_still_work() {
    let (old, new) = pair();
    let before = answers(&old);
    let expected = answers(&new);
    let shared = |p: &part::LoadedPart| {
        renamed(
            p,
            &[
                (face(p, "body_make_box1", 5), "f:shared.class"),
                (face(p, "body_make_box1", 6), "f:shared.class"),
            ],
        )
    };
    let old = shared(&old);
    let new = shared(&new);
    let evidence = resume::evidence(&old).unwrap();
    assert_eq!(evidence["schema"], 3);
    assert!(evidence["geometry"].is_object());
    for (i, name) in old.face_names.iter().enumerate() {
        if name.as_deref() == Some("f:shared.class") {
            assert!(evidence["face_names"][i].is_null());
            assert!(
                api::analyse(&old).unwrap()["faces"][i]
                    .get("name")
                    .is_none()
            );
        }
    }
    let out = rematch(&new, &before, &evidence);
    assert!(out.unresolved.is_empty(), "{out:?}");
    assert_eq!(out.answers, expected);
    assert_eq!(
        revision::diff(&old, &new, &before).unwrap().answers,
        diff_answers(&new, expected)
    );
    // Writing answers on the shared faces succeeds and resumes by geometry.
    let bytes = writer::write_bytes(
        &old,
        &api::preview(&api::analyse(&old).unwrap(), &before).unwrap(),
        Some(&before),
    )
    .unwrap()
    .0;
    let back = part::load_bytes("shared.step", bytes).unwrap();
    assert_eq!(
        api::analyse(&back).unwrap()["resume"]["answers"],
        json!(before)
    );
}

#[test]
fn shared_or_incidentally_unique_generic_labels_keep_schema_two() {
    let (p, _) = pair();
    // Even a namespace label is unusable when it occurs several times. Numeric
    // class labels and CAD's generic names are not authored identities, even once.
    for unique_generic in ["24", "Fill.1", "1103|1066129779", "f:"] {
        let labels: Vec<_> = (0..p.binding.face_count)
            .map(|i| {
                (
                    i,
                    if i == 0 {
                        unique_generic
                    } else {
                        "f:shared.class"
                    },
                )
            })
            .collect();
        let generic = renamed(&p, &labels);
        let evidence = resume::evidence(&generic).unwrap();
        assert_eq!(evidence["schema"], 2);
        assert!(evidence.get("face_names").is_none());
        assert!(evidence.get("geometry").is_none());
        let analysis = api::analyse(&generic).unwrap();
        assert!(
            analysis["faces"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f.get("name").is_none())
        );
        let a = object(json!({"part.material":"steel", "datum.A":[0]}));
        let out = rematch(&generic, &a, &evidence);
        assert_eq!(out.answers, a);
        assert!(out.unresolved.is_empty());
    }
}

#[test]
fn sidecar_write_refuses_contradictory_names_even_with_the_same_geometric_fingerprint() {
    let (old, _) = pair();
    let given = answers(&old);
    let saved = revision::diff(&old, &old, &given).unwrap().answers;
    let hole = face(&old, "body_make_cylinder1", 1);
    let changed = renamed(&old, &[(hole, "")]);
    assert_eq!(
        resume::fingerprint(&old).unwrap(),
        resume::fingerprint(&changed).unwrap()
    );
    let intent = api::preview(&api::analyse(&changed).unwrap(), &given).unwrap();
    let error = writer::write_bytes(&changed, &intent, Some(&saved)).unwrap_err();
    assert!(error.to_string().contains("stale answers"), "{error}");
}
