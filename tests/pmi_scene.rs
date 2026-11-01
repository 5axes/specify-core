use serde_json::{Map, Value, json};
use specify_core::{
    api, part,
    pmi_scene::{self, Scene},
    writer,
};
use std::{collections::BTreeSet, path::Path};
mod common;

fn plate() -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step"),
        None,
    )
    .unwrap()
}
fn given() -> Map<String, Value> {
    json!({"part.general_tolerance":"ISO 2768-m", "hole.function:holes:9":"fit:H7"})
        .as_object()
        .unwrap()
        .clone()
}
fn validate(scene: &Value) {
    assert_eq!(scene["pmi"]["schema"], "haecceity.pmi/1");
    assert_eq!(scene["presentation"]["schema"], "haecceity.presentation/1");
    let mut ids = BTreeSet::new();
    for rows in scene["pmi"]["pmi"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_array)
        .chain(
            scene["pmi"]["units"]
                .as_object()
                .unwrap()
                .values()
                .filter_map(Value::as_array),
        )
    {
        for row in rows {
            assert!(
                ids.insert(row["id"].as_str().unwrap()),
                "duplicate {}",
                row["id"]
            );
        }
    }
    if !scene["pmi"]["pmi"]["material"].is_null() {
        ids.insert("part.material");
    }
    let listed: BTreeSet<_> = scene["presentation"]["semantic_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    assert_eq!(ids, listed);
    let planes: BTreeSet<_> = scene["presentation"]["planes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    let mut graphics = BTreeSet::new();
    for (id, rows) in scene["presentation"]["items"].as_object().unwrap() {
        assert!(ids.contains(id.as_str()));
        // Datum-feature aliases and other supports can have graphics without an
        // interview status. Only requirement rows belong in the overlay.
        if scene["specify"].get(id).is_none() {
            assert!(
                ["features", "geometry"]
                    .iter()
                    .any(|key| scene["pmi"]["pmi"][key]
                        .as_array()
                        .is_some_and(|rows| rows.iter().any(|row| row["id"] == *id))),
                "{id}"
            );
        }
        for graphic in rows.as_array().unwrap() {
            validate_graphic(graphic, &planes, &mut graphics);
        }
    }
    for graphic in scene["presentation"]["unlinked"].as_array().unwrap() {
        validate_graphic(graphic, &planes, &mut graphics);
    }
    for view in scene["presentation"]["views"]
        .as_array()
        .into_iter()
        .flatten()
    {
        for id in view["graphics"].as_array().unwrap() {
            assert!(graphics.contains(id.as_str().unwrap()));
        }
    }
}
fn validate_graphic<'a>(graphic: &'a Value, planes: &BTreeSet<&str>, ids: &mut BTreeSet<&'a str>) {
    if let Some(id) = graphic["id"].as_str() {
        ids.insert(id);
    }
    if let Some(id) = graphic["plane_id"].as_str() {
        assert!(planes.contains(id));
    }
    for placement in graphic["text_placements"].as_array().into_iter().flatten() {
        if let Some(index) = placement["text_index"].as_u64() {
            assert!(index < graphic["text"].as_array().unwrap().len() as u64);
        }
        for field in ["origin", "x_axis", "normal"] {
            let vector = placement[field].as_array().unwrap();
            assert_eq!(vector.len(), 3);
            assert!(vector.iter().all(|v| v.as_f64().unwrap().is_finite()));
        }
    }
}
#[test]
fn requirement_ids_are_stable_and_statuses_use_review_and_stored_origin() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let answers = given();
    let first = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&first);
    let metadata = first["specify"].as_object().unwrap();
    assert_eq!(
        metadata["planner:specified:dim:hole:9/bore.diameter"]["status"],
        "answered"
    );
    assert_eq!(
        metadata["planner:specified:dim:hole:9/bore.diameter"]["question"],
        "hole.function:holes:9"
    );
    assert!(metadata.values().any(|m| m["status"] == "general"));
    let accepted_defaults = specify_core::rules::with_all_defaults(&analysis, &answers).unwrap();
    let state = specify_core::tolerancing::State {
        auto: Some(
            accepted_defaults
                .keys()
                .filter(|q| !answers.contains_key(*q))
                .cloned()
                .collect(),
        ),
        answers: accepted_defaults,
        gdt: None,
    };
    let defaults = cache
        .scene_with_state(&loaded, Some(&analysis), &state)
        .unwrap();
    assert!(
        defaults["specify"]
            .as_object()
            .unwrap()
            .values()
            .any(|m| m["status"] == "default")
    );
    let mut changed = answers.clone();
    changed.insert("hole.function:holes:9".into(), json!("fit:H8"));
    let second = cache
        .scene(&loaded, Some(&analysis), Some(&changed))
        .unwrap();
    validate(&second);
    for collection in first["pmi"]["pmi"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(key, value)| value.is_array().then_some(key.as_str()))
    {
        let ids = |scene: &Value| {
            scene["pmi"]["pmi"][collection]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["id"].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ids(&second), "{collection}");
    }
    // Real writer agreement for fully accepted answers and file-only reopening.
    let accepted = answers.clone();
    let intent = api::preview(&analysis, &accepted).unwrap();
    let bytes = writer::write_bytes(&loaded, &intent, Some(&accepted))
        .unwrap()
        .0;
    let stored = pmi_scene::pmi_scene(&bytes, None).unwrap();
    validate(&stored);
    assert_eq!(
        stored["specify"]["planner:specified:dim:hole:9/bore.diameter"]["number"],
        metadata["planner:specified:dim:hole:9/bore.diameter"]["number"]
    );
    assert_eq!(
        stored["specify"]["planner:specified:dim:hole:9/bore.diameter"]["origin"],
        "requirement"
    );
    let general: Vec<_> = stored["specify"]
        .as_object()
        .unwrap()
        .values()
        .filter(|m| m["origin"] == "general_tolerance")
        .collect();
    assert!(!general.is_empty());
    assert!(general.iter().all(|m| m["status"] == "general"));
}
#[test]
fn scene_face_anchors_are_snapshot_mesh_indices_even_when_step_faces_have_names() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let scene = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&given()))
        .unwrap();
    let features = scene["pmi"]["pmi"]["features"].as_array().unwrap();
    let size = scene["pmi"]["pmi"]["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == "planner:specified:dim:hole:9/bore.diameter")
        .unwrap();
    let fid = &size["value"]["kind"]["size"]["feature"];
    let feature = features.iter().find(|f| f["id"] == *fid).unwrap();
    // The plate's hole is face 9 in analyse/mesh, regardless of its STEP entity number.
    assert_eq!(feature["value"], json!({"items":[{"face":9}]}));
    assert_eq!(
        api::mesh(&loaded)["faces"].as_array().unwrap().len(),
        loaded.definition.faces.len()
    );
}
#[test]
fn foreign_nist_ctc01_is_view_only_and_keeps_as_drawn_graphics() {
    use std::io::Read;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz");
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_end(&mut bytes)
        .unwrap();
    let scene = pmi_scene::pmi_scene(&bytes, Some(0)).unwrap();
    validate(&scene);
    assert!(
        scene["specify"]
            .as_object()
            .unwrap()
            .values()
            .all(|m| m["status"] == "existing")
    );
    let graphics: Vec<_> = scene["presentation"]["items"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|v| v.as_array().unwrap())
        .chain(scene["presentation"]["unlinked"].as_array().unwrap())
        .collect();
    assert!(!graphics.is_empty());
    assert!(graphics.iter().all(|g| g["source"] == "file"));
    assert!(
        graphics
            .iter()
            .any(|g| !g["polylines"].as_array().unwrap().is_empty())
    );
}
#[test]
fn python_schemas_match_the_pinned_kernel() {
    // Resolve the actual locked dependency without enabling json-import just to
    // access its schema constants. Production and test builds stay serialise-only.
    // Not --offline: metadata resolves every target's dependencies (the WASM crate's
    // too), which a native test build never downloads, so offline depended on the cache.
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cargo: Value = serde_json::from_slice(&output.stdout).unwrap();
    let kernel = cargo["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "haecceity")
        .unwrap();
    let node = cargo["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == kernel["id"])
        .unwrap();
    assert!(
        node["features"]
            .as_array()
            .unwrap()
            .contains(&json!("json"))
    );
    assert!(
        !node["features"]
            .as_array()
            .unwrap()
            .contains(&json!("json-import"))
    );
    let directory = Path::new(kernel["manifest_path"].as_str().unwrap())
        .parent()
        .unwrap();
    for name in ["pmi", "presentation"] {
        let file = format!("haecceity.{name}-1.json");
        let packaged = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/specify-core-py/python/specify_core/schema")
            .join(&file);
        assert_eq!(
            std::fs::read(packaged).unwrap(),
            std::fs::read(directory.join("schema").join(file)).unwrap()
        );
    }
}
#[test]
fn patterned_requirements_put_the_count_in_notation_and_first_cell() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let mut answers = given();
    let q = api::questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with("hole.function:hole_patterns"))
        .unwrap();
    answers.insert(q.id.clone(), json!("fit:H7"));
    let scene = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    let ids: BTreeSet<_> = scene["specify"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(id, m)| {
            m["question"] == q.id && scene["presentation"]["items"].get(*id).is_some()
        })
        .map(|(id, _)| id)
        .collect();
    assert!(!ids.is_empty());
    for id in ids {
        for graphic in scene["presentation"]["items"][id].as_array().unwrap() {
            assert!(
                graphic["name"].as_str().unwrap().starts_with("6×"),
                "{}",
                graphic["name"]
            );
            assert!(graphic["text"][0].as_str().unwrap().starts_with("6×"));
        }
    }
}

