//! Revisions: answers carried from one revision of a part to the next through quiddity's
//! correspondence, and what changed.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};
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

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn revised(case: &str, given: Value) -> Revised {
    let (old, new) = pair(case);
    diff(&old, &new, &answers(given)).unwrap()
}

#[test]
fn removing_a_hole_renames_the_answers_of_the_holes_after_it() {
    // Old holes on faces 6 (Ø5), 7 (Ø7, removed), 8 (Ø8) and 9 (Ø6); the new part's faces
    // after 7 renumber.
    let r = revised(
        "hole_removed",
        json!({
            "part.material": "6061-T6",
            "hole.function:holes:6": "tapped:M6",
            "hole.function:holes:7": "fit:H7",
            "hole.function:holes:8": "clearance",
            "hole.function:holes:9": "general",
            "datum.A": [4],
        }),
    );
    assert_eq!(r.verdict, "decide");
    let unresolved: Vec<_> = r
        .unresolved
        .iter()
        .map(|u| (u.id.as_str(), u.class.as_str()))
        .collect();
    assert_eq!(unresolved, [("hole.function:holes:7", "orphaned")]);
    assert_eq!(r.answers["part.material"], "6061-T6");
    assert_eq!(r.answers["hole.function:holes:6"], "tapped:M6");
    assert_eq!(r.answers["datum.A"], json!([4]));
    let (old, new) = pair("hole_removed");
    let diameter = |p: &LoadedPart, id: &str| -> f64 {
        let a = specify_core::api::analyse(p).unwrap();
        a["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == id)
            .unwrap()["record"]["diameter"]
            .as_f64()
            .unwrap()
    };
    // Each carried answer is on the hole of the same size as before.
    for (before, after) in &r.migrated {
        let (b, a) = (
            before.split_once(':').unwrap().1,
            after.split_once(':').unwrap().1,
        );
        assert_eq!(diameter(&old, b), diameter(&new, a), "{before} → {after}");
    }
    assert_eq!(r.migrated.len(), 2, "the Ø8 and Ø6 holes renumber");
    assert!(
        r.features
            .iter()
            .any(|f| f.class == "orphaned" && f.old.as_deref() == Some("holes:7"))
    );
}

#[test]
fn a_moved_hole_keeps_its_answer_for_checking() {
    let r = revised(
        "hole_moved",
        json!({ "hole.function:holes:8": "clearance", "hole.function:holes:6": "general" }),
    );
    assert_eq!(r.verdict, "ok");
    assert!(r.unresolved.is_empty() && r.migrated.is_empty());
    let checked: Vec<_> = r.check.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(checked, ["hole.function:holes:8"]);
    assert!(r.check[0].changes.contains_key("position"));
    assert!(
        r.summary
            .contains("All 2 requirements still apply to the changed part."),
        "{}",
        r.summary
    );
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
    assert!(md.contains("requirement may no longer fit"), "{md}");
    assert!(
        !r.summary.contains("answer") && !md.contains("answer"),
        "{md}"
    );
    assert!(
        !r.gone.is_empty(),
        "the moved hole's old faces are drawn as a ghost"
    );
}

#[test]
fn an_added_hole_asks_its_questions() {
    let r = revised("hole_added", json!({ "part.material": "6061-T6" }));
    assert_eq!(r.verdict, "decide");
    assert!(
        r.features
            .iter()
            .any(|f| f.class == "new" && f.family == "holes")
    );
    assert!(
        r.questions
            .iter()
            .any(|q| q.id.starts_with("hole.function:")),
        "{:?}",
        r.questions
    );
}

#[test]
fn symmetric_holes_are_asked_not_guessed() {
    let (old, _) = pair("plate_thickened");
    let analysis = specify_core::api::analyse(&old).unwrap();
    let holes: Vec<String> = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["family"] == "holes")
        .map(|f| f["id"].as_str().unwrap().to_string())
        .collect();
    let given: Map<String, Value> = holes
        .iter()
        .map(|h| (format!("hole.function:{h}"), json!("clearance")))
        .collect();
    let r = revised("plate_thickened", Value::Object(given));
    assert!(
        r.unresolved
            .iter()
            .any(|u| u.class == "ambiguous" && u.candidates.len() > 1)
    );
    assert!(r.unresolved.iter().all(|u| u.class == "ambiguous"));
    assert_eq!(r.verdict, "decide");
}

#[test]
fn a_turned_part_carries_every_answer() {
    let r = revised(
        "whole_part_moved",
        json!({ "hole.function:holes:8": "clearance", "datum.A": [4] }),
    );
    assert_eq!(r.verdict, "ok", "{}", r.summary);
    assert!(r.features.is_empty());
    // The two requirements plus the reserved evidence/carry transport.
    assert_eq!(r.answers.len(), 3);
    assert_eq!(r.answers["_specify"]["unresolved"], json!({}));
    assert_eq!(
        r.summary,
        "No feature changed. All 2 requirements still apply to the changed part."
    );
}

