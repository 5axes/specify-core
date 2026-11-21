//! Revisions: what changed from one revision of a part to the next through quiddity's
//! correspondence, and the requirements each file's PMI states (ADR P11: nothing else is read).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use specify_core::part::{LoadedPart, load};
use specify_core::revision::{Revised, diff, picture};

fn pair(case: &str) -> (LoadedPart, LoadedPart) {
    let path = |which: &str| -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/revisions/{case}.{which}.step.gz"))
    };
    (
        load(&path("old"), None).unwrap(),
        load(&path("new"), None).unwrap(),
    )
}

/// *part* written with *given* and every default, as `write --accept-defaults` writes it.
fn annotated(part: &LoadedPart, given: Value) -> LoadedPart {
    let analysis = specify_core::api::analyse(part).unwrap();
    let mut given = given.as_object().unwrap().clone();
    given.remove("_specify");
    let answers = specify_core::rules::with_all_defaults(&analysis, &given).unwrap();
    let intent = specify_core::api::preview(&analysis, &answers).unwrap();
    let bytes = specify_core::writer::write_bytes(part, &intent, Some(&answers))
        .unwrap()
        .0;
    specify_core::part::load_bytes("annotated.step", bytes).unwrap()
}

fn revised(case: &str) -> Revised {
    let (old, new) = pair(case);
    diff(&old, &new).unwrap()
}

#[test]
fn a_moved_hole_s_requirements_are_flagged_for_checking() {
    // Each revision written with its requirements: the Ø6 hole moved X −15.
    let (old, new) = pair("hole_moved");
    let given = json!({ "part.material": "6061-T6", "hole.function:holes:8": "clearance" });
    let (old, new) = (written(&old, given.clone()), written(&new, given));
    let r = diff(&old, &new).unwrap();
    assert_eq!(r.verdict, "decide", "{}", r.summary);
    assert_eq!(r.check.len(), 1, "{:?}", r.check);
    assert!(r.check[0].changes.contains_key("position"));
    assert!(r.check[0].requirement.contains('⌀'), "{:?}", r.check);
    assert!(
        r.summary.contains("1 requirement needs checking:"),
        "{}",
        r.summary
    );
    let md = r.markdown("Plate", None);
    assert!(md.contains("**Requirements to check**"), "{md}");
    assert!(
        md.contains("Ø6 × 10 through hole moved X −15, so its"),
        "{md}"
    );
    assert!(md.contains("may no longer fit"), "{md}");
    assert!(
        !r.summary.contains("answer") && !md.contains("answer"),
        "{md}"
    );
    assert!(
        !r.gone.is_empty(),
        "the moved hole's old faces are drawn as a ghost"
    );
    let json = serde_json::to_value(&r).unwrap();
    assert!(json["check"][0]["requirement"].is_string());
    assert!(json["check"][0].get("answer").is_none());
}

#[test]
fn a_turned_part_has_not_changed() {
    // Each revision written with the same requirements.
    let (old, new) = pair("whole_part_moved");
    let given =
        json!({ "part.material": "6061-T6", "hole.function:holes:8": "clearance", "datum.A": [4] });
    let (old, new) = (written(&old, given.clone()), written(&new, given));
    assert!(
        !specify_core::characteristics::read(&new)
            .unwrap()
            .is_empty()
    );
    let r = diff(&old, &new).unwrap();
    assert!(edits(&r).is_empty(), "{:?}", r.pmi.items);
    assert_eq!(r.verdict, "ok", "{}", r.summary);
    assert!(r.features.is_empty());
    assert!(r.check.is_empty());
    assert_eq!(r.summary, "No feature changed.");
    let json = serde_json::to_value(&r).unwrap();
    for gone in ["answers", "unresolved", "questions", "problems", "migrated"] {
        assert!(json.get(gone).is_none(), "{gone}");
    }
}

#[test]
fn the_picture_is_a_png() {
    let (old, new) = pair("boss_resized");
    let r = diff(&old, &new).unwrap();
    let png = picture(&old, &new, &r, 300);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert!(
        height <= 2 * 300 + 2,
        "no taller than twice the width: {height}"
    );
    assert!(
        r.faces.values().any(|c| *c == "changed"),
        "the boss is coloured"
    );
}

#[test]
fn the_command_line_writes_the_report_and_picture_and_no_answers() {
    let dir = std::env::temp_dir().join(format!("specify-core-revision-{}", std::process::id()));
    let fixture = |which: &str| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/revisions/hole_removed.{which}.step.gz"
        ))
    };
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_file(dir.join("answers.json"));
    let cli = || Command::new(env!("CARGO_BIN_EXE_specify-core"));
    let out = cli()
        .arg("diff")
        .args([fixture("old"), fixture("new")])
        .arg("-o")
        .arg(&dir)
        .args(["--title", "Plate"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed["verdict"], "ok");
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(printed["summary"], report["summary"]);
    assert!(!printed["summary"].as_str().unwrap().contains("answer"));
    assert!(report.get("answers").is_none() && report.get("check_lines").is_none());
    // Diff reads the files' PMI only and writes no answers (ADR P11).
    assert!(!dir.join("answers.json").exists());
    let md = std::fs::read_to_string(dir.join("report.md")).unwrap();
    assert!(md.starts_with("### Plate: Specify\n\n✅"), "{md}");
    assert!(
        md.contains(
            "- Ø7 × 10 through hole removed: on the −Z side, at X 30, Y 30 from the −X, −Y corner"
        ),
        "{md}"
    );
    assert!(dir.join("model.png").is_file() && dir.join("report.json").is_file());
    // Old answers are not taken.
    let refused = cli()
        .arg("diff")
        .args([fixture("old"), fixture("new")])
        .args(["--answers", "old.answers.json", "-o"])
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
}