#[test]
fn generated_graphics_expose_model_space_placement_and_plane_ids() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = specify_core::rules::with_all_defaults(&analysis, &given()).unwrap();
    let scene = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&scene);
    assert!(
        !scene["presentation"]["planes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for rows in scene["presentation"]["items"].as_object().unwrap().values() {
        for graphic in rows.as_array().unwrap() {
            assert!(graphic["id"].is_string());
            assert!(graphic["plane_id"].is_string());
            let placements = graphic["text_placements"].as_array().unwrap();
            assert!(!placements.is_empty());
            assert!(
                placements
                    .iter()
                    .all(|placement| placement["stroked"] == true)
            );
        }
    }
    for datum in scene["pmi"]["pmi"]["datums"].as_array().unwrap() {
        assert!(
            !scene["presentation"]["items"][datum["id"].as_str().unwrap()]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn live_foreign_review_keeps_original_datum_ids_and_file_placement() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz");
    let loaded = part::load(&path, Some(0)).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let file = cache.scene(&loaded, None, None).unwrap();
    validate(&file);
    let review = cache
        .scene(&loaded, Some(&analysis), Some(&Map::new()))
        .unwrap();
    validate(&review);
    assert_eq!(file["pmi"]["pmi"]["datums"], review["pmi"]["pmi"]["datums"]);
    assert_eq!(
        file["presentation"]["views"],
        review["presentation"]["views"]
    );
    assert!(
        review["presentation"]["items"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|rows| rows.as_array().unwrap())
            .all(|g| g["source"] == "file")
    );
    assert!(!file["presentation"]["views"].as_array().unwrap().is_empty());
    assert!(
        !file["presentation"]["planes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        file["pmi"]["pmi"]["datums"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |datum| file["presentation"]["items"][datum["id"].as_str().unwrap()]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
            )
    );
}

#[test]
fn new_foreign_control_preserves_saved_views_planes_and_datum_callouts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz");
    let loaded = part::load(&path, Some(0)).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let file = cache.scene(&loaded, None, None).unwrap();
    let face = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "plane")
        .unwrap()["id"]
        .clone();
    let answers = json!({"extra.1":{"kind":"flatness", "faces":[face], "value":"0.05"}})
        .as_object()
        .unwrap()
        .clone();
    let review = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&review);
    assert_eq!(review["specify"]["flatness:extra.1"]["status"], "answered");
    assert_eq!(file["pmi"]["pmi"]["datums"], review["pmi"]["pmi"]["datums"]);
    // v0.4.1 adds a plane view for the new control alongside unchanged file views.
    let views = review["presentation"]["views"].as_array().unwrap();
    for view in file["presentation"]["views"].as_array().unwrap() {
        assert!(views.contains(view));
    }
    let generated: Vec<_> = views
        .iter()
        .filter(|view| view["id"].as_str().unwrap().starts_with("generated:"))
        .collect();
    assert_eq!(generated.len(), 1);
    assert_eq!(
        generated[0]["graphics"],
        json!(["generated:tolerances:flatness:extra.1"])
    );
    for plane in file["presentation"]["planes"].as_array().unwrap() {
        assert!(
            review["presentation"]["planes"]
                .as_array()
                .unwrap()
                .contains(plane)
        );
    }
    for datum in file["pmi"]["pmi"]["datums"].as_array().unwrap() {
        let id = datum["id"].as_str().unwrap();
        assert_eq!(
            file["presentation"]["items"][id],
            review["presentation"]["items"][id]
        );
        assert_eq!(review["specify"][id]["status"], "existing");
    }
}

