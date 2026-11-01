//! Two revisions of an assembly compared part by part: gramil's, with its shaft and drive
//! plate changed (tests/fixtures/revisions/gramil_assembly.py).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Map, Value, json};
use specify_core::part::load_all;
use specify_core::revision::assembly;

fn fixture(which: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/fixtures/revisions/gramil_assembly.{which}.step.gz"
    ))
}

fn dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "specify-core-assembly-{name}-{}",
        std::process::id()
    ))
}

fn diff(out: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([fixture("old"), fixture("new")])
        .args(["--title", "gramil_assembly.step", "-o"])
        .arg(out)
        .args(extra)
        .output()
        .unwrap()
}

fn json_file(path: PathBuf) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_parts_pair_by_name_and_only_the_two_changed_parts_are_changed() {
    let (old, new) = (
        load_all(&fixture("old")).unwrap(),
        load_all(&fixture("new")).unwrap(),
    );
    let a = assembly::diff(&old, &new, &Map::new()).unwrap();
    assert_eq!(a.parts.len(), 12);
    let changed: Vec<&str> = a
        .parts
        .iter()
        .filter(|p| p.status == "changed")
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(changed, ["GRM-02 shaft", "GRM-04 drive plate"]);
    assert_eq!(
        a.parts.iter().filter(|p| p.status == "unchanged").count(),
        10
    );
    let retainer = a
        .parts
        .iter()
        .find(|p| p.name == "GRM-06 blade retainer")
        .unwrap();
    assert_eq!(
        (
            retainer.old.as_ref().unwrap().instances,
            retainer.new.as_ref().unwrap().instances
        ),
        (4, 4)
    );
    assert_eq!(a.verdict, "ok");
    assert_eq!(
        a.summary,
        "2 parts changed (GRM-02 shaft, GRM-04 drive plate), 10 unchanged."
    );
}

#[test]
fn the_command_line_writes_the_parts_table_each_changed_part_and_the_picture() {
    let out = dir("report");
    let viewer = "https://draftwright.io/diff?repo=o/r&base=b&head=h&path=a.step";
    let run = diff(&out, &["--viewer", viewer]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let printed: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(printed["verdict"], "ok");
    let md = std::fs::read_to_string(out.join("report.md")).unwrap();
    assert!(
        md.starts_with(
            "### gramil_assembly.step: Specify\n\n✅ 2 parts changed (GRM-02 shaft, GRM-04 drive \
             plate), 10 unchanged.\n\n![The assembly: changed, re-counted and moved parts in orange, \
             added in green, removed in red](model.png)\n\n| Part | Quantity | Change |\n|---|---|---|\n\
             | GRM-02 shaft | 1 | ✅ changed |\n| GRM-04 drive plate | 1 | ✅ changed |\n\n\
             10 other parts are unchanged.\n\n#### GRM-02 shaft\n\n"
        ),
        "{md}"
    );
    assert!(
        md.contains("#### GRM-04 drive plate\n\n✅ 1 hole changed."),
        "{md}"
    );
    assert!(
        md.contains(&format!(
            "[Turn GRM-04 drive plate round in 3D]({viewer}&part=GRM-04%20drive%20plate)"
        )),
        "{md}"
    );
    assert!(md.contains(": diameter 2.2 → 2.4\n"), "{md}");
    assert!(
        md.contains("#### GRM-02 shaft\n\n✅ 1 hole changed."),
        "{md}"
    );
    assert!(md.contains(": depth 2.5 → 3.5\n"), "{md}");
    assert_eq!(md.matches("](model.png)").count(), 1, "one picture");
    let png = std::fs::read(out.join("model.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let report = json_file(out.join("report.json"));
    assert_eq!(report["parts"].as_array().unwrap().len(), 12);
    assert_eq!(report["parts"][1]["report"]["verdict"], "ok");
    assert_eq!(json_file(out.join("answers.json")), json!({}));
}

#[test]
fn an_assembly_s_answers_are_given_and_carried_by_part_name() {
    let out = dir("answers");
    std::fs::create_dir_all(&out).unwrap();
    let given = out.join("in.json");
    let write = |v: Value| std::fs::write(&given, v.to_string()).unwrap();
    let path = given.to_str().unwrap();

    write(json!({"GRM-04 drive plate": {"part.material": "6061-T6"}}));
    let run = diff(&out, &["--answers", path]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        json_file(out.join("answers.json")),
        json!({"GRM-04 drive plate": {"part.material": "6061-T6"}})
    );

    write(json!({"nope": {}}));
    let run = diff(&out, &["--answers", path]);
    assert_eq!(run.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("invalid answer: ") && stderr.contains("has no part named \"nope\""),
        "{stderr}"
    );

    write(json!({"part.material": "6061-T6"}));
    let run = diff(&out, &["--answers", path]);
    assert_eq!(run.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&run.stderr).contains(
            "invalid answer: assembly requirements use {part name: {question id: value}}"
        )
    );
}

#[test]
fn one_part_of_an_assembly_is_still_compared_on_its_own() {
    let out = dir("part");
    let run = diff(&out, &["--part", "5"]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let md = std::fs::read_to_string(out.join("report.md")).unwrap();
    assert!(
        md.starts_with("### gramil_assembly.step: Specify\n\n✅ 1 hole changed."),
        "{md}"
    );
    assert!(!md.contains("| Part |"), "{md}");
    assert!(json_file(out.join("report.json")).get("parts").is_none());
}

fn part_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/parts/{name}"))
}

/// `parts/assembly.step` (a plate, and a pin placed twice) with *edit* made to its text.
fn variant(name: &str, edit: impl Fn(&str) -> String) -> PathBuf {
    let text = std::fs::read_to_string(part_fixture("assembly.step")).unwrap();
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("assembly-{name}.step"));
    std::fs::write(&path, edit(&text)).unwrap();
    path
}

fn compared(old: &Path, new: &Path) -> assembly::Assembly {
    assembly::diff(
        &load_all(old).unwrap(),
        &load_all(new).unwrap(),
        &Map::new(),
    )
    .unwrap()
}

fn statuses(a: &assembly::Assembly) -> Vec<(&str, &str)> {
    a.parts
        .iter()
        .map(|p| (p.name.as_str(), p.status))
        .collect()
}

#[test]
fn each_kind_of_part_change_is_named_and_counted() {
    let base = part_fixture("assembly.step");
    let same = compared(&base, &base);
    assert_eq!(
        statuses(&same),
        [("plate", "unchanged"), ("pin", "unchanged")]
    );
    assert_eq!(same.summary, "No part changed (2 unchanged).");

    let moved = variant("moved", |t| {
        t.replace(
            "#24 = CARTESIAN_POINT('',(30.,0.,5.));",
            "#24 = CARTESIAN_POINT('',(40.,0.,5.));",
        )
    });
    let a = compared(&base, &moved);
    assert_eq!(statuses(&a), [("plate", "unchanged"), ("pin", "moved")]);
    assert_eq!(a.summary, "1 part moved (pin), 1 unchanged.");
    assert_eq!(a.parts[1].gone.len(), 1, "one placement went");

    // The second pin's placement taken out.
    let fewer = variant("fewer", |t| {
        let i = t.find("#1267 = CONTEXT").unwrap();
        let j = i + t[i..].find("ENDSEC;").unwrap();
        format!("{}{}", &t[..i], &t[j..])
    });
    let a = compared(&base, &fewer);
    assert_eq!(statuses(&a), [("plate", "unchanged"), ("pin", "quantity")]);
    assert_eq!(a.summary, "1 part quantity changed (pin), 1 unchanged.");
    let pin = &a.parts[1];
    assert_eq!(
        (
            pin.old.as_ref().unwrap().instances,
            pin.new.as_ref().unwrap().instances
        ),
        (2, 1)
    );
    assert_eq!(pin.gone.len(), 1);

    let renamed = variant("renamed", |t| {
        t.replace(
            "PRODUCT('plate','plate'",
            "PRODUCT('base plate','base plate'",
        )
    });
    let a = compared(&base, &renamed);
    assert_eq!(
        statuses(&a),
        [
            ("base plate", "added"),
            ("pin", "unchanged"),
            ("plate", "removed")
        ]
    );
    assert_eq!(
        a.summary,
        "1 part added (base plate), 1 removed (plate), 1 unchanged."
    );

    // A single part against an assembly: its part is paired by name like any other.
    let a = compared(&part_fixture("plate.step"), &base);
    assert_eq!(
        statuses(&a),
        [
            ("plate", "added"),
            ("pin", "added"),
            ("COMPOUND", "removed")
        ]
    );
}

#[test]
fn a_part_turned_round_in_its_own_frame_is_moved_not_unchanged() {
    let fixture = |which: &str| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/revisions/plate_turned.{which}.step.gz"
        ))
    };
    let a = compared(&fixture("old"), &fixture("new"));
    assert_eq!(statuses(&a), [("plate", "moved"), ("block", "unchanged")]);
    assert_eq!(a.summary, "1 part moved (plate), 1 unchanged.");
}