#[test]
fn the_report_says_what_changed_in_words() {
    let md = |case: &str| revised(case).markdown("Plate", None);
    let thickened = md("plate_thickened");
    // Alike holes changed alike read as one line, with where they are.
    assert!(
        thickened.contains(
            "- 3× Ø4 × 10 through holes, through the part along Z, at X 30, Y 30; X 50, Y 30; \
             X 70, Y 30 from the −X, −Y corner: depth 10 → 12"
        ),
        "{thickened}"
    );
    assert!(
        thickened.contains("still there, but alike, so the old and new can't be paired one to one"),
        "{thickened}"
    );
    // The whole part, before and after.
    for row in [
        "| Volume | 58,494 mm³ | 70,193 mm³ | +11,699 mm³ (+20%) |",
        "| Bounding box | 100 × 60 × 10 mm | 100 × 60 × 12 mm | Z +2 |",
        "| Faces | 15 | 15 | unchanged |",
    ] {
        assert!(thickened.contains(row), "{thickened}");
    }
    let moved = md("hole_moved");
    assert!(moved.contains("✅ 1 hole moved.\n"), "{moved}");
    assert!(
        moved.contains(
            "- Ø6 × 10 through hole, on the −Z side, at X 40, Y 45 from the −X, −Y corner: \
             moved X −15"
        ),
        "{moved}"
    );
    let resized = md("boss_resized");
    assert!(
        resized.contains(
            "- Ø20 × 8 boss, on the +Z side, at X 70, Y 40 from the −X, −Y corner: \
             diameter 20 → 26"
        ),
        "{resized}"
    );
}

#[test]
fn a_part_the_old_file_lacks_is_named() {
    let fixture = |which: &str| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/revisions/hole_moved.{which}.step.gz"
        ))
    };
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([fixture("old"), fixture("new")])
        .args(["--part", "1", "-o"])
        .arg(std::env::temp_dir().join("specify-core-revision-part"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("hole_moved.old.step.gz has 1 part(s); there is no part 1"),
        "{err}"
    );
}

/// *fixture* (a gzipped STEP) unpacked to *to*, with *edit* applied to its text.
fn unpacked(fixture: &str, to: &Path, edit: impl Fn(String) -> String) -> PathBuf {
    use std::io::Read;
    let mut text = String::new();
    flate2::read::GzDecoder::new(
        std::fs::File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture)).unwrap(),
    )
    .read_to_string(&mut text)
    .unwrap();
    std::fs::write(to, edit(text)).unwrap();
    to.to_path_buf()
}

