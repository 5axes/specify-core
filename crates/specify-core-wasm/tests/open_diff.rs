//! The page's `GET diff` (`specify-core open --diff`, docs/open.md) is exactly what the
//! browser's `diff()` gives for the same two files: one comparison, made natively.
#![cfg(not(target_arch = "wasm32"))]

use serde_json::Value;
use specify_core::open::{self, App, Options};
use specify_core::part;
use specify_core::session::{Opened, Session};
use specify_core_wasm::session;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn revision(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/revisions/{name}.step.gz"));
    part::File::read(&path).unwrap().bytes().to_vec()
}

/// `GET <path>` from the page server at `url`: the status and the JSON body.
fn get(url: &str, rel: &str) -> (u16, Value) {
    let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
    let (port, prefix) = rest.split_once('/').unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
    write!(
        stream,
        "GET /{prefix}{rel} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8(raw).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

/// Serves a diff session of `old` and `new` (written as plain STEP to a scratch directory) and
/// compares its `GET diff` with `diff()` of the same names and bytes.
fn same_as_the_browser(case: &str, old_name: &str, new_name: &str) {
    let dir = std::env::temp_dir().join(format!("specify-open-diff-{case}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("app")).unwrap();
    std::fs::write(dir.join("app/index.html"), b"<!doctype html>").unwrap();
    let (old, new) = (revision(old_name), revision(new_name));
    let (old_path, new_path): (PathBuf, PathBuf) = (dir.join("old.step"), dir.join("new.step"));
    std::fs::write(&old_path, &old).unwrap();
    std::fs::write(&new_path, &new).unwrap();
    let held = Session::diff(
        Opened::read(&old_path).unwrap(),
        Opened::read(&new_path).unwrap(),
        None,
        false,
    )
    .unwrap();
    let assets = open::assets(Some(&dir.join("app")), None).unwrap();
    let app = App::new(Arc::new(Mutex::new(held)), assets, &Options::default()).unwrap();
    let url = app.url();
    let served = std::thread::spawn(move || {
        app.serve(Duration::from_secs(60), Duration::from_millis(1500), |_| {})
            .unwrap()
    });
    let (status, served_diff) = get(&url, "diff");
    assert_eq!(status, 200);
    let browser = session::diff_with_options(
        ("old.step", &old),
        ("new.step", &new),
        "new.step",
        specify_core::revision::DiffOptions { scenes: true },
    )
    .unwrap();
    assert_eq!(served_diff, browser, "{case}");
    // With the PMI scenes, a part's at report.pmi.scenes and each compared part's in an assembly.
    let scenes = match served_diff.get("view") {
        Some(_) => vec![&served_diff["report"]["pmi"]["scenes"]],
        None => served_diff["parts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p.get("report").is_some())
            .map(|p| &p["report"]["pmi"]["scenes"])
            .collect(),
    };
    assert!(!scenes.is_empty(), "{case}");
    for s in scenes {
        assert!(s["old"].is_object() && s["new"].is_object(), "{case}: {s}");
    }
    served.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_page_s_diff_of_a_part_is_the_browser_s() {
    same_as_the_browser("part", "hole_moved.old", "hole_moved.new");
}

#[test]
fn the_page_s_diff_of_an_assembly_is_the_browser_s() {
    same_as_the_browser("assembly", "plate_turned.old", "plate_turned.new");
}