#[test]
fn a_renamed_part_s_ghost_does_not_tint_the_part_in_its_place() {
    let base = part_fixture("assembly.step");
    let renamed = variant("renamed-picture", |t| {
        t.replace(
            "PRODUCT('plate','plate'",
            "PRODUCT('base plate','base plate'",
        )
    });
    let (old, new) = (load_all(&base).unwrap(), load_all(&renamed).unwrap());
    let a = assembly::diff(&old, &new, &Map::new()).unwrap();
    let png = a.picture(&old, &new, 300);
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let pixels: Vec<[i32; 3]> = buf[..info.buffer_size()]
        .chunks(3)
        .map(|p| [p[0] as i32, p[1] as i32, p[2] as i32])
        .collect();
    // The added plate green, the pins grey: nothing red, orange or brown.
    let green = pixels.iter().filter(|p| p[1] > p[0] + 60).count();
    let reddish = pixels.iter().filter(|p| p[0] > p[1] + 30).count();
    assert!(
        green > pixels.len() / 10,
        "{green} green of {}",
        pixels.len()
    );
    assert_eq!(reddish, 0);
}

#[test]
fn a_single_part_s_answers_are_its_part_s_when_it_becomes_an_assembly() {
    let out = dir("single-answers");
    std::fs::create_dir_all(&out).unwrap();
    let text = std::fs::read_to_string(part_fixture("plate.step")).unwrap();
    let old = out.join("plate.step");
    std::fs::write(
        &old,
        text.replace("PRODUCT('COMPOUND','COMPOUND'", "PRODUCT('plate','plate'"),
    )
    .unwrap();
    let given = out.join("in.json");
    std::fs::write(&given, json!({"part.material": "6061-T6"}).to_string()).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([&old, &part_fixture("assembly.step")])
        .arg("--answers")
        .arg(&given)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        json_file(out.join("answers.json")),
        json!({"plate": {"part.material": "6061-T6"}})
    );
}
