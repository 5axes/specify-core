//! Assemblies on haecceity's part API (specify-core-rust #2, slice 2 of the assemblies design;
//! decisions D1, D3 and D4 of 2026-10-15), on `parts/assembly.step` (a plate, and a pin placed
//! twice; byte-identical to the editor's `scripts/fixtures/assembly.step`):
//!
//! - the file is read once (`part::File`, on haecceity's `StepFile`) and any part loaded from it,
//!   from a path or from bytes;
//! - `parts` reports each part's index with its name and product id (D1);
//! - a part's analysis is what it was before the change (the base commit's, committed as
//!   `parts/assembly_analyses_base.json`), its other parts loaded from the same read;
//! - one part is written alone, in memory (`api::write_bytes`), and then the other alone, each
//!   keeping the other's answers and PMI (D3), the pin's PMI written once for its two placements;
//! - an assembly with no part chosen is refused, naming its parts (D4).

mod common;

use std::path::{Path, PathBuf};

use haecceity::p21::Document;
use haecceity::pmi;
use haecceity::step::read_part_definitions;
use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::{self, File, LoadError};
use specify_core::resume::{answers_for, stored};
use specify_core::writer::WriteError;

const PLATE: usize = 0;
const PIN: usize = 1;

fn assembly() -> PathBuf {
    common::fixtures().join("parts").join("assembly.step")
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-assemblies-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn answers(k: usize) -> Map<String, Value> {
    let material = if k == PLATE { "steel" } else { "brass" };
    json!({ "part.material": material })
        .as_object()
        .cloned()
        .unwrap()
}

fn pmi_of(bytes: &[u8]) -> Vec<pmi::PartPmi> {
    let defs = read_part_definitions(bytes).unwrap();
    let doc = Document::parse(bytes.to_vec()).unwrap();
    pmi::read(&doc, &defs).unwrap().parts
}

fn datum_count(bytes: &[u8]) -> usize {
    regex::Regex::new(r"= ?DATUM\(")
        .unwrap()
        .find_iter(&String::from_utf8_lossy(bytes))
        .count()
}

/// `parts` lists exactly the plate and the pin, the pin with its two placements, each by its
/// index with its name and its product's id beside it (D1), from the library, the API and the
/// command line alike.
#[test]
fn parts_are_listed_by_index_with_name_and_product_id() {
    let listed = part::parts(&assembly()).unwrap();
    let got: Vec<(usize, &str, &str, usize, usize)> = listed
        .iter()
        .map(|p| {
            (
                p.part,
                p.name.as_str(),
                p.product_id.as_str(),
                p.faces,
                p.instances,
            )
        })
        .collect();
    assert_eq!(got, [(0, "plate", "plate", 13, 1), (1, "pin", "pin", 7, 2)]);

    let api = api::parts(&assembly()).unwrap();
    assert_eq!(api.len(), 2);
    for (value, summary) in api.iter().zip(&listed) {
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "part",
                "name",
                "product_id",
                "faces",
                "instances",
                "placements",
                "written",
                "same_as"
            ]
        );
        // The plate and the pin are different parts (#150).
        assert_eq!(value["same_as"], json!(null));
        assert_eq!(value["product_id"], json!(summary.product_id));
        assert_eq!(value["written"], json!(false));
    }

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("parts")
        .arg(assembly())
        .output()
        .unwrap();
    assert!(out.status.success());
    let cli: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(cli, Value::Array(api));
}

/// `got` equals `want` field for field, numbers within 1e-12 mm: the base was captured on
/// macOS, and other platforms' libm can move a frame origin by a few 1e-17.
fn assert_same(got: &Value, want: &Value, at: &str) {
    match (got, want) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            assert!((a - b).abs() <= 1e-12, "{at}: {a} != {b}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{at}: length");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                assert_same(x, y, &format!("{at}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(
                a.keys().collect::<Vec<_>>(),
                b.keys().collect::<Vec<_>>(),
                "{at}: keys"
            );
            for (k, x) in a {
                assert_same(x, &b[k], &format!("{at}.{k}"));
            }
        }
        _ => assert_eq!(got, want, "{at}"),
    }
}