#[test]
fn a_changed_cad_tolerance_is_reported() {
    let dir = std::env::temp_dir().join(format!("specify-core-diff-cad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let nist = "tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz";
    let old = unpacked(nist, &dir.join("old.stp"), |t| t);
    // Flatness.1 (#57) measures #82: 0.2 tightened to 0.1.
    let new = unpacked(nist, &dir.join("new.stp"), |t| {
        let at = t.find("#82=(").unwrap();
        let (head, tail) = t.split_at(at);
        format!(
            "{head}{}",
            tail.replacen("LENGTH_MEASURE(0.2)", "LENGTH_MEASURE(0.1)", 1)
        )
    });
    let (old, new) = (load(&old, None).unwrap(), load(&new, None).unwrap());
    let same = diff(&old, &old).unwrap();
    assert!(same.pmi.items.is_empty(), "{:?}", same.pmi.items);
    let r = diff(&old, &new).unwrap();
    assert_eq!(r.pmi.items.len(), 1, "{:?}", r.pmi.items);
    let c = &r.pmi.items[0];
    assert_eq!(c.class, "changed");
    assert_eq!(
        (c.old.as_deref(), c.new.as_deref()),
        (Some("⏥ 0.2"), Some("⏥ 0.1"))
    );
    let md = r.markdown("CTC 01", None);
    assert!(md.contains("- ⏥ 0.2 → ⏥ 0.1 (50 thick plate)"), "{md}");
    assert!(
        md.starts_with("### CTC 01: Specify\n\n✅ 1 change to the PMI."),
        "{md}"
    );
}

#[test]
fn specify_pmi_in_the_files_is_read_and_compared() {
    let dir = std::env::temp_dir().join(format!("specify-core-diff-pmi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cli = || Command::new(env!("CARGO_BIN_EXE_specify-core"));
    let fixture = "tests/fixtures/revisions/hole_removed";
    // Each revision written with Specify's PMI: the Ø8 hole (face 8, face 7 on the new part)
    // general before, fit:H8 after.
    let written = |which: &str, answers: Value| {
        let step = unpacked(
            &format!("{fixture}.{which}.step.gz"),
            &dir.join(format!("{which}.step")),
            |t| t,
        );
        let (analysis, given) = (
            dir.join(format!("{which}.json")),
            dir.join(format!("{which}.answers.json")),
        );
        std::fs::write(&given, answers.to_string()).unwrap();
        assert!(
            cli()
                .arg("analyse")
                .arg(&step)
                .arg("-o")
                .arg(&analysis)
                .status()
                .unwrap()
                .success()
        );
        let out = dir.join(format!("{which}.pmi.step"));
        assert!(
            cli()
                .arg("write")
                .args([&step, &analysis, &given])
                .arg("-o")
                .arg(&out)
                .arg("--accept-defaults")
                .status()
                .unwrap()
                .success()
        );
        load(&out, None).unwrap()
    };
    let old = written(
        "old",
        json!({ "part.material": "6061-T6", "hole.function:holes:8": "general" }),
    );
    let bare_path = dir.join("bare.step");
    let bare = load(
        &unpacked(&format!("{fixture}.new.step.gz"), &bare_path, |t| t),
        None,
    )
    .unwrap();
    // A new file without PMI states no requirement: every one of the old file's is removed.
    let r = diff(&old, &bare).unwrap();
    assert!(!r.pmi.items.is_empty());
    assert!(
        r.pmi.items.iter().all(|i| i.class == "removed"),
        "{:?}",
        r.pmi.items
    );
    // A new file with its own: the Ø8 hole's size changes, and nothing is read but the PMI.
    let new = written(
        "new",
        json!({ "part.material": "6061-T6", "hole.function:holes:7": "fit:H8" }),
    );
    let r = diff(&old, &new).unwrap();
    // The two files are numbered independently, so pairs are found by faces and kind.
    let changed: Vec<_> = r
        .pmi
        .items
        .iter()
        .filter(|c| c.class == "changed" && c.kind == "dimension" && c.on.contains("Ø8"))
        .filter(|c| c.old.as_deref() == Some("⌀8"))
        .collect();
    assert_eq!(changed.len(), 1, "{:?}", r.pmi.items);
    assert_eq!(changed[0].new.as_deref(), Some("⌀8 H8"), "{changed:?}");
}

#[test]
fn a_real_part_s_report_names_what_a_person_changed() {
    // build123d's cast bearing unit: the flange 11 → 13 thick, the bolt holes Ø10 → Ø9.
    // Each revision written with its requirements and every default.
    let (old, new) = pair("bearing_flange");
    let given = json!({
        "part.material": "EN AC-42100",
        "hole.function:hole_patterns.set:31.32": "clearance:M8",
        "datum.A": [17]
    });
    let r = diff(&written(&old, given.clone()), &written(&new, given)).unwrap();
    // The holes first; the rounds and drafted surfaces that follow them counted after.
    assert!(
        r.summary
            .starts_with("2 holes changed, 8 rounds and surfaces with them, 1 change to the PMI."),
        "{}",
        r.summary
    );
    let md = r.markdown("Bearing", None);
    assert!(
        md.contains(
            "- 2× Ø10 × 11 through holes, on the −Z side, at X 10.9, Y 28.4; X 87.4, Y 28.4 \
             from the −X, −Y corner: depth 11 → 13; diameter 10 → 9"
        ),
        "{md}"
    );
    // A drafted surface is reshaped, and a round riding it moves only as the flange grows.
    assert!(
        md.contains("- 4× freeform surfaces: reshaped; moved Z +2"),
        "{md}"
    );
    // quiddity #120 recognises all four tangent mirror rounds as fillets.
    let rounds: Vec<_> = md
        .lines()
        .filter(|line| line.starts_with("- 4× R1 fillets, at "))
        .collect();
    assert_eq!(rounds.len(), 1, "{md}");
    assert!(rounds[0].ends_with(": moved Z +2"), "{md}");
    assert!(!md.contains("support."), "{md}");
    // The new file's requirements on the pair of holes are listed to check, without the
    // holes' middles moving as they deepen.
    assert_eq!(r.check.len(), 1, "{:?}", r.check);
    assert!(
        md.contains("- pattern of 2 × Ø9 × 13 through holes: depth 11 → 13; diameter 10 → 9, so its ⌀9 H11 requirement may no longer fit.\n"),
        "{md}"
    );
}

/// The "What changed" lines of a `complex_*` pair (`tests/fixtures/revisions/complex.py`): a
/// 100 × 80 × 10 plate with 4 × Ø6 holes, a pocket, two chamfers and a boss, changed in ways a
/// report must still put plainly.
fn complex(case: &str) -> String {
    revised(case).markdown("Plate", None)
}

#[test]
fn a_pattern_laid_out_again_is_one_pattern_become_another() {
    let md = complex("complex_pattern");
    assert!(
        md.contains(
            "- pattern of 4 × Ø6 × 10 through holes → pattern of 5 × Ø6 × 10 through holes on a \
             Ø48 circle: on the +Z side, at X 50, Y 40 from the −X, −Y corner"
        ),
        "{md}"
    );
    // Its holes' own pairings are folded into it.
    for not in ["can't be paired", "are now", "moved X"] {
        assert!(!md.contains(not), "{not}: {md}");
    }
    // No PMI in either file: nothing to check, so nothing to decide (ADR P11: no questions).
    assert!(md.contains("✅ 1 hole pattern changed."), "{md}");
}

#[test]
fn many_changes_at_once_each_read_once() {
    let md = complex("complex_many");
    for line in [
        "- 2× 1 × 45° chamfers removed: at (0, 39.5, 9.5) and (0, -39.5, 9.5)",
        "- Ø10 × 6 boss removed: on the −Z side, at X 10, Y 68 from the −X, −Y corner",
        "- pattern of 4 × Ø6 × 10 through holes → pattern of 6 × Ø8 × 10 through holes on a Ø60 \
         circle: on the +Z side, at X 50, Y 40 from the −X, −Y corner",
        "- section recess: depth 4 → 6; length 30 → 36",
        "- 16 × 6 × 10 slot added",
    ] {
        assert!(md.contains(line), "{line}\n{md}");
    }
    // The pocket once, in words: not again by its section's sides.
    assert_eq!(md.matches("section recess:").count(), 1, "{md}");
    for not in ["section.sides", "run length", "no longer one to one"] {
        assert!(!md.contains(not), "{not}: {md}");
    }
}

#[test]
fn what_a_feature_borders_is_not_a_change_to_it() {
    // Filleting the corners: the chamfers they meet are the same chamfers. (build123d carries a
    // chamfer round a tangent fillet, so six chamfer faces are new.)
    let r = revised("complex_fillet_all");
    let md = r.markdown("Plate", None);
    assert!(md.contains("- 4× R5 fillets added"), "{md}");
    // The six faces continue the two old chamfers; they are not six added features.
    assert!(
        md.contains("- 2× chamfers now run round the new fillets"),
        "{md}"
    );
    assert!(!md.contains("chamfers added"), "{md}");
    let continued: Vec<_> = r
        .features
        .iter()
        .filter(|f| f.class == "continued")
        .collect();
    assert_eq!(continued.len(), 2);
    let faces: std::collections::BTreeSet<_> = continued
        .iter()
        .flat_map(|f| f.new_faces.iter().copied())
        .collect();
    assert_eq!(faces, [2, 5, 7, 18, 19, 20, 21, 22].into_iter().collect());
    assert!(faces.iter().all(|i| r.faces.get(i) == Some(&"changed")));
    assert!(continued.iter().all(|f| f.old_faces.len() == 1));
    assert!(!md.contains("neighbourhood"), "{md}");
    assert!(
        !md.contains("chamfers, at"),
        "no line for the chamfers kept: {md}"
    );
}

#[test]
fn a_hole_that_became_another_kind_of_hole_reads_as_that() {
    let md = complex("complex_counterbore");
    // Name the enlargement, not just the remaining 7 mm of narrow bore.
    assert!(
        md.contains(
            "- 4× Ø6 × 10 through holes → Ø6 through holes, counterbored Ø12 × 3 deep: on the +Z side"
        ),
        "{md}"
    );
    for not in ["removed", "added", "neighbourhood", "pattern of"] {
        assert!(!md.contains(not), "{not}: {md}");
    }
    assert!(md.contains("✅ 4 holes changed."), "{md}");
}

#[test]
fn a_whole_part_moved_is_said_and_its_changes_found_where_it_is() {
    let md = complex("complex_moved");
    assert!(
        md.contains(
            "The whole part moved between the revisions (turned 90° about Z, moved X +8, Y +15, \
             Z +3); each feature is compared where it is now."
        ),
        "{md}"
    );
    assert!(md.contains(": diameter 6 → 7"), "{md}");
    // The holes alone: diff asks no questions (ADR P11).
    assert_eq!(md.matches("\n- ").count(), 1, "the holes: {md}");
}

#[test]
fn the_same_part_built_another_way_has_not_changed() {
    let md = complex("complex_remodelled");
    assert!(md.contains("✅ No feature changed."), "{md}");
    assert!(!md.contains("The whole part moved"), "{md}");
}

#[test]
fn an_end_chamfer_is_one_change_despite_trimmed_recognition_records() {
    use quiddity::correspondence as qc;
    let (old, new) = pair("gramil_shaft");
    let was = qc::recognise(&old.part);
    let now = qc::recognise(&new.part);
    let before = serde_json::to_value(&was).unwrap();
    let after = serde_json::to_value(&now).unwrap();
    assert_eq!(before["bosses"][0]["height"], 37.6);
    // quiddity-rust#134 includes the end chamfer and retains the trimmed plate.
    assert_eq!(after["bosses"][0]["height"], 37.6);
    assert_eq!(before["flats"][1]["at"], json!([35.6, 0., -3.4]));
    assert_eq!(after["flats"][1]["at"], json!([35.052, 0., -3.4]));
    assert_eq!(before["plates"].as_array().unwrap().len(), 1);
    assert_eq!(after["plates"].as_array().unwrap().len(), 1);
    assert_eq!(after["chamfers"][0]["leg1"], 1.5);
    assert_eq!(after["chamfers"][0]["leg2"], 1.5);
    assert_eq!(after["chamfers"][0]["angle"], 45.);
    let r = diff(&old, &new).unwrap();
    let corr = r.correspondence.as_ref().unwrap();
    assert!(
        corr.features
            .iter()
            .any(|f| f.old == "plates/0" && f.class == qc::Class::Carried)
    );
    // quiddity-rust#134 recovers this retained end plane through the supporting plate.
    let end = corr.faces.iter().find(|f| f.old == 1).unwrap();
    assert_eq!(end.new, Some(13));
    assert_eq!(end.class, qc::Class::Adapted);
    assert!(end.changes.contains_key("area"));
    assert!(end.changes.contains_key("adjacency"));
    assert_eq!(r.features.len(), 1, "{:?}", r.features);
    assert_eq!(r.features[0].family, "chamfers");
    assert_eq!(r.features[0].class, "new");
    assert_eq!(
        r.change_lines()[0].0,
        "1.5 × 45° chamfer added: on the end of the Ø8 boss"
    );
}

fn shaft_control(which: &str) -> Revised {
    let (old, _) = pair("gramil_shaft");
    let new = load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/revisions/gramil_shaft.{which}.step.gz"
        )),
        None,
    )
    .unwrap();
    diff(&old, &new).unwrap()
}

#[test]
fn a_real_boss_shortening_alongside_a_chamfer_still_reports() {
    let r = shaft_control("shorter");
    assert!(
        r.features
            .iter()
            .any(|f| f.family == "bosses" && f.changes.contains_key("sizes.height")),
        "{:?}",
        r.features
    );
    // quiddity-rust#134 includes the 1.5 mm end chamfer in both physical heights.
    assert!(r.markdown("Shaft", None).contains("height 37.6 → 35.6"));
    // The existing diameter-resize fixture is still a reported size change too.
    assert!(
        revised("boss_resized")
            .features
            .iter()
            .any(|f| f.family == "bosses" && f.changes.contains_key("sizes.diameter"))
    );
}

#[test]
fn a_top_filleted_boss_shortened_reports_its_real_height_change() {
    // #181: both bosses include their R3 top fillet and cap, so 13 → 8, not 13 → 5.
    let r = revised("boss_shortened");
    let boss = r
        .features
        .iter()
        .find(|f| f.family == "bosses")
        .unwrap_or_else(|| panic!("{:?}", r.features));
    assert!(boss.changes.contains_key("sizes.height"), "{boss:?}");
    let md = r.markdown("Boss", None);
    assert!(md.contains("height 13 → 8"), "{md}");
}

#[test]
fn real_flat_moves_alongside_a_chamfer_still_report() {
    // Move the shaft's flat perpendicular to its support plane.
    let r = shaft_control("flat_moved");
    assert!(
        r.features
            .iter()
            .any(|f| f.family == "flats" && f.changes.contains_key("position")),
        "{:?}",
        r.features
    );
    // Move a finite flat WITHIN the same supporting plane. Plane equality alone is insufficient.
    let r = revised("flat_translated");
    assert!(
        r.features
            .iter()
            .any(|f| f.family == "flats" && f.changes.contains_key("position")),
        "{:?}",
        r.features
    );
    assert!(r.markdown("Shaft", None).contains("moved Z +3"));
}

#[test]
fn a_real_plate_removal_alongside_a_chamfer_still_reports() {
    let r = revised("plate_removed");
    assert!(
        r.features
            .iter()
            .any(|f| f.family == "plates" && f.class == "orphaned"),
        "{:?}",
        r.features
    );
    assert!(r.markdown("Bracket", None).contains("thick plate removed"));
}

#[test]
fn chamfer_trimming_is_compared_after_the_whole_part_moves() {
    let (old, mut new) = pair("gramil_shaft");
    new.part = new
        .part
        .transformed(&[[0., -1., 0., 10.], [1., 0., 0., -8.], [0., 0., 1., 3.]])
        .unwrap();
    let r = diff(&old, &new).unwrap();
    assert!(r.motion().is_some());
    assert_eq!(r.features.len(), 1, "{:?}", r.features);
    assert_eq!(
        r.change_lines()[0].0,
        "1.5 × 45° chamfer added: on the end of the Ø8 boss"
    );
}

#[test]
fn a_part_is_recognised_once_however_often_it_is_analysed_or_diffed() {
    // #180: an editing loop analyses the baseline and diffs it against several candidates.
    let (old, new) = pair("hole_moved");
    assert!(
        old.recognition.get().is_none(),
        "nothing is recognised on load"
    );
    let first = specify_core::api::analyse(&old).unwrap();
    let made = std::ptr::from_ref(old.recognition.get().expect("analyse recognised the part"));
    let r1 = diff(&old, &new).unwrap();
    let r2 = diff(&old, &new).unwrap();
    assert_eq!(
        std::ptr::from_ref(old.recognition()),
        made,
        "the recognition made by analyse is the one every diff reads"
    );
    assert!(new.recognition.get().is_some());
    // Caching changes no output.
    assert_eq!(specify_core::api::analyse(&old).unwrap(), first);
    assert_eq!(
        serde_json::to_value(&r1).unwrap(),
        serde_json::to_value(&r2).unwrap()
    );
}

#[test]
fn a_face_pushed_5_mm_that_no_feature_holds_is_a_face_change_with_its_offset() {
    // `block_face_pushed`: a 60 × 40 × 20 block whose top face is pushed to 25 high (see the
    // fixtures' README); no hole, fillet or other feature is touched.
    let r = revised("block_face_pushed");
    assert!(r.features.is_empty(), "{:?}", r.features);
    let pushed: Vec<_> = r
        .other_faces
        .iter()
        .filter(|f| f.offset.is_some_and(|d| d.abs() > 1e-6))
        .collect();
    assert_eq!(pushed.len(), 1, "{:#?}", r.other_faces);
    let top = pushed[0];
    assert_eq!(
        (top.class.as_str(), top.kind.as_str(), top.normal),
        ("moved", "plane", Some([0.0, 0.0, 1.0]))
    );
    assert_eq!(top.offset, Some(5.0), "outward along +Z: {top:?}");
    // The four sides grow 20 → 25 high, in the same plane: the surface is unchanged.
    let sides: Vec<_> = r
        .other_faces
        .iter()
        .filter(|f| f.area.is_some() && f.offset == Some(0.0))
        .collect();
    assert_eq!(sides.len(), 4, "{:#?}", r.other_faces);
    assert!(sides.iter().all(|f| f.class == "adapted"));
    // The bottom, still on z = 0, is the one unchanged face.
    assert_eq!(r.unchanged_faces, 1);
    assert_eq!(r.other_faces.len() + r.unchanged_faces + r.context_faces, 6);
    // The picture shows it: the pushed face is changed, its old place a ghost.
    assert_eq!(r.faces.get(&5), Some(&"changed"));
    assert!(r.gone.contains(&5));
    // The pushed face is listed first, then the faces only trimmed by it.
    assert_eq!(r.other_faces[0].offset, Some(5.0));
    let md = r.markdown("Block", None);
    assert!(
        md.contains("**Other faces** (no feature above explains them; 1 face unchanged)"),
        "{md}"
    );
    assert!(
        md.contains("- face 5 → 5 (plane, normal (0, 0, 1)): moved 5 mm outward"),
        "{md}"
    );
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["unchanged_faces"], 1);
    assert_eq!(json["other_faces"][0]["offset"], 5.0);
}

