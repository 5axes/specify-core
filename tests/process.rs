//! How a part is made (`part.process`, docs/process.md): the guess, the process's defaults,
//! the `specify process` attribute on each product definition, and the unchanged machined path.

use std::path::{Path, PathBuf};

use haecceity::p21::{self, Attribute as A, Document, Edit};
use serde_json::{Map, Value, json};
use specify_core::part::{self, LoadedPart};
use specify_core::process::{self, FDM, MACHINED, METAL, POWDER, RESIN};
use specify_core::rules::{self, Question};
use specify_core::{api, existing, resume, writer};

fn fixture(stem: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(format!("{stem}.step"))
}

fn plate() -> LoadedPart {
    part::load(&fixture("plate"), None).unwrap()
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn ids(qs: &[Question]) -> Vec<&str> {
    qs.iter().map(|q| q.id.as_str()).collect()
}

fn question<'q>(qs: &'q [Question], id: &str) -> &'q Question {
    qs.iter()
        .find(|q| q.id == id)
        .unwrap_or_else(|| panic!("{id}"))
}

/// Every `specify process` property definition in `bytes`, by the entity it is on.
fn process_owners(bytes: &[u8]) -> Vec<u64> {
    let doc = Document::parse(bytes.to_vec()).unwrap();
    doc.ids()
        .filter_map(|id| match doc.get(id) {
            Some(p21::RawEntity::Simple {
                name, attributes, ..
            }) if name == "PROPERTY_DEFINITION"
                && matches!(attributes.first(), Some(A::String(s)) if s == process::PROPERTY) =>
            {
                match attributes.get(2) {
                    Some(A::EntityRef(owner)) => Some(*owner),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

fn entity_name(bytes: &[u8], id: u64) -> String {
    let doc = Document::parse(bytes.to_vec()).unwrap();
    match doc.get(id) {
        Some(p21::RawEntity::Simple { name, .. }) => name.clone(),
        other => panic!("#{id}: {other:?}"),
    }
}

/// The file's process as read back from the part's PMI.
fn read_back(bytes: &[u8], part: Option<usize>) -> Option<specify_core::bom::Field> {
    let file = part::File::of_bytes(Path::new("process.step"), bytes.to_vec()).unwrap();
    let loaded = file.load(part).unwrap();
    let doc = Document::parse(bytes.to_vec()).unwrap();
    process::read(&doc, loaded.definition.product_definition)
}

#[test]
fn an_unanswered_part_is_machined_and_unchanged() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let given = answers(json!({"part.material": "steel"}));
    let qs = api::questions(&analysis, &given).unwrap();
    // The process is appended after every other question, so no question moves.
    assert_eq!(qs.last().unwrap().id, process::QUESTION);
    let q = qs.last().unwrap();
    assert_eq!(q.default, Some(json!(MACHINED)));
    assert_eq!(q.attention, "routine");
    assert_eq!(q.options, process::PROCESSES.map(String::from).to_vec());
    // An explicit machined answer asks exactly what no answer asks.
    let mut machined = given.clone();
    machined.insert(process::QUESTION.into(), json!(MACHINED));
    let same = api::questions(&analysis, &machined).unwrap();
    assert_eq!(
        same.iter().map(Question::to_dict).collect::<Vec<_>>(),
        qs.iter().map(Question::to_dict).collect::<Vec<_>>()
    );
    for (id, default) in [
        ("part.general_tolerance", "ISO 2768-m"),
        ("part.surface_finish", "Ra 3.2"),
        ("part.coating", "None (as machined)"),
        ("part.edges", "Break sharp edges 0.2 max"),
        ("part.locating", "position tolerances"),
    ] {
        assert_eq!(question(&qs, id).default, Some(json!(default)), "{id}");
    }
    assert!(ids(&qs).iter().any(|id| id.starts_with("hole.position:")));
    // Never open, never accepted as a machined default, never stored or written.
    assert!(
        rules::open_questions(&qs, &given)
            .iter()
            .all(|q| q.id != process::QUESTION)
    );
    let complete = rules::with_all_defaults(&analysis, &given).unwrap();
    assert!(!complete.contains_key(process::QUESTION));
    let intent = api::apply(&analysis, &given, true).unwrap();
    assert!(!intent.part.contains_key("process"));
    let (bytes, report) = writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    assert!(process_owners(&bytes).is_empty());
    assert!(!report.written.iter().any(|w| w.starts_with("process")));
    // An explicit answer is written; nothing else about the write changes.
    let answered = api::apply(&analysis, &machined, true).unwrap();
    let (with, _) = writer::write_bytes(&loaded, &answered, Some(&machined)).unwrap();
    assert_eq!(process_owners(&with).len(), 1);
    let mut a = intent.to_dict();
    let mut b = answered.to_dict();
    b["part"].as_object_mut().unwrap().remove("process");
    a["part"].as_object_mut().unwrap().remove("process");
    assert_eq!(a, b);
}

#[test]
fn the_material_class_suggests_a_printed_process_marked_check() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    for (material, expected) in [
        ("PLA (FDM)", FDM),
        ("petg (fdm)", FDM),
        ("Standard resin (SLA)", RESIN),
        ("PA12 (MJF)", POWDER),
        ("AlSi10Mg (DMLS)", METAL),
        ("6061-T6", MACHINED),
        ("steel", MACHINED),
    ] {
        let given = answers(json!({"part.material": material}));
        let qs = api::questions(&analysis, &given).unwrap();
        let q = question(&qs, process::QUESTION);
        assert_eq!(q.default, Some(json!(expected)), "{material}");
        let printed = expected != MACHINED;
        assert_eq!(q.attention == "check", printed, "{material}");
        assert_eq!(!q.reason.is_empty(), printed, "{material}");
        // A suggestion is not applied: until it is answered or accepted, the part is machined.
        assert_eq!(
            question(&qs, "part.surface_finish").default,
            Some(json!("Ra 3.2"))
        );
        let field = process::of(&analysis, &given).unwrap().unwrap();
        assert_eq!(
            (field.value, field.source.as_str()),
            (json!(expected), "default")
        );
    }
    let qs = api::questions(&analysis, &Map::new()).unwrap();
    assert_eq!(
        question(&qs, process::QUESTION).default,
        Some(json!(MACHINED))
    );
    // Accepting defaults takes the printed suggestion first, then that process's defaults.
    let given = answers(json!({"part.material": "PLA (FDM)"}));
    let complete = rules::with_all_defaults(&analysis, &given).unwrap();
    assert_eq!(complete[process::QUESTION], json!(FDM));
    assert_eq!(
        complete["part.general_tolerance"],
        json!("±0.3 mm or ±0.3 % (FDM)")
    );
    for id in ["part.surface_finish", "part.edges", "part.coating"] {
        assert!(!complete.contains_key(id), "{id}");
    }
    let intent = api::apply(&analysis, &given, true).unwrap();
    assert_eq!(intent.part["process"], json!(FDM));
    assert_eq!(intent.part["process_default"], json!(true));
    assert!(
        intent
            .requirements
            .iter()
            .all(|r| r["kind"] != rules::UNASKED)
    );
}

#[test]
fn fdm_defaults_tolerance_note_holes_locations_and_no_finish_questions() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let given = answers(json!({"part.material": "PLA (FDM)", "part.process": FDM}));
    let qs = api::questions(&analysis, &given).unwrap();
    let tolerance = question(&qs, "part.general_tolerance");
    assert_eq!(tolerance.default, Some(json!("±0.3 mm or ±0.3 % (FDM)")));
    assert_eq!(tolerance.attention, "check");
    assert!(tolerance.options.contains(&"ISO 2768-m".to_string()));
    for id in ["part.surface_finish", "part.edges", "part.coating"] {
        assert!(!ids(&qs).contains(&id), "{id} is asked");
    }
    assert!(ids(&qs).contains(&"part.heat_treatment"));
    assert_eq!(
        question(&qs, "part.locating").default,
        Some(json!("± dimensions"))
    );
    assert!(
        !ids(&qs)
            .iter()
            .any(|id| id.starts_with("hole.position:") || id.ends_with(".control"))
    );
    let functions: Vec<&Question> = qs
        .iter()
        .filter(|q| q.id.starts_with("hole.function:"))
        .collect();
    assert!(!functions.is_empty());
    for q in &functions {
        let default = q.default.as_ref().unwrap().as_str().unwrap();
        assert!(
            !default.starts_with("tapped") && !default.starts_with("fit"),
            "{default}"
        );
        assert!(q.options.contains(&"heat-set insert".to_string()));
        assert!(q.options.contains(&"printed thread".to_string()));
    }
    // The tap-drill hole a machined part would tap is a clearance hole.
    let machined = api::questions(&analysis, &answers(json!({"part.material": "steel"}))).unwrap();
    let tapped = machined
        .iter()
        .find(|q| {
            q.id.starts_with("hole.function:")
                && q.default
                    .as_ref()
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .starts_with("tapped")
        })
        .expect("a hole a machined plate taps");
    assert_eq!(question(&qs, &tapped.id).default, Some(json!("clearance")));

    // A heat-set insert is a note on the hole; the ± tolerance a note on the part.
    let mut chosen = given.clone();
    chosen.insert(tapped.id.clone(), json!("heat-set insert"));
    let intent = api::apply(&analysis, &chosen, true).unwrap();
    assert_eq!(
        intent.part["general_tolerance"],
        json!("±0.3 mm or ±0.3 % (FDM)")
    );
    for key in ["surface_finish", "edges", "coating"] {
        assert!(!intent.part.contains_key(key), "{key}");
    }
    let note = intent
        .requirements
        .iter()
        .find(|r| r["kind"] == "note")
        .expect("a hole note");
    assert_eq!(note["text"], json!("heat-set insert"));
    // General dimensions take the ± figure: the greater of 0.3 mm and 0.3 %.
    assert!(!intent.general.is_empty());
    for r in &intent.general {
        let upper = r["upper"].as_f64().unwrap();
        assert!(upper >= 0.3 - 1e-12, "{r:?}");
    }
    let (bytes, report) = writer::write_bytes(&loaded, &intent, Some(&chosen)).unwrap();
    assert!(
        report
            .written
            .contains(&"general tolerance note ±0.3 mm or ±0.3 % (FDM)".to_string()),
        "{:?}",
        report.written
    );
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("note heat-set insert on"))
    );
    assert!(report.written.contains(&format!("process {FDM}")));
    let back = part::File::of_bytes(Path::new("fdm.step"), bytes.clone())
        .unwrap()
        .load(None)
        .unwrap();
    let reading = existing::read(&back).unwrap();
    assert!(reading.model().general.is_empty());
    assert_eq!(
        reading.settings(&back)["general_tolerance"],
        json!("±0.3 mm or ±0.3 % (FDM)")
    );
    // Resumed, the answers come back, and the process with them.
    assert_eq!(resume::answers_for(&back).unwrap(), Some(chosen.clone()));
    let again = api::analyse(&back).unwrap();
    let qs = api::questions(&again, &chosen).unwrap();
    assert_eq!(question(&qs, process::QUESTION).id, process::QUESTION);
    // Writing the same answers again changes nothing.
    let intent = api::apply(&again, &chosen, true).unwrap();
    let (rewritten, _) = writer::write_bytes(&back, &intent, Some(&chosen)).unwrap();
    assert_eq!(rewritten, bytes);
}

