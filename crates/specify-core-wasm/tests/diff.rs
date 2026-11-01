//! The browser's diff: two revisions in memory, compared as the command line compares files.
use serde_json::Map;
use specify_core::{part, revision};
use specify_core_wasm::session;
use std::path::Path;

fn bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/revisions/{name}.step.gz"));
    part::File::read(&path).unwrap().bytes().to_vec()
}

#[test]
fn the_browser_s_diff_is_the_command_line_s_with_what_a_viewer_draws() {
    let (old, new) = (bytes("bearing_flange.old"), bytes("bearing_flange.new"));
    let got = session::diff(
        ("old.step", &old),
        ("new.step", &new),
        &Map::new(),
        "flange",
    )
    .unwrap();
    let (before, after) = (
        part::load_bytes("old.step", old.clone()).unwrap(),
        part::load_bytes("new.step", new.clone()).unwrap(),
    );
    let revised = revision::diff(&before, &after, &Map::new()).unwrap();
    assert_eq!(got["report"], serde_json::to_value(&revised).unwrap());
    assert_eq!(got["markdown"], revised.markdown("flange", None));
    let view = &got["view"];
    // Both meshes, one face range per face, and the classes the picture colours by.
    assert_eq!(
        view["new"]["faces"].as_array().unwrap().len(),
        after.part.faces.len()
    );
    assert_eq!(
        view["old"]["faces"].as_array().unwrap().len(),
        before.part.faces.len()
    );
    let faces = view["faces"].as_object().unwrap();
    assert!(!faces.is_empty());
    assert!(
        faces
            .values()
            .all(|c| ["changed", "new", "ambiguous"].contains(&c.as_str().unwrap()))
    );
    assert!(view["gone"].as_array().is_some());
    assert!(view["motion"].is_null() || view["motion"]["rotation"].as_array().unwrap().len() == 3);
    // The "What changed" lines, as the Markdown lists them, each with its features' faces.
    let changes = view["changes"].as_array().unwrap();
    assert!(!changes.is_empty());
    let markdown = got["markdown"].as_str().unwrap();
    let mut seen = Vec::new();
    for c in changes {
        let line = c["line"].as_str().unwrap();
        assert!(markdown.contains(&format!("- {line}\n")), "{line}");
        let features: Vec<usize> = c["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_u64().unwrap() as usize)
            .collect();
        let mut new: Vec<usize> = features
            .iter()
            .flat_map(|&i| revised.features[i].new_faces.clone())
            .collect();
        new.sort_unstable();
        new.dedup();
        assert_eq!(c["new"], serde_json::json!(new), "{line}");
        assert!(
            !new.is_empty() || !c["old"].as_array().unwrap().is_empty(),
            "{line}"
        );
        seen.extend(features);
    }
    seen.sort_unstable();
    assert_eq!(seen, (0..revised.features.len()).collect::<Vec<_>>());
}

#[test]
fn a_compressed_or_unreadable_revision_is_refused_by_name() {
    let new = bytes("bearing_flange.new");
    let gz = [0x1f, 0x8b, 0, 0];
    let error =
        session::diff(("old.step.gz", &gz), ("new.step", &new), &Map::new(), "x").unwrap_err();
    assert!(error.contains("Decompress"), "{error}");
    let error = session::diff(
        ("old.step", b"garbage"),
        ("new.step", &new),
        &Map::new(),
        "x",
    )
    .unwrap_err();
    assert!(error.contains("old.step"), "{error}");
}

#[test]
fn an_assembly_is_compared_part_by_part_each_changed_part_shaped_as_a_part_s_diff() {
    let (old, new) = (bytes("gramil_assembly.old"), bytes("gramil_assembly.new"));
    let got = session::diff(
        ("old.step", &old),
        ("new.step", &new),
        &Map::new(),
        "gramil_assembly.step",
    )
    .unwrap();
    assert!(
        got["markdown"]
            .as_str()
            .unwrap()
            .starts_with("### gramil_assembly.step: Specify\n\n✅ 2 parts changed")
    );
    assert_eq!(got["report"]["parts"].as_array().unwrap().len(), 12);
    let after = part::File::of_bytes(Path::new("new.step"), new.clone())
        .unwrap()
        .parts();
    let parts = got["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 12);
    for p in parts {
        let name = p["name"].as_str().unwrap();
        if p["status"] == "changed" {
            let faces = after.iter().find(|s| s.name == name).unwrap().faces;
            assert_eq!(p["view"]["new"]["faces"].as_array().unwrap().len(), faces);
            assert!(
                p["markdown"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("#### {name}\n\n✅ ")),
                "{name}"
            );
            assert!(p["report"]["summary"].is_string());
        } else {
            assert_eq!(p["status"], "unchanged", "{name}");
            assert!(
                p.get("view").is_none() && p.get("markdown").is_none(),
                "{name}"
            );
            assert!(p["report"].is_object(), "{name}");
        }
    }
    let changed: Vec<&str> = parts
        .iter()
        .filter(|p| p["status"] == "changed")
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(changed, ["GRM-02 shaft", "GRM-04 drive plate"]);
}

#[test]
fn numbered_requirement_changes_use_the_same_identity_as_pmi_scene() {
    use serde_json::json;
    use specify_core::{api, pmi_scene, writer};
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/parts/plate.step");
    let bare = part::load(&fixture, None).unwrap();
    let write = |p: &part::LoadedPart, fit| {
        let answers = json!({"hole.function:holes:9":fit})
            .as_object()
            .unwrap()
            .clone();
        let bytes = writer::write_bytes(
            p,
            &api::preview(&api::analyse(p).unwrap(), &answers).unwrap(),
            Some(&answers),
        )
        .unwrap()
        .0;
        part::load_bytes("written.step", bytes).unwrap()
    };
    let old = write(&bare, "fit:H7");
    let new = write(&old, "fit:H8");
    let result = session::diff(
        ("old.step", &old.bytes),
        ("new.step", &new.bytes),
        &Map::new(),
        "Fit",
    )
    .unwrap();
    let scene = pmi_scene::pmi_scene(&new.bytes, None).unwrap();
    let items = result["report"]["pmi"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["class"], "changed");
    let meta = &scene["specify"][items[0]["id"].as_str().unwrap()];
    assert_eq!(items[0]["number"], meta["number"]);
    assert_eq!(items[0]["status"], meta["status"]);
}

#[test]
fn optional_scenes_use_the_same_report_without_changing_defaults() {
    let (old, new) = (bytes("hole_moved.old"), bytes("hole_moved.new"));
    let answers = serde_json::from_value(serde_json::json!({"part.notes":["Keep dry"]})).unwrap();
    let default =
        session::diff(("old.step", &old), ("new.step", &new), &answers, "Scenes").unwrap();
    let mut included = session::diff_with_options(
        ("old.step", &old),
        ("new.step", &new),
        &answers,
        "Scenes",
        revision::DiffOptions { scenes: true },
    )
    .unwrap();
    let scenes = included["report"]["pmi"]
        .as_object_mut()
        .unwrap()
        .remove("scenes")
        .unwrap();
    assert_eq!(included, default);
    let old = session::Session::open("old.step", &old).unwrap();
    assert_eq!(scenes["old"], old.pmi_scene(&answers).unwrap());
}