#[test]
fn a_side_pushed_on_a_part_the_holes_anchor_is_found_after_the_alignment() {
    // `plate_side_pushed`: four holes anchor the match (an alignment is found, so the faces are
    // `adapted`, not `moved`); the +X side face is pushed 5 mm.
    let (old, new) = pair("plate_side_pushed");
    let r = diff(&old, &new).unwrap();
    assert!(r.correspondence.as_ref().unwrap().alignment.found);
    assert!(r.features.is_empty(), "{:?}", r.features);
    assert!(r.other_faces.iter().all(|f| f.class == "adapted"));
    let side = &r.other_faces[0];
    assert_eq!(
        (side.kind.as_str(), side.normal, side.offset),
        ("plane", Some([1.0, 0.0, 0.0]), Some(5.0))
    );
    // The other four planes only trimmed (top, bottom, front, back: 60 → 65 long); the holes and
    // the −X side are unchanged.
    assert_eq!(r.other_faces.len(), 5, "{:#?}", r.other_faces);
    assert!(r.other_faces[1..].iter().all(|f| f.offset == Some(0.0)));
    assert_eq!(r.unchanged_faces, 5);
    assert_eq!(r.context_faces, 0);
    assert_eq!(r.faces.get(&5), Some(&"changed"));
    // Pushed the other way, the sign flips: the side moves inward.
    let back = diff(&new, &old).unwrap();
    assert_eq!(back.other_faces[0].offset, Some(-5.0));
    assert!(back.markdown("Back", None).contains("moved 5 mm inward"));
}