#[test]
fn an_explicit_answer_wins_over_every_process_default() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let machined = api::questions(&analysis, &answers(json!({"part.material": "steel"}))).unwrap();
    let tapped = machined
        .iter()
        .find(|q| {
            q.id.starts_with("hole.function:")
                && q.default
                    .as_ref()
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .starts_with("tapped")
        })
        .unwrap();
    let tap = tapped.default.clone().unwrap();
    let given = answers(json!({
        "part.material": "PLA (FDM)",
        "part.process": FDM,
        "part.general_tolerance": "ISO 2768-c",
        "part.locating": "position tolerances",
        tapped.id.clone(): tap.clone(),
    }));
    let qs = api::questions(&analysis, &given).unwrap();
    assert!(ids(&qs).iter().any(|id| id.starts_with("hole.position:")));
    let intent = api::apply(&analysis, &given, true).unwrap();
    assert_eq!(intent.part["general_tolerance"], json!("ISO 2768-c"));
    assert!(intent.requirements.iter().any(|r| r["kind"] == "thread"));
    assert!(intent.requirements.iter().any(|r| r["kind"] == "position"));
    // The person's own ± figure, typed as they would, is the general tolerance note.
    let typed = specify_core::choices::interpret(
        "part.general_tolerance",
        "+-0.2 mm",
        &question(&qs, "part.general_tolerance").options,
        None,
        None,
    )
    .unwrap();
    assert_eq!(typed, "±0.2 mm");
    let mut own = given.clone();
    own.insert("part.general_tolerance".into(), json!(typed));
    let intent = api::apply(&analysis, &own, true).unwrap();
    assert_eq!(intent.part["general_tolerance"], json!("±0.2 mm"));
    assert!(intent.general.iter().all(|r| r["upper"] == json!(0.2)));
    let (_, report) = writer::write_bytes(&loaded, &intent, Some(&own)).unwrap();
    assert!(
        report
            .written
            .contains(&"general tolerance note ±0.2 mm".to_string())
    );
}