/// The base includes quiddity #92 schema-6 file coordinates, checked against the face below.
/// A part's analysis agrees field for field (faces, features, mates, existing,
/// binding), whether the part comes from a path, from a `File` whose path has since gone (the
/// other parts the mates need are loaded from the same read, not from disk), or from bytes held
/// in memory under another name.
#[test]
fn a_part_is_analysed_as_before_from_one_read() {
    // quiddity #123 adds the pin's whole turned-body classification. The captured
    // base includes that one record; every face, feature and PMI field stays checked.
    // #75 adds the complete typed shared-planner input to both frozen analyses.
    let base: Value = serde_json::from_str(
        &std::fs::read_to_string(
            common::fixtures()
                .join("parts")
                .join("assembly_analyses_base.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let dir = temp("one-read");
    let copy = dir.join("assembly.step");
    std::fs::copy(assembly(), &copy).unwrap();
    let file = File::read(&copy).unwrap();
    std::fs::remove_file(&copy).unwrap();
    let in_memory = File::of_bytes(
        Path::new("browser.step"),
        std::fs::read(assembly()).unwrap(),
    )
    .unwrap();
    for k in [PLATE, PIN] {
        let want = &base[k.to_string()];
        let bom = serde_json::to_value(&specify_core::bom::read(&file).unwrap()[k]).unwrap();
        let legacy = |mut analysis: Value, at: &str| {
            // For #79 the only additive analysis field is the sourced BOM row.
            // Keep comparing every pre-existing key, value and ordering to the old capture.
            let difference = common::Difference {
                case: format!("parts/assembly#{k}"),
                path: "bom".into(),
                python: None,
                rust: analysis.get("bom").cloned(),
            };
            assert!(
                common::explained(&["analysis"], &[difference]).is_empty(),
                "unrecorded BOM addition"
            );
            assert_eq!(
                analysis.as_object_mut().unwrap().shift_remove("bom"),
                Some(bom.clone())
            );
            assert_same(&analysis, want, at);
        };
        legacy(
            api::analyse_file(&assembly(), Some(k)).unwrap(),
            &format!("part {k}"),
        );

        let loaded = file.load(Some(k)).unwrap();
        assert!(!loaded.rebuilt && loaded.degraded().is_none());
        assert_eq!(loaded.unresolved_faces, loaded.part.unresolved_faces());
        let (analysis, warnings) = api::analyse_reporting_in(&file, &loaded).unwrap();
        legacy(analysis.clone(), &format!("part {k}, its file gone"));
        assert!(warnings.is_empty(), "{warnings:?}");
        // #92: the old working X was -file Z. The converted station must still be on
        // riser face 2; its height interval must still span that face in file Y.
        if k == PIN {
            // #123 inserts a turned-body record before risers in family order.
            let record = &analysis["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["id"] == "risers:2")
                .unwrap()["record"];
            assert_eq!(record["axis"], "z");
            assert_eq!(record["height_axis"], "y");
            let station =
                record["origin"][2].as_f64().unwrap() + record["positions"][0].as_f64().unwrap();
            let bounds = loaded.part.face_bounds(2);
            assert!((station - bounds.min[2]).abs() < 0.000501);
            assert!((station - bounds.max[2]).abs() < 0.000501);
            for (key, y) in [("z_lo", bounds.min[1]), ("z_hi", bounds.max[1])] {
                assert!(
                    (record["origin"][1].as_f64().unwrap() + record[key].as_f64().unwrap() - y)
                        .abs()
                        < 1e-9
                );
            }
        }

        let loaded = in_memory.load(Some(k)).unwrap();
        legacy(
            api::analyse(&loaded).unwrap(),
            &format!("part {k} from memory"),
        );
    }
    // A part is analysed only against its own file.
    let plate = part::load(&common::fixtures().join("parts").join("plate.step"), None).unwrap();
    assert!(api::analyse_reporting_in(&file, &plate).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// D3, in memory: the pin written alone with its answers, then the plate written alone with its
/// own. Each write keeps the earlier part's answers and PMI; `parts` then shows both written;
/// each part reads back its own material; and the pin, placed twice, has its PMI once (the file
/// has no per-placement entities, so its datums are not doubled). The bytes equal the file
/// writer's for the same write.
#[test]
fn parts_written_alone_in_memory_keep_each_others_answers() {
    let dir = temp("alone");
    let original = std::fs::read(assembly()).unwrap();
    let write = |bytes: &[u8], k: usize| -> (Vec<u8>, Value) {
        let file = File::of_bytes(Path::new("a.step"), bytes.to_vec()).unwrap();
        let analysis = api::analyse(&file.load(Some(k)).unwrap()).unwrap();
        let intent = api::apply(&analysis, &answers(k), true).unwrap();
        let datums = json!(intent.datums.len());
        let (out, report) =
            api::write_bytes(bytes.to_vec(), &[(intent, Some(answers(k)))]).unwrap();
        assert_eq!(report.output, PathBuf::from(api::IN_MEMORY));
        assert!(!report.written.is_empty());
        (out, datums)
    };

    let (after_pin, pin_datums) = write(&original, PIN);
    let read = pmi_of(&after_pin);
    assert_eq!(
        read[PLATE],
        pmi::PartPmi::default(),
        "the plate is untouched"
    );
    assert_eq!(read[PIN].material.as_ref().unwrap().id, "brass");
    assert_eq!(json!(read[PIN].datums.len()), pin_datums);
    assert_eq!(
        json!(datum_count(&after_pin)),
        pin_datums,
        "the pin's datums, once"
    );
    assert_eq!(
        stored(&after_pin).keys().copied().collect::<Vec<_>>(),
        [PIN]
    );

    let (after_plate, plate_datums) = write(&after_pin, PLATE);
    let read_again = pmi_of(&after_plate);
    assert_eq!(read_again[PIN], read[PIN], "the pin's PMI is kept");
    assert_eq!(read_again[PLATE].material.as_ref().unwrap().id, "steel");
    assert_eq!(
        json!(datum_count(&after_plate)),
        json!(plate_datums.as_u64().unwrap() + pin_datums.as_u64().unwrap())
    );
    let file = File::of_bytes(Path::new("b.step"), after_plate.clone()).unwrap();
    for k in [PLATE, PIN] {
        assert_eq!(
            answers_for(&file.load(Some(k)).unwrap()).unwrap(),
            Some(answers(k)),
            "part {k}"
        );
    }
    let path = dir.join("both.step");
    std::fs::write(&path, &after_plate).unwrap();
    let listed = api::parts(&path).unwrap();
    assert!(
        listed.iter().all(|p| p["written"] == json!(true)),
        "{listed:?}"
    );

    // The file writer writes the same bytes for the same write.
    let start = dir.join("pin.step");
    std::fs::write(&start, &after_pin).unwrap();
    let analysis = api::analyse_file(&start, Some(PLATE)).unwrap();
    let intent = api::apply(&analysis, &answers(PLATE), true).unwrap();
    let out = dir.join("out.step");
    api::write_parts(&start, &[(intent, Some(answers(PLATE)))], &out).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), after_plate);
    let _ = std::fs::remove_dir_all(&dir);
}

/// D4: an assembly with no part chosen is refused, naming its parts, from a path, from bytes and
/// from a `File`; a part the file does not have is refused by the bytes writer.
#[test]
fn an_assembly_with_no_part_chosen_is_refused_naming_its_parts() {
    let bytes = std::fs::read(assembly()).unwrap();
    let file = File::of_bytes(Path::new("asm.step"), bytes.clone()).unwrap();
    let Err(LoadError::Assembly { names, .. }) = file.load(None) else {
        panic!("an assembly loads with no part chosen");
    };
    assert_eq!(names, ["plate", "pin"]);
    let message = part::load(&assembly(), None).unwrap_err().to_string();
    assert!(
        message.ends_with("is an assembly of 2 parts; choose one (0: plate, 1: pin)"),
        "{message}"
    );
    assert!(part::load_bytes("asm.step", bytes.clone()).is_err());

    let analysis = api::analyse(&file.load(Some(PIN)).unwrap()).unwrap();
    let mut intent = api::apply(&analysis, &answers(PIN), true).unwrap();
    intent.binding["part"] = json!(5);
    match api::write_bytes(bytes, &[(intent, Some(answers(PIN)))]) {
        Err(api::Error::Write(WriteError::Invalid(m))) => {
            assert_eq!(m, format!("{} has no part 5", api::IN_MEMORY))
        }
        other => panic!("not refused: {other:?}"),
    }
}
