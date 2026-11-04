//! `specify-core open` (src/open.rs): the protocol of draftwright-editor's docs/OPEN-HANDOFF.md,
//! against the built binary on a real loopback port, with `--no-browser` so no browser opens.
//! The server answers Chrome's Private Network Access preflight, serves the file once to the
//! editor's origin at exactly `Host: 127.0.0.1:<port>` and the one-off path, and then exits;
//! everything else is refused without the file.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use specify_core::open::{self, Handoff};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/thread-depth-alpha7.step.gz")
}

/// A running `specify-core open ... --no-browser`, killed if a test fails before it exits.
struct Opened {
    child: Child,
    stdout: BufReader<ChildStdout>,
    /// The page the browser would open.
    page: String,
    /// The file's loopback address, from the page's `?open=`.
    file: String,
    port: u16,
}

impl Opened {
    fn start(extra: &[&str]) -> Opened {
        Opened::start_on(&fixture(), extra)
    }

    fn start_on(step: &Path, extra: &[&str]) -> Opened {
        let name = step.file_name().unwrap().to_string_lossy().into_owned();
        let mut child = Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .arg("open")
            .arg(step)
            .arg("--no-browser")
            .args(extra)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let page = line
            .trim()
            .strip_prefix(&format!("Open {name}: "))
            .unwrap_or_else(|| panic!("the address line: {line:?}"))
            .to_string();
        let file = decode(page.split_once("?open=").expect("?open=").1);
        let port: u16 = file
            .strip_prefix("http://127.0.0.1:")
            .and_then(|r| r.split('/').next())
            .and_then(|p| p.parse().ok())
            .unwrap_or_else(|| panic!("a loopback address: {file}"));
        Opened {
            child,
            stdout,
            page,
            file,
            port,
        }
    }

    /// The file's path on the server: `/<token>/<name>`.
    fn path(&self) -> String {
        self.file[format!("http://127.0.0.1:{}", self.port).len()..].to_string()
    }

    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Waits for the command to exit: its code and the rest of stdout.
    fn exit(mut self) -> (i32, String) {
        let deadline = Instant::now() + Duration::from_secs(10);
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

/// Percent-decoding, for `?open=`.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
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
}

/// One raw HTTP/1.1 request to the port, with exactly these headers.
fn request(port: u16, method: &str, target: &str, headers: &[(&str, &str)]) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("the port is open");
    let mut head = format!("{method} {target} HTTP/1.1\r\n");
    for (n, v) in headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
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
type Refused<'a> = (&'a str, &'a str, Vec<(&'a str, &'a str)>, u16);

const EDITOR: &str = "https://draftwright.io";

#[test]
fn the_preflight_then_one_get_serves_the_file_and_the_command_exits() {
    let mut opened = Opened::start(&[]);
    let (host, path) = (opened.host(), opened.path());

    let preflight = request(
        opened.port,
        "OPTIONS",
        &path,
        &[
            ("Host", &host),
            ("Origin", EDITOR),
            ("Access-Control-Request-Method", "GET"),
            ("Access-Control-Request-Private-Network", "true"),
        ],
    );
    assert_eq!(preflight.status, 204);
    assert_eq!(
        preflight.header("access-control-allow-origin"),
        Some(EDITOR)
    );
    assert_eq!(preflight.header("vary"), Some("Origin"));
    assert_eq!(
        preflight.header("access-control-allow-methods"),
        Some("GET")
    );
    assert_eq!(
        preflight.header("access-control-allow-private-network"),
        Some("true")
    );
    assert!(preflight.body.is_empty());
    assert!(opened.still_running(), "the preflight does not end it");

    // The other origin the editor is served from.
    let origin = "https://editor.draftwright.io";
    let got = request(
        opened.port,
        "GET",
        &path,
        &[("Host", &host), ("Origin", origin)],
    );
    assert_eq!(got.status, 200);
    assert_eq!(got.body, std::fs::read(fixture()).unwrap());
    assert_eq!(got.header("access-control-allow-origin"), Some(origin));
    assert_eq!(got.header("vary"), Some("Origin"));
    assert_eq!(got.header("cache-control"), Some("no-store"));

    let port = opened.port;
    let (code, rest) = opened.exit();
    assert_eq!(code, 0);
    assert_eq!(rest, "Opened in https://draftwright.io/specify.\n");
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "served once: the port is closed"
    );
}

