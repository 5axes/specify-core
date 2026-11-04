//! `specify-core open`: a STEP file opened in draftwright.io/specify, in the person's own
//! browser, never uploaded (draftwright-editor's docs/OPEN-HANDOFF.md, "From this computer").
//!
//! The file is served once, on 127.0.0.1 at `/<32 lowercase hex>/<name>` (the token from the
//! operating system's random source), and the browser opened at
//! `https://draftwright.io/specify?open=<that address>`, which fetches it. The server answers
//! only a request whose `Host` is exactly `127.0.0.1:<port>` (a page that finds the port through
//! DNS rebinding is refused) and whose `Origin` is the editor's, allows Chrome's Private Network
//! Access preflight, serves the file to one `GET` and stops. A request with no `Origin` gets
//! 403: stricter than the editor's scripts/dw-open.mjs, and right, because the editor's
//! cross-origin fetch always sends one.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The editor's own origins, always answered.
pub const ORIGINS: [&str; 2] = ["https://draftwright.io", "https://editor.draftwright.io"];

/// How long the file waits to be fetched.
pub const TIMEOUT: Duration = Duration::from_secs(600);

/// Why the file was not opened.
#[derive(Debug)]
pub enum Error {
    /// The name, or `--site`, refused before anything is served (the command line's exit 2).
    Refused(String),
    /// The file could not be read, the port opened or the token drawn.
    Io(String),
    /// Nothing fetched the file in time.
    TimedOut(Duration),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Refused(m) | Error::Io(m) => f.write_str(m),
            Error::TimedOut(t) => write!(
                f,
                "nothing fetched the file within {} minutes; stopping",
                t.as_secs() / 60
            ),
        }
    }
}

/// Whether the editor opens a file of this name: `.step` or `.stp`, or either gzipped, in any
/// case (the editor's `loopbackFile`).
pub fn is_step_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.strip_suffix(".gz").unwrap_or(&lower);
    base.ends_with(".step") || base.ends_with(".stp")
}

/// JavaScript's `encodeURIComponent`: every byte of the UTF-8 text percent-encoded except
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
pub fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `--site`: an origin, `http(s)://host[:port]` with nothing after it but an optional `/`,
/// lowercased and without a default port (`:443` for https, `:80` for http), as a browser
/// reports it in `Origin`.
pub fn site_origin(site: &str) -> Result<String, Error> {
    let refused = || {
        Error::Refused(format!(
            "--site takes an origin such as https://draftwright.io, not {site}"
        ))
    };
    let lower = site.trim_end_matches('/').to_ascii_lowercase();
    let host = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .ok_or_else(refused)?;
    let allowed = |c: char| c.is_ascii_alphanumeric() || ".-:[]".contains(c);
    if host.is_empty() || !host.chars().all(allowed) {
        return Err(refused());
    }
    let default_port = if lower.starts_with("https://") {
        ":443"
    } else {
        ":80"
    };
    Ok(lower
        .strip_suffix(default_port)
        .unwrap_or(&lower)
        .to_string())
}

/// The page the browser opens: `/specify`, or the full editor at `/editor` (on draftwright.io,
/// `/` is the landing page), with `?open=` the file's loopback address.
pub fn page_url(site: &str, editor: bool, file_url: &str) -> String {
    let page = if editor { "/editor" } else { "/specify" };
    format!("{site}{page}?open={}", encode_component(file_url))
}

/// 16 bytes from the operating system's random source, as 32 lowercase hex characters.
fn token() -> Result<String, Error> {
    let mut bytes = [0u8; 16];
    random(&mut bytes).map_err(|e| Error::Io(format!("cannot draw a random token: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(unix)]
fn random(buf: &mut [u8]) -> io::Result<()> {
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

#[cfg(windows)]
fn random(buf: &mut [u8]) -> io::Result<()> {
    // BCryptGenRandom with the system's preferred generator (BCRYPT_USE_SYSTEM_PREFERRED_RNG),
    // from bcrypt.dll, which every supported Windows has.
    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(alg: *mut u8, buf: *mut u8, len: u32, flags: u32) -> i32;
    }
    let len = u32::try_from(buf.len()).map_err(io::Error::other)?;
    // SAFETY: a null algorithm handle is allowed with this flag; `buf` is valid for `len` bytes.
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), len, 2) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!("BCryptGenRandom: {status:#x}")))
    }
}

#[cfg(not(any(unix, windows)))]
fn random(_: &mut [u8]) -> io::Result<()> {
    Err(io::Error::other("no random source on this platform"))
}

/// A file ready to be fetched once: its port open, its address and the page to open known.
pub struct Handoff {
    listener: TcpListener,
    port: u16,
    path: String,
    name: String,
    bytes: Arc<Vec<u8>>,
    origins: Vec<String>,
    page: String,
}