#[test]
fn the_picture_of_a_bare_face_change_colours_it() {
    let (old, new) = pair("block_face_pushed");
    let r = diff(&old, &new).unwrap();
    let png = picture(&old, &new, &r, 300);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    // A zero or an enormous width is clamped, not a crash.
    assert_eq!(&picture(&old, &new, &r, 0)[..4], b"\x89PNG");
}

#[test]
fn faces_a_feature_explains_are_not_listed_again() {
    for case in ["hole_moved", "hole_removed", "hole_added", "boss_resized"] {
        let r = revised(case);
        assert!(!r.features.is_empty(), "{case}");
        // Everything these edits change is a feature's or its context: nothing is left over.
        assert!(r.other_faces.is_empty(), "{case}: {:?}", r.other_faces);
        assert!(r.context_faces > 0, "{case}");
        assert!(
            r.unchanged_faces + r.context_faces <= r.measures.new.faces,
            "{case}"
        );
        for face in &r.other_faces {
            for f in &r.features {
                assert!(
                    face.old.is_none_or(|o| !f.old_faces.contains(&o)),
                    "{case}: old face {face:?} is held by {}",
                    f.what
                );
                assert!(
                    face.new.is_none_or(|n| !f.new_faces.contains(&n)),
                    "{case}: new face {face:?} is held by {}",
                    f.what
                );
            }
        }
    }
}

