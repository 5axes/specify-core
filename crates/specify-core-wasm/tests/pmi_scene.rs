use serde_json::{Map, Value, json};
use specify_core_wasm::session::Session;

#[test]
fn open_part_scene_faces_use_the_same_indices_and_frame_as_snapshot() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/parts/assembly.step"),
    )
    .unwrap();
    let parts = Session::parts("assembly.step", &bytes).unwrap();
    for row in parts.as_array().unwrap() {
        let part = row["part"].as_u64().unwrap() as usize;
        let engine = Session::open_part("assembly.step", &bytes, part).unwrap();
        let snapshot = engine.snapshot();
        let face = snapshot["analysis"]["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["kind"] == "plane")
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let answers = json!({"extra.1":{"kind":"flatness", "faces":[face], "value":"0.05"}})
            .as_object()
            .unwrap()
            .clone();
        let scene = engine.pmi_scene(&answers).unwrap();
        assert_eq!(scene["specify"]["flatness:extra.1"]["question"], "extra.1");
        let control = &scene["pmi"]["pmi"]["tolerances"][0]["value"]["target"]["feature"];
        let linked = scene["pmi"]["pmi"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == *control)
            .unwrap();
        assert_eq!(linked["value"]["items"], json!([{"face": face}]));
        let count = snapshot["mesh"]["faces"].as_array().unwrap().len();
        for feature in scene["pmi"]["pmi"]["features"].as_array().unwrap() {
            if let Some(items) = feature["value"]["items"].as_array() {
                for anchor in items {
                    if let Some(face) = anchor.get("face") {
                        assert!(face.as_u64().unwrap() < count as u64);
                        assert_eq!(
                            snapshot["analysis"]["faces"][face.as_u64().unwrap() as usize]["id"],
                            json!(face)
                        );
                    }
                }
            }
        }
    }
}

fn point(value: &Value) -> [f64; 3] {
    std::array::from_fn(|i| value[i].as_f64().unwrap())
}

// #75 represents shared sizes by real pattern/multiple-element groups. Resolve
// every member and retain the same per-face radius, mesh and leader checks.
fn feature_faces(features: &[Value], id: &Value) -> Vec<usize> {
    let feature = features.iter().find(|f| f["id"] == *id).unwrap();
    if let Some(items) = feature["value"]["items"].as_array() {
        items
            .iter()
            .map(|a| a["face"].as_u64().unwrap() as usize)
            .collect()
    } else {
        feature["value"]["group"]["members"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|id| feature_faces(features, id))
            .collect()
    }
}