#[test]
fn part_finish_keeps_its_characteristic_number_live_and_from_the_file() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let mut answers = given();
    answers.insert("part.surface_finish".into(), json!("Ra 3.2"));
    answers.insert("part.edges".into(), json!("Break sharp edges 0.2 max"));
    let live = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    let number = live["specify"]["part.surface_finish"]["number"]
        .as_u64()
        .expect("part finish is an inspection characteristic");
    assert!(number > 0);
    assert_eq!(live["specify"]["part.edges"]["question"], "part.edges");
    assert_eq!(live["specify"]["part.edges"]["status"], "answered");
    let intent = api::preview(&analysis, &answers).unwrap();
    let written = writer::write_bytes(&loaded, &intent, Some(&answers))
        .unwrap()
        .0;
    let file = pmi_scene::pmi_scene(&written, None).unwrap();
    validate(&file);
    assert_eq!(file["specify"]["part.surface_finish"]["number"], number);
    assert_eq!(
        file["specify"]["part.surface_finish"]["question"],
        "part.surface_finish"
    );
}

#[test]
fn every_part_setting_uses_the_rules_question_before_and_after_review_and_write() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = json!({
        "part.coating":"Zinc plated", "part.heat_treatment":"Harden to 40 HRC",
        "part.edges":"Break sharp edges 0.2 max", "part.surface_finish":"Ra 3.2",
        "part.general_tolerance":"ISO 2768-m", "part.notes":["Keep dry"]
    })
    .as_object()
    .unwrap()
    .clone();
    let cache = Scene::new(&loaded).unwrap();
    let state = specify_core::tolerancing::State {
        answers: answers.clone(),
        auto: Some(answers.keys().cloned().collect()),
        gdt: None,
    };
    let unreviewed = cache
        .scene_with_state(&loaded, Some(&analysis), &state)
        .unwrap();
    let answered = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    let written = writer::write_bytes(
        &loaded,
        &api::preview(&analysis, &answers).unwrap(),
        Some(&answers),
    )
    .unwrap()
    .0;
    let file = pmi_scene::pmi_scene(&written, None).unwrap();
    let questions = api::questions(&analysis, &Map::new()).unwrap();
    for (scene, status) in [
        (&unreviewed, "default"),
        (&answered, "answered"),
        (&file, "answered"),
    ] {
        validate(scene);
        for question in answers.keys() {
            assert!(questions.iter().any(|q| &q.id == question));
            let rows: Vec<_> = scene["specify"]
                .as_object()
                .unwrap()
                .iter()
                .filter(|(_, meta)| meta["question"] == *question)
                .collect();
            assert!(!rows.is_empty(), "{question}: {scene}");
            for (id, meta) in rows {
                assert!(id.starts_with(question), "{id}");
                assert_eq!(meta["status"], status, "{question}");
            }
        }
    }
}