/// A fixture's own answer file (`tests/fixtures/revisions/{case}.answers.json`).
fn answer_file(case: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/revisions/{case}.answers.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn a_symmetric_tie_is_matched_in_the_file_s_frame_and_said() {
    // A plate symmetric about its centre: Ø6 holes at (30, 30), datum B and H7, and (70, 30),
    // which moves to (75, 30). Turned 180° about Z, v1 fits v2 as well; the file frame wins.
    let r = revised("symmetric_tie");
    assert_eq!(r.verdict, "ok", "{}", r.summary);
    let tie = r.frame_tie.as_ref().expect("the tie is reported");
    assert_eq!(tie.motion, None);
    assert_eq!(tie.alternative.as_deref(), Some("a 180° turn about Z"));
    let md = r.markdown("Plate", None);
    assert!(
        md.contains("Matched in the file's frame; a 180° turn about Z fits equally."),
        "{md}"
    );
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["frame_tie"]["alternative"], "a 180° turn about Z");
    // The hole at (30, 30) is face 6 of the new part, as of the old.
    let (_, new) = pair("symmetric_tie");
    let analysis = specify_core::api::analyse(&new).unwrap();
    let hole = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "holes:6")
        .unwrap();
    assert_eq!(hole["record"]["location"], json!([30.0, 30.0, 10.0]));
}

#[test]
fn a_part_without_symmetry_has_no_tie() {
    let r = revised("hole_moved");
    assert!(r.frame_tie.is_none(), "{:?}", r.frame_tie);
    assert!(serde_json::to_value(&r).unwrap().get("frame_tie").is_none());
    assert!(!r.markdown("Plate", None).contains("file's frame"));
}

#[test]
fn a_removed_hole_s_requirements_are_not_paired_with_the_hole_taking_its_faces() {
    // The Ø8 hole (face 7) is removed; the Ø5 pair's faces 8, 9 become 7, 8. The removed
    // hole's location 80 reused a face-numbered id the Ø5 hole now has.
    // Each revision written with its requirements: the new file's are the old file's on the
    // holes that remain, the Ø5 pair now on faces 7, 8.
    let (old, new) = pair("pair_after_removal");
    let given = answer_file("pair_after_removal");
    let mut kept = given.as_object().unwrap().clone();
    kept.retain(|id, _| !id.ends_with(":holes:7") && !id.contains("set:8.9"));
    kept.insert(
        "hole.function:hole_patterns.set:7.8".into(),
        given["hole.function:hole_patterns.set:8.9"].clone(),
    );
    kept.insert(
        "hole.position:hole_patterns.set:7.8".into(),
        given["hole.position:hole_patterns.set:8.9"].clone(),
    );
    let r = diff(
        &annotated(&old, given.clone()),
        &annotated(&new, Value::Object(kept)),
    )
    .unwrap();
    let items = &r.pmi.items;
    let removed: Vec<_> = items.iter().filter(|i| i.class == "removed").collect();
    let mut gone: Vec<_> = removed.iter().map(|i| i.old.as_deref().unwrap()).collect();
    gone.sort_unstable();
    // The 15 that located both the Ø6 and the Ø8 hole now locates the Ø6 alone: its support
    // changed, so it is one removed and another added.
    assert_eq!(gone, ["15", "80", "⌀8 H11", "⌖ ⌀0.5Ⓜ A B C"], "{items:?}");
    assert!(
        removed
            .iter()
            .all(|i| i.on.contains("Ø8") || i.old.as_deref() == Some("15")),
        "{items:?}"
    );
    assert!(
        items
            .iter()
            .any(|i| i.class == "added" && i.new.as_deref() == Some("15") && i.on.contains("Ø6")),
        "{items:?}"
    );
    // Nothing changes on the Ø5 holes, and no value changes anywhere.
    assert!(!items.iter().any(|i| i.on.contains("Ø5")), "{items:?}");
    assert!(!items.iter().any(|i| i.class == "changed"), "{items:?}");
    // Fresh numbers retire nothing: the new file gives #15-#17 to other items, so the Ø8
    // hole's items are removed with their old numbers, not retired.
    assert_eq!(removed.len(), 4, "{items:?}");
    assert!(
        removed
            .iter()
            .all(|i| i.number.is_some() && i.reason.is_none()),
        "{items:?}"
    );
    let md = r.markdown("Plate", None);
    assert!(!md.contains("80 → 30") && !md.contains(" retired:"), "{md}");
}