#[test]
fn an_answer_the_old_part_never_had_is_stale() {
    let r = revised(
        "hole_moved",
        json!({ "hole.function:holes:42": "clearance" }),
    );
    assert_eq!(r.unresolved[0].class, "stale");
    assert!(
        r.summary.contains("1 requirement could not be applied"),
        "{}",
        r.summary
    );
    assert!(
        r.markdown("Plate", None)
            .contains("**Requirements not applied**")
    );
}

#[test]
fn the_picture_is_a_png() {
    let (old, new) = pair("boss_resized");
    let r = diff(&old, &new, &Map::new()).unwrap();
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
fn the_command_line_writes_the_answers_report_and_picture() {
    let dir = std::env::temp_dir().join(format!("specify-core-revision-{}", std::process::id()));
    let fixture = |which: &str| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/revisions/hole_removed.{which}.step.gz"
        ))
    };
    std::fs::create_dir_all(&dir).unwrap();
    let answers = dir.join("answers.in.json");
    std::fs::write(
        &answers,
        json!({ "hole.function:holes:7": "fit:H7", "hole.function:holes:9": "general" })
            .to_string(),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([fixture("old"), fixture("new")])
        .arg("--answers")
        .arg(&answers)
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
    assert_eq!(printed["verdict"], "decide");
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(printed["summary"], report["summary"]);
    assert!(printed["summary"].as_str().unwrap().contains("requirement"));
    assert!(!printed["summary"].as_str().unwrap().contains("answer"));
    assert!(report.get("answers").is_some() && report.get("check_lines").is_none());
    let carried: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("answers.json")).unwrap()).unwrap();
    // The carried requirement includes destination evidence. The removed hole's old ID
    // cannot block the different hole now using it; its refusal stays in the report.
    let destination = load(&fixture("new"), None).unwrap();
    assert_eq!(
        carried,
        json!({
            "hole.function:holes:8": "general",
            "_specify": {
                "evidence": specify_core::resume::evidence(&destination).unwrap(),
                "unresolved": {},
            },
        })
    );
    let md = std::fs::read_to_string(dir.join("report.md")).unwrap();
    assert!(md.starts_with("### Plate: Specify\n\n⚠️"), "{md}");
    assert!(
        md.contains(
            "- **fit:H7** (`hole.function:holes:7`): the Ø7 × 10 \
             through hole was removed"
        ),
        "{md}"
    );
    assert!(
        md.contains(
            "- Ø7 × 10 through hole removed: on the −Z side, at X 30, Y 30 from the −X, −Y corner"
        ),
        "{md}"
    );
    assert!(dir.join("model.png").is_file() && dir.join("report.json").is_file());
}