#[test]
fn a_part_answer_the_process_leaves_unasked_is_not_written() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    // Carried from when the part was machined (#129): the answer is kept, not written.
    let given = answers(json!({
        "part.material": "PLA (FDM)",
        "part.surface_finish": "Ra 1.6",
        "part.process": FDM,
    }));
    let intent = api::apply(&analysis, &given, true).unwrap();
    assert!(!intent.part.contains_key("surface_finish"));
    let row = intent
        .requirements
        .iter()
        .find(|r| r["kind"] == rules::UNASKED)
        .expect("an unasked row");
    assert_eq!(row["answer"], json!("part.surface_finish"));
    assert_eq!(
        row["not_written_reason"],
        json!(
            "part.surface_finish is not asked when part.process is '3D printed: FDM', so its \
             answer was not written"
        )
    );
    assert_eq!(
        rules::unasked_rows(&analysis, &given).unwrap(),
        vec![row.clone()]
    );
    let (bytes, report) = writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    assert!(
        report
            .not_written
            .contains(&row["not_written_reason"].as_str().unwrap().to_string())
    );
    // The answer itself is kept, and is written again once the part is machined.
    let back = part::File::of_bytes(Path::new("p.step"), bytes)
        .unwrap()
        .load(None)
        .unwrap();
    let mut stored = resume::answers_for(&back).unwrap().unwrap();
    assert_eq!(stored["part.surface_finish"], json!("Ra 1.6"));
    stored.insert("part.process".into(), json!(MACHINED));
    let intent = api::apply(&analysis, &stored, true).unwrap();
    assert_eq!(intent.part["surface_finish"], json!("Ra 1.6"));
    assert!(
        intent
            .requirements
            .iter()
            .all(|r| r["kind"] != rules::UNASKED)
    );
}

