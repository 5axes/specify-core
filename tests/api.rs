//! The API composed from the stage 1-3 modules: specify-core's `tests/test_assembly.py` (its API
//! and serve tests: parts listing, an assembly analysed a part at a time, written together and
//! again through the service, an unanswered part named, an intent or analysis made before parts
//! were told apart), the serve halves of `tests/test_preview.py` and `tests/test_anchors.py`
//! (preview with anchors), and the analysis's shape on every synthetic part.
//!
//! Feature ids are the Rust engine's (defining faces, upstream need U2), so where a Python test
//! names one (`holes:3.5`) the test here finds the feature by what it is.

mod common;

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use specify_core::api::{self, Error};
use specify_core::part::load;
use specify_core::rules::{self, IncompleteError};
use specify_core::serve::Server;

const PLATE: usize = 0;
const PIN: usize = 1;

fn part(stem: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{stem}.step"))
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

/// The answers test_assembly.py gives each part.
fn assembly_answers(k: usize) -> Map<String, Value> {
    answers(json!({"part.material": if k == PLATE { "steel" } else { "brass" }}))
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-api-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// test_assembly.py::test_each_part_is_listed_once_with_where_its_instances_are
#[test]
fn each_part_is_listed_once_with_where_its_instances_are() {
    let parts = api::parts(&part("assembly")).unwrap();
    let [plate, pin] = parts.as_slice() else {
        panic!("two parts: {parts:?}")
    };
    let summary = |p: &Value| {
        (
            p["name"].clone(),
            p["faces"].clone(),
            p["instances"].clone(),
        )
    };
    assert_eq!(summary(plate), (json!("plate"), json!(13), json!(1)));
    assert_eq!(summary(pin), (json!("pin"), json!(7), json!(2)));
    let offsets: Vec<Vec<Value>> = pin["placements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            p.as_array()
                .unwrap()
                .iter()
                .map(|row| row[3].clone())
                .collect()
        })
        .collect();
    assert_eq!(
        offsets,
        vec![
            vec![json!(-30.0), json!(0.0), json!(5.0)],
            vec![json!(30.0), json!(0.0), json!(5.0)]
        ]
    );
    assert_eq!(plate["written"], json!(false));
    assert_eq!(pin["written"], json!(false));
    assert_eq!(pin["same_as"], json!(null));
    // Python's key order, with the product's id after the name (decision D1), then `same_as`
    // (#150), which Python's listing does not have.
    let keys: Vec<&String> = pin.as_object().unwrap().keys().collect();
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
}

/// test_assembly.py::test_an_assembly_is_analysed_a_part_at_a_time
#[test]
fn an_assembly_is_analysed_a_part_at_a_time() {
    let refused = api::analyse_file(&part("assembly"), None).unwrap_err();
    assert!(
        matches!(refused, Error::Load(_))
            && refused
                .to_string()
                .contains("an assembly of 2 parts; choose one"),
        "{refused}"
    );
    let pin = api::analyse_file(&part("assembly"), Some(PIN)).unwrap();
    assert_eq!(pin["binding"]["part"], json!(PIN));
    assert_eq!(pin["faces"].as_array().unwrap().len(), 7);
    let defaults: Vec<Value> = api::questions(&pin, &Map::new())
        .unwrap()
        .into_iter()
        .filter_map(|q| q.default)
        .collect();
    assert!(defaults.contains(&json!("tapped:M5")), "{defaults:?}");
    let mesh = api::mesh_file(&part("assembly"), Some(PIN)).unwrap();
    assert_eq!(mesh["faces"].as_array().unwrap().len(), 7);
}

/// The analysis retains Python key order, with the shared planner input after features (#75).
/// `mates` and contacts appear only
/// for a part of an assembly, `existing` and `part` for a file specify-core did not write.
#[test]
fn analyses_have_python_layout() {
    for stem in [
        "plate",
        "pin",
        "wrapped",
        "a_placed_part_is_meshed_in_the_frame_the_analysis_uses",
    ] {
        let a = api::analyse_file(&part(stem), None).unwrap();
        let keys: Vec<&String> = a.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "schema",
                "binding",
                "faces",
                "features",
                "dimensioning",
                "existing",
                "part"
            ],
            "{stem}"
        );
        assert_eq!(a["schema"], json!(api::ANALYSIS_SCHEMA));
        let input: quiddity::dimensioning::Input =
            serde_json::from_value(a["dimensioning"].clone()).unwrap();
        assert!(
            input.specified.is_empty(),
            "unwritten input has no stored contracts"
        );
    }
    let mut differences = Vec::new();
    for (stem, count) in [("stack", 3), ("assembly", 2)] {
        let file = specify_core::part::File::read(&part(stem)).unwrap();
        let rows = specify_core::bom::read(&file).unwrap();
        for (k, row) in rows.iter().enumerate().take(count) {
            let mut a = api::analyse_file(&part(stem), Some(k)).unwrap();
            let case = format!("parts/{stem}#{k}");
            // Record the actual additive value, rather than silently allowing a key.
            // Missing BOM rows leave stale ledger entries; any other layout change
            // still fails the exact Python key-order assertion below.
            if let Some(bom) = a.as_object_mut().unwrap().shift_remove("bom") {
                assert_eq!(
                    bom,
                    serde_json::to_value(row).unwrap(),
                    "{case}: BOM reader"
                );
                assert_eq!(bom["part"], json!(k));
                assert_eq!(
                    bom["name"],
                    json!({"value":file.definitions()[k].name,"source":"file"})
                );
                assert_eq!(
                    bom["quantity"],
                    json!({"value":file.definitions()[k].placements.len(),"source":"file"})
                );
                assert_eq!(bom["item_number"], json!({"value":null,"source":"default"}));
                differences.push(common::Difference {
                    case: case.clone(),
                    path: "bom".into(),
                    python: None,
                    rust: Some(bom),
                });
            }
            let input: quiddity::dimensioning::Input =
                serde_json::from_value(a["dimensioning"].clone()).unwrap();
            assert!(
                input.specified.is_empty(),
                "{case}: unwritten planner contracts"
            );
            let keys: Vec<&String> = a.as_object().unwrap().keys().collect();
            assert_eq!(
                keys,
                [
                    "schema",
                    "binding",
                    "faces",
                    "features",
                    "dimensioning",
                    "mates",
                    "existing",
                    "part"
                ],
                "{case}"
            );
        }
        assert_eq!(rows.len(), count);
    }
    common::check_known("analysis", &differences);
    // The stack's parts rest on one another: some face of the base has a contact.
    let base = api::analyse_file(&part("stack"), Some(0)).unwrap();
    let contacts: Vec<&Value> = base["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f.get("contact"))
        .collect();
    assert!(!contacts.is_empty());
    assert!(
        contacts
            .iter()
            .all(|c| c["part"].is_string() && c["area"].is_number())
    );
}