#[test]
fn supporting_rows_have_no_interview_status_live_or_from_the_file() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = given();
    let live = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    let bytes = writer::write_bytes(
        &loaded,
        &api::preview(&analysis, &answers).unwrap(),
        Some(&answers),
    )
    .unwrap()
    .0;
    let file = pmi_scene::pmi_scene(&bytes, None).unwrap();
    for scene in [live, file] {
        for collection in ["features", "geometry"] {
            for row in scene["pmi"]["pmi"][collection].as_array().unwrap() {
                assert!(scene["specify"].get(row["id"].as_str().unwrap()).is_none());
            }
        }
        for rows in scene["pmi"]["units"].as_object().unwrap().values() {
            for row in rows.as_array().unwrap() {
                assert!(scene["specify"].get(row["id"].as_str().unwrap()).is_none());
            }
        }
        for row in scene["pmi"]["pmi"]["attributes"].as_array().unwrap() {
            if matches!(
                row["value"]["name"].as_str(),
                Some("specify characteristic numbers" | "pmi-assist answers")
            ) {
                assert!(scene["specify"].get(row["id"].as_str().unwrap()).is_none());
            }
        }
    }
}

#[test]
fn patterned_datums_have_no_count_but_sizes_and_tolerances_do() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let pattern = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == "hole_patterns")
        .unwrap();
    let question = format!("hole.function:{}", pattern["id"].as_str().unwrap());
    let mut answers = given();
    answers.insert(question.clone(), json!("fit:H7"));
    answers.insert("datum.A".into(), json!([pattern["faces"][0]]));
    answers.insert("part.locating".into(), json!("position tolerances"));
    answers.insert(
        format!("hole.position:{}", pattern["id"].as_str().unwrap()),
        json!("0.05"),
    );
    let defaults = specify_core::rules::with_all_defaults(&analysis, &answers).unwrap();
    for (id, value) in defaults {
        if id.starts_with("datum.") || id.starts_with("hole.frame:") {
            answers.entry(id).or_insert(value);
        }
    }
    let scene = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap_or_else(|e| {
            panic!(
                "{e}\n{}",
                api::preview(&analysis, &answers).unwrap().to_dict()
            )
        });
    let datum = scene["presentation"]["items"]["datum:A"]
        .as_array()
        .unwrap();
    assert!(!datum.is_empty());
    for graphic in datum {
        assert!(!graphic["name"].as_str().unwrap().contains("×"));
        assert!(
            graphic["text"]
                .as_array()
                .unwrap()
                .iter()
                .all(|cell| !cell.as_str().unwrap().contains("×"))
        );
    }
    // The selected datum member leaves the group; both size and FCF cover five holes (#75).
    for collection in ["dimensions", "tolerances"] {
        let expected_question = if collection == "dimensions" {
            question.clone()
        } else {
            format!("hole.position:{}", pattern["id"].as_str().unwrap())
        };
        let graphics: Vec<_> = scene["pmi"]["pmi"][collection]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                scene["specify"][row["id"].as_str().unwrap()]["question"] == expected_question
            })
            .flat_map(|row| {
                scene["presentation"]["items"][row["id"].as_str().unwrap()]
                    .as_array()
                    .unwrap()
            })
            .collect();
        assert!(!graphics.is_empty(), "{collection}");
        for graphic in graphics {
            assert!(
                graphic["name"].as_str().unwrap().starts_with("5×"),
                "{}",
                graphic["name"]
            );
            assert!(graphic["text"][0].as_str().unwrap().starts_with("5×"));
        }
    }
}

