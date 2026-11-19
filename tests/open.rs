//! `specify-core open` (src/open.rs, src/session.rs, docs/open.md): the local Specify app,
//! against the built binary on a real loopback port with a stub app and engine, and
//! `--no-browser` so no browser opens. The app, engine, file and config are served under the
//! token at exactly `Host: 127.0.0.1:<port>`; the session's answers are read, replaced and
//! streamed; saves come only from the app's own origin, are written by the command from the
//! session's answers with `write`'s writer, and never overwrite (unless `--in-place`, which
//! keeps a `.bak`); `closed` stops the server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use specify_core::open::{self, App, Assets, Options};
use specify_core::session::{Opened as OpenedFile, Session};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thread-depth-alpha7.step.gz")
}

fn revision(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/revisions")
        .join(name)
}

/// The fixture's STEP text.
fn step_text() -> Vec<u8> {
    gunzip(&std::fs::read(fixture()).unwrap())
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut text = Vec::new();
    flate2::read::MultiGzDecoder::new(bytes)
        .read_to_end(&mut text)
        .unwrap();
    text
}

/// A scratch directory holding a stub app (`app/`), a stub engine (`pkg/`), a file outside
/// both (`secret.txt`) and a plain STEP input (`work/part.step`); removed when dropped.
struct Scratch {
    root: PathBuf,
    /// The same directory as first named, not resolved (on macOS `/var/…`, not
    /// `/private/var/…`).
    given: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Scratch {
        let root =
            std::env::temp_dir().join(format!("specify-core-open-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["app/assets", "pkg", "work"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let given = root.clone();
        let root = std::fs::canonicalize(root).unwrap();
        let write = |p: &str, b: &[u8]| std::fs::write(root.join(p), b).unwrap();
        write("app/index.html", b"<!doctype html><title>stub app</title>");
        write("app/app.css", b"body{}");
        write("app/assets/main.mjs", b"export {};");
        for ext in ["ico", "jpg", "jpeg", "webp", "woff", "ttf", "map", "woff2"] {
            write(&format!("app/assets/a.{ext}"), ext.as_bytes());
        }
        write("pkg/specify_core_wasm.js", b"export default 1;");
        write("pkg/specify_core_wasm_bg.wasm", b"\0asm\x01\0\0\0");
        write("secret.txt", b"secret");
        std::fs::write(root.join("work/part.step"), step_text()).unwrap();
        Scratch { root, given }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn input(&self) -> PathBuf {
        self.path("work/part.step")
    }

    /// The names in `work/`, sorted.
    fn work(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.path("work"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn assets(&self) -> Assets {
        open::assets(Some(&self.path("app")), Some(&self.path("pkg"))).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// `specify-core open <inputs> --no-browser` with the scratch app and engine.
fn command(inputs: &[&Path], scratch: &Scratch, extra: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_specify-core"));
    c.arg("open");
    if let [old, new] = inputs {
        c.arg("--diff").arg(old).arg(new);
    } else {
        c.args(inputs);
    }
    c.arg("--no-browser")
        .arg("--app")
        .arg(scratch.path("app"))
        .arg("--wasm")
        .arg(scratch.path("pkg"))
        .args(extra)
        .env_remove("SPECIFY_APP_DIR")
        .env_remove("SPECIFY_WASM_DIR");
    c
}

/// A running `specify-core open ... --no-browser`, killed if a test fails before it exits.
struct Opened {
    child: Child,
    stdout: BufReader<ChildStdout>,
    port: u16,
    /// `/<token>/`.
    prefix: String,
}

impl Opened {
    fn start(step: &Path, scratch: &Scratch, extra: &[&str]) -> Opened {
        Opened::run(&[step], scratch, extra)
    }

    /// `open --diff OLD NEW`.
    fn diff(old: &Path, new: &Path, scratch: &Scratch, extra: &[&str]) -> Opened {
        Opened::run(&[old, new], scratch, extra)
    }

    fn run(inputs: &[&Path], scratch: &Scratch, extra: &[&str]) -> Opened {
        let step = inputs.last().unwrap();
        let name = step.file_name().unwrap().to_string_lossy().into_owned();
        let mut child = command(inputs, scratch, extra)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let url = line
            .trim()
            .strip_prefix(&format!("Open {name}: "))
            .unwrap_or_else(|| panic!("the address line: {line:?}"));
        let rest = url.strip_prefix("http://127.0.0.1:").expect("on 127.0.0.1");
        let (port, prefix) = rest.split_once('/').unwrap();
        let prefix = format!("/{prefix}");
        let token = prefix.trim_matches('/');
        assert_eq!(token.len(), 32, "{url}");
        assert!(
            token.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "{token}"
        );
        assert!(prefix.ends_with('/'), "{url}");
        Opened {
            child,
            stdout,
            port: port.parse().unwrap(),
            prefix,
        }
    }

    fn page(&self) -> Page {
        Page {
            port: self.port,
            prefix: self.prefix.clone(),
        }
    }

    fn get(&self, rel: &str) -> Response {
        self.page().get(rel)
    }

    fn config(&self) -> Value {
        self.page().config()
    }

    /// A save from the app's own page.
    fn save(&self, rev: u64, approved: Value) -> Response {
        self.page().save(rev, approved)
    }

    /// The next line on stdout.
    fn line(&mut self) -> String {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        line
    }

    fn still_running(&mut self) -> bool {
        std::thread::sleep(Duration::from_millis(100));
        self.child.try_wait().unwrap().is_none()
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A page server's address, and the requests the app makes.
#[derive(Clone)]
struct Page {
    port: u16,
    /// `/<token>/`.
    prefix: String,
}

impl Page {
    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn path(&self, rel: &str) -> String {
        format!("{}{rel}", self.prefix)
    }

    fn get(&self, rel: &str) -> Response {
        request(
            self.port,
            "GET",
            &self.path(rel),
            &[("Host", &self.host())],
            b"",
        )
    }

    fn config(&self) -> Value {
        let got = self.get("config");
        assert_eq!(got.status, 200);
        assert_eq!(got.header("content-type"), Some("application/json"));
        got.json()
    }

    fn answers(&self) -> Value {
        let got = self.get("answers");
        assert_eq!(got.status, 200);
        got.json()
    }

    /// A JSON request from the app's own page.
    fn send(&self, method: &str, rel: &str, body: &Value) -> Response {
        let (host, origin) = (self.host(), self.origin());
        request(
            self.port,
            method,
            &self.path(rel),
            &[
                ("Host", &host),
                ("Origin", &origin),
                ("Content-Type", "application/json"),
            ],
            body.to_string().as_bytes(),
        )
    }

    fn put(&self, rev: u64, part: usize, answers: Value) -> Response {
        self.send(
            "PUT",
            "answers",
            &json!({"rev": rev, "part": part, "answers": answers}),
        )
    }

    /// A save from the page, approving `approved` (`{id: value}`) on part 0.
    fn save(&self, rev: u64, approved: Value) -> Response {
        self.send(
            "POST",
            "save",
            &json!({"rev": rev, "acceptDefaults":
            if approved.as_object().is_some_and(|a| a.is_empty()) {
                json!({})
            } else {
                json!({"0": approved})
            }}),
        )
    }

    fn events(&self) -> Events {
        Events::open(self)
    }
}

/// An open `events` stream, read event by event.
struct Events {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl Events {
    fn open(page: &Page) -> Events {
        let mut stream = TcpStream::connect(("127.0.0.1", page.port)).unwrap();
        write!(
            stream,
            "GET {} HTTP/1.1\r\nHost: {}\r\n\r\n",
            page.path("events"),
            page.host()
        )
        .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut events = Events {
            stream,
            buffer: Vec::new(),
        };
        let head = events.block();
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(
            head.to_ascii_lowercase()
                .contains("content-type: text/event-stream"),
            "{head}"
        );
        events
    }

    /// The next block up to a blank line.
    fn block(&mut self) -> String {
        loop {
            let text = String::from_utf8_lossy(&self.buffer).into_owned();
            let ends = ["\r\n\r\n", "\n\n"]
                .iter()
                .filter_map(|end| text.find(end).map(|i| (i, end.len())))
                .min();
            if let Some((i, len)) = ends {
                self.buffer.drain(..i + len);
                return text[..i].to_string();
            }
            let mut chunk = [0u8; 4096];
            let n = self.stream.read(&mut chunk).expect("an event in time");
            assert!(n > 0, "the stream ended");
            self.buffer.extend_from_slice(&chunk[..n]);
        }
    }

    /// The next event (comments skipped): its name and data.
    fn next(&mut self) -> (String, Value) {
        loop {
            let block = self.block();
            let mut name = String::new();
            let mut data = String::new();
            for line in block.lines() {
                if let Some(n) = line.strip_prefix("event: ") {
                    name = n.to_string();
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data.push_str(d);
                }
            }
            if !name.is_empty() {
                return (name, serde_json::from_str(&data).unwrap());
            }
        }
    }
}

/// A response: status code, headers (names lowercased) and body.
struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&self.body)))
    }
}

/// One raw HTTP/1.1 request to the port, with exactly these headers (and a Content-Length when
/// there is a body or the method is POST or PUT and none is given).
fn request(
    port: u16,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the port is open");
    let mut head = format!("{method} {target} HTTP/1.1\r\n");
    for (n, v) in headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    let has_length = headers
        .iter()
        .any(|(n, _)| n.eq_ignore_ascii_case("content-length"));
    if !has_length && (method == "POST" || method == "PUT" || !body.is_empty()) {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    // A refusal may answer before the body is read; the response is what is checked.
    let _ = stream.write_all(body);
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a head");
    let text = String::from_utf8(raw[..split].to_vec()).unwrap();
    let mut lines = text.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Response {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

/// A refused request: what it is, its target, its headers and the status it gets.
type Refused<'a> = (&'a str, String, Vec<(&'a str, &'a str)>, u16);

/// A refused save: what it is, its headers, its body and the status it gets.
type RefusedSave<'a> = (&'a str, Vec<(&'a str, &'a str)>, Vec<u8>, u16);

/// A session of the scratch input, as `specify-core open` makes it.
fn session_of(path: &Path) -> Arc<Mutex<Session>> {
    let session = Session::open(OpenedFile::read(path).unwrap(), false).unwrap();
    Arc::new(Mutex::new(session))
}

/// An [`App`] served on a thread with these delays, its notices collected.
struct Served {
    port: u16,
    /// `/<token>/`.
    prefix: String,
    notices: Arc<Mutex<Vec<String>>>,
    thread: std::thread::JoinHandle<()>,
}

impl Served {
    fn start(scratch: &Scratch, close_delay: Duration, idle: Duration) -> Served {
        let app = App::new(
            session_of(&scratch.input()),
            scratch.assets(),
            &Options::default(),
        )
        .unwrap();
        let port = app.port();
        let prefix = app
            .url()
            .strip_prefix(&format!("http://127.0.0.1:{port}"))
            .unwrap()
            .to_string();
        let notices = Arc::new(Mutex::new(Vec::new()));
        let told = notices.clone();
        let thread = std::thread::spawn(move || {
            app.serve(close_delay, idle, |n| {
                told.lock().unwrap().push(n.to_string())
            })
            .unwrap()
        });
        Served {
            port,
            prefix,
            notices,
            thread,
        }
    }

    fn page(&self) -> Page {
        Page {
            port: self.port,
            prefix: self.prefix.clone(),
        }
    }

    fn post(&self, rel: &str, origin: Option<&str>) -> Response {
        let host = self.page().host();
        let mut headers = vec![("Host", host.as_str())];
        if let Some(o) = origin {
            headers.push(("Origin", o));
        }
        request(
            self.port,
            "POST",
            &format!("{}{rel}", self.prefix),
            &headers,
            b"",
        )
    }

    /// `closed` from the app's own page.
    fn closed(&self) -> Response {
        let origin = format!("http://127.0.0.1:{}", self.port);
        self.post("closed", Some(&origin))
    }

    /// An open page's heartbeat.
    fn heartbeat(&self) {
        assert_eq!(self.page().get("config").status, 200);
    }

    /// Heartbeats every `every` for `during`; the server must still be serving after it.
    fn heartbeats(&self, every: Duration, during: Duration) {
        let end = Instant::now() + during;
        while Instant::now() < end {
            self.heartbeat();
            std::thread::sleep(every);
        }
        assert!(!self.thread.is_finished(), "the heartbeats kept it running");
    }

    /// Waits up to `within` for the server to stop: its notices.
    fn stopped(self, within: Duration) -> Vec<String> {
        let deadline = Instant::now() + within;
        while !self.thread.is_finished() {
            assert!(Instant::now() < deadline, "the server did not stop");
            std::thread::sleep(Duration::from_millis(20));
        }
        self.thread.join().unwrap();
        assert!(
            TcpStream::connect(("127.0.0.1", self.port)).is_err(),
            "the port is closed"
        );
        self.notices.lock().unwrap().clone()
    }
}

/// `analyse`'s JSON of `path` (part 0).
fn analysis_of(path: &Path) -> Value {
    specify_core::api::analyse_file(path, None).unwrap()
}

/// The required questions open given these answers.
fn open_ids(analysis: &Value, answers: &Map<String, Value>) -> Vec<String> {
    let qs = specify_core::api::questions(analysis, answers).unwrap();
    specify_core::rules::open_questions(&qs, answers)
        .iter()
        .map(|q| q.id.clone())
        .collect()
}

/// How many times `needle` occurs in the STEP text of `path` (gzipped or not).
fn occurrences(path: &Path, needle: &str) -> usize {
    let bytes = std::fs::read(path).unwrap();
    let text = if bytes.starts_with(&[0x1f, 0x8b]) {
        gunzip(&bytes)
    } else {
        bytes
    };
    String::from_utf8_lossy(&text).matches(needle).count()
}

fn is_step(path: &Path) -> bool {
    std::fs::read(path).unwrap().starts_with(b"ISO-10303-21;")
}

/// The open questions of part 0 that have a default, with it: what the page offers to approve.
fn open_defaults(analysis: &Value, answers: &Map<String, Value>) -> Map<String, Value> {
    let qs = specify_core::api::questions(analysis, answers).unwrap();
    specify_core::rules::open_questions(&qs, answers)
        .iter()
        .filter_map(|q| q.default.clone().map(|d| (q.id.clone(), d)))
        .collect()
}

/// Approves every default offered, round by round (taking one can ask another), each with the
/// value shown, as the page would: the last save's reply.
fn approve_all(page: &Page, analysis: &Value) -> Value {
    loop {
        let state = page.answers();
        let rev = state["rev"].as_u64().unwrap();
        let answers: Map<String, Value> =
            serde_json::from_value(state["parts"]["0"].clone()).unwrap_or_default();
        let offered = open_defaults(analysis, &answers);
        let got = page.save(rev, Value::Object(offered.clone()));
        assert_eq!(got.status, 200, "{}", String::from_utf8_lossy(&got.body));
        let got = got.json();
        let left = open_defaults(
            analysis,
            &serde_json::from_value(page.answers()["parts"]["0"].clone()).unwrap_or_default(),
        );
        if offered.is_empty() || left.is_empty() {
            return got;
        }
    }
}

/// A write report without where it was written.
fn report_without_output(report: &Value) -> Value {
    let mut r = report.as_object().unwrap().clone();
    r.remove("output");
    Value::Object(r)
}

#[test]
fn the_app_engine_config_and_file_are_served_under_the_token() {
    let scratch = Scratch::new("served");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);

    let index = opened.get("");
    assert_eq!(index.status, 200);
    assert_eq!(index.body, b"<!doctype html><title>stub app</title>");
    assert_eq!(
        index.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(index.header("cache-control"), Some("no-store"));
    assert_eq!(index.header("x-content-type-options"), Some("nosniff"));
    let csp = index.header("content-security-policy").unwrap();
    assert!(csp.contains("default-src 'self'"), "{csp}");
    assert!(csp.contains("'wasm-unsafe-eval'"), "{csp}");
    assert!(csp.contains("font-src 'self' data:"), "{csp}");
    assert_eq!(opened.get("index.html").body, index.body);

    for (rel, content_type, body) in [
        ("app.css", "text/css; charset=utf-8", &b"body{}"[..]),
        (
            "assets/main.mjs",
            "text/javascript; charset=utf-8",
            b"export {};",
        ),
        (
            "wasm/specify_core_wasm.js",
            "text/javascript; charset=utf-8",
            b"export default 1;",
        ),
        (
            "wasm/specify_core_wasm_bg.wasm",
            "application/wasm",
            b"\0asm\x01\0\0\0",
        ),
        ("assets/a.ico", "image/x-icon", b"ico"),
        ("assets/a.jpg", "image/jpeg", b"jpg"),
        ("assets/a.jpeg", "image/jpeg", b"jpeg"),
        ("assets/a.webp", "image/webp", b"webp"),
        ("assets/a.woff", "font/woff", b"woff"),
        ("assets/a.woff2", "font/woff2", b"woff2"),
        ("assets/a.ttf", "font/ttf", b"ttf"),
        ("assets/a.map", "application/json", b"map"),
    ] {
        let got = opened.get(rel);
        assert_eq!(got.status, 200, "{rel}");
        assert_eq!(got.header("content-type"), Some(content_type), "{rel}");
        assert_eq!(got.body, body, "{rel}");
    }

    // The token without its slash is sent to the app's address.
    let page = opened.page();
    let bare = request(
        opened.port,
        "GET",
        opened.prefix.trim_end_matches('/'),
        &[("Host", &page.host())],
        b"",
    );
    assert_eq!(bare.status, 308);
    assert_eq!(bare.header("location"), Some(opened.prefix.as_str()));

    let file = opened.get("file");
    assert_eq!(file.status, 200);
    assert_eq!(file.body, std::fs::read(scratch.input()).unwrap());
    assert_eq!(
        file.header("content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(opened.get("file?rev=new").body, file.body);
    // Not a diff: no OLD and no report.
    assert_eq!(opened.get("file?rev=old").status, 404);
    assert_eq!(opened.get("diff").status, 404);

    let config = opened.config();
    assert_eq!(config["api"], 1);
    assert_eq!(config["mode"], "specify");
    assert_eq!(config["name"], "part.step");
    assert_eq!(config["path"], scratch.input().display().to_string());
    assert_eq!(config["part"], Value::Null);
    assert_eq!(config["oldName"], Value::Null);
    assert_eq!(config["oldPath"], Value::Null);
    assert_eq!(config["readOnly"], false);
    assert_eq!(config["wasmUrl"], "wasm/specify_core_wasm.js");
    assert_eq!(config["specifyCore"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        config["saveTarget"],
        scratch
            .path("work/part.specified.step")
            .display()
            .to_string()
    );
    let keys: Vec<&String> = config.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "api",
            "mode",
            "name",
            "path",
            "part",
            "oldName",
            "oldPath",
            "readOnly",
            "wasmUrl",
            "saveTarget",
            "specifyCore"
        ]
    );
    // The fixture is a file Specify wrote: the session starts from the answers it stores, as
    // `analyse` resumes them.
    let resumed = analysis_of(&scratch.input())["resume"]["answers"].clone();
    assert!(resumed.as_object().is_some_and(|a| !a.is_empty()));
    assert_eq!(
        opened.page().answers(),
        json!({"rev": 0, "parts": {"0": resumed}})
    );
}

#[test]
fn other_hosts_tokens_origins_and_paths_outside_the_app_are_refused() {
    let scratch = Scratch::new("refused");
    #[cfg(unix)]
    std::os::unix::fs::symlink(scratch.path("secret.txt"), scratch.path("app/link.txt")).unwrap();
    let mut opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let host = page.host();
    let other_port = format!("127.0.0.1:{}", opened.port.wrapping_add(1));
    let localhost = format!("localhost:{}", opened.port);
    let index = page.path("");
    let other_token = format!("/{}/", "0".repeat(32));
    let other_token_file = format!("/{}/file", "0".repeat(32));
    let cases: Vec<Refused> = vec![
        (
            "host localhost",
            index.clone(),
            vec![("Host", &localhost)],
            403,
        ),
        (
            "host on another port",
            index.clone(),
            vec![("Host", &other_port)],
            403,
        ),
        ("no host", index.clone(), vec![], 403),
        (
            "rebinding host",
            index.clone(),
            vec![("Host", "attacker.example:80")],
            403,
        ),
        (
            "another origin's page",
            page.path("file"),
            vec![("Host", &host), ("Origin", "https://example.com")],
            403,
        ),
        (
            "another origin reading the answers",
            page.path("answers"),
            vec![("Host", &host), ("Origin", "https://example.com")],
            403,
        ),
        (
            "another origin's event stream",
            page.path("events"),
            vec![("Host", &host), ("Origin", "https://example.com")],
            403,
        ),
        ("wrong token", other_token, vec![("Host", &host)], 404),
        (
            "wrong token's file",
            other_token_file,
            vec![("Host", &host)],
            404,
        ),
        ("the root", "/".to_string(), vec![("Host", &host)], 404),
        (
            "dot-dot",
            page.path("../secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "encoded dot-dot",
            page.path("%2e%2e/secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "encoded slash",
            page.path("assets%2f..%2f..%2fsecret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "out of the engine",
            page.path("wasm/../../secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "backslash",
            page.path("assets%5c..%5c..%5csecret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "a directory",
            page.path("assets/"),
            vec![("Host", &host)],
            404,
        ),
        (
            "a directory without its slash",
            page.path("assets"),
            vec![("Host", &host)],
            404,
        ),
        (
            "the engine directory without its slash",
            page.path("wasm"),
            vec![("Host", &host)],
            404,
        ),
        (
            "the token and more",
            format!("{}x/", opened.prefix.trim_end_matches('/')),
            vec![("Host", &host)],
            404,
        ),
        (
            "the token cut short",
            format!("{}/", &opened.prefix[..opened.prefix.len() - 2]),
            vec![("Host", &host)],
            404,
        ),
        (
            "the token cut short, no slash",
            opened.prefix[..opened.prefix.len() - 2].to_string(),
            vec![("Host", &host)],
            404,
        ),
        (
            "a missing file",
            page.path("missing.js"),
            vec![("Host", &host)],
            404,
        ),
        #[cfg(unix)]
        (
            "a link leading out",
            page.path("link.txt"),
            vec![("Host", &host)],
            404,
        ),
    ];
    for (what, target, headers, status) in cases {
        let got = request(opened.port, "GET", &target, &headers, b"");
        assert_eq!(got.status, status, "{what}");
        assert!(got.body.is_empty(), "{what}: nothing served");
    }
    let put = request(
        opened.port,
        "PUT",
        &page.path("file"),
        &[("Host", &host)],
        b"",
    );
    assert_eq!(put.status, 405);
    let post = request(
        opened.port,
        "POST",
        &page.path("answers"),
        &[("Host", &host)],
        b"",
    );
    assert_eq!(post.status, 405);
    assert_eq!(post.header("allow"), Some("GET, PUT"));
    assert!(opened.still_running());
}

#[test]
fn saves_take_the_next_free_name_and_never_overwrite() {
    let scratch = Scratch::new("save");
    let input = std::fs::read(scratch.input()).unwrap();
    let mut opened = Opened::start(&scratch.input(), &scratch, &[]);
    let work = |n: &str| scratch.path("work").join(n);

    let first = opened.save(0, json!({}));
    assert_eq!(
        first.status,
        200,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let first = first.json();
    assert_eq!(
        first["path"],
        work("part.specified.step").display().to_string()
    );
    assert!(is_step(&work("part.specified.step")));
    assert_eq!(
        first["bytes"],
        std::fs::metadata(work("part.specified.step"))
            .unwrap()
            .len()
    );
    assert_eq!(first["report"]["output"], first["path"]);
    assert_eq!(
        opened.line(),
        format!(
            "Saved {} ({} bytes).\n",
            first["path"].as_str().unwrap(),
            first["bytes"]
        )
    );
    assert_eq!(
        opened.config()["saveTarget"],
        work("part.specified-2.step").display().to_string()
    );

    let second = opened.save(0, json!({}));
    assert_eq!(second.status, 200);
    assert_eq!(
        second.json()["path"],
        work("part.specified-2.step").display().to_string()
    );
    // A file already at the next name is left alone.
    std::fs::write(work("part.specified-3.step"), b"mine").unwrap();
    let third = opened.save(0, json!({}));
    assert_eq!(third.status, 200);
    assert_eq!(
        third.json()["path"],
        work("part.specified-4.step").display().to_string()
    );
    assert_eq!(
        std::fs::read(work("part.specified-3.step")).unwrap(),
        b"mine"
    );
    assert_eq!(std::fs::read(scratch.input()).unwrap(), input);
    assert_eq!(
        scratch.work(),
        [
            "part.specified-2.step",
            "part.specified-3.step",
            "part.specified-4.step",
            "part.specified.step",
            "part.step"
        ],
        "no temporary file left"
    );
    assert!(opened.still_running());
}

#[test]
fn a_save_from_elsewhere_of_another_type_without_defaults_said_or_too_large_is_refused() {
    let scratch = Scratch::new("save-refused");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let (host, origin) = (page.host(), page.origin());
    let localhost_origin = format!("http://localhost:{}", opened.port);
    let target = page.path("save");
    let json_type = ("Content-Type", "application/json");
    let body = br#"{"rev": 0, "acceptDefaults": {}}"#.to_vec();
    let over = (64 * 1024 + 1).to_string();
    let cases: Vec<RefusedSave> = vec![
        (
            "no origin",
            vec![("Host", &host), json_type],
            body.clone(),
            403,
        ),
        (
            "another origin",
            vec![
                ("Host", &host),
                ("Origin", "https://example.com"),
                json_type,
            ],
            body.clone(),
            403,
        ),
        (
            "localhost origin",
            vec![("Host", &host), ("Origin", &localhost_origin), json_type],
            body.clone(),
            403,
        ),
        (
            "null origin",
            vec![("Host", &host), ("Origin", "null"), json_type],
            body.clone(),
            403,
        ),
        (
            "STEP bytes, as before",
            vec![
                ("Host", &host),
                ("Origin", &origin),
                ("Content-Type", "application/octet-stream"),
            ],
            step_text(),
            415,
        ),
        (
            "no content type",
            vec![("Host", &host), ("Origin", &origin)],
            body.clone(),
            415,
        ),
        (
            "chunked",
            vec![
                ("Host", &host),
                ("Origin", &origin),
                json_type,
                ("Transfer-Encoding", "chunked"),
            ],
            body.clone(),
            411,
        ),
        (
            "not JSON",
            vec![("Host", &host), ("Origin", &origin), json_type],
            b"<html>not a save</html>".to_vec(),
            400,
        ),
        (
            "acceptDefaults absent",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0}"#.to_vec(),
            400,
        ),
        (
            "acceptDefaults a bool, as before",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0, "acceptDefaults": true}"#.to_vec(),
            400,
        ),
        (
            "ids listed without their values, as before",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0, "acceptDefaults": {"0": ["part.material"]}}"#.to_vec(),
            400,
        ),
        (
            "acceptDefaults a list, not by part",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0, "acceptDefaults": []}"#.to_vec(),
            400,
        ),
        (
            "acceptDefaults not by part",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"rev": 0, "acceptDefaults": "yes"}"#.to_vec(),
            400,
        ),
        (
            "rev absent",
            vec![("Host", &host), ("Origin", &origin), json_type],
            br#"{"acceptDefaults": {}}"#.to_vec(),
            400,
        ),
        (
            "over 64 KB",
            vec![
                ("Host", &host),
                ("Origin", &origin),
                json_type,
                ("Content-Length", &over),
            ],
            Vec::new(),
            413,
        ),
    ];
    for (what, headers, body, status) in cases {
        let got = request(opened.port, "POST", &target, &headers, &body);
        assert_eq!(got.status, status, "{what}");
        assert!(got.json()["error"].is_string(), "{what}");
    }
    // An id that is not an open question with a default is named, and nothing is written.
    let unknown = page.save(0, json!({"no.such.question": "x"}));
    assert_eq!(unknown.status, 400);
    assert_eq!(unknown.json()["question"], "no.such.question");
    assert_eq!(unknown.json()["part"], 0);
    assert!(unknown.json()["error"].is_string());
    assert_eq!(page.answers()["rev"], 0, "nothing approved");
    // A rev the session has moved on from.
    assert_eq!(
        page.put(0, 0, json!({"part.material": "6061-T6"})).status,
        200
    );
    let stale = page.save(0, json!({}));
    assert_eq!(stale.status, 409);
    assert_eq!(stale.json()["rev"], 1);
    assert!(stale.json()["error"].is_string());
    let get = request(opened.port, "GET", &target, &[("Host", &host)], b"");
    assert_eq!(get.status, 405);
    assert_eq!(scratch.work(), ["part.step"], "nothing written");
}

#[test]
fn read_only_refuses_saves_and_answer_changes_and_writes_nothing() {
    let scratch = Scratch::new("read-only");
    let opened = Opened::start(&scratch.input(), &scratch, &["--read-only"]);
    let config = opened.config();
    assert_eq!(config["mode"], "read-only");
    assert_eq!(config["readOnly"], true);
    assert_eq!(config["saveTarget"], Value::Null);
    let got = opened.save(0, json!({}));
    assert_eq!(got.status, 409);
    assert!(got.json()["error"].is_string());
    let put = opened.page().put(0, 0, json!({"part.material": "6061-T6"}));
    assert_eq!(put.status, 409);
    assert!(put.json()["error"].is_string());
    assert_eq!(opened.page().answers()["rev"], 0);
    assert_eq!(scratch.work(), ["part.step"]);
}

/// `file?rev=saved`: 404 before a save, then exactly the bytes the last save wrote (read from
/// disk, its path in `X-Specify-Path`), following each new copy; a copy changed on disk is 409
/// and one removed 404, never other bytes.
#[test]
fn rev_saved_serves_what_the_last_save_wrote() {
    let scratch = Scratch::new("rev-saved");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let work = |n: &str| scratch.path("work").join(n);
    let before = opened.get("file?rev=saved");
    assert_eq!(before.status, 404);
    assert!(before.json()["error"].is_string());

    let saved = |expected: &Path| {
        let got = opened.get("file?rev=saved");
        assert_eq!(got.status, 200, "{}", String::from_utf8_lossy(&got.body));
        assert_eq!(got.header("content-type"), Some("application/octet-stream"));
        assert_eq!(
            got.header("x-specify-path"),
            Some(expected.display().to_string().as_str())
        );
        assert_eq!(got.body, std::fs::read(expected).unwrap());
        got.body
    };

    let first = opened.save(0, json!({}));
    assert_eq!(first.status, 200);
    assert_eq!(
        first.json()["path"],
        work("part.specified.step").display().to_string()
    );
    let first_bytes = saved(&work("part.specified.step"));
    assert!(first_bytes.starts_with(b"ISO-10303-21;"));
    // The same rules as every GET: another page's Origin, or another Host, is refused.
    let page = opened.page();
    let (host, path) = (page.host(), page.path("file?rev=saved"));
    let foreign = request(
        page.port,
        "GET",
        &path,
        &[("Host", &host), ("Origin", "http://evil.example")],
        b"",
    );
    assert_eq!(foreign.status, 403);
    assert!(foreign.body.is_empty());
    let rebound = request(page.port, "GET", &path, &[("Host", "evil.example")], b"");
    assert_eq!(rebound.status, 403);
    // `file` is still the input as opened.
    assert_eq!(
        opened.get("file").body,
        std::fs::read(scratch.input()).unwrap()
    );

    // A different save to the next name: `rev=saved` follows it.
    assert_eq!(
        opened
            .page()
            .put(0, 0, json!({"part.material": "6061-T6"}))
            .status,
        200
    );
    let second = opened.save(1, json!({}));
    assert_eq!(second.status, 200);
    assert_eq!(
        second.json()["path"],
        work("part.specified-2.step").display().to_string()
    );
    let second_bytes = saved(&work("part.specified-2.step"));
    assert_ne!(
        second_bytes, first_bytes,
        "the second save wrote the new answer"
    );

    // Changed on disk since the save: refused, never the other bytes.
    let mut changed = second_bytes.clone();
    changed.extend_from_slice(b"/* edited */\n");
    std::fs::write(work("part.specified-2.step"), &changed).unwrap();
    let got = opened.get("file?rev=saved");
    assert_eq!(got.status, 409);
    assert_eq!(got.header("content-type"), Some("application/json"));
    let error = got.json();
    assert!(
        error["error"].as_str().unwrap().contains("changed"),
        "{error}"
    );
    assert_eq!(
        error["path"],
        work("part.specified-2.step").display().to_string()
    );
    // Put back as saved, it is served again; removed, it is 404.
    std::fs::write(work("part.specified-2.step"), &second_bytes).unwrap();
    saved(&work("part.specified-2.step"));
    std::fs::remove_file(work("part.specified-2.step")).unwrap();
    let got = opened.get("file?rev=saved");
    assert_eq!(got.status, 404);
    assert!(
        got.json()["error"]
            .as_str()
            .unwrap()
            .contains("no longer exists")
    );
}

/// `file?rev=saved` after an in-place save is the input as that save left it.
#[test]
fn rev_saved_after_an_in_place_save_is_the_input() {
    let scratch = Scratch::new("rev-saved-in-place");
    let input = std::fs::read(scratch.input()).unwrap();
    let opened = Opened::start(&scratch.input(), &scratch, &["--in-place"]);
    assert_eq!(opened.get("file?rev=saved").status, 404);
    assert_eq!(
        opened
            .page()
            .put(0, 0, json!({"part.material": "6061-T6"}))
            .status,
        200
    );
    let got = opened.save(1, json!({}));
    assert_eq!(got.status, 200);
    assert_eq!(got.json()["path"], scratch.input().display().to_string());
    let now = std::fs::read(scratch.input()).unwrap();
    assert_ne!(now, input);
    let got = opened.get("file?rev=saved");
    assert_eq!(got.status, 200);
    assert_eq!(
        got.header("x-specify-path"),
        Some(scratch.input().display().to_string().as_str())
    );
    assert_eq!(got.body, now);
    assert_eq!(opened.get("file").body, now);
}

/// `--read-only` never saves: `file?rev=saved` is always 404.
#[test]
fn rev_saved_read_only_is_not_found() {
    let scratch = Scratch::new("rev-saved-read-only");
    let opened = Opened::start(&scratch.input(), &scratch, &["--read-only"]);
    assert_eq!(opened.get("file?rev=saved").status, 404);
    assert_eq!(opened.save(0, json!({})).status, 409);
    assert_eq!(opened.get("file?rev=saved").status, 404);
}

/// A save made outside the page (MCP's `save`, on the same session) is what `rev=saved` serves.
#[test]
fn rev_saved_follows_a_save_from_the_agent() {
    let scratch = Scratch::new("rev-saved-agent");
    let session = session_of(&scratch.input());
    let app = App::new(Arc::clone(&session), scratch.assets(), &Options::default()).unwrap();
    let port = app.port();
    let prefix = app
        .url()
        .strip_prefix(&format!("http://127.0.0.1:{port}"))
        .unwrap()
        .to_string();
    std::thread::spawn(move || {
        let _ = app.serve(Duration::from_secs(60), Duration::from_secs(60), |_| {});
    });
    let page = Page { port, prefix };
    assert_eq!(page.get("file?rev=saved").status, 404);
    let saved = specify_core::session::save(&session, None, &Default::default(), "mcp").unwrap();
    let written = scratch.path("work/part.specified.step");
    assert_eq!(saved.path, written.display().to_string());
    let got = page.get("file?rev=saved");
    assert_eq!(got.status, 200);
    assert_eq!(
        got.header("x-specify-path"),
        Some(written.display().to_string().as_str())
    );
    assert_eq!(got.body, std::fs::read(&written).unwrap());
}

/// The session's answers: read, replaced at the current rev only, and streamed to every open
/// page, at most four streams at once.
#[test]
fn answers_are_replaced_at_the_current_rev_and_streamed() {
    let scratch = Scratch::new("answers");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let mut events = page.events();
    let (name, start) = events.next();
    assert_eq!(name, "answers");
    let resumed = analysis_of(&scratch.input())["resume"]["answers"].clone();
    assert_eq!(
        start,
        json!({"rev": 0, "parts": {"0": resumed}, "source": "start"})
    );

    let put = page.put(0, 0, json!({"part.material": "6061-T6"}));
    assert_eq!(put.status, 200);
    assert_eq!(put.json(), json!({"rev": 1}));
    let (name, change) = events.next();
    assert_eq!(name, "answers");
    assert_eq!(
        change,
        json!({"rev": 1, "part": 0, "answers": {"part.material": "6061-T6"}, "source": "page"})
    );
    assert_eq!(
        page.answers(),
        json!({"rev": 1, "parts": {"0": {"part.material": "6061-T6"}}})
    );

    // Another change on rev 0: refused with the current state, nothing changed.
    let stale = page.put(0, 0, json!({"part.material": "6082-T6"}));
    assert_eq!(stale.status, 409);
    let stale = stale.json();
    assert_eq!(stale["rev"], 1);
    assert_eq!(stale["parts"], json!({"0": {"part.material": "6061-T6"}}));
    assert!(stale["error"].is_string());
    // Not an answer map, no part, no such part, another origin.
    for body in [
        json!({"rev": 1, "part": 0, "answers": ["part.material"]}),
        json!({"rev": 1, "answers": {}}),
        json!({"rev": 1, "part": 7, "answers": {}}),
    ] {
        let got = page.send("PUT", "answers", &body);
        assert_eq!(got.status, 400, "{body}");
        assert!(got.json()["error"].is_string());
    }
    let elsewhere = request(
        opened.port,
        "PUT",
        &page.path("answers"),
        &[
            ("Host", &page.host()),
            ("Origin", "https://example.com"),
            ("Content-Type", "application/json"),
        ],
        br#"{"rev": 1, "part": 0, "answers": {}}"#,
    );
    assert_eq!(elsewhere.status, 403);
    assert_eq!(page.answers()["rev"], 1);

    // Clearing is a change too; every page gets it.
    let mut second = page.events();
    assert_eq!(second.next().1["rev"], 1);
    assert_eq!(page.put(1, 0, json!({})).json(), json!({"rev": 2}));
    for stream in [&mut events, &mut second] {
        let (_, change) = stream.next();
        assert_eq!(change["rev"], 2);
        assert_eq!(change["answers"], json!({}));
    }

    // Four streams at once: a fifth is refused until one closes.
    let _third = page.events();
    let _fourth = page.events();
    let fifth = page.get("events");
    assert_eq!(fifth.status, 503);
    assert_eq!(fifth.header("retry-after"), Some("1"));
    assert!(fifth.json()["error"].is_string());
}

/// An incomplete save is written: what is answered, the answers stored, and reopening it
/// resumes them with the same questions open.
#[test]
fn an_incomplete_save_is_written_and_resumes_with_the_same_questions_open() {
    let scratch = Scratch::new("incomplete");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let answers = json!({"part.material": "6061-T6"});
    assert_eq!(page.put(0, 0, answers.clone()).status, 200);
    let mut events = page.events();
    events.next();
    let saved = page.save(1, json!({}));
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let saved = saved.json();
    assert_eq!(saved["complete"], false);
    assert_eq!(saved["rev"], 1);
    let missing: Vec<String> = serde_json::from_value(saved["missing"].clone()).unwrap();
    assert!(!missing.is_empty());
    let analysis = analysis_of(&scratch.input());
    let answers = answers.as_object().unwrap().clone();
    assert_eq!(missing, open_ids(&analysis, &answers));
    let (name, told) = events.next();
    assert_eq!(name, "saved");
    assert_eq!(
        told,
        json!({"path": saved["path"], "bytes": saved["bytes"], "rev": 1, "source": "page"})
    );
    let written = scratch.path("work/part.specified.step");
    assert!(is_step(&written));
    drop(opened);

    // Reopened: the session resumes the stored answers, with the same questions open.
    let reopened = Opened::start(&written, &scratch, &[]);
    assert_eq!(
        reopened.page().answers(),
        json!({"rev": 0, "parts": {"0": answers}})
    );
    let resumed = analysis_of(&written);
    assert_eq!(open_ids(&resumed, &answers), missing);

    // With every default approved, it is complete.
    let defaults = approve_all(&reopened.page(), &resumed);
    assert_eq!(defaults["complete"], true);
    assert_eq!(defaults["missing"], json!([]));
}

/// A complete save writes the PMI `specify-core write` writes for the same answers.
#[test]
fn a_complete_save_writes_what_write_writes() {
    let scratch = Scratch::new("as-write");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let given = json!({"part.material": "6082-T6"});
    assert_eq!(page.put(0, 0, given.clone()).status, 200);
    // Every default approved: they become answers, and the file stores them.
    let given = given.as_object().unwrap().clone();
    let input_analysis = analysis_of(&scratch.input());
    let saved = approve_all(&page, &input_analysis);
    assert_eq!(saved["complete"], true);
    let answers = page.answers()["parts"]["0"].clone();
    assert!(answers.as_object().unwrap().len() > given.len());
    for (id, value) in &given {
        assert_eq!(&answers[id], value);
    }

    let analysis = scratch.path("analysis.json");
    let answers_file = scratch.path("answers.json");
    let out = scratch.path("written.step");
    std::fs::write(&answers_file, answers.to_string()).unwrap();
    let cli = |args: &[&std::ffi::OsStr]| {
        let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .args(args)
            .output()
            .unwrap();
        assert!(run.status.success(), "{run:?}");
        String::from_utf8(run.stdout).unwrap()
    };
    cli(&[
        "analyse".as_ref(),
        scratch.input().as_os_str(),
        "-o".as_ref(),
        analysis.as_os_str(),
    ]);
    let report: Value = serde_json::from_str(&cli(&[
        "write".as_ref(),
        scratch.input().as_os_str(),
        analysis.as_os_str(),
        answers_file.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
    ]))
    .unwrap();
    assert_eq!(
        report_without_output(&saved["report"]),
        report_without_output(&report)
    );
    let saved_path = PathBuf::from(saved["path"].as_str().unwrap());
    assert_eq!(analysis_of(&saved_path), analysis_of(&out));
    for entity in [
        "DATUM_FEATURE(",
        "GEOMETRIC_TOLERANCE",
        "DIMENSIONAL_SIZE(",
        "pmi-assist answers",
    ] {
        assert_eq!(
            occurrences(&saved_path, entity),
            occurrences(&out, entity),
            "{entity}"
        );
    }
}

#[test]
fn in_place_keeps_one_bak_per_run_and_writes_from_the_original() {
    let scratch = Scratch::new("in-place");
    let input = std::fs::read(scratch.input()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(scratch.input(), std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let opened = Opened::start(&scratch.input(), &scratch, &["--in-place"]);
    let page = opened.page();
    assert_eq!(
        opened.config()["saveTarget"],
        scratch.input().display().to_string()
    );
    assert_eq!(
        page.put(0, 0, json!({"part.material": "6061-T6"})).status,
        200
    );
    let given: Map<String, Value> =
        serde_json::from_value(json!({"part.material": "6061-T6"})).unwrap();
    assert!(!given.is_empty());
    let got = approve_all(&page, &analysis_of(&scratch.input()));
    let rev = got["rev"].as_u64().unwrap();
    assert!(rev >= 2);
    assert_eq!(got["path"], scratch.input().display().to_string());
    let once = std::fs::read(scratch.input()).unwrap();
    assert!(once.starts_with(b"ISO-10303-21;") && once != input);
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        input
    );
    assert_eq!(opened.get("file").body, once);
    let pmi = |path: &Path| {
        [
            "DATUM_FEATURE(",
            "GEOMETRIC_TOLERANCE",
            "pmi-assist answers",
        ]
        .map(|e| occurrences(path, e))
    };
    let first = pmi(&scratch.input());
    assert!(first[2] > 0, "the answers are stored");

    // A later save replaces the file, from the file as opened (its PMI is not added twice),
    // and makes no other backup: the .bak still holds the original.
    assert_eq!(opened.save(rev, json!({})).status, 200);
    assert_eq!(pmi(&scratch.input()), first);
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        input
    );
    assert_eq!(scratch.work(), ["part.step", "part.step.bak"]);
    // The replacement keeps the original's permissions.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(scratch.input())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640);
    }
    // A file changed on disk by someone else is not written over.
    let mut theirs = std::fs::read(scratch.input()).unwrap();
    theirs.extend_from_slice(b"\n");
    std::fs::write(scratch.input(), &theirs).unwrap();
    let refused = opened.save(rev, json!({}));
    assert_eq!(refused.status, 409);
    assert_eq!(refused.json()["rev"], rev);
    assert_eq!(std::fs::read(scratch.input()).unwrap(), theirs);
    drop(opened);

    // A new run (of the file the last one wrote, which resumes its answers) keeps the .bak and
    // backs up to .bak-2, once.
    let before = std::fs::read(scratch.input()).unwrap();
    let opened = Opened::start(&scratch.input(), &scratch, &["--in-place"]);
    assert_eq!(
        opened.page().answers()["parts"]["0"]["part.material"],
        "6061-T6"
    );
    assert_eq!(opened.save(0, json!({})).status, 200);
    assert_eq!(opened.save(0, json!({})).status, 200);
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        input
    );
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak-2")).unwrap(),
        before
    );
    assert_eq!(pmi(&scratch.input()), first);
    assert_eq!(
        scratch.work(),
        ["part.step", "part.step.bak", "part.step.bak-2"]
    );
}

/// `proj/part.step` a link to `lib/abc.step`, reached through `alias`, a link to `proj`; the
/// app opened through the links as given, the root not resolved.
#[cfg(unix)]
#[test]
fn a_linked_input_saves_beside_the_link_and_shows_the_path_as_given() {
    let scratch = Scratch::new("linked");
    for dir in ["proj", "lib"] {
        std::fs::create_dir_all(scratch.path(dir)).unwrap();
    }
    std::fs::write(scratch.path("lib/abc.step"), step_text()).unwrap();
    std::os::unix::fs::symlink(scratch.path("lib/abc.step"), scratch.path("proj/part.step"))
        .unwrap();
    std::os::unix::fs::symlink(scratch.path("proj"), scratch.path("alias")).unwrap();
    let given = scratch.given.join("alias/part.step");
    let names = |dir: &str| {
        let mut n: Vec<String> = std::fs::read_dir(scratch.path(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        n.sort();
        n
    };
    let shown = |name: &str| scratch.given.join("alias").join(name).display().to_string();

    let opened = Opened::start(&given, &scratch, &[]);
    assert_eq!(opened.config()["name"], "part.step");
    assert_eq!(opened.config()["path"], shown("part.step"));
    assert_eq!(opened.config()["saveTarget"], shown("part.specified.step"));
    let got = opened.save(0, json!({}));
    assert_eq!(got.status, 200);
    assert_eq!(got.json()["path"], shown("part.specified.step"));
    assert!(is_step(&scratch.path("proj/part.specified.step")));
    assert_eq!(
        names("lib"),
        ["abc.step"],
        "nothing in the link's target's directory"
    );
    drop(opened);

    // In place: the link's target is replaced, the link kept, the .bak beside the link.
    let opened = Opened::start(&given, &scratch, &["--in-place"]);
    assert_eq!(opened.config()["saveTarget"], shown("part.step"));
    let got = opened.save(0, json!({}));
    assert_eq!(got.status, 200);
    assert_eq!(got.json()["path"], shown("part.step"));
    assert_eq!(opened.save(0, json!({})).status, 200);
    assert!(
        std::fs::symlink_metadata(scratch.path("proj/part.step"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link is kept"
    );
    assert!(occurrences(&scratch.path("lib/abc.step"), "pmi-assist answers") > 0);
    assert_eq!(
        std::fs::read(scratch.path("proj/part.step.bak")).unwrap(),
        step_text()
    );
    assert_eq!(names("lib"), ["abc.step"], "no temporary file left");
    assert_eq!(
        names("proj"),
        ["part.specified.step", "part.step", "part.step.bak"]
    );
}

#[test]
fn stp_and_gzipped_inputs_save_as_plain_specified_step() {
    let scratch = Scratch::new("names");
    let stp = scratch.path("work/a.stp");
    std::fs::write(&stp, step_text()).unwrap();
    let gz = scratch.path("work/b.step.gz");
    std::fs::copy(fixture(), &gz).unwrap();
    for (input, saved) in [(&stp, "a.specified.step"), (&gz, "b.specified.step")] {
        let opened = Opened::start(input, &scratch, &[]);
        let saved = scratch.path("work").join(saved);
        assert_eq!(opened.config()["saveTarget"], saved.display().to_string());
        let got = opened.save(0, json!({}));
        assert_eq!(got.status, 200);
        assert_eq!(got.json()["path"], saved.display().to_string());
        assert!(is_step(&saved), "uncompressed");
    }
    assert_eq!(
        std::fs::read(&gz).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
    assert_eq!(
        scratch.work(),
        [
            "a.specified.step",
            "a.stp",
            "b.specified.step",
            "b.step.gz",
            "part.step"
        ]
    );
}

#[test]
fn in_place_on_a_gzipped_input_writes_it_gzipped() {
    let scratch = Scratch::new("in-place-gz");
    let gz = scratch.path("work/part.step.gz");
    std::fs::copy(fixture(), &gz).unwrap();
    let opened = Opened::start(&gz, &scratch, &["--in-place"]);
    let saved = opened.save(0, json!({}));
    assert_eq!(saved.status, 200);
    let stored = std::fs::read(&gz).unwrap();
    assert_eq!(saved.json()["bytes"], stored.len());
    let text = gunzip(&stored);
    assert!(text.starts_with(b"ISO-10303-21;"));
    assert!(String::from_utf8_lossy(&text).contains("pmi-assist answers"));
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.gz.bak")).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
}

/// `open --diff OLD NEW`: the comparison `specify-core diff` makes, OLD and NEW served, and
/// the session starting from the answers it carried.
#[test]
fn a_diff_serves_both_files_and_starts_from_the_carried_answers() {
    let scratch = Scratch::new("diff");
    let old = scratch.path("work/plate.old.step.gz");
    let new = scratch.path("work/plate.new.step.gz");
    std::fs::copy(revision("hole_moved.old.step.gz"), &old).unwrap();
    std::fs::copy(revision("hole_moved.new.step.gz"), &new).unwrap();
    let given = scratch.path("old-answers.json");
    std::fs::write(&given, r#"{"part.material": "6061-T6"}"#).unwrap();
    let given_arg = given.display().to_string();
    let opened = Opened::diff(&old, &new, &scratch, &["--answers", &given_arg]);

    // What `specify-core diff` writes for the same pair.
    let dir = scratch.path("diffed");
    let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([&old, &new])
        .arg("-o")
        .arg(&dir)
        .arg("--answers")
        .arg(&given)
        .arg("--pmi-scenes")
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
    let read = |name: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    };

    let config = opened.config();
    assert_eq!(config["mode"], "diff");
    assert_eq!(config["name"], "plate.new.step.gz");
    assert_eq!(config["path"], new.display().to_string());
    assert_eq!(config["oldName"], "plate.old.step.gz");
    assert_eq!(config["oldPath"], old.display().to_string());
    assert_eq!(config["readOnly"], false);
    assert_eq!(
        config["saveTarget"],
        scratch
            .path("work/plate.new.specified.step")
            .display()
            .to_string()
    );
    assert_eq!(
        opened.get("file?rev=old").body,
        std::fs::read(&old).unwrap()
    );
    assert_eq!(
        opened.get("file?rev=new").body,
        std::fs::read(&new).unwrap()
    );
    assert_eq!(opened.get("file").body, std::fs::read(&new).unwrap());
    assert_eq!(opened.get("file?rev=other").status, 404);
    assert_eq!(
        opened.get("file?rev=saved").status,
        404,
        "nothing saved yet"
    );
    let report = opened.get("diff");
    assert_eq!(report.status, 200);
    // As WASM `diff()`: `diff`'s report.json, the report as Markdown, and the view to draw.
    let served = report.json();
    assert_eq!(served["report"], read("report.json"));
    assert!(served["markdown"].is_string() && served["view"]["new"].is_object());
    let carried = read("answers.json");
    assert_eq!(carried["part.material"], "6061-T6");
    assert_eq!(
        opened.page().answers(),
        json!({"rev": 0, "parts": {"0": carried}})
    );
    // Save writes NEW, beside it.
    let saved = opened.save(0, json!({}));
    assert_eq!(saved.status, 200);
    let written = scratch.path("work/plate.new.specified.step");
    assert_eq!(saved.json()["path"], written.display().to_string());
    // `rev=saved` serves what it wrote, as in the other modes.
    let got = opened.get("file?rev=saved");
    assert_eq!(got.status, 200);
    assert_eq!(got.body, std::fs::read(&written).unwrap());
    assert_eq!(
        got.header("x-specify-path"),
        Some(written.display().to_string().as_str())
    );
}

#[test]
fn the_close_delay_outlasts_the_heartbeat() {
    assert!(open::CLOSE_DELAY > open::HEARTBEAT);
    assert_eq!(open::HEARTBEAT, Duration::from_secs(30));
    assert_eq!(open::CLOSE_DELAY, Duration::from_secs(90));
    assert_eq!(open::IDLE, Duration::from_secs(30 * 60));
    assert_eq!(open::KEEP_ALIVE, Duration::from_secs(15));
    assert_eq!(open::MAX_STREAMS, 4);
}

#[test]
fn closed_stops_the_server_unless_a_page_comes_back_or_is_still_open() {
    let scratch = Scratch::new("closed");
    let delay = Duration::from_millis(1500);
    let beat = Duration::from_millis(300);
    let served = Served::start(&scratch, delay, Duration::from_secs(600));
    let own = format!("http://127.0.0.1:{}", served.port);

    // Only from the app's own page.
    assert_eq!(
        served.post("closed", Some("https://example.com")).status,
        403
    );
    assert_eq!(served.post("closed", None).status, 403);

    // A reload: closed, then the page asks again.
    assert_eq!(served.closed().status, 204);
    assert_eq!(served.page().get("").status, 200);
    std::thread::sleep(delay + Duration::from_millis(500));
    assert!(!served.thread.is_finished(), "a reload keeps it running");

    // One tab closed, another still open: its heartbeats keep the server past the delay.
    assert_eq!(served.post("closed", Some(&own)).status, 204);
    served.heartbeats(beat, delay * 2);

    // An open event stream keeps it too, and its end counts as the last request.
    let events = served.page().events();
    assert_eq!(served.closed().status, 204);
    std::thread::sleep(delay + Duration::from_millis(500));
    assert!(
        !served.thread.is_finished(),
        "an open stream keeps it running"
    );
    drop(events);

    // The last tab closed: it stops once no request has come for the delay.
    assert_eq!(served.closed().status, 204);
    assert_eq!(served.stopped(delay + Duration::from_secs(5)), ["Closed."]);
}

#[test]
fn heartbeats_keep_an_idle_server_running() {
    let scratch = Scratch::new("heartbeat");
    // An idle time well above the beat, so a beat delayed on a loaded machine (other tests
    // save in parallel) does not look like silence.
    let idle = Duration::from_millis(3000);
    let served = Served::start(&scratch, Duration::from_secs(600), idle);
    served.heartbeats(Duration::from_millis(200), idle * 3);
    let notices = served.stopped(idle + Duration::from_secs(5));
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].starts_with("Stopped: no request for"),
        "{notices:?}"
    );
}

#[test]
fn a_server_without_requests_stops_after_its_idle_time() {
    let scratch = Scratch::new("idle");
    let app = App::new(
        session_of(&scratch.input()),
        scratch.assets(),
        &Options::default(),
    )
    .unwrap();
    let port = app.port();
    let mut notices = Vec::new();
    app.serve(Duration::from_secs(3), Duration::from_millis(300), |n| {
        notices.push(n.to_string())
    })
    .unwrap();
    assert_eq!(notices.len(), 1);
    assert!(
        notices[0].starts_with("Stopped: no request for"),
        "{notices:?}"
    );
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the port is closed"
    );
}

#[test]
fn bad_arguments_no_app_and_names_that_are_not_step_are_refused_up_front() {
    let scratch = Scratch::new("args");
    let stl = scratch.path("work/part.stl");
    std::fs::write(&stl, b"solid").unwrap();
    let input = scratch.input();

    for (inputs, extra) in [
        (vec![input.as_path()], vec!["--read-only", "--in-place"]),
        (vec![input.as_path(), input.as_path()], vec!["--read-only"]),
    ] {
        let conflict = command(&inputs, &scratch, &extra).output().unwrap();
        assert_eq!(conflict.status.code(), Some(2), "{extra:?}");
        assert!(conflict.stdout.is_empty());
        let stderr = String::from_utf8(conflict.stderr).unwrap();
        assert!(stderr.contains("cannot be used with"), "{stderr}");
    }
    // --answers is for a diff.
    let answers = command(&[input.as_path()], &scratch, &["--answers", "a.json"])
        .output()
        .unwrap();
    assert_eq!(answers.status.code(), Some(2));

    for inputs in [vec![stl.as_path()], vec![stl.as_path(), input.as_path()]] {
        let out = command(&inputs, &scratch, &[]).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.starts_with("cannot open: "), "{stderr}");
        assert!(stderr.contains("is not a STEP file"), "{stderr}");
    }

    let no_app = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("open")
        .arg(scratch.input())
        .arg("--no-browser")
        .env_remove("SPECIFY_APP_DIR")
        .env_remove("SPECIFY_WASM_DIR")
        .output()
        .unwrap();
    assert_eq!(no_app.status.code(), Some(2));
    assert!(no_app.stdout.is_empty());
    let stderr = String::from_utf8(no_app.stderr).unwrap();
    assert!(stderr.starts_with("cannot open: "), "{stderr}");
    assert!(stderr.contains("not bundled"), "{stderr}");
    assert!(stderr.contains("--app DIR"), "{stderr}");

    // The environment names the app as well as --app does.
    let mut from_env = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("open")
        .arg(scratch.input())
        .arg("--no-browser")
        .env("SPECIFY_APP_DIR", scratch.path("app"))
        .env("SPECIFY_WASM_DIR", scratch.path("pkg"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(from_env.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let _ = from_env.kill();
    let _ = from_env.wait();
    assert!(
        line.starts_with("Open part.step: http://127.0.0.1:"),
        "{line}"
    );

    let not_app = command(
        &[input.as_path()],
        &scratch,
        &["--app", scratch.path("pkg").to_str().unwrap()],
    )
    .output()
    .unwrap();
    assert_eq!(not_app.status.code(), Some(2));
    assert!(
        String::from_utf8(not_app.stderr)
            .unwrap()
            .contains("not a directory holding index.html")
    );

    let missing = command(&[&scratch.path("work/missing.step")], &scratch, &[])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .starts_with("error: ")
    );

    let help = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(["open", "-h"])
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0));
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(
        help.starts_with("Open a STEP file in Specify in your browser"),
        "{help}"
    );
    assert!(help.contains("--diff <OLD> <NEW>"), "{help}");
    assert!(
        !help.contains("--site") && !help.contains("--editor"),
        "{help}"
    );
    assert!(!help.contains("draftwright.io"), "{help}");
}

#[test]
fn step_names_are_step_or_stp_or_either_gzipped() {
    for name in ["a.step", "a.STP", "a.Step.GZ", "a.stp.gz"] {
        assert!(open::is_step_name(name), "{name}");
    }
    for name in [
        "a.stl",
        "a.step.zip",
        "a.gz",
        "step",
        "a.steps",
        "a.step.gz.txt",
    ] {
        assert!(!open::is_step_name(name), "{name}");
    }
}

/// Saves racing answer changes: every file written holds exactly the answers of the rev it was
/// asked for, and a save of a rev the session has left is refused (409), never written.
#[test]
fn racing_saves_write_exactly_their_rev() {
    let scratch = Scratch::new("race");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let start = page.answers();
    let mut by_rev = std::collections::BTreeMap::new();
    by_rev.insert(0u64, start["parts"]["0"].clone());
    let materials = ["6061-T6", "6082-T6", "7075-T6", "2024-T3"];
    // Every other save races a change sent as the save is sent; the others race nothing.
    let (go, went) = std::sync::mpsc::channel::<()>();
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let changes = {
        let page = page.clone();
        std::thread::spawn(move || {
            let mut written = Vec::new();
            let mut rev = 0;
            for i in 0.. {
                if went.recv().is_err() {
                    break;
                }
                let answers = json!({"part.material": materials[i % materials.len()]});
                let got = page.put(rev, 0, answers.clone());
                assert_eq!(got.status, 200);
                rev = got.json()["rev"].as_u64().unwrap();
                written.push((rev, answers));
                done.send(()).unwrap();
            }
            written
        })
    };
    let mut saved = Vec::new();
    let mut stale = 0;
    for i in 0..8 {
        let rev = page.answers()["rev"].as_u64().unwrap();
        let racing = i % 2 == 1;
        if racing {
            go.send(()).unwrap();
        }
        let got = page.save(rev, json!({}));
        if racing {
            finished.recv().unwrap();
        }
        match got.status {
            200 => {
                let body = got.json();
                assert_eq!(body["rev"], rev);
                saved.push((rev, PathBuf::from(body["path"].as_str().unwrap())));
            }
            409 if racing => {
                assert!(got.json()["rev"].as_u64().unwrap() > rev);
                stale += 1;
            }
            other => panic!("save {i}: {other}"),
        }
    }
    drop(go);
    by_rev.extend(changes.join().unwrap());
    // One more, certainly stale.
    assert_eq!(page.save(0, json!({})).status, 409);
    assert!(saved.len() >= 4, "{stale} stale");
    for (rev, path) in saved {
        assert_eq!(
            analysis_of(&path)["resume"]["answers"],
            by_rev[&rev],
            "{} holds rev {rev}",
            path.display()
        );
    }
}

/// An event stream that stops reading is dropped once it is more than `MAX_BEHIND` events
/// behind (its receiver ends after what it was sent); the others carry on.
#[test]
fn a_stream_too_far_behind_is_dropped_and_the_others_carry_on() {
    let scratch = Scratch::new("behind");
    let shared = session_of(&scratch.input());
    let (stuck, reading) = {
        let mut s = shared.lock().unwrap();
        (s.subscribe(), s.subscribe())
    };
    assert!(reading.try_recv().is_ok(), "the start event");
    let limit = specify_core::session::MAX_BEHIND;
    for i in 0..=limit {
        let material = if i % 2 == 0 { "6061-T6" } else { "6082-T6" };
        let answers: Map<String, Value> =
            serde_json::from_value(json!({ "part.material": material })).unwrap();
        assert!(shared.lock().unwrap().set(0, answers, "page").is_some());
        assert!(reading.try_recv().is_ok(), "change {i} reaches the reader");
    }
    // The stuck one got the start and `limit` changes, then was dropped.
    assert_eq!(stuck.try_iter().count(), 1 + limit);
    assert!(matches!(
        stuck.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    let answers: Map<String, Value> =
        serde_json::from_value(json!({"part.material": "7075-T6"})).unwrap();
    shared.lock().unwrap().set(0, answers, "page");
    assert!(reading.try_recv().is_ok(), "the reader carries on");
}

/// ADR P9: a default is written only when the person approves it, per id. The approved ones
/// become answers (a new rev, an event naming them), are stored and written as answers are;
/// the others stay open and are not written.
#[test]
fn only_the_approved_defaults_are_written() {
    let scratch = Scratch::new("approved");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let given = json!({"part.material": "6061-T6"});
    assert_eq!(page.put(0, 0, given.clone()).status, 200);
    let given = given.as_object().unwrap().clone();
    let analysis = analysis_of(&scratch.input());
    let qs = specify_core::api::questions(&analysis, &given).unwrap();
    let offered: Vec<(String, Value)> = specify_core::rules::open_questions(&qs, &given)
        .iter()
        .filter_map(|q| q.default.clone().map(|d| (q.id.clone(), d)))
        .collect();
    assert!(offered.len() >= 2, "{offered:?}");
    let (approved, default) = offered[0].clone();
    // A value that is not the default now: refused, naming it, and nothing written.
    let stale = page.save(1, json!({&approved: "not what the person saw"}));
    assert_eq!(stale.status, 400);
    let stale = stale.json();
    assert_eq!(stale["part"], 0);
    assert_eq!(stale["question"], json!(approved));
    assert_eq!(stale["expected"], "not what the person saw");
    assert_eq!(stale["current"], default);
    assert_eq!(page.answers()["rev"], 1);
    assert_eq!(scratch.work(), ["part.step"], "nothing written");
    let mut events = page.events();
    events.next();
    let saved = page.save(1, json!({&approved: default}));
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let saved = saved.json();
    assert_eq!(saved["rev"], 2);
    assert_eq!(saved["complete"], false);
    let missing: Vec<String> = serde_json::from_value(saved["missing"].clone()).unwrap();
    assert!(!missing.contains(&approved));
    for (other, _) in &offered[1..] {
        assert!(missing.contains(other), "{other} stays open");
    }
    let mut answered = given.clone();
    answered.insert(approved.clone(), default);
    let (name, change) = events.next();
    assert_eq!(name, "answers");
    assert_eq!(
        change,
        json!({"rev": 2, "part": 0, "answers": answered, "source": "page",
               "defaults": {"0": [approved]}})
    );
    assert_eq!(events.next().0, "saved");
    drop(opened);

    // Reopened: the approved default is an answer, the others are open, and the PMI is what
    // the same answers given explicitly write.
    let written = scratch.path("work/part.specified.step");
    let resumed = analysis_of(&written);
    assert_eq!(
        resumed["resume"]["answers"],
        Value::Object(answered.clone())
    );
    let open = open_ids(&resumed, &answered);
    assert!(!open.contains(&approved));
    for (other, _) in &offered[1..] {
        assert!(open.contains(other), "{other} is open on reopening");
    }
    let explicit = Opened::start(&scratch.input(), &scratch, &[]);
    assert_eq!(
        explicit.page().put(0, 0, Value::Object(answered)).status,
        200
    );
    let same = explicit.save(1, json!({})).json();
    assert_eq!(
        report_without_output(&same["report"]),
        report_without_output(&saved["report"])
    );
}

/// PMI counts that a write keeps or loses, from the STEP text.
fn pmi_counts(path: &Path) -> [usize; 3] {
    ["DATUM_FEATURE(", "GEOMETRIC_TOLERANCE", "DIMENSIONAL_SIZE("].map(|e| occurrences(path, e))
}

/// A file saved in the boolean era (every default written, the stored answers only metadata)
/// keeps its PMI when a session of it saves with nothing to change: a part whose answers are
/// only `_`-keys is not written, and a file with nothing to write is saved as it was.
#[test]
fn a_part_with_only_metadata_answers_keeps_its_pmi() {
    let scratch = Scratch::new("metadata-only");
    let plate = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step");
    let analysis = analysis_of(&plate);
    let loaded = specify_core::part::load(&plate, None).unwrap();
    let given: Map<String, Value> =
        serde_json::from_value(json!({"part.material": "6061-T6"})).unwrap();
    let mut stored = specify_core::api::export_answers(
        &loaded,
        &analysis,
        &given,
        &specify_core::api::Approved::new(),
    )
    .unwrap()
    .answers;
    // Only the metadata, as a boolean-era save with every default taken stores it.
    stored.retain(|k, _| k.starts_with('_'));
    assert!(!stored.is_empty());
    let intent = specify_core::api::apply(&analysis, &given, true).unwrap();
    let old = scratch.path("work/old.step");
    specify_core::api::write_file(&plate, &intent, &old, Some(&stored)).unwrap();
    let before = pmi_counts(&old);
    assert!(
        before.iter().sum::<usize>() > 0,
        "the defaults' PMI is written"
    );
    let resumed = analysis_of(&old);
    assert!(
        resumed["resume"]["answers"]
            .as_object()
            .unwrap()
            .keys()
            .all(|k| k.starts_with('_')),
        "{}",
        resumed["resume"]
    );

    let shared = session_of(&old);
    let saved = specify_core::session::save(&shared, None, &Default::default(), "page")
        .unwrap_or_else(|e| panic!("{e}"));
    let path = PathBuf::from(&saved.path);
    assert_eq!(pmi_counts(&path), before, "nothing dropped");
    assert_eq!(saved.report["written"], json!([]));
}

/// Several approvals in one save are checked against the defaults as they stand and applied
/// together: each is written with the value approved.
#[test]
fn approvals_are_applied_together_with_the_values_approved() {
    let scratch = Scratch::new("together");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let given = json!({"part.material": "6061-T6"});
    assert_eq!(page.put(0, 0, given.clone()).status, 200);
    let given = given.as_object().unwrap().clone();
    let analysis = analysis_of(&scratch.input());
    let offered = open_defaults(&analysis, &given);
    assert!(offered.len() >= 2, "{offered:?}");
    let two: Map<String, Value> = offered.into_iter().take(2).collect();
    let saved = page.save(1, Value::Object(two.clone()));
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let stored = analysis_of(&PathBuf::from(saved.json()["path"].as_str().unwrap()));
    for (id, value) in &two {
        assert_eq!(&stored["resume"]["answers"][id], value, "{id}");
    }
}

/// Clearing every answer of a part reaches the file: the session changed it, so it is written,
/// and reopening does not bring the answers back.
#[test]
fn clearing_every_answer_reaches_the_file() {
    let scratch = Scratch::new("cleared");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let resumed = page.answers()["parts"]["0"].clone();
    assert!(resumed.as_object().is_some_and(|a| !a.is_empty()));
    assert_eq!(page.put(0, 0, json!({})).status, 200);
    let saved = page.save(1, json!({}));
    assert_eq!(
        saved.status,
        200,
        "{}",
        String::from_utf8_lossy(&saved.body)
    );
    let written = PathBuf::from(saved.json()["path"].as_str().unwrap());
    drop(opened);
    let reopened = Opened::start(&written, &scratch, &[]);
    let parts = reopened.page().answers()["parts"].clone();
    assert!(
        parts
            .get("0")
            .is_none_or(|a| a.as_object().unwrap().keys().all(|k| k.starts_with('_'))),
        "{parts}"
    );
}

/// Sets part 1's answers, as a page's first change to it would, between a save's two holds of
/// the session's lock.
fn part_one_answered_meanwhile(shared: &specify_core::session::Shared) {
    let answers: Map<String, Value> =
        serde_json::from_value(json!({"part.material": "6061-T6"})).unwrap();
    specify_core::session::lock(shared).set(1, answers, "page");
}

/// A part joining while an agent's save (no rev) is under way: refused as stale, not a panic.
#[test]
fn a_part_joining_during_a_save_without_a_rev_is_refused_not_a_crash() {
    let scratch = Scratch::new("joined-meanwhile");
    let two = scratch.path("work/two.step");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/parts/a_file_with_two_parts_must_say_which.step"),
        &two,
    )
    .unwrap();
    let shared = session_of(&two);
    {
        let mut s = specify_core::session::lock(&shared);
        let answers: Map<String, Value> =
            serde_json::from_value(json!({"part.material": "6082-T6"})).unwrap();
        s.set(0, answers, "mcp");
        s.between_locks = Some(part_one_answered_meanwhile);
    }
    let refused = specify_core::session::save(&shared, None, &Default::default(), "mcp");
    assert!(
        matches!(refused, Err(specify_core::session::SaveError::Stale(_))),
        "{refused:?}"
    );
    assert_eq!(scratch.work(), ["part.step", "two.step"], "nothing written");
    specify_core::session::lock(&shared).between_locks = None;
    let saved = specify_core::session::save(&shared, None, &Default::default(), "mcp");
    assert!(saved.is_ok(), "{saved:?}");
}

/// Approvals on several parts make one rev; an id given twice with the same value is taken
/// once, and with different values refused.
#[test]
fn approvals_on_several_parts_make_one_rev_and_duplicates_are_checked() {
    let scratch = Scratch::new("one-rev");
    let two = scratch.path("work/two.step");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/parts/a_file_with_two_parts_must_say_which.step"),
        &two,
    )
    .unwrap();
    let shared = session_of(&two);
    let mut approved = specify_core::session::Approved::new();
    for k in 0..2 {
        let analysis = specify_core::session::analysis(&shared, k).unwrap();
        let offered = open_defaults(&analysis, &Map::new());
        let (id, value) = offered.into_iter().next().unwrap();
        approved.insert(k, Map::from_iter([(id, value)]));
    }
    let before = specify_core::session::lock(&shared).rev();
    let saved = specify_core::session::save(&shared, None, &approved, "page").unwrap();
    assert_eq!(saved.rev, before + 1);
    let s = specify_core::session::lock(&shared);
    for (k, ids) in &approved {
        for (id, value) in ids {
            assert_eq!(&s.answers(*k).unwrap()[id], value);
        }
    }
    drop(s);

    // Through the page: a duplicate id with the same value is one approval; with another, 400.
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let page = opened.page();
    let answers: Map<String, Value> =
        serde_json::from_value(page.answers()["parts"]["0"].clone()).unwrap();
    let (id, value) = open_defaults(&analysis_of(&scratch.input()), &answers)
        .into_iter()
        .next()
        .unwrap();
    let send = |body: String| {
        request(
            page.port,
            "POST",
            &page.path("save"),
            &[
                ("Host", &page.host()),
                ("Origin", &page.origin()),
                ("Content-Type", "application/json"),
            ],
            body.as_bytes(),
        )
    };
    let conflict = send(format!(
        r#"{{"rev": 0, "acceptDefaults": {{"0": {{"{id}": {value}, "{id}": "other"}}}}}}"#
    ));
    assert_eq!(conflict.status, 400);
    assert_eq!(conflict.json()["question"], json!(id));
    assert_eq!(conflict.json()["part"], 0);
    let same = send(format!(
        r#"{{"rev": 0, "acceptDefaults": {{"0": {{"{id}": {value}, "{id}": {value}}}}}}}"#
    ));
    assert_eq!(same.status, 200, "{}", String::from_utf8_lossy(&same.body));
    assert_eq!(page.answers()["parts"]["0"][&id], value);
}
