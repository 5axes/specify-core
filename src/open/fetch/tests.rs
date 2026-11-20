use std::io::{BufRead, BufReader};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use flate2::{Compression, write::GzEncoder};
use serde_json::json;
use tar::{EntryType, Header};

use super::*;
use crate::open::{API_VERSION, MANIFEST, resolve_assets};

const APP_VERSION: &str = CORE_VERSION;
const APP_URL: &str = concat!(
    "/pzfreo/specify-core/releases/download/v",
    env!("CARGO_PKG_VERSION"),
    "/specify-ui-app-",
    env!("CARGO_PKG_VERSION"),
    ".tgz"
);
const APP_CHECKSUM_URL: &str = concat!(
    "/pzfreo/specify-core/releases/download/v",
    env!("CARGO_PKG_VERSION"),
    "/specify-ui-app-",
    env!("CARGO_PKG_VERSION"),
    ".tgz.sha256"
);
const ENGINE_URL: &str = concat!(
    "/pzfreo/specify-core/releases/download/v",
    env!("CARGO_PKG_VERSION"),
    "/specify-core-wasm-",
    env!("CARGO_PKG_VERSION"),
    ".tgz"
);
const DIFFERENT_SHA: &str = "1111111111111111111111111111111111111111111111111111111111111111";

/// The release's `specify-ui-app-<version>.tgz.sha256`, as `sha256sum` writes it.
fn app_checksum(digest: &str) -> Vec<u8> {
    format!("{digest}  specify-ui-app-{APP_VERSION}.tgz\n").into_bytes()
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

/// Path to (status line and headers, body, how many times it must be served).
type Routes = BTreeMap<String, (String, Vec<u8>, usize)>;

/// One request the test server saw, with how it ended.
#[derive(Debug)]
struct Request {
    peer: String,
    line: String,
    outcome: Result<(), String>,
}

/// A loopback release server. Each connection is handled on its own thread and answered by path,
/// so no client's IO error can stop the others being served. `finish` shuts it down and checks
/// that every route was answered exactly as often as it was listed, with nothing else asked for.
struct Server {
    addr: SocketAddr,
    routes: Arc<Routes>,
    log: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

fn serve(routes: Vec<(&str, &str, Vec<u8>)>) -> (String, Server) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let mut table = Routes::new();
    for (path, status, body) in routes {
        let route = table
            .entry(path.to_owned())
            .or_insert_with(|| (status.to_owned(), body.clone(), 0));
        assert_eq!((route.0.as_str(), &route.1), (status, &body), "{path}");
        route.2 += 1;
    }
    let routes = Arc::new(table);
    let log = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let acceptor = thread::spawn({
        let (routes, log, stop) = (routes.clone(), log.clone(), stop.clone());
        move || {
            let mut handlers = Vec::new();
            for stream in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let (routes, log) = (routes.clone(), log.clone());
                handlers.push(thread::spawn(move || match stream {
                    Ok(stream) => respond(stream, &routes, &log),
                    Err(e) => log.lock().unwrap().push(Request {
                        peer: String::new(),
                        line: String::new(),
                        outcome: Err(format!("accept failed: {e}")),
                    }),
                }));
            }
            drop(listener);
            for handler in handlers {
                handler.join().unwrap();
            }
        }
    });
    let server = Server {
        addr,
        routes,
        log,
        stop,
        acceptor: Some(acceptor),
    };
    (format!("http://{addr}"), server)
}

/// Read one request and write the whole response in a single call. Partial writes let a client
/// that follows a redirect as soon as it sees `Location` close the socket under a later write.
fn respond(mut stream: TcpStream, routes: &Routes, log: &Mutex<Vec<Request>>) {
    let peer = stream
        .peer_addr()
        .map_or_else(|e| e.to_string(), |addr| addr.to_string());
    let mut line = String::new();
    let outcome = (|| {
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut reader = BufReader::new(&mut stream);
        reader.read_line(&mut line)?;
        let mut header = String::new();
        while reader.read_line(&mut header)? != 0 && header != "\r\n" {
            header.clear();
        }
        let route = line
            .strip_prefix("GET ")
            .and_then(|rest| rest.strip_suffix(" HTTP/1.1\r\n"))
            .and_then(|path| routes.get(path));
        let Some((status, body, _)) = route else {
            stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            return Err(io::Error::other("unexpected request"));
        };
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        stream.write_all(&response)?;
        stream.flush()
    })();
    log.lock().unwrap().push(Request {
        peer,
        line: line.trim_end().to_owned(),
        outcome: outcome.map_err(|e| e.to_string()),
    });
}

impl Server {
    /// Stops the server; `false` if its acceptor thread panicked.
    fn shutdown(&mut self) -> bool {
        let Some(acceptor) = self.acceptor.take() else {
            return true;
        };
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept so it sees the stop flag.
        let _ = TcpStream::connect(self.addr);
        acceptor.join().is_ok()
    }

    /// Stop the server, then require exactly the listed responses and no other requests.
    fn finish(mut self) {
        assert!(self.shutdown(), "the test server's acceptor panicked");
        let log = std::mem::take(&mut *self.log.lock().unwrap());
        let mut served = BTreeMap::<&str, usize>::new();
        for request in &log {
            let path = request
                .line
                .strip_prefix("GET ")
                .and_then(|rest| rest.strip_suffix(" HTTP/1.1"));
            match (path, &request.outcome) {
                (Some(path), Ok(())) if self.routes.contains_key(path) => {
                    *served.entry(path).or_default() += 1;
                }
                _ => panic!(
                    "unexpected or failed request from {}: {:?} ({:?}); all requests: {log:#?}",
                    request.peer, request.line, request.outcome
                ),
            }
        }
        let expected: BTreeMap<&str, usize> = self
            .routes
            .iter()
            .map(|(path, (_, _, count))| (path.as_str(), *count))
            .collect();
        assert_eq!(served, expected, "responses served; all requests: {log:#?}");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // A test that ends early is already failing; do not panic again while unwinding.
        self.shutdown();
    }
}