#[test]
fn foreign_numeric_ids_are_not_characteristic_numbers_without_specify_history() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let face = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "plane")
        .unwrap()["id"]
        .clone();
    let mut answers = given();
    answers.insert("datum.A".into(), json!([face]));
    // No saved interview: the history alone still identifies Specify's numbers.
    let bytes = writer::write_bytes(&loaded, &api::preview(&analysis, &answers).unwrap(), None)
        .unwrap()
        .0;
    let numbered = pmi_scene::pmi_scene(&bytes, None).unwrap();
    for collection in ["datums", "dimensions"] {
        let rows = numbered["pmi"]["pmi"][collection].as_array().unwrap();
        assert!(!rows.is_empty());
        for row in rows {
            let id = row["id"].as_str().unwrap();
            assert_eq!(
                numbered["specify"][id]["number"],
                id.parse::<u64>().unwrap()
            );
        }
    }
    // Model a foreign producer's numeric ids with no Specify history or origin.
    let foreign = String::from_utf8(bytes)
        .unwrap()
        .replace(
            specify_core::characteristics::PROPERTY,
            "producer identity records",
        )
        .replace(
            specify_core::pmi_origin::PROPERTY,
            "producer dimension records",
        )
        .into_bytes();
    let loaded = part::load_bytes("foreign.step", foreign.clone()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let added = json!({"extra.1":{"kind":"flatness", "faces":[face], "value":"0.05"}})
        .as_object()
        .unwrap()
        .clone();
    let live = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&added))
        .unwrap();
    let file = pmi_scene::pmi_scene(&foreign, None).unwrap();
    for collection in ["datums", "dimensions"] {
        for row in numbered["pmi"]["pmi"][collection].as_array().unwrap() {
            let id = row["id"].as_str().unwrap();
            for scene in [&file, &live] {
                assert!(
                    scene["pmi"]["pmi"][collection]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|r| r["id"] == id)
                );
                assert_eq!(scene["specify"][id]["status"], "existing");
                assert!(scene["specify"][id].get("number").is_none(), "{id}");
            }
        }
    }
}