#[test]
fn the_report_says_what_changed_in_words() {
    let md = |case: &str| revised(case, json!({})).markdown("Plate", None);
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
    assert!(
        moved.contains("✅ 1 hole moved. There are no saved requirements to apply"),
        "{moved}"
    );
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
fn a_datum_s_settings_stay_open_with_their_datum() {
    // Face 7 is the removed hole's bore.
    let r = revised(
        "hole_removed",
        json!({ "datum.A": [7], "datum.A.flatness": "0.01", "datum.B": [4] }),
    );
    let unresolved: Vec<_> = r
        .unresolved
        .iter()
        .map(|u| (u.id.as_str(), u.class.as_str()))
        .collect();
    assert_eq!(
        unresolved,
        [("datum.A", "orphaned"), ("datum.A.flatness", "orphaned")]
    );
    assert_eq!(r.unresolved[1].reason, "its datum datum.A is unresolved");
    // Unresolved datum settings are now transported with their parent, not silently lost.
    assert_eq!(
        r.answers.keys().collect::<Vec<_>>(),
        ["datum.B", "_specify"]
    );
    let (_, destination) = pair("hole_removed");
    assert_eq!(
        r.answers["_specify"],
        json!({
            "evidence": specify_core::resume::evidence(&destination).unwrap(),
            "unresolved": {
                "datum.A": r.unresolved[0].reason,
                "datum.A.flatness": "its datum datum.A is unresolved",
            },
        })
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
    let same = diff(&old, &old, &Map::new()).unwrap();
    assert!(same.pmi.existing.is_empty());
    assert_eq!(same.pmi.existing_count, [21, 21]);
    let r = diff(&old, &new, &Map::new()).unwrap();
    assert_eq!(r.pmi.existing.len(), 1, "{:?}", r.pmi.existing);
    let c = &r.pmi.existing[0];
    assert_eq!(c.class, "changed");
    assert_eq!(
        (c.old.as_deref(), c.new.as_deref()),
        (Some("flatness 0.2"), Some("flatness 0.1"))
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
    // A new file without PMI: the old file's answers carry, to be written.
    let r = diff(&old, &bare, &Map::new()).unwrap();
    assert_eq!(r.pmi.answers_from, "stored");
    assert!(!r.pmi.new_stored);
    assert_eq!(r.answers["hole.function:holes:7"], "general");
    let md = r.markdown("Plate", None);
    assert!(
        md.contains("The comparison uses the requirements stored in the old file's Specify PMI."),
        "{md}"
    );
    assert!(md.contains("The new file has no Specify PMI yet"), "{md}");
    // A new file with its own: the difference from the answers carried.
    let new = written(
        "new",
        json!({ "part.material": "6061-T6", "hole.function:holes:7": "fit:H8" }),
    );
    let r = diff(&old, &new, &Map::new()).unwrap();
    assert!(r.pmi.new_stored);
    let changed: Vec<_> = r
        .pmi
        .stored
        .iter()
        .filter(|c| c.id == "hole.function:holes:7")
        .collect();
    assert_eq!(changed.len(), 1, "{:?}", r.pmi.stored);
    assert_eq!(changed[0].class, "changed");
    assert_eq!(
        (changed[0].old.as_deref(), changed[0].new.as_deref()),
        (Some("general"), Some("fit:H8"))
    );
    // Answers given win over the old file's.
    let given = answers(json!({ "part.material": "7075-T6" }));
    assert_eq!(diff(&old, &bare, &given).unwrap().pmi.answers_from, "given");
}

#[test]
fn a_real_part_s_report_names_what_a_person_changed() {
    // build123d's cast bearing unit: the flange 11 → 13 thick, the bolt holes Ø10 → Ø9.
    let r = revised(
        "bearing_flange",
        json!({
            "part.material": "EN AC-42100",
            "hole.function:hole_patterns.set:31.32": "clearance:M8",
            "datum.A": [17]
        }),
    );
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
    // The answer on the pair of holes is carried and listed to check, without the holes'
    // middles moving as they deepen.
    let check: Vec<&str> = r.check.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(check, ["hole.function:hole_patterns.set:31.32"]);
    assert!(
        md.contains("- pattern of 2 × Ø9 × 13 through holes depth 11 → 13; diameter 10 → 9, so its ⌀9 H11 requirement may no longer fit.\n"),
        "{md}"
    );
}

/// The "What changed" lines of a `complex_*` pair (`tests/fixtures/revisions/complex.py`): a
/// 100 × 80 × 10 plate with 4 × Ø6 holes, a pocket, two chamfers and a boss, changed in ways a
/// report must still put plainly.
fn complex(case: &str) -> String {
    revised(case, json!({})).markdown("Plate", None)
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
    assert!(md.contains("⚠️ 1 hole pattern changed."), "{md}");
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
    let r = revised("complex_fillet_all", json!({}));
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
    assert_eq!(
        md.matches("\n- ").count(),
        2,
        "the holes, and the question they ask: {md}"
    );
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
    let r = diff(&old, &new, &Map::new()).unwrap();
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
    diff(&old, &new, &Map::new()).unwrap()
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
        revised("boss_resized", json!({}))
            .features
            .iter()
            .any(|f| f.family == "bosses" && f.changes.contains_key("sizes.diameter"))
    );
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
    let r = revised("flat_translated", json!({}));
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
    let r = revised("plate_removed", json!({}));
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
    let r = diff(&old, &new, &Map::new()).unwrap();
    assert!(r.motion().is_some());
    assert_eq!(r.features.len(), 1, "{:?}", r.features);
    assert_eq!(
        r.change_lines()[0].0,
        "1.5 × 45° chamfer added: on the end of the Ø8 boss"
    );
}

#[test]
fn an_orphaned_feature_id_does_not_block_the_different_hole_reusing_it() {
    let (old, new) = pair("hole_removed");
    let given = answers(json!({"part.material":"6061-T6", "hole.function:holes:7":"fit:H7"}));
    let r = diff(&old, &new, &given).unwrap();
    assert_eq!(r.unresolved.len(), 1);
    assert_eq!(r.unresolved[0].id, "hole.function:holes:7");
    assert_eq!(r.unresolved[0].class, "orphaned");
    let analysis = specify_core::api::analyse(&new).unwrap();
    let questions = specify_core::api::questions(&analysis, &r.answers).unwrap();
    let reused = questions
        .iter()
        .find(|q| q.id == "hole.function:holes:7")
        .unwrap();
    assert!(
        reused.default.is_some(),
        "the new hole has its own suggestion"
    );
    assert!(
        r.answers["_specify"]["unresolved"]
            .get(&reused.id)
            .is_none()
    );
    let accepted = specify_core::rules::with_all_defaults(&analysis, &r.answers).unwrap();
    assert_eq!(accepted[&reused.id], *reused.default.as_ref().unwrap());
    let intent = specify_core::api::apply(&analysis, &r.answers, true).unwrap();
    let (bytes, _) = specify_core::writer::write_bytes(&new, &intent, Some(&r.answers)).unwrap();
    let back = specify_core::part::load_bytes("new.step", bytes).unwrap();
    assert!(
        !specify_core::resume::answers_for(&back)
            .unwrap()
            .unwrap()
            .contains_key(&reused.id)
    );
    // Files from the earlier implementation must not shadow a reused (or absent) feature ID.
    let mut earlier = r.answers.clone();
    earlier["_specify"]["unresolved"][&reused.id] = json!(r.unresolved[0].reason);
    earlier["_specify"]["unresolved"]["hole.function:holes:999"] = json!("removed hole");
    specify_core::api::apply(&analysis, &earlier, true).unwrap();
}
