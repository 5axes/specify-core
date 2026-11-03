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
    let listed: BTreeSet<_> = scene["presentation"]["semantic_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    assert_eq!(ids, listed);
    // `part.material` aliases the scalar material in `items` and `specify` only.
    if !scene["pmi"]["pmi"]["material"].is_null() {
        ids.insert("part.material");
    }
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
        if scene["specify"][id].get("notation").is_some() {
            assert!(
                scene["specify"][id]["notation"]
                    .as_str()
                    .unwrap()
                    .starts_with("6×")
            );
        }
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
                assert!(
                    meta["notation"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty() && s != "note"),
                    "{meta}"
                );
                if collection == "knurls" {
                    assert_eq!(meta["notation"], "process: Controlled");
                }
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
        assert_notes_clear(&scene);
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
    assert_eq!(
        file,
        cache.scene(&loaded, Some(&analysis), None).unwrap(),
        "the combined scene must not replace the cached file's step:726 ink"
    );
    assert_eq!(file, pmi_scene::pmi_scene(&loaded.bytes, None).unwrap());
    // Scene projection must not mutate the source bytes or change a subsequent write.
    let intent = api::preview(&analysis, &answers).unwrap();
    let (first, report) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let retained: Vec<_> = report
        .preserved_normalizations
        .iter()
        .filter(|r| r.normalization == "legacy-endpoint-retirement")
        .map(|r| r.id)
        .collect();
    assert_eq!(
        retained,
        [490, 508, 515],
        "the kernel reports each retained legacy wrapper"
    );
    assert_eq!(
        report.to_dict()["preserved_normalizations"],
        serde_json::to_value(&report.preserved_normalizations).unwrap()
    );
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
    let reopened = pmi_scene::pmi_scene(after.bytes(), None).unwrap();
    validate(&reopened);
    let blocks = notes_blocks(&reopened);
    assert_eq!(
        blocks.len(),
        1,
        "append must not redraw both physical blocks"
    );
    let block = blocks.values().next().unwrap();
    let text = block["text"].to_string();
    assert!(text.contains(edge));
    assert!(text.contains("MATERIAL: Alu_G6061_T6"));
    assert!(!text.contains("Break sharp edges 0.2 max"));
    assert_eq!(reopened["specify"]["part.material"]["status"], "existing");
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

// #171's ink-box check projects into the callout plane, rather than model Z.
fn assert_notes_clear(scene: &Value) -> f64 {
    use haecceity::geom;
    let blocks = notes_blocks(scene);
    assert_eq!(blocks.len(), 1);
    let block = blocks.values().next().unwrap();
    let note = scene["pmi"]["pmi"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["value"]["text"] == "Mask these features before coating with primer")
        .unwrap();
    let feature = &scene["presentation"]["items"][note["id"].as_str().unwrap()][0];
    let plane = |g: &Value| {
        scene["presentation"]["planes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == g["plane_id"])
            .unwrap()
    };
    let vector = |p: &Value| std::array::from_fn(|i| p[i].as_f64().unwrap());
    let normal = vector(&plane(block)["normal"]);
    let along = vector(&plane(block)["x_axis"]);
    let up = geom::cross(normal, along);
    // The regression deliberately uses a Y-normal notes plane, not a Z-normal one.
    assert!(geom::dot(normal, [0., 0., 1.]).abs() < 1e-9);
    assert!(geom::dot(normal, vector(&plane(feature)["normal"])).abs() > 1. - 1e-8);
    let bounds = |g: &Value| {
        let mut b = geom::Bounds::empty();
        for point in g["polylines"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|l| l.as_array().unwrap())
        {
            let p = vector(point);
            b.add([geom::dot(p, along), geom::dot(p, up), 0.]);
        }
        b
    };
    let a = bounds(block);
    let b = bounds(feature);
    let gap = (0..2)
        .map(|k| (a.min[k] - b.max[k]).max(b.min[k] - a.max[k]))
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        gap >= -2e-6,
        "#171: notes ink overlaps in its plane: {a:?} / {b:?}"
    );
    let required = 2. * feature["text_placements"][0]["height"].as_f64().unwrap();
    let vertical_gap = a.min[1] - b.max[1];
    assert!(
        vertical_gap >= required - 2e-6,
        "notes gap {vertical_gap} < {required}"
    );
    println!("notes plane {normal:?}: clearance={vertical_gap:.6} mm, required={required:.6} mm");
    vertical_gap / required
}

#[test]
fn rotated_xcaf_plate_notes_are_placed_together_in_their_plane() {
    let original = part::load(
        &common::fixtures().join("revisions/xcaf_pmi_plate.step.gz"),
        None,
    )
    .unwrap();
    // Rigidly turn the STEP geometry 90 degrees about X, including its original ink.
    // This is input transformation only; Specify never edits scene coordinates.
    let rotation =
        regex::Regex::new(r"((?:CARTESIAN_POINT|DIRECTION)\('[^']*',\()([^()]+)(\)\))").unwrap();
    let text = std::str::from_utf8(&original.bytes).unwrap();
    let turned = rotation.replace_all(text, |c: &regex::Captures<'_>| {
        let xyz: Vec<f64> = c[2].split(',').map(|v| v.trim().parse().unwrap()).collect();
        if xyz.len() == 3 {
            format!("{}{:?},{:?},{:?}{}", &c[1], xyz[0], -xyz[2], xyz[1], &c[3])
        } else {
            c[0].to_owned()
        }
    });
    assert_ne!(turned, text);
    let rotated = part::load_bytes("rotated-xcaf.step", turned.as_bytes().to_vec()).unwrap();
    let mut margins = vec![];
    for loaded in [&original, &rotated] {
        let analysis = api::analyse(loaded).unwrap();
        let answers = specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap();
        let scene = Scene::new(loaded)
            .unwrap()
            .scene_with_state(
                loaded,
                Some(&analysis),
                &specify_core::tolerancing::State {
                    auto: Some(answers.keys().cloned().collect()),
                    answers,
                    gdt: None,
                },
            )
            .unwrap();
        validate(&scene);
        margins.push(assert_notes_clear(&scene));
    }
    assert!(
        margins[1] >= margins[0] - 1e-6,
        "rotated margin {margins:?}"
    );
}