#[test]
fn a_cut_or_a_resize_is_not_a_move() {
    // The four mounting holes Ø3 → Ø3.4 and a Ø8 entry cut into the +X wall. The body, its
    // section recess and the holes' pattern keep their faces where they were.
    let (old, new) = pair("enclosure_pmi");
    let r = diff(&old, &new).unwrap();
    let families: Vec<&str> = r.features.iter().map(|f| f.family.as_str()).collect();
    for family in ["thin_wall_bodies", "section_recesses", "hole_patterns"] {
        assert!(!families.contains(&family), "{family}: {:?}", r.features);
    }
    assert!(
        r.features
            .iter()
            .all(|f| !f.changes.contains_key("position")),
        "{:?}",
        r.features
    );
    assert_eq!(
        r.features.iter().filter(|f| f.class == "new").count(),
        1,
        "{:?}",
        r.features
    );
    assert!(
        !r.summary.contains("moved") && !r.summary.contains("bodie"),
        "{}",
        r.summary
    );
    // The pattern's two requirements to check read as one line, naming the position frame
    // as the PMI list does, not its planned locations.
    assert_eq!(r.check.len(), 2);
    assert_eq!(
        r.summary.matches("pattern of 4").count(),
        1,
        "{}",
        r.summary
    );
    assert!(
        r.summary.contains("⌖ ⌀0.2Ⓜ A B C requirements"),
        "{}",
        r.summary
    );
    assert!(!r.summary.contains("70; 50"), "{}", r.summary);
    assert!(
        r.summary.contains("blind holes: diameter 3 → 3.4, so its"),
        "{}",
        r.summary
    );
    assert!(
        r.summary
            .contains("2 characteristic numbers changed (requirements unchanged)"),
        "{}",
        r.summary
    );
    // The new file numbers its flatness and finish afresh: listed after the changes as
    // unchanged, and not counted as changes.
    let renumbered: Vec<_> = r
        .pmi
        .items
        .iter()
        .filter(|i| i.class == "renumbered")
        .collect();
    assert_eq!(renumbered.len(), 2, "{:?}", r.pmi.items);
    assert!(renumbered.iter().all(|i| i.old == i.new));
    let changes = r.pmi.items.len() - renumbered.len();
    assert!(
        r.summary.contains(&format!("{changes} changes to the PMI")),
        "{}",
        r.summary
    );
    let md = r.markdown("Enclosure", None);
    assert!(
        md.contains(": unchanged; characteristic number #22 → #25"),
        "{md}"
    );
}

#[test]
fn small_and_unaligned_moves_are_reported() {
    // A 0.1 mm hole move is a move; a pocket or slot moved where no alignment is found is
    // compared in the parts' own frames (tests/fixtures/revisions/small_moves.py).
    for (case, family, words) in [
        ("hole_nudged", "holes", "moved X +0.1"),
        ("pocket_moved", "pockets", "moved X +3"),
        ("slot_slid", "slots", "moved X +4"),
    ] {
        let r = revised(case);
        assert!(
            r.features.iter().any(|f| f.family == family
                && f.class == "adapted"
                && f.changes.contains_key("position")),
            "{case}: {:?}",
            r.features
        );
        assert!(
            !r.summary.starts_with("No feature changed"),
            "{case}: {}",
            r.summary
        );
        let md = r.markdown(case, None);
        assert!(md.contains(words), "{case}: {md}");
    }
}

#[test]
fn a_hole_cut_in_a_pocket_floor_does_not_move_the_pocket() {
    // A Ø0.5 hole through the floor of an unmoved pocket (small_moves.py): the floor loses well
    // under 0.1% of its area and its centroid shifts, but no face of the pocket moved.
    let r = revised("pocket_pin");
    assert!(
        r.features
            .iter()
            .all(|f| !f.changes.contains_key("position")),
        "{:?}",
        r.features
    );
    assert!(
        r.features
            .iter()
            .any(|f| f.class == "new" && f.family == "holes"),
        "{:?}",
        r.features
    );
    assert!(
        r.change_lines()
            .iter()
            .all(|(line, _)| !line.contains(": moved") && !line.contains("; moved")),
        "{:?}",
        r.change_lines()
    );
    assert!(!r.summary.contains(" moved"), "{}", r.summary);
}

/// *part* written with *given* only (no defaults taken).
fn written(part: &LoadedPart, given: Value) -> LoadedPart {
    let analysis = specify_core::api::analyse(part).unwrap();
    let given = given.as_object().unwrap().clone();
    let intent = specify_core::api::preview(&analysis, &given).unwrap();
    let bytes = specify_core::writer::write_bytes(part, &intent, Some(&given))
        .unwrap()
        .0;
    specify_core::part::load_bytes("written.step", bytes).unwrap()
}

/// The PMI items of a report that are more than a renumbering.
fn edits(r: &Revised) -> Vec<&specify_core::revision::PmiItemChange> {
    r.pmi
        .items
        .iter()
        .filter(|c| c.class != "renumbered")
        .collect()
}

#[test]
fn removing_a_hole_removes_its_requirements_and_the_holes_after_it_keep_theirs() {
    // Old holes on faces 6 (Ø5), 7 (Ø7, removed), 8 (Ø8) and 9 (Ø6); the new part's faces
    // after 7 renumber. Each file states its own requirements; the new one adds a flatness.
    let (old, new) = pair("hole_removed");
    let old = written(
        &old,
        json!({
            "part.material": "6061-T6",
            "hole.function:holes:6": "tapped:M6",
            "hole.function:holes:7": "fit:H7",
            "hole.function:holes:8": "fit:H8",
            "hole.function:holes:9": "fit:H9",
            "datum.A": [4],
        }),
    );
    let new = written(
        &new,
        json!({
            "part.material": "6061-T6",
            "hole.function:holes:6": "tapped:M6",
            "hole.function:holes:7": "fit:H8",
            "hole.function:holes:8": "fit:H9",
            "datum.A": [4],
            "datum.A.flatness": "0.02",
        }),
    );
    let r = diff(&old, &new).unwrap();
    let edits = edits(&r);
    // The removed Ø7 hole's requirements go, and only they.
    let removed: Vec<_> = edits.iter().filter(|c| c.class == "removed").collect();
    assert!(!removed.is_empty(), "{edits:?}");
    assert!(removed.iter().all(|c| c.on.contains("Ø7")), "{edits:?}");
    // The holes after it keep their requirements on the hole of the same size: nothing on
    // them is changed, added or removed.
    for size in ["Ø5", "Ø8", "Ø6"] {
        assert!(
            !edits.iter().any(|c| c.on.contains(size)),
            "{size}: {edits:?}"
        );
    }
    // The flatness the new file adds is added.
    let added: Vec<_> = edits.iter().filter(|c| c.class == "added").collect();
    assert_eq!(added.len(), 1, "{edits:?}");
    assert_eq!(added[0].new.as_deref(), Some("⏥ 0.02"));
    assert!(
        r.features
            .iter()
            .any(|f| f.class == "orphaned" && f.old.as_deref() == Some("holes:7"))
    );
}