fn source<'a>(base: &'a str, cache: &'a Path) -> Source<'a> {
    Source {
        base,
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
        (APP_CHECKSUM_URL, "200 OK", app_checksum(&app_digest)),
        (APP_URL, "302 Found\r\nLocation: /redirected-app", vec![]),
        ("/redirected-app", "200 OK", app),
        (&checksum_path, "200 OK", checksum_body.into_bytes()),
        (ENGINE_URL, "200 OK", engine),
    ]);

    let source = source(&base, cache.path());
    thread::scope(|scope| {
        let concurrent = scope.spawn(|| resolve_assets(None, None, &source).unwrap());
        let first = resolve_assets(None, None, &source).unwrap();
        let other = concurrent.join().unwrap();
        assert_eq!(first.app, other.app);
        assert_eq!(first.wasm, other.wasm);
    });
    server.finish();

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
    let (base, server) = serve(vec![
        (APP_CHECKSUM_URL, "200 OK", app_checksum(digest)),
        (APP_URL, "200 OK", archive),
    ]);
    refused(source(&base, cache.path()).app(), message);
    server.finish();
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
fn checks_app_against_the_release_checksum_before_unpacking() {
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
        source(&base, cache.path()).wasm(),
        "SHA-256 checksum mismatch",
    );
    server.finish();
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
         update the app, or run without --app to use the released one",
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
    let mut routes = vec![(APP_CHECKSUM_URL, "200 OK", app_checksum(&digest)); 3];
    routes.extend(vec![(APP_URL, "200 OK", bytes); 3]);
    let (base, server) = serve(routes);
    let source = source(&base, cache.path());
    let root = source.app().unwrap();
    fs::remove_file(root.join(MANIFEST)).unwrap();
    assert_eq!(source.app().unwrap(), root);
    assert!(root.join(MANIFEST).is_file());
    fs::write(root.join("extra.js"), "extra").unwrap();
    assert_eq!(source.app().unwrap(), root);
    assert!(!root.join("extra.js").exists());
    server.finish();
}

#[test]
fn reports_a_refused_connection() {
    let cache = tempfile::tempdir().unwrap();
    // Close the listener before downloading so the connection is refused.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    refused(
        source(&base, cache.path()).app(),
        "check your internet connection",
    );
}

#[test]
fn a_dropped_download_is_refused_and_leaves_no_partial_app() {
    let cache = tempfile::tempdir().unwrap();
    let bytes = app_archive(Some(API_VERSION));
    let digest = sha256(&bytes);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    // The checksum in full; then the archive promises more bytes than are sent, and closes: the
    // connection drops mid-body.
    let server = thread::spawn(move || {
        // Fail rather than hang if the download never connects.
        listener.set_nonblocking(true).unwrap();
        for (path, body, length) in [
            (APP_CHECKSUM_URL, app_checksum(&digest), None),
            (APP_URL, bytes.clone(), Some(bytes.len() + 1000)),
        ] {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("the download never connected: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line, format!("GET {path} HTTP/1.1\r\n"));
            while reader.read_line(&mut line).unwrap() != 0 && line != "\r\n" {
                line.clear();
            }
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                length.unwrap_or(body.len())
            )
            .into_bytes();
            response.extend_from_slice(&body);
            stream.write_all(&response).unwrap();
        }
    });
    refused(
        source(&base, cache.path()).app(),
        "check your internet connection",
    );
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
fn reports_an_http_failure() {
    let cache = tempfile::tempdir().unwrap();
    let (base, server) = serve(vec![
        (APP_CHECKSUM_URL, "200 OK", app_checksum(DIFFERENT_SHA)),
        (APP_URL, "503 Service Unavailable", vec![]),
    ]);
    refused(source(&base, cache.path()).app(), "HTTP 503");
    server.finish();
}

#[test]
fn refuses_an_invalid_release_checksum_before_downloading_the_app() {
    let cache = tempfile::tempdir().unwrap();
    let (base, server) = serve(vec![(
        APP_CHECKSUM_URL,
        "200 OK",
        b"not a digest\n".to_vec(),
    )]);
    refused(
        source(&base, cache.path()).app(),
        "invalid SHA-256 checksum at",
    );
    server.finish();
    assert!(!cache.path().join("app").join(APP_VERSION).exists());
}

#[test]
fn reports_an_unusable_cache() {
    let file = tempfile::NamedTempFile::new().unwrap();
    refused(
        source("", file.path()).app(),
        "cannot use the Specify app cache",
    );
}

#[test]
fn a_stale_app_is_replaced_and_its_bundled_engine_preserved() {
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
    let (base, server) = serve(vec![
        (APP_CHECKSUM_URL, "200 OK", app_checksum(&digest)),
        (APP_URL, "200 OK", bytes),
    ]);
    let source = source(&base, cache.path());
    let assets = resolve_assets(None, None, &source).unwrap();
    server.finish();
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
    let source = source("", &cache);
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