#[test]
fn the_attribute_is_on_the_product_definition_and_reads_back_with_its_source() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    // Answered: no marker.
    let given = answers(json!({"part.material": "Tough resin (SLA)", "part.process": RESIN}));
    let intent = api::apply(&analysis, &given, true).unwrap();
    assert!(!intent.part.contains_key("process_default"));
    let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    let owners = process_owners(&bytes);
    assert_eq!(owners.len(), 1);
    assert_eq!(entity_name(&bytes, owners[0]), "PRODUCT_DEFINITION");
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("GENERAL_PROPERTY('','specify process',$)"));
    assert!(!text.contains(process::DEFAULT_ITEM));
    let field = read_back(&bytes, None).unwrap();
    assert_eq!(
        (field.value, field.source.as_str()),
        (json!(RESIN), "answered")
    );
    let back = part::File::of_bytes(Path::new("p.step"), bytes.clone())
        .unwrap()
        .load(None)
        .unwrap();
    let resumed = api::analyse(&back).unwrap();
    let answered = resumed["resume"]["answers"].as_object().unwrap().clone();
    assert_eq!(answered[process::QUESTION], json!(RESIN));
    assert_eq!(
        resumed["process"],
        json!({"value": RESIN, "source": "answered"})
    );
    let field = process::of(&resumed, &answered).unwrap().unwrap();
    assert_eq!(
        (field.value, field.source.as_str()),
        (json!(RESIN), "answered")
    );

    // Accepted suggestion: marked, read as a default, and the question's default next time.
    let given = answers(json!({"part.material": "PA12 (MJF)"}));
    let intent = api::apply(&analysis, &given, true).unwrap();
    let (bytes, report) = writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    assert!(
        report
            .written
            .contains(&format!("process {POWDER} (suggested)"))
    );
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        text.contains("DESCRIPTIVE_REPRESENTATION_ITEM('default.manufacturing_process','true')")
    );
    let field = read_back(&bytes, None).unwrap();
    assert_eq!(
        (field.value, field.source.as_str()),
        (json!(POWDER), "default")
    );
    let back = part::File::of_bytes(Path::new("p.step"), bytes.clone())
        .unwrap()
        .load(None)
        .unwrap();
    let resumed = api::analyse(&back).unwrap();
    let answered = resumed["resume"]["answers"].as_object().unwrap().clone();
    assert!(!answered.contains_key(process::QUESTION));
    let qs = api::questions(&resumed, &answered).unwrap();
    assert_eq!(
        question(&qs, process::QUESTION).default,
        Some(json!(POWDER))
    );
    let intent = api::apply(&resumed, &answered, true).unwrap();
    let (again, _) = writer::write_bytes(&back, &intent, Some(&answered)).unwrap();
    assert_eq!(again, bytes);
}