// Verify actual recognised diameter items, without supplying any face picks. Each
// item's referenced mesh faces must have its nominal radius and its callout leader
// must reach their bounds in the part's own frame.
fn check_diameters(scene: &Value, snapshot: &Value) -> usize {
    let mesh = &snapshot["mesh"];
    let positions = mesh["positions"].as_array().unwrap();
    let indices = mesh["indices"].as_array().unwrap();
    // A tessellated cylinder can miss the exact extremum between two vertices.
    let low = point(&mesh["bbox"][0]);
    let high = point(&mesh["bbox"][1]);
    let tolerance = (0..3)
        .map(|i| (high[i] - low[i]).powi(2))
        .sum::<f64>()
        .sqrt()
        * specify_core::mesh::DEFLECTION
        + 0.0002;
    let features = scene["pmi"]["pmi"]["features"].as_array().unwrap();
    let mut checked = 0;
    for row in scene["pmi"]["pmi"]["dimensions"].as_array().unwrap() {
        let size = &row["value"]["kind"]["size"];
        if size["kind"] != "diameter" {
            continue;
        }
        let nominal = row["value"]["nominal"]["quantity"]["length"]["value"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let mut bounds = Vec::new();
        for face in feature_faces(features, &size["feature"]) {
            let analysis = &snapshot["analysis"]["faces"][face];
            assert_eq!(analysis["id"], face);
            assert_eq!(analysis["kind"], "cylinder");
            assert!((2. * analysis["radius"].as_f64().unwrap() - nominal).abs() < 1e-6);
            let origin = point(&analysis["origin"]);
            let axis = point(&analysis["direction"]);
            let range = &mesh["faces"][face];
            let start = range[0].as_u64().unwrap() as usize;
            let count = range[1].as_u64().unwrap() as usize;
            assert!(count > 0);
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for index in &indices[start * 3..(start + count) * 3] {
                let vertex = index.as_u64().unwrap() as usize * 3;
                let p: [f64; 3] = std::array::from_fn(|i| positions[vertex + i].as_f64().unwrap());
                let offset: [f64; 3] = std::array::from_fn(|i| p[i] - origin[i]);
                let axial: f64 = (0..3).map(|i| offset[i] * axis[i]).sum();
                let radius = (0..3)
                    .map(|i| (offset[i] - axial * axis[i]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                assert!(
                    (2. * radius - nominal).abs() < 0.001,
                    "face {face} has the wrong diameter/frame"
                );
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
            bounds.push((min, max));
            checked += 1;
        }
        let graphics = scene["presentation"]["items"][row["id"].as_str().unwrap()]
            .as_array()
            .unwrap();
        let min: [f64; 3] = std::array::from_fn(|i| {
            bounds
                .iter()
                .map(|(min, _)| min[i])
                .fold(f64::INFINITY, f64::min)
        });
        let max: [f64; 3] = std::array::from_fn(|i| {
            bounds
                .iter()
                .map(|(_, max)| max[i])
                .fold(f64::NEG_INFINITY, f64::max)
        });
        assert!(
            graphics
                .iter()
                .flat_map(|g| g["polylines"].as_array().unwrap())
                .flat_map(|line| line.as_array().unwrap())
                .any(|p| {
                    let p = point(p);
                    (0..3).all(|i| p[i] >= min[i] - tolerance && p[i] <= max[i] + tolerance)
                }),
            "no leader reaches item {} (nominal {nominal}) face bounds {min:?}..{max:?}; part bounds {}; graphics {:?}",
            row["id"],
            mesh["bbox"],
            graphics
                .iter()
                .map(|g| (&g["source"], g["polylines"].as_array().unwrap().last()))
                .collect::<Vec<_>>()
        );
    }
    checked
}

#[test]
fn assembly_live_and_view_only_diameters_match_actual_mesh_faces_and_frame() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/parts/assembly.step"),
    )
    .unwrap();
    for part in [0, 1] {
        let engine = Session::open_part("assembly.step", &bytes, part).unwrap();
        let snapshot = engine.snapshot();
        let analysis = &snapshot["analysis"];
        let mut answers = specify_core::rules::with_all_defaults(analysis, &Map::new()).unwrap();
        let question = specify_core::api::questions(analysis, &answers)
            .unwrap()
            .into_iter()
            .find(|q| q.id.starts_with("hole.function:") || q.id.starts_with("diameter.fit:"))
            .unwrap();
        answers.insert(
            question.id.clone(),
            json!(if question.id.starts_with("hole.") {
                "fit:H7"
            } else {
                "fit:h6"
            }),
        );
        let live = engine.pmi_scene(&answers).unwrap();
        assert!(check_diameters(&live, &snapshot) > 0);
        let output = engine.write_pmi(&answers, None, None).unwrap();
        let written: Vec<u8> = serde_json::from_value(output["bytes"].clone()).unwrap();
        let reopened = Session::open_part("written.step", &written, part).unwrap();
        let after = reopened.snapshot();
        assert_eq!(snapshot["mesh"]["positions"], after["mesh"]["positions"]);
        assert_eq!(snapshot["mesh"]["faces"], after["mesh"]["faces"]);
        let view = specify_core::pmi_scene::pmi_scene(&written, Some(part)).unwrap();
        assert!(check_diameters(&view, &after) > 0);
        assert!(check_diameters(&reopened.pmi_scene(&answers).unwrap(), &after) > 0);
    }
}