#[test]
fn requests_other_than_the_editors_are_refused_and_the_file_is_not_served() {
    let mut opened = Opened::start(&[]);
    let (host, path) = (opened.host(), opened.path());
    let bytes = std::fs::read(fixture()).unwrap();
    let other_port = format!("127.0.0.1:{}", opened.port.wrapping_add(1));
    let localhost = format!("localhost:{}", opened.port);
    let token = &path[1..33];
    let other_token = format!("/{}/thread-depth-alpha7.step.gz", "0".repeat(32));
    let other_name = format!("/{token}/other.step");
    let with_query = format!("{path}?x=1");
    let with_fragment = format!("{path}#x");
    let cases: Vec<Refused> = vec![
        (
            "wrong origin",
            &path,
            vec![("Host", &host), ("Origin", "https://example.com")],
            403,
        ),
        (
            "an origin's prefix",
            &path,
            vec![
                ("Host", &host),
                ("Origin", "https://draftwright.io.example.com"),
            ],
            403,
        ),
        ("no origin", &path, vec![("Host", &host)], 403),
        (
            "host localhost",
            &path,
            vec![("Host", &localhost), ("Origin", EDITOR)],
            403,
        ),
        (
            "host on another port",
            &path,
            vec![("Host", &other_port), ("Origin", EDITOR)],
            403,
        ),
        ("no host", &path, vec![("Origin", EDITOR)], 403),
        (
            "rebinding host",
            &path,
            vec![("Host", "attacker.example:80"), ("Origin", EDITOR)],
            403,
        ),
        (
            "wrong token",
            &other_token,
            vec![("Host", &host), ("Origin", EDITOR)],
            404,
        ),
        (
            "wrong name",
            &other_name,
            vec![("Host", &host), ("Origin", EDITOR)],
            404,
        ),
        (
            "the root",
            "/",
            vec![("Host", &host), ("Origin", EDITOR)],
            404,
        ),
        (
            "a query",
            &with_query,
            vec![("Host", &host), ("Origin", EDITOR)],
            404,
        ),
        (
            "a fragment",
            &with_fragment,
            vec![("Host", &host), ("Origin", EDITOR)],
            404,
        ),
    ];
    for (what, target, headers, status) in cases {
        let got = request(opened.port, "GET", target, &headers);
        assert_eq!(got.status, status, "{what}");
        assert!(got.body.is_empty(), "{what}: nothing served");
        assert_eq!(got.header("access-control-allow-origin"), None, "{what}");
        assert!(
            opened.still_running(),
            "{what}: still waiting for the editor"
        );
    }
    // A preflight from elsewhere is refused too, and a method other than GET is not served.
    let preflight = request(
        opened.port,
        "OPTIONS",
        &path,
        &[("Host", &host), ("Origin", "https://example.com")],
    );
    assert_eq!(preflight.status, 403);
    assert_eq!(
        preflight.header("access-control-allow-private-network"),
        None
    );
    let posted = request(
        opened.port,
        "POST",
        &path,
        &[("Host", &host), ("Origin", EDITOR)],
    );
    assert_eq!(posted.status, 405);
    assert!(posted.body.is_empty());
    assert!(opened.still_running());

    // The file was not served: the editor's own request still gets it.
    let got = request(
        opened.port,
        "GET",
        &path,
        &[("Host", &host), ("Origin", EDITOR)],
    );
    assert_eq!(got.status, 200);
    assert_eq!(got.body, bytes);
    assert_eq!(opened.exit().0, 0);
}