/// `bytes` with another tool's UDA, as CAx-IF practice 1.8 forms it, on `owner`.
fn foreign_uda(bytes: Vec<u8>, owner: u64, key: &str, value: &str) -> Vec<u8> {
    uda_set(bytes, owner, "Exporter attributes", key, value)
}

fn uda_set(bytes: Vec<u8>, owner: u64, set: &str, key: &str, value: &str) -> Vec<u8> {
    let doc = Document::parse(bytes).unwrap();
    let mut edit = Edit::new();
    let s = |v: &str| A::String(v.into());
    let item = edit.add(p21::simple(
        "DESCRIPTIVE_REPRESENTATION_ITEM",
        vec![s(key), s(value)],
    ));
    let context = edit.add(p21::simple(
        "REPRESENTATION_CONTEXT",
        vec![s(""), s("user defined attributes")],
    ));
    let rep = edit.add(p21::simple(
        "REPRESENTATION",
        vec![s(""), A::List(vec![item.to_ref()]), context.to_ref()],
    ));
    let property = edit.add(p21::simple(
        "PROPERTY_DEFINITION",
        vec![s(set), s("user defined attribute"), A::EntityRef(owner)],
    ));
    edit.add(p21::simple(
        "PROPERTY_DEFINITION_REPRESENTATION",
        vec![property.to_ref(), rep.to_ref()],
    ));
    let general = edit.add(p21::simple(
        "GENERAL_PROPERTY",
        vec![s(""), s(set), A::Unset],
    ));
    edit.add(p21::simple(
        "GENERAL_PROPERTY_ASSOCIATION",
        vec![s(""), A::Unset, general.to_ref(), property.to_ref()],
    ));
    doc.apply(&edit).unwrap().bytes
}

#[test]
fn another_tools_attribute_is_the_files_and_is_not_asked() {
    let loaded = plate();
    let bytes = foreign_uda(
        loaded.bytes.as_ref().clone(),
        loaded.definition.product_definition,
        "Manufacturing Process",
        "3d printed: resin",
    );
    let field = read_back(&bytes, None).unwrap();
    assert_eq!((field.value, field.source.as_str()), (json!(RESIN), "file"));
    let back = part::File::of_bytes(Path::new("p.step"), bytes)
        .unwrap()
        .load(None)
        .unwrap();
    let analysis = api::analyse(&back).unwrap();
    assert_eq!(analysis["part"]["process"], json!(RESIN));
    assert_eq!(
        analysis["process"],
        json!({"value": RESIN, "source": "file"})
    );
    let given = answers(json!({"part.material": "steel"}));
    let qs = api::questions(&analysis, &given).unwrap();
    assert!(!ids(&qs).contains(&process::QUESTION));
    // The file's process sets the defaults, and nothing is written for it.
    assert!(!ids(&qs).contains(&"part.surface_finish"));
    let field = process::of(&analysis, &given).unwrap().unwrap();
    assert_eq!((field.value, field.source.as_str()), (json!(RESIN), "file"));
    let intent = api::apply(&analysis, &given, true).unwrap();
    assert!(!intent.part.contains_key("process"));
    // A value that is no process is ignored.
    let odd = foreign_uda(
        loaded.bytes.as_ref().clone(),
        loaded.definition.product_definition,
        "manufacturing_process",
        "laser cut",
    );
    assert!(read_back(&odd, None).is_none());
    // OpenCascade writes its metadata back as one item named after the set (XCAF keeps one
    // value per set): Specify's set so rewritten has lost any marker, and is the file's.
    let rewritten = uda_set(
        loaded.bytes.as_ref().clone(),
        loaded.definition.product_definition,
        process::PROPERTY,
        process::PROPERTY,
        POWDER,
    );
    let field = read_back(&rewritten, None).unwrap();
    assert_eq!(
        (field.value, field.source.as_str()),
        (json!(POWDER), "file")
    );
}