/// test_assembly.py's `written` fixture and the tests on it: both parts written together and
/// verified, each resuming its own answers, written again through the service (its PMI
/// replaced, not added to), listed as written, and analysed with its answers back.
#[test]
fn an_assembly_is_written_together_and_again_through_the_service() {
    let dir = temp("written");
    let assembly = part("assembly");
    let intents: Vec<_> = [PLATE, PIN]
        .iter()
        .map(|&k| {
            let analysis = api::analyse_file(&assembly, Some(k)).unwrap();
            let intent = api::apply(&analysis, &assembly_answers(k), true).unwrap();
            (intent, Some(assembly_answers(k)))
        })
        .collect();
    let out = dir.join("written.step");
    let report = api::write_parts(&assembly, &intents, &out).unwrap();
    // test_both_parts_are_written_and_verified (the Rust writer lists notes as not written).
    assert!(
        report
            .written
            .contains(&"plate: material steel".to_string())
    );
    assert!(
        report
            .written
            .iter()
            .any(|l| l.starts_with("pin: internal thread M5x0.8-6H on holes:")),
        "{:?}",
        report.written
    );
    assert!(report.not_written.is_empty(), "{:?}", report.not_written);
    // test_each_part_resumes_its_own_answers
    let listed = api::parts(&out).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|p| p["written"].clone())
            .collect::<Vec<_>>(),
        [json!(true), json!(true)]
    );
    for k in [PLATE, PIN] {
        let back = api::analyse_file(&out, Some(k)).unwrap();
        assert_eq!(back["resume"], json!({"answers": assembly_answers(k)}));
        assert_eq!(back["existing"], json!([]));
        assert_eq!(back["part"], json!({}));
    }
    // test_written_again_through_the_service_its_pmi_is_replaced
    let again = dir.join("again.step");
    let parts: Vec<Value> = [PLATE, PIN]
        .iter()
        .map(|&k| json!({"analysis": api::analyse_file(&out, Some(k)).unwrap(), "answers": assembly_answers(k)}))
        .collect();
    let mut server = Server::default();
    let result = server.reply(&json!({
        "id": 1, "op": "write", "step": out, "output": again,
        "accept_defaults": true, "parts": parts,
    }));
    assert_eq!(result["ok"], json!(true), "{result}");
    assert_eq!(result["result"]["intents"].as_array().unwrap().len(), 2);
    let datums = |p: &PathBuf| {
        let text = std::fs::read_to_string(p).unwrap();
        regex::Regex::new(r"= ?DATUM\(")
            .unwrap()
            .find_iter(&text)
            .count()
    };
    assert_eq!(datums(&again), datums(&out));
    assert!(datums(&out) > 0);
    let listed = server.reply(&json!({"id": 2, "op": "parts", "step": again}));
    assert_eq!(listed["result"]["parts"][PIN]["written"], json!(true));
    let analysed = server.reply(&json!({"id": 3, "op": "analyse", "step": again, "part": PIN}));
    assert_eq!(
        analysed["result"]["analysis"]["resume"],
        json!({"answers": assembly_answers(PIN)})
    );
    assert_eq!(
        analysed["result"]["mesh"]["faces"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// test_assembly.py::test_an_unanswered_part_is_named
#[test]
fn an_unanswered_part_is_named() {
    let dir = temp("unanswered");
    let assembly = part("assembly");
    let parts: Vec<Value> = [PLATE, PIN]
        .iter()
        .map(|&k| {
            let answers = if k == PLATE {
                json!(assembly_answers(k))
            } else {
                json!({})
            };
            json!({"analysis": api::analyse_file(&assembly, Some(k)).unwrap(), "answers": answers})
        })
        .collect();
    let result = Server::default().reply(&json!({
        "id": 1, "op": "write", "step": assembly, "output": dir.join("x.step"),
        "accept_defaults": true, "parts": parts,
    }));
    let error = &result["error"];
    assert_eq!(error["code"], json!("incomplete"), "{result}");
    assert_eq!(error["part"], json!(PIN));
    assert!(error["message"].as_str().unwrap().starts_with("part 1: "));
    assert!(!dir.join("x.step").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// test_assembly.py::test_an_intent_made_before_parts_were_told_apart_still_writes
#[test]
fn an_intent_made_before_parts_were_told_apart_still_writes() {
    let dir = temp("old-intent");
    let pin = part("pin");
    let analysis = api::analyse_file(&pin, None).unwrap();
    let mut intent =
        api::apply(&analysis, &answers(json!({"part.material": "brass"})), true).unwrap();
    intent.binding.as_object_mut().unwrap().remove("part");
    assert_eq!(api::intent_part(&intent).unwrap(), 0);
    let report = api::write_file(&pin, &intent, &dir.join("old.step"), None).unwrap();
    // As in Python, nothing is left unwritten (the part's notes too, Q3).
    assert!(report.not_written.is_empty(), "{:?}", report.not_written);
    let _ = std::fs::remove_dir_all(&dir);
}

/// test_assembly.py::test_an_analysis_from_before_parts_still_previews_with_anchors
#[test]
fn an_analysis_from_before_parts_still_previews_with_anchors() {
    let pin = part("pin");
    let mut analysis = api::analyse_file(&pin, None).unwrap();
    analysis["binding"].as_object_mut().unwrap().remove("part");
    // #20: labels exist only for accepted specifications, including legacy bindings.
    let accepted = specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap();
    let reply = Server::default().reply(&json!({
        "id": 1, "op": "preview", "analysis": analysis, "answers": accepted, "step": pin,
    }));
    let anchors = &reply["result"]["intent"]["anchors"];
    assert!(
        anchors.as_object().is_some_and(|a| !a.is_empty()),
        "{reply}"
    );
}

/// test_anchors.py::test_preview_carries_anchors_for_its_labels_when_given_the_file
#[test]
fn preview_carries_anchors_for_its_labels_when_given_the_file() {
    let pin = part("pin");
    let analysis = api::analyse_file(&pin, None).unwrap();
    let mut server = Server::default();
    // #20: anchor geometry is tested on explicitly accepted specifications.
    let accepted = specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap();
    let request = json!({"id": 1, "op": "preview", "analysis": analysis, "answers": accepted});
    let plain = server.reply(&request);
    assert!(plain["result"]["intent"].get("anchors").is_none());
    let mut with_step = request.clone();
    with_step["step"] = json!(pin);
    let intent = server.reply(&with_step)["result"]["intent"].clone();
    let anchors = intent["anchors"].as_object().unwrap();
    let labelled: Vec<String> = ["requirements", "datums"]
        .iter()
        .flat_map(|k| intent[*k].as_array().unwrap().iter())
        .filter_map(|item| {
            let faces: Vec<usize> = item["faces"]
                .as_array()?
                .iter()
                .map(|f| f.as_u64().unwrap() as usize)
                .collect();
            (!faces.is_empty()).then(|| specify_core::anchors::key(&faces))
        })
        .collect();
    assert!(!labelled.is_empty());
    for key in &labelled {
        assert!(
            anchors.contains_key(key),
            "{key} has no anchor: {anchors:?}"
        );
    }
    assert!(
        anchors
            .values()
            .all(|a| a["at"].as_array().unwrap().len() == 3
                && a["out"].as_array().unwrap().len() == 3)
    );
    // Asked again, the cached anchors are the same.
    let again = server.reply(&with_step)["result"]["intent"]["anchors"].clone();
    assert_eq!(&again, &intent["anchors"]);
}

/// test_anchors.py::test_anchors_are_not_given_for_a_different_file
#[test]
fn anchors_are_not_given_for_a_different_file() {
    let analysis = api::analyse_file(&part("pin"), None).unwrap();
    // #20: use accepted PMI so an empty result actually checks the foreign binding.
    let accepted = specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap();
    let reply = Server::default().reply(&json!({
        "id": 1, "op": "preview", "analysis": analysis, "answers": accepted, "step": part("plate"),
    }));
    assert_eq!(reply["result"]["intent"]["anchors"], json!({}));
}

/// Anchors are kept for the last two parts only; a third part pushes the first out, and the
/// first is loaded again when asked for.
#[test]
fn anchors_are_cached_for_the_last_two_parts() {
    let mut server = Server::default();
    for stem in ["pin", "plate", "wrapped", "pin"] {
        let analysis = api::analyse_file(&part(stem), None).unwrap();
        // #20: keep exercising actual label anchors as parts enter and leave the cache.
        let accepted = specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap();
        let reply = server.reply(&json!({
            "id": 1, "op": "preview", "analysis": analysis, "answers": accepted, "step": part(stem),
        }));
        let anchors = reply["result"]["intent"]["anchors"].as_object().unwrap();
        assert!(
            !anchors.is_empty(),
            "{stem}: accepted specifications have labels"
        );
        let groups = api::preview(&analysis, &accepted).unwrap();
        let direct = api::anchors(
            &load(&part(stem), None).unwrap(),
            &groups
                .datums
                .iter()
                .chain(&groups.requirements)
                .map(|d| {
                    d["faces"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|f| f.as_u64().unwrap() as usize)
                        .collect()
                })
                .collect::<Vec<Vec<usize>>>(),
        );
        // Every anchor served is the one computed directly (the analysis has no existing PMI).
        for (k, v) in anchors {
            assert_eq!(Some(v), direct.get(k), "{stem} {k}");
        }
        assert_eq!(anchors.len(), direct.len(), "{stem}");
    }
}

/// An intent made before parts were told apart is of part 0; a `part` that is not an index is
/// refused rather than guessed.
#[test]
fn an_intent_part_is_an_index() {
    let analysis = api::analyse_file(&part("pin"), None).unwrap();
    let mut intent =
        api::apply(&analysis, &answers(json!({"part.material": "brass"})), true).unwrap();
    assert_eq!(api::intent_part(&intent).unwrap(), 0);
    intent.binding["part"] = json!("one");
    assert!(api::intent_part(&intent).is_err());
}

/// An apply that leaves questions open says which; `accept_defaults` takes every default.
#[test]
fn apply_refuses_open_questions_and_takes_defaults_when_asked() {
    let analysis = api::analyse_file(&part("plate"), None).unwrap();
    let steel = answers(json!({"part.material": "steel"}));
    match api::apply(&analysis, &steel, false) {
        Err(Error::Rules(rules::Error::Incomplete(IncompleteError {
            missing,
            part: None,
            ..
        }))) => {
            assert!(missing.contains(&"part.general_tolerance".to_string()))
        }
        other => panic!("{other:?}"),
    }
    let intent = api::apply(&analysis, &steel, true).unwrap();
    assert_eq!(intent.part["material"], json!("steel"));
    assert!(!intent.datums.is_empty());
}
