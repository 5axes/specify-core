//! Compiles the Specify app and its WASM engine into the command (ADR P12): `open` and MCP
//! `show` serve them from the binary, with nothing downloaded at run time (src/open/bundle.rs).
//!
//! With the `bundled-app` feature, the app is the first of `ui/dist-app` (`npm run build:app`)
//! and `bundle/app` (the copy a published crate carries) that holds `index.html` and a
//! `specify-ui-app.json` naming this version; the engine is the first of
//! `crates/specify-core-wasm/pkg` (wasm-pack) and `bundle/wasm` that holds its glue and WASM.
//! Each file is stored gzipped. A build without them still compiles, with nothing built in, and
//! `open` then asks for `--app`; `SPECIFY_REQUIRE_BUNDLE=1` (the release jobs) makes that an error.
//! The `bundle-test-fixture` feature (cargo test) builds in the small fake app and engine of
//! tests/fixtures/bundle instead.

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;

const MANIFEST: &str = "specify-ui-app.json";
const ENGINE: [&str; 2] = ["specify_core_wasm.js", "specify_core_wasm_bg.wasm"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SPECIFY_REQUIRE_BUNDLE");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let feature = |name: &str| env::var_os(format!("CARGO_FEATURE_{name}")).is_some();
    let require = env::var_os("SPECIFY_REQUIRE_BUNDLE").is_some_and(|v| v == "1");

    let (app, wasm) = if feature("BUNDLE_TEST_FIXTURE") {
        let fixture = root.join("tests/fixtures/bundle");
        watch(&root, &fixture);
        let app = fixture.join("app");
        let wasm = fixture.join("wasm");
        (
            app.join("index.html").is_file().then_some(app),
            is_engine(&wasm).then_some(wasm),
        )
    } else if feature("BUNDLED_APP") {
        let app = ["ui/dist-app", "bundle/app"].map(|p| root.join(p));
        let wasm = ["crates/specify-core-wasm/pkg", "bundle/wasm"].map(|p| root.join(p));
        for dir in app.iter().chain(&wasm) {
            watch(&root, dir);
        }
        let app = app.into_iter().find(|dir| match app_version(dir) {
            Some(found) if found == version => true,
            Some(found) => {
                println!(
                    "cargo:warning={} is Specify app {found}, not {version}: not built in (rebuild it: npm run build:app in ui/)",
                    dir.display()
                );
                false
            }
            None => false,
        });
        (app, wasm.into_iter().find(|dir| is_engine(dir)))
    } else {
        (None, None)
    };

    if feature("BUNDLED_APP") && (app.is_none() || wasm.is_none()) {
        let missing = match (&app, &wasm) {
            (None, None) => "the Specify app and engine",
            (None, Some(_)) => "the Specify app",
            _ => "the Specify engine",
        };
        if require {
            panic!(
                "SPECIFY_REQUIRE_BUNDLE=1, but {missing} for {version} is not built: build the WASM \
                 (wasm-pack build crates/specify-core-wasm --target web --profile wasm-release) and \
                 the app (npm run build:app in ui/) first"
            );
        }
        if !feature("BUNDLE_TEST_FIXTURE") {
            println!(
                "cargo:warning=no {missing} to build in: `open` and MCP `show` will need --app/--wasm (see docs/open.md)"
            );
        }
    }

    let mut code = String::new();
    let app_files = app.map(|dir| files(&dir, None)).unwrap_or_default();
    let wasm_files = wasm
        .map(|dir| files(&dir, Some(&ENGINE)))
        .unwrap_or_default();
    for (name, dir, list) in [("APP", "app", &app_files), ("WASM", "wasm", &wasm_files)] {
        let store = out.join("bundle").join(dir);
        let _ = fs::remove_dir_all(&store);
        fs::create_dir_all(&store).unwrap();
        writeln!(code, "pub(super) static {name}: Files = &[").unwrap();
        for (index, (rel, path)) in list.iter().enumerate() {
            let gz = store.join(format!("{index}.gz"));
            let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
            encoder.write_all(&fs::read(path).unwrap()).unwrap();
            fs::write(&gz, encoder.finish().unwrap()).unwrap();
            writeln!(
                code,
                "    ({rel:?}, include_bytes!({:?})),",
                gz.to_str().expect("a UTF-8 OUT_DIR")
            )
            .unwrap();
        }
        code.push_str("];\n");
    }
    fs::write(out.join("bundle.rs"), code).unwrap();
}

/// Rerun when `dir` changes or, while it is missing, when its parent does (a missing path
/// would rerun the script on every build). The crate's root itself is never watched.
fn watch(root: &Path, dir: &Path) {
    let watched = if dir.exists() {
        Some(dir)
    } else {
        dir.parent().filter(|p| *p != root && p.exists())
    };
    if let Some(path) = watched {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// The version a built app's manifest names, if `dir` holds an app.
fn app_version(dir: &Path) -> Option<String> {
    if !dir.join("index.html").is_file() {
        return None;
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join(MANIFEST)).ok()?).ok()?;
    Some(manifest.get("version")?.as_str()?.to_string())
}

fn is_engine(dir: &Path) -> bool {
    ENGINE.iter().all(|name| dir.join(name).is_file())
}

/// The files under `dir` by their `/`-separated path, sorted (hidden files left out); only
/// `only`'s names when given.
fn files(dir: &Path, only: Option<&[&str]>) -> Vec<(String, PathBuf)> {
    if let Some(names) = only {
        return names
            .iter()
            .map(|name| (name.to_string(), dir.join(name)))
            .collect();
    }
    let mut found = Vec::new();
    let mut pending = vec![(String::new(), dir.to_path_buf())];
    while let Some((prefix, path)) = pending.pop() {
        for entry in fs::read_dir(&path).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().expect("UTF-8 file names");
            if name.starts_with('.') {
                continue;
            }
            let rel = format!("{prefix}{name}");
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                pending.push((format!("{rel}/"), entry.path()));
            } else {
                found.push((rel, entry.path()));
            }
        }
    }
    found.sort();
    found
}