#[test]
fn an_exact_tap_drill_default_writes_thread_semantics_instead_of_a_clearance_fit() {
    let loaded = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/GRM-01_shank.step.gz"),
        None,
    )
    .unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let defaults = specify_core::rules::with_all_defaults(&analysis, &Map::new()).unwrap();
    assert_eq!(defaults["hole.function:holes:7"], "tapped:M3");
    let scene = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&defaults))
        .unwrap();
    let threads = scene["pmi"]["pmi"]["threads"].as_array().unwrap();
    let thread = threads
        .iter()
        .find(|r| r["id"] == "thread:holes:7")
        .unwrap();
    let feature = &thread["value"]["feature"];
    assert_eq!(thread["value"]["qualifier"], "M3x0.5");
    assert!(
        !scene["pmi"]["pmi"]["dimensions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["value"]["kind"]["size"]["feature"] == *feature)
    );
    assert!(
        !scene["presentation"]["items"]
            .as_object()
            .unwrap()
            .contains_key("size:holes:7")
    );
    for (id, text) in [
        ("thread:holes:7", "M3×0.5-6H THRU"),
        ("thread:holes:20.25", "M6×1-6H THRU"),
    ] {
        let g = &scene["presentation"]["items"][id][0];
        assert_eq!(g["text"][0], text);
        assert_eq!(
            g["polyline_roles"].as_array().unwrap().len(),
            g["polylines"].as_array().unwrap().len()
        );
        assert!(g["text_placements"][0]["cell_box"].is_array());
    }
    assert_eq!(notes_blocks(&scene).len(), 1);
    assert!(!scene["presentation"].to_string().contains("⌀2.5 H11"));
    // An explicit clearance decision is still honoured.
    let mut clearance = defaults;
    clearance.insert("hole.function:holes:7".into(), json!("clearance:M2"));
    let explicit = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&clearance))
        .unwrap();
    assert!(
        !explicit["pmi"]["pmi"]["threads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "thread:holes:7")
    );
}

// The editor's `linksFromPmi` rule: ids of the array collections under `pmi.pmi` and
// `pmi.units` must equal `semantic_ids`. Material is an object, so neither its own id
// nor the `part.material` alias counts.
fn editor_ids(scene: &Value) -> BTreeSet<String> {
    ["pmi", "units"]
        .iter()
        .flat_map(|key| scene["pmi"][key].as_object().unwrap().values())
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|row| row["id"].as_str().map(str::to_owned))
        .collect()
}
#[test]
fn answered_material_keeps_semantic_ids_to_the_pmi_ids() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let mut answers = given();
    answers.insert("part.material".into(), json!("steel"));
    let live = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    let intent = api::preview(&analysis, &answers).unwrap();
    let bytes = writer::write_bytes(&loaded, &intent, Some(&answers))
        .unwrap()
        .0;
    let stored = pmi_scene::pmi_scene(&bytes, None).unwrap();
    for scene in [&live, &stored] {
        assert!(scene["pmi"]["pmi"]["material"].is_object());
        let listed: BTreeSet<String> = scene["presentation"]["semantic_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(editor_ids(scene), listed);
        assert!(!listed.contains("part.material"));
        assert!(!scene["presentation"]["items"]["part.material"].is_null());
        assert_eq!(
            scene["specify"]["part.material"]["notation"],
            "MATERIAL: steel"
        );
    }
}