#[test]
fn feature_owned_manufacturing_attributes_inherit_the_thread_or_knurl_question() {
    use haecceity::pmi::{AttributeSet, AttributeValue, FeatureId, NoteOwner};
    let loaded = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/pin.step"),
        None,
    )
    .unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let question = api::questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| {
            q.id.starts_with("diameter.fit:") && q.options.iter().any(|v| v.starts_with("thread:"))
        })
        .unwrap();
    let thread = question
        .options
        .iter()
        .find(|v| v.starts_with("thread:"))
        .unwrap();
    for (collection, answer, name) in [
        ("threads", thread.as_str(), "thread manufacturing"),
        ("knurls", "knurl:straight", "knurl manufacturing"),
    ] {
        let answers = json!({question.id.clone():answer})
            .as_object()
            .unwrap()
            .clone();
        let cache = Scene::new(&loaded).unwrap();
        let live = cache
            .scene(&loaded, Some(&analysis), Some(&answers))
            .unwrap();
        let primary = &live["pmi"]["pmi"][collection][0];
        let owner = &primary["value"]["feature"];
        let feature = live["pmi"]["pmi"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .position(|row| row["id"] == *owner)
            .unwrap();
        let extra = AttributeSet {
            name: name.into(),
            description: None,
            general_property: None,
            on: Some(NoteOwner::Feature(FeatureId(feature))),
            items: vec![("process".into(), AttributeValue::Text("Controlled".into()))],
        };
        let bytes = writer::write_bytes_with(
            &loaded,
            &api::preview(&analysis, &answers).unwrap(),
            Some(&answers),
            &[extra],
        )
        .unwrap()
        .0;
        let file = pmi_scene::pmi_scene(&bytes, None).unwrap();
        let default = cache
            .scene_with_state(
                &loaded,
                Some(&analysis),
                &specify_core::tolerancing::State {
                    answers: answers.clone(),
                    auto: Some(vec![question.id.clone()]),
                    gdt: None,
                },
            )
            .unwrap();
        for (scene, status) in [
            (&live, "answered"),
            (&default, "default"),
            (&file, "answered"),
        ] {
            let attributes: Vec<_> = scene["pmi"]["pmi"]["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["value"]["name"] == name)
                .collect();
            // Live knurl parameters are typed semantics; this producer's extra UDA
            // exists only in the written file. Threads also author a live UDA.
            if collection == "threads" || std::ptr::eq(scene, &file) {
                assert!(!attributes.is_empty());
            }
            for attribute in attributes {
                let meta = &scene["specify"][attribute["id"].as_str().unwrap()];
                assert_eq!(meta["question"], question.id);
                assert_eq!(meta["status"], status);
            }
        }
    }
}

#[test]
fn a_pattern_and_its_datum_member_keep_distinct_location_ids_and_numbers() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let pattern = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == "hole_patterns")
        .unwrap();
    let supplied = json!({
        "datum.A":[pattern["faces"][0]], "part.locating":"position tolerances",
        format!("hole.function:{}", pattern["id"].as_str().unwrap()):"fit:H7"
    })
    .as_object()
    .unwrap()
    .clone();
    let answers = specify_core::rules::with_all_defaults(&analysis, &supplied).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let live = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&live);
    let mut altered = answers.clone();
    altered.insert(
        format!("hole.function:{}", pattern["id"].as_str().unwrap()),
        json!("fit:H8"),
    );
    let changed = cache
        .scene(&loaded, Some(&analysis), Some(&altered))
        .unwrap();
    let ids = |s: &Value| {
        s["pmi"]["pmi"]["dimensions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&live), ids(&changed));
    let bytes = writer::write_bytes(
        &loaded,
        &api::preview(&analysis, &answers).unwrap(),
        Some(&answers),
    )
    .unwrap()
    .0;
    let written = part::load_bytes("pattern.step", bytes.clone()).unwrap();
    let characteristics = specify_core::characteristics::read(&written).unwrap();
    let file = pmi_scene::pmi_scene(&bytes, None).unwrap();
    validate(&file);
    assert_eq!(ids(&live), ids(&file));
    for scene in [&live, &file] {
        let numbers: Vec<_> = scene["pmi"]["pmi"]["dimensions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                scene["specify"][row["id"].as_str().unwrap()]["number"]
                    .as_u64()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            numbers.len(),
            numbers.iter().copied().collect::<BTreeSet<_>>().len()
        );
        assert_eq!(
            numbers.iter().copied().collect::<BTreeSet<_>>(),
            characteristics
                .iter()
                .filter(|c| c.kind == "dimension")
                .map(|c| c.number)
                .collect()
        );
    }
}

#[test]
fn shared_general_notes_have_individual_semantic_labels_live_and_from_the_file() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = json!({
        "part.general_tolerance":"ISO 2768-m", "part.surface_finish":"Ra 3.2",
        "part.edges":"Break sharp edges 0.2 max", "part.coating":"Zinc plated",
        "part.heat_treatment":"Harden to 40 HRC", "part.notes":["Keep dry", "Do not stack"]
    })
    .as_object()
    .unwrap()
    .clone();
    let live = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    let bytes = writer::write_bytes(
        &loaded,
        &api::preview(&analysis, &answers).unwrap(),
        Some(&answers),
    )
    .unwrap()
    .0;
    let file = pmi_scene::pmi_scene(&bytes, None).unwrap();
    for scene in [live, file] {
        validate(&scene);
        let graphic_id = &scene["presentation"]["items"]["part.notes:1"][0]["id"];
        assert!(graphic_id.is_string());
        for (id, text) in [
            ("part.general_tolerance", "GENERAL TOLERANCES ISO 2768-m"),
            (
                "part.surface_finish",
                "SURFACE FINISH Ra 3.2 UNLESS OTHERWISE STATED",
            ),
            ("part.edges", "Break sharp edges 0.2 max"),
            ("part.coating", "Zinc plated"),
            ("part.heat_treatment", "Harden to 40 HRC"),
            ("part.notes:1", "Keep dry"),
            ("part.notes:2", "Do not stack"),
        ] {
            assert_eq!(scene["specify"][id]["notation"], text, "{id}");
            // Preserve the upstream graphic as a single block with shared identity.
            assert_eq!(
                &scene["presentation"]["items"][id][0]["id"], graphic_id,
                "{id}"
            );
        }
    }
}