#[test]
fn an_added_hole_s_requirements_are_added_on_it() {
    let (old, new) = pair("hole_added");
    let given = json!({ "part.material": "6061-T6" });
    let r = diff(&annotated(&old, given.clone()), &annotated(&new, given)).unwrap();
    assert!(
        r.features
            .iter()
            .any(|f| f.class == "new" && f.family == "holes" && f.what.contains("Ø7"))
    );
    let edits = edits(&r);
    assert!(!edits.is_empty());
    // Every requirement the new file states on the new hole is added; nothing else changes.
    assert!(
        edits
            .iter()
            .all(|c| c.class == "added" && c.on.contains("Ø7")),
        "{edits:?}"
    );
    assert!(
        edits.iter().any(|c| c.new.as_deref() == Some("⌀7 H11")),
        "{edits:?}"
    );
}

#[test]
fn symmetric_holes_are_not_paired_by_a_guess() {
    // The plate 10 → 12 thick; its pattern holes are symmetric repeats, which the matching
    // cannot tell apart. Each file states its own requirements.
    let (old, new) = pair("plate_thickened");
    let given = json!({ "part.material": "6061-T6" });
    let r = diff(&annotated(&old, given.clone()), &annotated(&new, given)).unwrap();
    assert!(
        r.features.iter().any(|f| f.class == "ambiguous"),
        "{:?}",
        r.features
    );
    let edits = edits(&r);
    // The only requirement changed is the thickness; a requirement on a hole the matching
    // cannot follow is never changed into another's value.
    let changed: Vec<_> = edits.iter().filter(|c| c.class == "changed").collect();
    assert_eq!(changed.len(), 1, "{edits:?}");
    assert_eq!(
        (changed[0].old.as_deref(), changed[0].new.as_deref()),
        (Some("10"), Some("12"))
    );
    // What it cannot pair is listed removed and added, value for value.
    let values = |class: &str| {
        let mut v: Vec<_> = edits
            .iter()
            .filter(|c| c.class == class)
            .map(|c| c.old.clone().or(c.new.clone()).unwrap())
            .collect();
        v.sort();
        v
    };
    assert_eq!(values("removed"), values("added"), "{edits:?}");
}

// For #125: a requirement follows its faces when another feature takes lower face numbers.
// The new Ø7 hole takes face 7, so the old Ø8 hole (7) becomes 8, the Ø6 hole (8) 9, the boss
// (9) 10 and its top (10) 11.
#[test]
fn an_added_requirement_follows_its_faces_when_a_feature_takes_lower_ids() {
    let (old, new) = pair("hole_added");
    let extras = |hole: usize, boss: usize, top: usize| {
        json!({
            "part.material": "6061-T6",
            "extra.hole.size": {"faces": [hole], "kind": "size", "value": "±0.1"},
            "extra.hole.place": {"faces": [hole], "reference": [0], "kind": "distance", "value": "±0.1"},
            "extra.boss.size": {"faces": [boss], "kind": "size", "value": "±0.2"},
            "extra.boss.height": {"faces": [top], "reference": [2], "kind": "distance", "value": "±0.1"},
        })
    };
    let old = written(&old, extras(7, 9, 10));
    // The same requirements on the same features, at their new face numbers: unchanged.
    let r = diff(&old, &written(&new, extras(8, 10, 11))).unwrap();
    assert!(edits(&r).is_empty(), "{:?}", edits(&r));
    // Kept at the old numbers, they would name other features: never an unchanged report.
    let hole_only = |hole: usize| json!({"part.material": "6061-T6", "extra.hole.size": {"faces": [hole], "kind": "size", "value": "±0.1"}});
    let r = diff(
        &written(&pair("hole_added").0, hole_only(7)),
        &written(&new, hole_only(7)),
    )
    .unwrap();
    assert!(
        edits(&r).iter().any(|c| c.class == "removed"),
        "{:?}",
        edits(&r)
    );
}

// Added requirements on unchanged geometry, including the symmetric repeats of
// plate_thickened's patterns, which the matching must not confuse: nothing changes.
#[test]
fn an_added_requirement_on_unchanged_geometry_carries_unchanged() {
    let (old, _) = pair("plate_thickened");
    let old = written(
        &old,
        json!({
            "part.material": "6061-T6",
            "datum.A": [4],
            "extra.end.size": {"faces": [7], "kind": "size", "value": "±0.1"},
            "extra.other_end.place": {"faces": [10], "reference": [0], "kind": "distance", "value": "±0.1"},
            "extra.row.size": {"faces": [12, 13, 14], "kind": "size", "value": "±0.05"},
            "extra.top.flatness": {"faces": [2], "kind": "flatness", "value": "0.05"},
        }),
    );
    let r = diff(&old, &old).unwrap();
    assert!(r.pmi.items.is_empty(), "{:?}", r.pmi.items);
    assert!(r.check.is_empty());
    assert_eq!(r.verdict, "ok");
}
