use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, TcpListener};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use flate2::{Compression, write::GzEncoder};
use serde_json::json;
use tar::{EntryType, Header};

use super::*;
use crate::open::{API_VERSION, MANIFEST, resolve_assets};

const APP_VERSION: &str = "0.2.0";
const APP_URL: &str = "/pzfreo/specify-ui/releases/download/v0.2.0/specify-ui-app-0.2.0.tgz";
const ENGINE_URL: &str = concat!(
    "/pzfreo/specify-core/releases/download/v",
    env!("CARGO_PKG_VERSION"),
    "/specify-core-wasm-",
    env!("CARGO_PKG_VERSION"),
    ".tgz"
);
const DIFFERENT_SHA: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn pin(digest: &str) -> String {
    json!({
        "repository": "pzfreo/specify-ui", "tag": "v0.2.0", "asset": "specify-ui-app-0.2.0.tgz",
        "sha256": digest, "api": API_VERSION,
    })
    .to_string()
}

fn manifest(api: u32) -> String {
    json!({"name": "specify-ui-app", "version": APP_VERSION, "api": api}).to_string()
}

fn archive(files: &[(&str, EntryType, &[u8])]) -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for &(path, kind, bytes) in files {
        let mut header = Header::new_gnu();
        // Set the tar name directly so the test can model paths a well-behaved writer rejects.
        assert!(path.len() < 100);
        header.as_mut_bytes()[..path.len()].copy_from_slice(path.as_bytes());
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(kind);
        if kind.is_symlink() || kind.is_hard_link() {
            header.set_link_name("index.html").unwrap();
        }
        header.set_cksum();
        tar.append(&header, bytes).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

fn app_archive(api: Option<u32>) -> Vec<u8> {
    let mut files = vec![("index.html", EntryType::Regular, b"stub app".as_slice())];
    let manifest = api.map(manifest);
    if let Some(manifest) = &manifest {
        files.push((MANIFEST, EntryType::Regular, manifest.as_bytes()));
    }
    archive(&files)
}

fn engine_archive() -> Vec<u8> {
    archive(&[
        ("pkg/", EntryType::Directory, b""),
        (
            "pkg/specify_core_wasm.js",
            EntryType::Regular,
            b"export default 1;",
        ),
        (
            "pkg/specify_core_wasm_bg.wasm",
            EntryType::Regular,
            b"\0asm\x01\0\0\0",
        ),
    ])
}

fn serve(routes: Vec<(&str, &str, Vec<u8>)>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes: Vec<_> = routes
        .into_iter()
        .map(|(path, status, body)| (path.to_owned(), status.to_owned(), body))
        .collect();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        for (path, status, body) in routes {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("expected a request for {path}: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line, format!("GET {path} HTTP/1.1\r\n"));
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    (base, worker)
}

fn source<'a>(base: &'a str, pin: &'a str, cache: &'a Path) -> Source<'a> {
    Source {
        base,
        pin,
        cache: Some(cache),
    }
}

fn refused<T: std::fmt::Debug>(result: Result<T, Error>, message: &str) {
    let Error::Refused(error) = result.unwrap_err() else {
        panic!("expected a refusal");
    };
    assert!(error.contains(message), "{error}");
    assert!(
        error.contains("pass --app DIR / set SPECIFY_APP_DIR"),
        "{error}"
    );
}

#[test]
fn downloads_app_and_engine_once_and_uses_the_cache_offline() {
    let cache = tempfile::tempdir().unwrap();
    let app = app_archive(Some(API_VERSION));
    let app_digest = sha256(&app);
    let engine = engine_archive();
    let engine_digest = sha256(&engine);
    let checksum_path = format!("{ENGINE_URL}.sha256");
    let checksum_body = format!("{engine_digest}  specify-core-wasm-{CORE_VERSION}.tgz\n");
    let (base, server) = serve(vec![
        (APP_URL, "302 Found\r\nLocation: /redirected-app", vec![]),
        ("/redirected-app", "200 OK", app),
        (&checksum_path, "200 OK", checksum_body.into_bytes()),
        (ENGINE_URL, "200 OK", engine),
    ]);

    let pin = pin(&app_digest);
    let source = source(&base, &pin, cache.path());
    thread::scope(|scope| {
        let concurrent = scope.spawn(|| resolve_assets(None, None, &source).unwrap());
        let first = resolve_assets(None, None, &source).unwrap();
        let other = concurrent.join().unwrap();
        assert_eq!(first.app, other.app);
        assert_eq!(first.wasm, other.wasm);
    });
    server.join().unwrap();

    // The server has gone away. Only published, verified assets can satisfy this request.
    let second = resolve_assets(None, None, &source).unwrap();
    let cache = cache.path().canonicalize().unwrap();
    assert_eq!(second.app, cache.join("app").join(APP_VERSION));
    let wasm = second.wasm.unwrap();
    assert_eq!(wasm, cache.join("wasm").join(CORE_VERSION));
    assert_eq!(
        fs::read(second.app.join("index.html")).unwrap(),
        b"stub app"
    );
    assert_eq!(
        fs::read(wasm.join("specify_core_wasm.js")).unwrap(),
        b"export default 1;"
    );
    assert_eq!(
        fs::read_to_string(second.app.join(MARKER)).unwrap().trim(),
        app_digest
    );
    assert_eq!(
        fs::read_to_string(wasm.join(MARKER)).unwrap().trim(),
        engine_digest
    );
    assert!(!wasm.join("pkg").exists());
}

fn reject_app(archive: Vec<u8>, digest: &str, message: &str) {
    let cache = tempfile::tempdir().unwrap();
    let (base, server) = serve(vec![(APP_URL, "200 OK", archive)]);
    refused(source(&base, &pin(digest), cache.path()).app(), message);
    server.join().unwrap();
    let entries: Vec<_> = fs::read_dir(cache.path().join("app"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        entries,
        [std::ffi::OsString::from(format!("{APP_VERSION}.lock"))],
        "a partial app was left behind"
    );
}

#[test]
fn checks_app_sha256_before_unpacking() {
    reject_app(
        app_archive(Some(API_VERSION)),
        DIFFERENT_SHA,
        "SHA-256 checksum mismatch",
    );
}

#[test]
fn checks_engine_against_the_release_checksum() {
    let cache = tempfile::tempdir().unwrap();
    let checksum_path = format!("{ENGINE_URL}.sha256");
    let (base, server) = serve(vec![
        (&checksum_path, "200 OK", DIFFERENT_SHA.as_bytes().to_vec()),
        (ENGINE_URL, "200 OK", engine_archive()),
    ]);
    refused(
        source(&base, APP_LOCK, cache.path()).wasm(),
        "SHA-256 checksum mismatch",
    );
    server.join().unwrap();
    assert!(!cache.path().join("wasm").join(CORE_VERSION).exists());
}

#[test]
fn requires_the_app_manifest() {
    let bytes = app_archive(None);
    let digest = sha256(&bytes);
    reject_app(bytes, &digest, "cannot read app manifest");
}

#[test]
fn rejects_an_incompatible_app_api() {
    let bytes = app_archive(Some(API_VERSION + 1));
    let digest = sha256(&bytes);
    reject_app(
        bytes,
        &digest,
        &format!(
            "this app speaks session API {}, specify-core {CORE_VERSION} speaks {API_VERSION}: \
         update the app, or run without --app to use the pinned one",
            API_VERSION + 1
        ),
    );
}

#[test]
fn rejects_unsafe_paths_and_links() {
    for (path, kind, message) in [
        (
            "nested/../index.html",
            EntryType::Regular,
            "unsafe archive path",
        ),
        ("/index.html", EntryType::Regular, "unsafe archive path"),
        ("link", EntryType::Symlink, "unsupported archive entry"),
        ("link", EntryType::Link, "unsupported archive entry"),
    ] {
        let bytes = archive(&[(path, kind, b"")]);
        let digest = sha256(&bytes);
        reject_app(bytes, &digest, message);
    }
}

#[test]
fn raw_header_budget_includes_pax_and_gnu_extensions() {
    let names: Vec<_> = (0..=MAX_ARCHIVE_ENTRIES / 3)
        .map(|i| (format!("file{i}"), format!("file{i}\0")))
        .collect();
    let files: Vec<_> = names
        .iter()
        .flat_map(|(name, long_name)| {
            [
                ("pax", EntryType::XHeader, b"11 mtime=0\n".as_slice()),
                ("long-name", EntryType::GNULongName, long_name.as_bytes()),
                (name.as_str(), EntryType::Regular, b"".as_slice()),
            ]
        })
        .collect();
    let bytes = archive(&files);
    let digest = sha256(&bytes);
    reject_app(bytes, &digest, "too many entries in archive");
}

#[test]
fn archive_budget_counts_deep_implicit_directories() {
    let names: Vec<_> = (0..150)
        .map(|i| format!("{i}/{}file", "a/".repeat(30)))
        .collect();
    let files: Vec<_> = names
        .iter()
        .map(|name| (name.as_str(), EntryType::Regular, b"".as_slice()))
        .collect();
    let bytes = archive(&files);
    let digest = sha256(&bytes);
    reject_app(bytes, &digest, "too many entries in archive");
}

#[test]
fn release_redirect_policy_refuses_plain_http() {
    // ureq applies the same HTTPS-only check to the destination of each redirect.
    let target = "http://127.0.0.1:1/asset";
    let result = download_agent(GITHUB).get(target).call();
    assert!(matches!(result, Err(ureq::Error::RequireHttpsOnly(url)) if url == target));
}

#[test]
fn missing_manifest_and_extra_file_both_trigger_reinstallation() {
    let cache = tempfile::tempdir().unwrap();
    let bytes = app_archive(Some(API_VERSION));
    let digest = sha256(&bytes);
    let (base, server) = serve(vec![(APP_URL, "200 OK", bytes); 3]);
    let pin = pin(&digest);
    let source = source(&base, &pin, cache.path());
    let root = source.app().unwrap();
    fs::remove_file(root.join(MANIFEST)).unwrap();
    assert_eq!(source.app().unwrap(), root);
    assert!(root.join(MANIFEST).is_file());
    fs::write(root.join("extra.js"), "extra").unwrap();
    assert_eq!(source.app().unwrap(), root);
    assert!(!root.join("extra.js").exists());
    server.join().unwrap();
}

#[test]
fn reports_a_refused_connection() {
    let cache = tempfile::tempdir().unwrap();
    // Close the listener before downloading so the connection is refused.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    refused(
        source(&base, &pin(DIFFERENT_SHA), cache.path()).app(),
        "check your internet connection",
    );
}

#[test]
fn reports_an_http_failure() {
    let cache = tempfile::tempdir().unwrap();
    let (base, server) = serve(vec![(APP_URL, "503 Service Unavailable", vec![])]);
    refused(
        source(&base, &pin(DIFFERENT_SHA), cache.path()).app(),
        "HTTP 503",
    );
    server.join().unwrap();
}

#[test]
fn reports_an_unusable_cache() {
    let file = tempfile::NamedTempFile::new().unwrap();
    refused(
        source("", &pin(DIFFERENT_SHA), file.path()).app(),
        "cannot use the Specify app cache",
    );
}

#[test]
fn placeholder_refuses_without_touching_the_cache() {
    let pin = pin(UNPINNED);
    let cache = tempfile::tempdir().unwrap();
    refused(source("", &pin, cache.path()).app(), "no pinned app yet");
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
fn a_changed_pin_replaces_a_stale_app_and_preserves_its_bundled_engine() {
    let cache = tempfile::tempdir().unwrap();
    let dest = cache.path().join("app").join(APP_VERSION);
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("index.html"), "old app").unwrap();
    fs::write(dest.join(MARKER), DIFFERENT_SHA).unwrap();

    let bytes = archive(&[
        ("index.html", EntryType::Regular, b"new app"),
        (
            MANIFEST,
            EntryType::Regular,
            manifest(API_VERSION).as_bytes(),
        ),
        ("wasm/specify_core_wasm.js", EntryType::Regular, b"engine"),
    ]);
    let digest = sha256(&bytes);
    let (base, server) = serve(vec![(APP_URL, "200 OK", bytes)]);
    let pin = pin(&digest);
    let source = source(&base, &pin, cache.path());
    let assets = resolve_assets(None, None, &source).unwrap();
    server.join().unwrap();
    assert_eq!(assets.app, dest.canonicalize().unwrap());
    assert!(assets.wasm.is_none());
    assert_eq!(fs::read(dest.join("index.html")).unwrap(), b"new app");
    assert_eq!(
        fs::read_to_string(dest.join(MARKER)).unwrap().trim(),
        digest
    );
    assert!(!cache.path().join("wasm").exists());

    // An explicit engine still takes precedence over the app's copy.
    let engine = cache.path().join("engine");
    fs::create_dir(&engine).unwrap();
    let assets = resolve_assets(None, Some(engine.clone()), &source).unwrap();
    assert_eq!(assets.wasm, Some(engine.canonicalize().unwrap()));
}

#[test]
fn local_app_skips_downloads_and_checks_its_manifest_when_present() {
    let scratch = tempfile::tempdir().unwrap();
    let cache = scratch.path().join("must-stay-absent");
    let source = source("", "unused", &cache);
    let app = scratch.path().join("app");
    fs::create_dir(&app).unwrap();
    fs::write(app.join("index.html"), "local app").unwrap();
    let local = resolve_assets(Some(app.clone()), None, &source).unwrap();
    assert_eq!(local.app, app.canonicalize().unwrap());
    assert!(local.wasm.is_none());
    fs::write(app.join(MANIFEST), manifest(API_VERSION)).unwrap();
    let engine = scratch.path().join("pkg");
    fs::create_dir(&engine).unwrap();
    let local = resolve_assets(Some(app.clone()), Some(engine.clone()), &source).unwrap();
    assert_eq!(local.wasm, Some(engine.canonicalize().unwrap()));
    fs::write(app.join(MANIFEST), manifest(API_VERSION + 1)).unwrap();
    refused(
        resolve_assets(Some(app), Some(engine), &source),
        "this app speaks session API",
    );
    assert!(!cache.exists());
}