impl Handoff {
    /// Reads `step` (a `.step` or `.stp` name, or either gzipped) and opens a port for it on
    /// 127.0.0.1. `site` is another deployment's origin, answered as well as the editor's own
    /// and opened instead of draftwright.io; `editor` opens `/editor` instead of `/specify`.
    pub fn new(step: &Path, site: Option<&str>, editor: bool) -> Result<Handoff, Error> {
        let name = step
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !is_step_name(&name) {
            return Err(Error::Refused(format!(
                "{} is not a STEP file: the editor opens .step or .stp, or either gzipped",
                step.display()
            )));
        }
        let site = match site {
            Some(s) => Some(site_origin(s)?),
            None => None,
        };
        let bytes =
            std::fs::read(step).map_err(|e| Error::Io(format!("{}: {e}", step.display())))?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|e| Error::Io(format!("cannot open a port on 127.0.0.1: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::Io(e.to_string()))?
            .port();
        let path = format!("/{}/{}", token()?, encode_component(&name));
        let mut origins: Vec<String> = ORIGINS.iter().map(|o| o.to_string()).collect();
        if let Some(s) = &site
            && !origins.contains(s)
        {
            origins.push(s.clone());
        }
        let file_url = format!("http://127.0.0.1:{port}{path}");
        let page = page_url(site.as_deref().unwrap_or(ORIGINS[0]), editor, &file_url);
        Ok(Handoff {
            listener,
            port,
            path,
            name,
            bytes: Arc::new(bytes),
            origins,
            page,
        })
    }

    /// The page to open in the browser.
    pub fn page(&self) -> &str {
        &self.page
    }

    /// The file's name, as served.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The loopback port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Answers requests until the file has been served once, then closes the port; or gives up
    /// after `timeout` if nothing has been served (a transfer under way is never cut off). Each connection is answered on its own thread, so a browser's idle
    /// preconnection does not hold up the fetch.
    pub fn serve(self, timeout: Duration) -> Result<(), Error> {
        let io = |e: io::Error| Error::Io(format!("serving on 127.0.0.1:{}: {e}", self.port));
        self.listener.set_nonblocking(true).map_err(io)?;
        let deadline = Instant::now() + timeout;
        let served = Arc::new(AtomicBool::new(false));
        let (done, finished) = mpsc::channel::<()>();
        let rules = Arc::new(Rules {
            host: format!("127.0.0.1:{}", self.port),
            path: self.path,
            origins: self.origins,
        });
        loop {
            if finished.try_recv().is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline && !served.load(Ordering::SeqCst) {
                return Err(Error::TimedOut(timeout));
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let (rules, bytes, served, done) = (
                        rules.clone(),
                        self.bytes.clone(),
                        served.clone(),
                        done.clone(),
                    );
                    // A thread that cannot be started drops the connection, which the browser
                    // can retry.
                    let _ = thread::Builder::new().spawn(move || {
                        if answer(stream, &rules, &bytes, &served) {
                            let _ = done.send(());
                        }
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(io(e)),
            }
        }
    }
}

/// What a request must match.
struct Rules {
    host: String,
    path: String,
    origins: Vec<String>,
}

/// A request's method, target and headers, as far as they are read.
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Reads a request's head (up to 16 KiB, all of it within 10 s), or None.
fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        if head.len() > 16 * 1024 {
            return None;
        }
        let left = deadline.checked_duration_since(Instant::now())?;
        if left.is_zero() {
            return None;
        }
        stream.set_read_timeout(Some(left)).ok()?;
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        head.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target, version) = (first.next()?, first.next()?, first.next()?);
    if !version.starts_with("HTTP/1.") || first.next().is_some() {
        return None;
    }
    let headers = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .collect();
    Some(Request {
        method: method.to_string(),
        target: target.to_string(),
        headers,
    })
}

/// Answers one connection; true when it was the one that served the file.
fn answer(mut stream: TcpStream, rules: &Rules, bytes: &[u8], served: &AtomicBool) -> bool {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let Some(request) = read_request(&mut stream) else {
        let _ = respond(&mut stream, "400 Bad Request", &[], b"");
        return false;
    };
    // This address only: a page elsewhere that finds the port (DNS rebinding) is refused.
    if request.header("host") != Some(rules.host.as_str()) {
        let _ = respond(&mut stream, "403 Forbidden", &[], b"");
        return false;
    }
    // The one address, exactly: no other path, no query, nothing once it has been served.
    if request.target != rules.path || served.load(Ordering::SeqCst) {
        let _ = respond(&mut stream, "404 Not Found", &[], b"");
        return false;
    }
    // The editor's fetch, from its own origin only.
    let Some(origin) = request
        .header("origin")
        .filter(|o| rules.origins.iter().any(|a| a == o))
    else {
        let _ = respond(&mut stream, "403 Forbidden", &[], b"");
        return false;
    };
    let cors = [("Access-Control-Allow-Origin", origin), ("Vary", "Origin")];
    match request.method.as_str() {
        // Chrome asks first when a page on the internet reaches this computer (Private Network
        // Access); this allows it.
        "OPTIONS" => {
            let headers = [
                cors[0],
                cors[1],
                ("Access-Control-Allow-Methods", "GET"),
                ("Access-Control-Allow-Private-Network", "true"),
            ];
            let _ = respond(&mut stream, "204 No Content", &headers, b"");
            false
        }
        "GET" => {
            if served.swap(true, Ordering::SeqCst) {
                let _ = respond(&mut stream, "404 Not Found", &[], b"");
                return false;
            }
            let headers = [
                cors[0],
                cors[1],
                ("Content-Type", "application/octet-stream"),
                ("Cache-Control", "no-store"),
            ];
            // Served or not, it is served once: a failed send is not offered again.
            let _ = respond(&mut stream, "200 OK", &headers, bytes);
            true
        }
        _ => {
            let _ = respond(
                &mut stream,
                "405 Method Not Allowed",
                &[("Allow", "GET, OPTIONS")],
                b"",
            );
            false
        }
    }
}

/// Writes a response and closes the connection.
fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    stream.shutdown(Shutdown::Write)
}

/// Opens `url` in the default browser: `open` on macOS, `cmd /c start "" URL` on Windows,
/// `xdg-open` elsewhere.
pub fn launch_browser(url: &str) -> io::Result<()> {
    use std::process::{Command, Stdio};
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/c", "start", "", url]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}