#[test]
fn a_name_with_spaces_and_reserved_characters_is_served_at_its_encoded_path() {
    // `?` is not allowed in a Windows file name: there, the name is without it.
    let (name, encoded) = if cfg!(windows) {
        (
            "my part #1 %ä.step.gz",
            "my%20part%20%231%20%25%C3%A4.step.gz",
        )
    } else {
        (
            "my part #1 ?%ä.step.gz",
            "my%20part%20%231%20%3F%25%C3%A4.step.gz",
        )
    };
    let dir = std::env::temp_dir().join(format!("specify-core-open-name-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let step = dir.join(name);
    std::fs::copy(fixture(), &step).unwrap();
    let opened = Opened::start_on(&step, &[]);
    let (host, path) = (opened.host(), opened.path());
    let (token, served) = path[1..].split_once('/').unwrap();
    assert_eq!(token.len(), 32);
    assert!(
        token.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
        "{token}"
    );
    assert_eq!(served, encoded);
    let got = request(
        opened.port,
        "GET",
        &format!("/{token}/{encoded}"),
        &[("Host", &host), ("Origin", EDITOR)],
    );
    assert_eq!(got.status, 200);
    assert_eq!(got.body, std::fs::read(fixture()).unwrap());
    assert_eq!(opened.exit().0, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_default_port_is_left_out_of_site_as_browsers_leave_it_out_of_origin() {
    let origin = |s: &str| open::site_origin(s).unwrap();
    assert_eq!(
        origin("https://Preview.Example:443/"),
        "https://preview.example"
    );
    assert_eq!(origin("http://localhost:80"), "http://localhost");
    assert_eq!(origin("https://x:8443"), "https://x:8443");
    assert_eq!(origin("http://x:443"), "http://x:443");
    assert_eq!(origin("https://x:80"), "https://x:80");
}

#[test]
fn site_is_opened_and_answered_as_well_as_draftwright() {
    let opened = Opened::start(&["--site", "http://localhost:5173/"]);
    assert!(
        opened
            .page
            .starts_with("http://localhost:5173/specify?open="),
        "{}",
        opened.page
    );
    let (host, path) = (opened.host(), opened.path());
    let preflight = request(
        opened.port,
        "OPTIONS",
        &path,
        &[("Host", &host), ("Origin", "http://localhost:5173")],
    );
    assert_eq!(preflight.status, 204);
    assert_eq!(
        preflight.header("access-control-allow-origin"),
        Some("http://localhost:5173")
    );
    let got = request(
        opened.port,
        "GET",
        &path,
        &[("Host", &host), ("Origin", "http://localhost:5173")],
    );
    assert_eq!(got.status, 200);
    assert_eq!(got.body, std::fs::read(fixture()).unwrap());
    let (code, rest) = opened.exit();
    assert_eq!(code, 0);
    assert_eq!(rest, "Opened in http://localhost:5173/specify.\n");
}

/// The address the editor accepts (its `loopbackFile`): http on 127.0.0.1, `/<32 lowercase
/// hex>/<name>`, no query or fragment, and the whole of it encoded as `?open=`'s value.
fn check_address(page: &str, file: &str, prefix: &str) {
    let encoded = page
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("{page}"));
    assert_eq!(encoded, open::encode_component(file));
    assert!(!encoded.contains(['/', ':', '?', '&', '#']), "{encoded}");
    let rest = file.strip_prefix("http://127.0.0.1:").unwrap();
    let (port, path) = rest.split_once('/').unwrap();
    assert!(port.parse::<u16>().is_ok());
    let (token, name) = path.split_once('/').unwrap();
    assert_eq!(token.len(), 32);
    assert!(
        token.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
        "{token}"
    );
    assert_eq!(name, "thread-depth-alpha7.step.gz");
    assert!(!file.contains(['?', '#']));
}

#[test]
fn the_page_is_specify_or_with_editor_the_full_editor() {
    let specify = Opened::start(&[]);
    check_address(
        &specify.page,
        &specify.file,
        "https://draftwright.io/specify?open=",
    );
    let editor = Opened::start(&["--editor"]);
    check_address(
        &editor.page,
        &editor.file,
        "https://draftwright.io/editor?open=",
    );
    assert_ne!(specify.path(), editor.path(), "a fresh token each time");
}

#[test]
fn names_are_encoded_as_encode_uri_component_does() {
    assert_eq!(
        open::encode_component("my part (v2)·ä.step"),
        "my%20part%20(v2)%C2%B7%C3%A4.step"
    );
    assert_eq!(
        open::encode_component("a&b=c?d#e/f%.stp"),
        "a%26b%3Dc%3Fd%23e%2Ff%25.stp"
    );
    assert_eq!(
        open::page_url(
            "https://draftwright.io",
            false,
            "http://127.0.0.1:1/x/a b.step"
        ),
        "https://draftwright.io/specify?open=http%3A%2F%2F127.0.0.1%3A1%2Fx%2Fa%20b.step"
    );
    assert_eq!(
        open::page_url(
            "https://draftwright.io",
            true,
            "http://127.0.0.1:1/x/a.step"
        ),
        "https://draftwright.io/editor?open=http%3A%2F%2F127.0.0.1%3A1%2Fx%2Fa.step"
    );
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

#[test]
fn a_name_the_editor_would_not_open_or_a_bad_site_is_refused_up_front() {
    let dir = std::env::temp_dir().join(format!("specify-core-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("part.stl");
    std::fs::write(&stl, b"solid").unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .arg("open")
            .args(args)
            .arg("--no-browser")
            .output()
            .unwrap()
    };
    let out = run(&[stl.as_os_str()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("cannot open: "), "{stderr}");
    assert!(stderr.contains("is not a STEP file"), "{stderr}");

    for site in [
        "draftwright.io",
        "ftp://x",
        "https://x/path",
        "https://a b",
        "https://",
    ] {
        let out = run(&[fixture().as_os_str(), "--site".as_ref(), site.as_ref()]);
        assert_eq!(out.status.code(), Some(2), "{site}");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(
            stderr.starts_with("cannot open: --site"),
            "{site}: {stderr}"
        );
    }

    let missing = dir.join("missing.step");
    let out = run(&[missing.as_os_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .starts_with("error: ")
    );

    let help = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(["open", "-h"])
        .output()
        .unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .starts_with("usage: specify-core open")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_fetched_in_time_gives_up() {
    let handoff = Handoff::new(&fixture(), None, false).unwrap();
    let port = handoff.port();
    let err = handoff.serve(Duration::from_millis(200)).unwrap_err();
    assert!(matches!(err, open::Error::TimedOut(_)));
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the port is closed"
    );
    let message = open::Error::TimedOut(open::TIMEOUT).to_string();
    assert_eq!(
        message,
        "nothing fetched the file within 10 minutes; stopping"
    );
}