#[test]
fn diff_scenes_keep_editor_ids_and_old_ghost_links() {
    use specify_core::{revision, rules};
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/revisions");
    for case in ["spindle_pmi", "hole_removed"] {
        let old = part::load(&fixtures.join(format!("{case}.old.step.gz")), None).unwrap();
        let new = part::load(&fixtures.join(format!("{case}.new.step.gz")), None).unwrap();
        let answers = rules::with_all_defaults(
            &api::analyse(&old).unwrap(),
            &json!({"part.material":"steel"})
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap();
        // Exercise both the rebuilt answer scene and the file-only scene.
        for given in [&answers, &Map::new()] {
            let report = revision::diff_with_options(
                &old,
                &new,
                given,
                revision::DiffOptions { scenes: true },
            )
            .unwrap();
            let scenes = report.pmi.scenes.as_ref().unwrap();
            for scene in [&scenes.old, &scenes.new] {
                validate(scene);
                let listed = scene["presentation"]["semantic_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_str().unwrap().to_owned())
                    .collect();
                assert_eq!(editor_ids(scene), listed, "{case}");
            }
            let mut ghosts = 0;
            for change in &report.pmi.items {
                if change.old.is_none() {
                    continue;
                }
                let id = change.old_id.as_deref().unwrap_or(&change.id);
                assert!(editor_ids(&scenes.old).contains(id), "{case}: {id}");
                assert!(scenes.old["specify"][id].is_object(), "{case}: {id}");
                // Semantic-only requirements need no ink. Every drawn old
                // requirement must keep its callout and links to old mesh faces.
                if let Some(items) = scenes.old["presentation"]["items"][id].as_array() {
                    assert!(!items.is_empty(), "{case}: {id}");
                    for face in &change.faces.old {
                        assert!(face.face < old.definition.faces.len(), "{case}: {id}");
                    }
                    ghosts += 1;
                }
            }
            // Bare hole_removed has no file PMI when no answers are supplied.
            if case == "spindle_pmi" || !given.is_empty() {
                assert!(ghosts > 0, "{case}: no old callouts checked");
            }
        }
    }
}

#[test]
fn mixed_resume_preview_keeps_foreign_control_and_replaces_owned_control() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let face = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "plane")
        .unwrap()["id"]
        .clone();
    let mut answers = json!({"extra.1":{"kind":"flatness","faces":[face],"value":"0.011"}})
        .as_object()
        .unwrap()
        .clone();
    let intent = api::preview(&analysis, &answers).unwrap();
    let bytes = writer::write_bytes(&loaded, &intent, None).unwrap().0;
    let bytes = String::from_utf8(bytes)
        .unwrap()
        .replace(writer::PMI_PROVENANCE, "independent producer")
        .into_bytes();
    let foreign = part::load_bytes("foreign.step", bytes).unwrap();
    answers["extra.1"]["value"] = json!("0.031");
    let intent = api::preview(&api::analyse(&foreign).unwrap(), &answers).unwrap();
    let bytes = writer::write_bytes(&foreign, &intent, Some(&answers))
        .unwrap()
        .0;
    let loaded = part::load_bytes("mixed.step", bytes).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let before = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&before);
    answers["extra.1"]["value"] = json!("0.047");
    let scene = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&scene);
    let tolerances = &scene["pmi"]["pmi"]["tolerances"];
    assert_eq!(tolerances.as_array().unwrap().len(), 2);
    assert!(tolerances.to_string().contains("0.011"));
    assert!(tolerances.to_string().contains("0.047"));
    assert!(!tolerances.to_string().contains("0.031"));
    assert_eq!(
        scene["specify"]["flatness:extra.1"]["number"],
        before["specify"]["flatness:extra.1"]["number"]
    );
    assert_eq!(scene["specify"]["flatness:extra.1"]["status"], "answered");
}
