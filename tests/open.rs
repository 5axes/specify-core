//! `specify-core open` (src/open.rs, docs/open.md): the local Specify app, against the built
//! binary on a real loopback port with a stub app and engine, and `--no-browser` so no browser
//! opens. The app, engine, file and config are served under the token at exactly
//! `Host: 127.0.0.1:<port>`; saves come only from the app's own origin and never overwrite
//! (unless `--in-place`, which keeps a `.bak`); `closed` stops the server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use specify_core::open::{self, App, Assets, Options};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thread-depth-alpha7.step.gz")
}

/// The fixture's STEP text.
fn step_text() -> Vec<u8> {
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(fixture()).unwrap())
        .read_to_end(&mut text)
        .unwrap();
    text
}

/// A scratch directory holding a stub app (`app/`), a stub engine (`pkg/`), a file outside
/// both (`secret.txt`) and a plain STEP input (`work/part.step`); removed when dropped.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Scratch {
        let root =
            std::env::temp_dir().join(format!("specify-core-open-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["app/assets", "pkg", "work"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let root = std::fs::canonicalize(root).unwrap();
        let write = |p: &str, b: &[u8]| std::fs::write(root.join(p), b).unwrap();
        write("app/index.html", b"<!doctype html><title>stub app</title>");
        write("app/app.css", b"body{}");
        write("app/assets/main.mjs", b"export {};");
        write("pkg/specify_core_wasm.js", b"export default 1;");
        write("pkg/specify_core_wasm_bg.wasm", b"\0asm\x01\0\0\0");
        write("secret.txt", b"secret");
        std::fs::write(root.join("work/part.step"), step_text()).unwrap();
        Scratch { root }
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

fn command(step: &Path, scratch: &Scratch, extra: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_specify-core"));
    c.arg("open")
        .arg(step)
        .arg("--no-browser")
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
        let name = step.file_name().unwrap().to_string_lossy().into_owned();
        let mut child = command(step, scratch, extra)
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

    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn at(&self, rel: &str) -> String {
        format!("{}{rel}", self.prefix)
    }

    fn get(&self, rel: &str) -> Response {
        request(
            self.port,
            "GET",
            &self.at(rel),
            &[("Host", &self.host())],
            b"",
        )
    }

    fn config(&self) -> Value {
        let got = self.get("config");
        assert_eq!(got.status, 200);
        assert_eq!(got.header("content-type"), Some("application/json"));
        serde_json::from_slice(&got.body).unwrap()
    }

    /// A save from the app's own page.
    fn save(&self, body: &[u8]) -> Response {
        let (host, origin) = (self.host(), self.origin());
        request(
            self.port,
            "POST",
            &self.at("save"),
            &[
                ("Host", &host),
                ("Origin", &origin),
                ("Content-Type", "application/octet-stream"),
            ],
            body,
        )
    }

    fn closed(&self) -> Response {
        let (host, origin) = (self.host(), self.origin());
        request(
            self.port,
            "POST",
            &self.at("closed"),
            &[
                ("Host", &host),
                ("Origin", &origin),
                ("Content-Type", "text/plain;charset=UTF-8"),
            ],
            b"",
        )
    }

    /// The next line on stdout.
    fn line(&mut self) -> String {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        line
    }

    /// Waits up to `within` for the command to exit: its code and the rest of stdout.
    fn exit(mut self, within: Duration) -> (i32, String) {
        let deadline = Instant::now() + within;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                let mut rest = String::new();
                self.stdout.read_to_string(&mut rest).unwrap();
                return (status.code().unwrap_or(-1), rest);
            }
            assert!(Instant::now() < deadline, "the command did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
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
        serde_json::from_slice(&self.body).unwrap()
    }
}

/// One raw HTTP/1.1 request to the port, with exactly these headers (and a Content-Length when
/// there is a body or the method is POST and none is given).
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
    if !has_length && (method == "POST" || !body.is_empty()) {
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

/// A STEP body, distinguishable by `tag`.
fn step_body(tag: &str) -> Vec<u8> {
    format!("ISO-10303-21;\nHEADER;\n/* {tag} */\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n")
        .into_bytes()
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
    ] {
        let got = opened.get(rel);
        assert_eq!(got.status, 200, "{rel}");
        assert_eq!(got.header("content-type"), Some(content_type), "{rel}");
        assert_eq!(got.body, body, "{rel}");
    }

    // The token without its slash is sent to the app's address.
    let bare = request(
        opened.port,
        "GET",
        opened.prefix.trim_end_matches('/'),
        &[("Host", &opened.host())],
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

    let config = opened.config();
    assert_eq!(config["name"], "part.step");
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
        ["name", "readOnly", "wasmUrl", "saveTarget", "specifyCore"]
    );
}

#[test]
fn other_hosts_tokens_origins_and_paths_outside_the_app_are_refused() {
    let scratch = Scratch::new("refused");
    #[cfg(unix)]
    std::os::unix::fs::symlink(scratch.path("secret.txt"), scratch.path("app/link.txt")).unwrap();
    let mut opened = Opened::start(&scratch.input(), &scratch, &[]);
    let host = opened.host();
    let other_port = format!("127.0.0.1:{}", opened.port.wrapping_add(1));
    let localhost = format!("localhost:{}", opened.port);
    let index = opened.at("");
    let other_token = format!("/{}/", "0".repeat(32));
    let other_token_file = format!("/{}/file", "0".repeat(32));
    let cases: Vec<(&str, String, Vec<(&str, &str)>, u16)> = vec![
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
            opened.at("file"),
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
            opened.at("../secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "encoded dot-dot",
            opened.at("%2e%2e/secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "encoded slash",
            opened.at("assets%2f..%2f..%2fsecret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "out of the engine",
            opened.at("wasm/../../secret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "backslash",
            opened.at("assets%5c..%5c..%5csecret.txt"),
            vec![("Host", &host)],
            404,
        ),
        (
            "a directory",
            opened.at("assets/"),
            vec![("Host", &host)],
            404,
        ),
        (
            "a missing file",
            opened.at("missing.js"),
            vec![("Host", &host)],
            404,
        ),
        #[cfg(unix)]
        (
            "a link leading out",
            opened.at("link.txt"),
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
        &opened.at("file"),
        &[("Host", &host)],
        b"",
    );
    assert_eq!(put.status, 405);
    assert!(opened.still_running());
}

#[test]
fn saves_take_the_next_free_name_and_never_overwrite() {
    let scratch = Scratch::new("save");
    let input = std::fs::read(scratch.input()).unwrap();
    let mut opened = Opened::start(&scratch.input(), &scratch, &[]);
    let work = |n: &str| scratch.path("work").join(n);

    let first = opened.save(&step_body("first"));
    assert_eq!(first.status, 200);
    let first = first.json();
    assert_eq!(
        first["path"],
        work("part.specified.step").display().to_string()
    );
    assert_eq!(first["bytes"], step_body("first").len());
    assert_eq!(
        std::fs::read(work("part.specified.step")).unwrap(),
        step_body("first")
    );
    assert_eq!(
        opened.line(),
        format!(
            "Saved {} ({} bytes).\n",
            first["path"].as_str().unwrap(),
            step_body("first").len()
        )
    );
    assert_eq!(
        opened.config()["saveTarget"],
        work("part.specified-2.step").display().to_string()
    );

    let second = opened.save(&step_body("second"));
    assert_eq!(second.status, 200);
    assert_eq!(
        second.json()["path"],
        work("part.specified-2.step").display().to_string()
    );
    // A file already at the next name is left alone.
    std::fs::write(work("part.specified-3.step"), b"mine").unwrap();
    // A BOM and leading whitespace are allowed.
    let mut third_body = b"\xEF\xBB\xBF \r\n".to_vec();
    third_body.extend(step_body("third"));
    let third = opened.save(&third_body);
    assert_eq!(third.status, 200);
    assert_eq!(
        third.json()["path"],
        work("part.specified-4.step").display().to_string()
    );

    assert_eq!(
        std::fs::read(work("part.specified.step")).unwrap(),
        step_body("first")
    );
    assert_eq!(
        std::fs::read(work("part.specified-2.step")).unwrap(),
        step_body("second")
    );
    assert_eq!(
        std::fs::read(work("part.specified-3.step")).unwrap(),
        b"mine"
    );
    assert_eq!(
        std::fs::read(work("part.specified-4.step")).unwrap(),
        third_body
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
fn a_save_from_elsewhere_of_another_type_not_step_or_too_large_is_refused() {
    let scratch = Scratch::new("save-refused");
    let opened = Opened::start(&scratch.input(), &scratch, &[]);
    let (host, origin) = (opened.host(), opened.origin());
    let localhost_origin = format!("http://localhost:{}", opened.port);
    let target = opened.at("save");
    let octets = ("Content-Type", "application/octet-stream");
    let body = step_body("refused");
    // 4× the STEP text is the cap.
    let over = (4 * step_text().len() + 1).to_string();
    let at_cap = (4 * step_text().len()).to_string();
    let cases: Vec<(&str, Vec<(&str, &str)>, &[u8], u16)> = vec![
        ("no origin", vec![("Host", &host), octets], &body, 403),
        (
            "another origin",
            vec![("Host", &host), ("Origin", "https://example.com"), octets],
            &body,
            403,
        ),
        (
            "localhost origin",
            vec![("Host", &host), ("Origin", &localhost_origin), octets],
            &body,
            403,
        ),
        (
            "null origin",
            vec![("Host", &host), ("Origin", "null"), octets],
            &body,
            403,
        ),
        (
            "text",
            vec![
                ("Host", &host),
                ("Origin", &origin),
                ("Content-Type", "text/plain"),
            ],
            &body,
            415,
        ),
        (
            "no content type",
            vec![("Host", &host), ("Origin", &origin)],
            &body,
            415,
        ),
        (
            "not STEP",
            vec![("Host", &host), ("Origin", &origin), octets],
            b"<html>not a part</html>",
            400,
        ),
        (
            "over the cap",
            vec![
                ("Host", &host),
                ("Origin", &origin),
                octets,
                ("Content-Length", &over),
            ],
            b"",
            413,
        ),
    ];
    for (what, headers, body, status) in cases {
        let got = request(opened.port, "POST", &target, &headers, body);
        assert_eq!(got.status, status, "{what}");
        assert!(got.json()["error"].is_string(), "{what}");
    }
    // At the cap is not refused for its size (a short body is refused as cut off).
    let short = request(
        opened.port,
        "POST",
        &target,
        &[
            ("Host", &host),
            ("Origin", &origin),
            octets,
            ("Content-Length", &at_cap),
        ],
        &body,
    );
    assert_ne!(short.status, 413);
    let get = request(opened.port, "GET", &target, &[("Host", &host)], b"");
    assert_eq!(get.status, 405);
    assert_eq!(scratch.work(), ["part.step"], "nothing written");
}

#[test]
fn read_only_refuses_saves_and_writes_nothing() {
    let scratch = Scratch::new("read-only");
    let opened = Opened::start(&scratch.input(), &scratch, &["--read-only"]);
    let config = opened.config();
    assert_eq!(config["readOnly"], true);
    assert_eq!(config["saveTarget"], Value::Null);
    let got = opened.save(&step_body("ro"));
    assert_eq!(got.status, 409);
    assert!(got.json()["error"].is_string());
    assert_eq!(scratch.work(), ["part.step"]);
}

#[test]
fn in_place_keeps_a_bak_and_replaces_the_input() {
    let scratch = Scratch::new("in-place");
    let input = std::fs::read(scratch.input()).unwrap();
    let opened = Opened::start(&scratch.input(), &scratch, &["--in-place"]);
    assert_eq!(
        opened.config()["saveTarget"],
        scratch.input().display().to_string()
    );
    let got = opened.save(&step_body("one"));
    assert_eq!(got.status, 200);
    assert_eq!(got.json()["path"], scratch.input().display().to_string());
    assert_eq!(std::fs::read(scratch.input()).unwrap(), step_body("one"));
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        input
    );
    assert_eq!(opened.get("file").body, step_body("one"));

    // An existing .bak is kept: the next goes to .bak-2.
    assert_eq!(opened.save(&step_body("two")).status, 200);
    assert_eq!(std::fs::read(scratch.input()).unwrap(), step_body("two"));
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak")).unwrap(),
        input
    );
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.bak-2")).unwrap(),
        step_body("one")
    );
    assert_eq!(
        scratch.work(),
        ["part.step", "part.step.bak", "part.step.bak-2"]
    );
}

#[test]
fn in_place_on_a_gzipped_input_writes_it_gzipped() {
    let scratch = Scratch::new("in-place-gz");
    let gz = scratch.path("work/part.step.gz");
    std::fs::copy(fixture(), &gz).unwrap();
    let opened = Opened::start(&gz, &scratch, &["--in-place"]);
    assert_eq!(opened.save(&step_body("gz")).status, 200);
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&gz).unwrap())
        .read_to_end(&mut text)
        .unwrap();
    assert_eq!(text, step_body("gz"));
    assert_eq!(
        std::fs::read(scratch.path("work/part.step.gz.bak")).unwrap(),
        std::fs::read(fixture()).unwrap()
    );
}

#[test]
fn closed_stops_the_server_unless_the_page_comes_back() {
    let scratch = Scratch::new("closed");
    let mut opened = Opened::start(&scratch.input(), &scratch, &[]);
    let host = opened.host();

    // Only from the app's own page.
    let foreign = request(
        opened.port,
        "POST",
        &opened.at("closed"),
        &[("Host", &host), ("Origin", "https://example.com")],
        b"",
    );
    assert_eq!(foreign.status, 403);
    let none = request(
        opened.port,
        "POST",
        &opened.at("closed"),
        &[("Host", &host)],
        b"",
    );
    assert_eq!(none.status, 403);

    // A reload: closed, then the page asks again.
    assert_eq!(opened.closed().status, 204);
    assert_eq!(opened.get("").status, 200);
    std::thread::sleep(open::CLOSE_DELAY + Duration::from_secs(1));
    assert!(opened.still_running(), "a reload keeps it running");

    assert_eq!(opened.closed().status, 204);
    let port = opened.port;
    let (code, rest) = opened.exit(open::CLOSE_DELAY + Duration::from_secs(5));
    assert_eq!(code, 0);
    assert_eq!(rest, "Closed.\n");
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the port is closed"
    );
}

#[test]
fn a_server_without_requests_stops_after_its_idle_time() {
    let scratch = Scratch::new("idle");
    let app = App::new(&scratch.input(), scratch.assets(), &Options::default()).unwrap();
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

    let conflict = command(&scratch.input(), &scratch, &["--read-only", "--in-place"])
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(2));
    assert!(conflict.stdout.is_empty());
    let stderr = String::from_utf8(conflict.stderr).unwrap();
    assert!(stderr.contains("cannot be used with"), "{stderr}");

    let out = command(&stl, &scratch, &[]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("cannot open: "), "{stderr}");
    assert!(stderr.contains("is not a STEP file"), "{stderr}");

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
        &scratch.input(),
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

    let missing = command(&scratch.path("work/missing.step"), &scratch, &[])
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