fn notes_blocks(scene: &Value) -> std::collections::BTreeMap<String, Value> {
    scene["presentation"]["items"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .chain(scene["presentation"]["unlinked"].as_array().unwrap())
        .filter(|g| {
            g["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("general notes"))
        })
        .map(|g| (g["id"].as_str().unwrap().to_owned(), g.clone()))
        .collect()
}

#[test]
fn xcaf_plate_combines_imported_and_accepted_notes_without_another_edge_requirement() {
    let loaded = part::load(
        &common::fixtures().join("revisions/xcaf_pmi_plate.step.gz"),
        None,
    )
    .unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let edge = "Remove all burrs and break sharp edges";
    let mask = "Mask these features before coating with primer";
    assert_eq!(analysis["part"]["edges"], edge);
    assert!(
        !api::questions(&analysis, &Map::new())
            .unwrap()
            .iter()
            .any(|q| q.id == "part.edges")
    );
    let answers = specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap();
    assert!(!answers.contains_key("part.edges"));
    let cache = Scene::new(&loaded).unwrap();
    let file = cache.scene(&loaded, Some(&analysis), None).unwrap();
    validate(&file);
    let file_block = &file["presentation"]["items"]["part.material"][0];
    assert_eq!(file_block["id"], "step:726");
    assert_eq!(file_block["source"], "file");
    assert!(
        file_block["step_ids"]
            .as_array()
            .unwrap()
            .contains(&json!(725))
    );
    assert!(
        !file["presentation"]["unlinked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "step:726")
    );
    for state in [
        specify_core::tolerancing::State {
            answers: answers.clone(),
            auto: Some(answers.keys().cloned().collect()),
            gdt: None,
        },
        specify_core::tolerancing::State {
            answers: answers.clone(),
            auto: Some(vec![]),
            gdt: None,
        },
    ] {
        let scene = cache
            .scene_with_state(&loaded, Some(&analysis), &state)
            .unwrap();
        validate(&scene);
        let blocks = notes_blocks(&scene);
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        let block = blocks.values().next().unwrap();
        assert_eq!(block["id"], "generated:general-notes");
        let text = block["text"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("GENERAL TOLERANCES ISO 2768-m"), "{text}");
        assert!(text.contains("SURFACE FINISH Ra 3.2"), "{text}");
        assert!(text.contains(edge), "{text}");
        assert!(text.contains("MATERIAL: Alu_G6061_T6"), "{text}");
        assert!(!text.contains("Break sharp edges 0.2 max"));
        assert!(text.find("TOLERANCES").unwrap() < text.find("SURFACE FINISH").unwrap());
        assert!(text.find("SURFACE FINISH").unwrap() < text.find(edge).unwrap());
        assert!(text.find(edge).unwrap() < text.find("MATERIAL:").unwrap());
        assert_eq!(
            scene["presentation"]["items"]["part.material"][0]["id"],
            block["id"]
        );
        let notes = scene["pmi"]["pmi"]["notes"].as_array().unwrap();
        let edge_row = notes.iter().find(|r| r["value"]["text"] == edge).unwrap();
        let edge_id = edge_row["id"].as_str().unwrap();
        assert_eq!(scene["specify"][edge_id]["status"], "existing");
        assert_eq!(
            scene["presentation"]["items"][edge_id][0]["id"],
            block["id"]
        );
        let feature_note = notes.iter().find(|r| r["value"]["text"] == mask).unwrap();
        assert!(!feature_note["value"]["on"].is_null());
        let graphic = &scene["presentation"]["items"][feature_note["id"].as_str().unwrap()][0];
        assert!(
            graphic["text"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t.as_str().unwrap().contains(mask))
        );
        assert!(
            !scene["presentation"]["items"]
                .as_object()
                .unwrap()
                .values()
                .filter_map(Value::as_array)
                .flatten()
                .any(|g| g["id"] == "step:726")
        );
    }
    // Scene projection must not mutate the source bytes or change a subsequent write.
    let intent = api::preview(&analysis, &answers).unwrap();
    let first = writer::write_bytes(&loaded, &intent, Some(&answers))
        .unwrap()
        .0;
    cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    assert_eq!(
        first,
        writer::write_bytes(&loaded, &intent, Some(&answers))
            .unwrap()
            .0
    );
    let before = haecceity::p21::Document::parse(loaded.bytes.to_vec()).unwrap();
    let after = haecceity::p21::Document::parse(first).unwrap();
    for id in before.ids() {
        assert_eq!(
            before.bytes()[before.span(id).unwrap()],
            after.bytes()[after.span(id).unwrap()],
            "#{id}"
        );
    }
}

#[test]
fn spindle_ctc_and_both_plates_have_one_live_notes_block() {
    for name in [
        "revisions/spindle-q137.step.gz",
        "nist/nist_ctc_01_asme1_ap242.stp.gz",
        "revisions/presentation_plate.step.gz",
        "revisions/cadgen_notes_plate.step.gz",
    ] {
        let loaded = part::load(&common::fixtures().join(name), None).unwrap();
        let analysis = api::analyse(&loaded).unwrap();
        let answers = specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap();
        let scene = Scene::new(&loaded)
            .unwrap()
            .scene_with_state(
                &loaded,
                Some(&analysis),
                &specify_core::tolerancing::State {
                    auto: Some(answers.keys().cloned().collect()),
                    answers,
                    gdt: None,
                },
            )
            .unwrap();
        validate(&scene);
        assert_eq!(notes_blocks(&scene).len(), 1, "{name}");
    }
}