fn assembly() -> part::File {
    part::File::read(&fixture("assembly")).unwrap()
}

#[test]
fn bought_parts_and_the_assembly_product_have_no_process_and_shared_products_have_one() {
    let file = assembly();
    let parts = file.load_all().unwrap();
    assert_eq!(parts[1].placements.len(), 2, "the pin is placed twice");
    let analyses: Vec<Value> = parts
        .iter()
        .map(|p| api::analyse_reporting_in(&file, p).unwrap().0)
        .collect();
    // A bought part is not asked, and an answer given for it is not written.
    let bought = answers(json!({"part.material": "steel", "bom.make_buy": "bought"}));
    let qs = api::questions(&analyses[1], &bought).unwrap();
    assert!(!ids(&qs).contains(&process::QUESTION));
    assert_eq!(process::of(&analyses[1], &bought).unwrap(), None);
    let mut lapsed = bought.clone();
    lapsed.insert(process::QUESTION.into(), json!(FDM));
    let intent = api::apply(&analyses[1], &lapsed, true).unwrap();
    assert!(!intent.part.contains_key("process"));
    let row = intent
        .requirements
        .iter()
        .find(|r| r["kind"] == rules::UNASKED)
        .unwrap();
    assert_eq!(
        row["not_written_reason"],
        json!("part.process is not asked of a bought part, so its answer was not written")
    );
    // The machined questions are asked of it, as before.
    assert!(ids(&qs).contains(&"part.surface_finish"));
    // A guessed bought part is still asked: a suggestion is never applied.
    let made = answers(json!({"part.material": "steel"}));
    assert_eq!(analyses[1]["bom"]["make_buy"]["source"], json!("default"));
    assert!(ids(&api::questions(&analyses[1], &made).unwrap()).contains(&process::QUESTION));

    // Each product gets one set on its product definition, however often it is placed.
    let given = [
        answers(json!({"part.material": "PLA (FDM)", "part.process": FDM})),
        answers(json!({"part.material": "steel", "part.process": MACHINED})),
    ];
    let intents: Vec<_> = analyses
        .iter()
        .zip(&given)
        .map(|(a, g)| api::apply(a, g, true).unwrap())
        .collect();
    let jobs: Vec<_> = parts
        .iter()
        .zip(&intents)
        .zip(&given)
        .map(|((p, i), a)| (p, i, Some(a)))
        .collect();
    let (bytes, _) = writer::write_parts_bytes(&jobs).unwrap();
    let mut owners = process_owners(&bytes);
    owners.sort();
    let mut expected: Vec<u64> = parts
        .iter()
        .map(|p| p.definition.product_definition)
        .collect();
    expected.sort();
    assert_eq!(owners, expected);
    let field = read_back(&bytes, Some(0)).unwrap();
    assert_eq!(field.value, json!(FDM));
    let field = read_back(&bytes, Some(1)).unwrap();
    assert_eq!(field.value, json!(MACHINED));
}

#[test]
fn a_process_answer_is_checked_and_round_trips_through_the_answer_file() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let wrong = answers(json!({"part.material": "steel", "part.process": "printed"}));
    assert!(api::questions(&analysis, &wrong).is_err());
    assert!(specify_core::input::answers(&analysis, &wrong).is_err());
    let given = answers(json!({"part.material": "steel", "part.process": "cast"}));
    specify_core::input::answers(&analysis, &given).unwrap();
    let qs = api::questions(&analysis, &given).unwrap();
    assert!(!ids(&qs).contains(&"part.surface_finish"));
    assert!(ids(&qs).contains(&"part.edges"));
    let exported = api::export_answers(&loaded, &analysis, &given).unwrap();
    assert_eq!(exported[process::QUESTION], json!("cast"));
}
