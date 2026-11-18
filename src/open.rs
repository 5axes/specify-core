//! The Specify page server (`specify-core open`, and MCP's `show`): one session's STEP file
//! opened in the Specify app, served on 127.0.0.1 to the person's own browser, never uploaded,
//! and saved back to disk by this process from the session's answers (src/session.rs).
//!
//! Everything is under `http://127.0.0.1:<port>/<token>/` (the token 32 lowercase hex from the
//! operating system's random source), see docs/open.md:
//!
//! - `GET <token>/…` the static app (`index.html` at `<token>/`), `GET <token>/wasm/…` the
//!   engine, `GET <token>/file` the STEP bytes (`?rev=old` OLD's in a diff), `GET
//!   <token>/config` the app's settings, `GET <token>/diff` a diff's report;
//! - `GET`/`PUT <token>/answers` the session's answers, `GET <token>/events` their changes as
//!   server-sent events;
//! - `POST <token>/save` the specified STEP written from the session's answers, next to the
//!   input (never over a file, unless in place, which keeps the original once as `.bak`);
//! - `POST <token>/closed` the page went away: the server stops [`CLOSE_DELAY`] after the last
//!   request unless another arrives (a reload, or another open page's heartbeat).
//!
//! An open page sends `GET <token>/config` every [`HEARTBEAT`]; any request resets the idle
//! time and cancels a pending close, and an open event stream keeps the server running.
//!
//! A request whose `Host` is not exactly `127.0.0.1:<port>` is refused (a page that finds the
//! port through DNS rebinding), as is any other token, without saying why. `save`, `PUT
//! answers` and `closed` must carry `Origin: http://127.0.0.1:<port>`; a `GET` with any other
//! `Origin` is refused. A forgotten server stops after [`IDLE`] without a request.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::session;

/// How often an open page asks for `config` (the contract's heartbeat; the server does not
/// use it, it sizes [`CLOSE_DELAY`]).
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// How long after `closed` the server stops, unless another request arrives: longer than
/// [`HEARTBEAT`], so another open page keeps it.
pub const CLOSE_DELAY: Duration = Duration::from_secs(90);

/// How long the server waits without a request before it stops (a forgotten server: an open
/// page's heartbeat keeps it).
pub const IDLE: Duration = Duration::from_secs(30 * 60);

/// The largest `save` body accepted.
pub const MAX_SAVE_BODY: u64 = 64 * 1024;

/// The largest `PUT answers` body accepted.
pub const MAX_ANSWERS: u64 = 16 * 1024 * 1024;

/// The most event streams open at once; another is answered 503.
pub const MAX_STREAMS: usize = 4;

/// How often an event stream with nothing to say sends a comment.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The most connections answered at once; another is answered 503 and closed.
pub const MAX_CONNECTIONS: usize = 16;

/// The engine's entry point, relative to the app (`config.wasmUrl`).
pub const WASM_URL: &str = "wasm/specify_core_wasm.js";

/// The page's Content-Security-Policy: everything from this server only, the engine compiled
/// with `'wasm-unsafe-eval'`, inline styles (which UI libraries set), `data:` fonts and
/// `data:`/`blob:` images and downloads allowed.
pub const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; \
connect-src 'self' blob:; \
worker-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; \
form-action 'none'";

/// Why the file was not opened.
#[derive(Debug)]
pub enum Error {
    /// Refused before anything is served: a name that is not STEP, no app (exit 2).
    Refused(String),
    /// The file could not be read, the port opened or the token drawn.
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Refused(m) | Error::Io(m) => f.write_str(m),
        }
    }
}

/// Whether Specify opens a file of this name: `.step` or `.stp`, or either gzipped, in any
/// case.
pub fn is_step_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.strip_suffix(".gz").unwrap_or(&lower);
    base.ends_with(".step") || base.ends_with(".stp")
}

/// Compares two strings without stopping at the first difference (tokens).
pub(crate) fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// 16 bytes from the operating system's random source, as 32 lowercase hex characters (also
/// `specify-core mcp --http`'s bearer token).
pub(crate) fn token() -> Result<String, Error> {
    let mut bytes = [0u8; 16];
    random(&mut bytes).map_err(|e| Error::Io(format!("cannot draw a random token: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(unix)]
fn random(buf: &mut [u8]) -> io::Result<()> {
    fs::File::open("/dev/urandom")?.read_exact(buf)
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

/// Where the app and the engine are served from.
#[derive(Debug, Clone)]
pub struct Assets {
    /// The static app: `index.html` at its root.
    pub app: PathBuf,
    /// A wasm-pack `pkg/` directory served at `wasm/`; without one, `wasm/` is the app's own.
    pub wasm: Option<PathBuf>,
}

/// The app and engine directories: `--app`/`--wasm`, else `SPECIFY_APP_DIR`/
/// `SPECIFY_WASM_DIR`. The app is not bundled yet; this is where a download will go.
pub fn assets(app: Option<&Path>, wasm: Option<&Path>) -> Result<Assets, Error> {
    let from_env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let app = app
        .map(Path::to_path_buf)
        .or_else(|| from_env("SPECIFY_APP_DIR"))
        .ok_or_else(|| {
            Error::Refused(
                "the Specify app is not bundled with specify-core yet: pass --app DIR (or set \
                 SPECIFY_APP_DIR) to a directory holding the built app's index.html, and --wasm \
                 DIR (or SPECIFY_WASM_DIR) to the engine's wasm-pack pkg/ directory"
                    .to_string(),
            )
        })?;
    let app = fs::canonicalize(&app)
        .ok()
        .filter(|d| d.join("index.html").is_file())
        .ok_or_else(|| {
            Error::Refused(format!(
                "--app {}: not a directory holding index.html",
                app.display()
            ))
        })?;
    let wasm = match wasm
        .map(Path::to_path_buf)
        .or_else(|| from_env("SPECIFY_WASM_DIR"))
    {
        Some(w) => Some(
            fs::canonicalize(&w)
                .ok()
                .filter(|d| d.is_dir())
                .ok_or_else(|| {
                    Error::Refused(format!("--wasm {}: not a directory", w.display()))
                })?,
        ),
        None => None,
    };
    Ok(Assets { app, wasm })
}

/// How the page is served.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// No answer changes and no saves from the page (409).
    pub read_only: bool,
    /// The port on 127.0.0.1 (0: any free port).
    pub port: u16,
    /// `config.part`: the part the page is shown for (MCP's `show` of a document of a file of
    /// several parts); None for the whole file.
    pub part: Option<usize>,
}

/// The page server for one session, its port open and its app found, ready to serve.
pub struct App {
    listener: TcpListener,
    port: u16,
    token: String,
    shared: Shared,
}

/// A page server started in the background (MCP's `show`): its address and mode, whether it
/// is still running, and how to stop it.
pub struct Page {
    pub url: String,
    pub mode: &'static str,
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Page {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Stops the page server (its event streams end).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts a page server for `session` on a thread of its own (MCP's `show`): it stops as
/// [`App::serve`] does, or when the returned [`Page`] is stopped or dropped. Its notices go to
/// stderr (stdout may carry a protocol).
pub fn start(
    session: session::Shared,
    assets: Assets,
    options: &Options,
    close_delay: Duration,
    idle: Duration,
) -> Result<Page, Error> {
    let app = App::new(session, assets, options)?;
    let url = app.url();
    let mode = app.shared.mode;
    let stop = Arc::clone(&app.shared.stop);
    let running = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&running);
    thread::Builder::new()
        .spawn(move || {
            let _ = app.serve(close_delay, idle, |n| eprintln!("{n}"));
            flag.store(false, Ordering::SeqCst);
        })
        .map_err(|e| Error::Io(format!("cannot start the page server: {e}")))?;
    Ok(Page {
        url,
        mode,
        running,
        stop,
    })
}

impl App {
    /// Opens a port on 127.0.0.1 for `session`'s page.
    pub fn new(session: session::Shared, assets: Assets, options: &Options) -> Result<App, Error> {
        let (diff, in_place, name) = {
            let s = session::lock(&session);
            (s.old().is_some(), s.in_place(), s.file().name.clone())
        };
        if options.read_only && in_place {
            return Err(Error::Refused(
                "--read-only and --in-place cannot be combined".to_string(),
            ));
        }
        if options.read_only && diff {
            return Err(Error::Refused(
                "--read-only and --diff cannot be combined".to_string(),
            ));
        }
        let mode = if options.read_only {
            "read-only"
        } else if diff {
            "diff"
        } else {
            "specify"
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, options.port)).map_err(|e| {
            Error::Io(format!(
                "cannot open port {} on 127.0.0.1: {e}",
                options.port
            ))
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::Io(e.to_string()))?
            .port();
        let token = token()?;
        let shared = Shared {
            host: format!("127.0.0.1:{port}"),
            origin: format!("http://127.0.0.1:{port}"),
            prefix: format!("/{token}/"),
            name,
            mode,
            session,
            assets,
            read_only: options.read_only,
            part: options.part,
            streams: AtomicUsize::new(0),
            stop: Arc::new(AtomicBool::new(false)),
            activity: Mutex::new(Activity {
                last: Instant::now(),
                accepted: 0,
                close_at: None,
            }),
        };
        Ok(App {
            listener,
            port,
            token,
            shared,
        })
    }

    /// The app's address, to open in the browser.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/{}/", self.port, self.token)
    }

    /// The file's name (`NEW`'s in a diff).
    pub fn name(&self) -> &str {
        &self.shared.name
    }

    /// The loopback port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Answers requests, each connection on its own thread (at most [`MAX_CONNECTIONS`] at
    /// once), until `close_delay` after `closed` with no request and no event stream since, or
    /// `idle` without a request (an open event stream keeps it running). `notice` gets each
    /// save's and the stop's one-line notice.
    pub fn serve(
        self,
        close_delay: Duration,
        idle: Duration,
        mut notice: impl FnMut(&str),
    ) -> Result<(), Error> {
        let port = self.port;
        let io = |e: io::Error| Error::Io(format!("serving on 127.0.0.1:{port}: {e}"));
        self.listener.set_nonblocking(true).map_err(io)?;
        let shared = Arc::new(self.shared);
        let active = Arc::new(AtomicUsize::new(0));
        let (tell, notices) = mpsc::channel::<String>();
        loop {
            while let Ok(n) = notices.try_recv() {
                notice(&n);
            }
            if shared.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            if active.load(Ordering::SeqCst) == 0 {
                let now = Instant::now();
                let a = lock(&shared.activity);
                if a.close_at.is_some_and(|t| now >= t) {
                    notice("Closed.");
                    return Ok(());
                }
                if now.duration_since(a.last) >= idle {
                    notice(&format!(
                        "Stopped: no request for {} minutes.",
                        idle.as_secs() / 60
                    ));
                    return Ok(());
                }
            }
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let seq = {
                        let mut a = lock(&shared.activity);
                        a.last = Instant::now();
                        a.accepted += 1;
                        a.close_at = None;
                        a.accepted
                    };
                    let _ = stream.set_nonblocking(false);
                    if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        active.fetch_sub(1, Ordering::SeqCst);
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                        let _ = respond(
                            &mut stream,
                            "503 Service Unavailable",
                            &[("Retry-After", "1")],
                            b"",
                        );
                        continue;
                    }
                    let slot = Slot(active.clone());
                    let (shared, tell) = (shared.clone(), tell.clone());
                    // A thread that cannot be started drops the connection (and its slot),
                    // which the browser can retry.
                    let _ = thread::Builder::new().spawn(move || {
                        let _slot = slot;
                        let ctx = Ctx {
                            shared: &shared,
                            seq,
                            close_delay,
                            tell: &tell,
                        };
                        exchange(&mut stream, &ctx);
                        drain(&mut stream);
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

/// What every connection shares.
struct Shared {
    host: String,
    origin: String,
    /// `/<token>/`.
    prefix: String,
    name: String,
    /// `config.mode`: `specify`, `read-only` or `diff`.
    mode: &'static str,
    session: session::Shared,
    assets: Assets,
    read_only: bool,
    /// `config.part`.
    part: Option<usize>,
    /// Event streams open now.
    streams: AtomicUsize,
    /// Set to stop the server (and end its event streams).
    stop: Arc<AtomicBool>,
    activity: Mutex<Activity>,
}

/// When the server last heard from anyone, and whether it is closing.
struct Activity {
    last: Instant,
    /// Connections accepted so far: `closed` stops the server only if none came after it.
    accepted: u64,
    close_at: Option<Instant>,
}

struct Ctx<'a> {
    shared: &'a Shared,
    /// This connection's place in [`Activity::accepted`].
    seq: u64,
    close_delay: Duration,
    tell: &'a mpsc::Sender<String>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A served connection, counted until its thread ends.
pub(crate) struct Slot(pub(crate) Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The most read from a connection after its response before it is closed anyway.
const MAX_DRAIN: usize = 1024 * 1024;

/// After the response is sent and the write side shut: reads what the client still sends (a
/// body a refusal never read) until it closes, briefly, so closing does not reset the
/// connection and lose the response.
pub(crate) fn drain(stream: &mut TcpStream) {
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut sink = [0u8; 16 * 1024];
    let mut left = MAX_DRAIN;
    while left > 0 {
        let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if wait.is_zero() || stream.set_read_timeout(Some(wait)).is_err() {
            break;
        }
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => left = left.saturating_sub(n),
        }
    }
}

/// Headers on every answer from the app's address.
const COMMON: [(&str, &str); 4] = [
    ("Cache-Control", "no-store"),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("Content-Security-Policy", CSP),
];

fn reply(stream: &mut TcpStream, status: &str, content_type: Option<&str>, body: &[u8]) {
    let mut headers: Vec<(&str, &str)> = COMMON.to_vec();
    if let Some(t) = content_type {
        headers.push(("Content-Type", t));
    }
    let _ = respond(stream, status, &headers, body);
}

fn reply_json(stream: &mut TcpStream, status: &str, value: &serde_json::Value) {
    reply(
        stream,
        status,
        Some("application/json"),
        value.to_string().as_bytes(),
    );
}

fn refuse(stream: &mut TcpStream, status: &str, error: &str) {
    reply_json(stream, status, &json!({ "error": error }));
}

/// The value of `name` in a request target's query, if it has one.
fn query<'a>(target: &'a str, name: &str) -> Option<&'a str> {
    target
        .split_once('?')?
        .1
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

/// Answers one request.
fn exchange(stream: &mut TcpStream, ctx: &Ctx) {
    let shared = ctx.shared;
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let Some((request, body)) = read_head(stream) else {
        let _ = respond(stream, "400 Bad Request", &[], b"");
        return;
    };
    // This address only: a page that finds the port through DNS rebinding is refused.
    if request.header("host") != Some(shared.host.as_str()) {
        let _ = respond(stream, "403 Forbidden", &[], b"");
        return;
    }
    let path = request.target.split('?').next().unwrap_or("");
    // The token, exactly (compared in constant time); anything else is not found, without
    // saying more.
    let token = &shared.prefix[1..shared.prefix.len() - 1];
    let (first, rest) = match path.strip_prefix('/') {
        Some(p) => match p.split_once('/') {
            Some((first, rest)) => (first, Some(rest)),
            None => (p, None),
        },
        None => ("", None),
    };
    if !same(first, token) {
        let _ = respond(stream, "404 Not Found", &[], b"");
        return;
    }
    let Some(rest) = rest else {
        let location = shared.prefix.as_str();
        let _ = respond(
            stream,
            "308 Permanent Redirect",
            &[("Location", location)],
            b"",
        );
        return;
    };
    let origin = request.header("origin");
    let same_origin = origin == Some(shared.origin.as_str());
    let diff = shared.mode == "diff";
    match (request.method.as_str(), rest) {
        ("POST", "save") => {
            if !same_origin {
                return refuse(stream, "403 Forbidden", "save only from the app's own page");
            }
            save(stream, ctx, &request, body);
        }
        ("PUT", "answers") => {
            if !same_origin {
                return refuse(
                    stream,
                    "403 Forbidden",
                    "answers change only from the app's own page",
                );
            }
            put_answers(stream, ctx, &request, body);
        }
        ("POST", "closed") => {
            if !same_origin {
                return refuse(stream, "403 Forbidden", "only from the app's own page");
            }
            {
                let mut a = lock(&shared.activity);
                if a.accepted == ctx.seq {
                    a.close_at = Some(Instant::now() + ctx.close_delay);
                }
            }
            reply(stream, "204 No Content", None, b"");
        }
        (_, "save" | "closed") => {
            let _ = respond(stream, "405 Method Not Allowed", &[("Allow", "POST")], b"");
        }
        ("GET", _) if origin.is_some() && !same_origin => {
            let _ = respond(stream, "403 Forbidden", &[], b"");
        }
        ("GET", "answers") => {
            let s = session::lock(&shared.session);
            let answers = json!({"rev": s.rev(), "parts": s.parts()});
            drop(s);
            reply_json(stream, "200 OK", &answers);
        }
        (_, "answers") => {
            let _ = respond(
                stream,
                "405 Method Not Allowed",
                &[("Allow", "GET, PUT")],
                b"",
            );
        }
        ("GET", "file") => {
            let bytes = match query(&request.target, "rev") {
                None | Some("new") => session::lock(&shared.session).current(),
                Some("old") if diff => {
                    let s = session::lock(&shared.session);
                    Arc::clone(&s.old().expect("diff mode has OLD").bytes)
                }
                Some(_) => return reply(stream, "404 Not Found", None, b""),
            };
            reply(stream, "200 OK", Some("application/octet-stream"), &bytes);
        }
        ("GET", "diff") => {
            let report = session::lock(&shared.session).comparison().cloned();
            match report {
                Some(report) if diff => reply_json(stream, "200 OK", &report),
                _ => reply(stream, "404 Not Found", None, b""),
            }
        }
        ("GET", "events") => events(stream, ctx),
        ("GET", "config") => {
            let s = session::lock(&shared.session);
            let target = if shared.read_only {
                None
            } else {
                s.save_target().map(|p| p.display().to_string())
            };
            let old = s.old();
            let config = json!({
                "mode": shared.mode,
                "name": s.file().name,
                "path": s.file().path.display().to_string(),
                "part": shared.part,
                "oldName": old.map(|o| o.name.clone()),
                "oldPath": old.map(|o| o.path.display().to_string()),
                "readOnly": shared.read_only,
                "wasmUrl": WASM_URL,
                "saveTarget": target,
                "specifyCore": env!("CARGO_PKG_VERSION"),
            });
            drop(s);
            reply_json(stream, "200 OK", &config);
        }
        ("GET", rest) => {
            let rest = if rest.is_empty() { "index.html" } else { rest };
            let (root, rel) = match (&shared.assets.wasm, rest.strip_prefix("wasm/")) {
                (Some(wasm), Some(rel)) => (wasm, rel),
                _ => (&shared.assets.app, rest),
            };
            match static_file(root, rel) {
                Some((bytes, content_type)) => reply(stream, "200 OK", Some(content_type), &bytes),
                None => reply(stream, "404 Not Found", None, b""),
            }
        }
        _ => {
            let _ = respond(stream, "405 Method Not Allowed", &[("Allow", "GET")], b"");
        }
    }
}

/// A JSON request body: `Content-Type: application/json` (415), a `Content-Length` and no
/// `Transfer-Encoding` (411), at most `max` bytes (413), all of it within 30 s and 1 s more per
/// MiB (408), parsed (400). The refusal's status and message otherwise.
fn json_body(
    stream: &mut TcpStream,
    request: &Request,
    mut body: Vec<u8>,
    max: u64,
) -> Result<serde_json::Value, (&'static str, String)> {
    let is_json = request
        .header("content-type")
        .and_then(|t| t.split(';').next())
        .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"));
    if !is_json {
        return Err((
            "415 Unsupported Media Type",
            "send the body as application/json".into(),
        ));
    }
    // The body is read by its Content-Length only: a chunked (or otherwise encoded) body is
    // refused rather than misread.
    let length = request
        .header("content-length")
        .and_then(|l| l.trim().parse::<u64>().ok());
    let (Some(length), None) = (length, request.header("transfer-encoding")) else {
        return Err((
            "411 Length Required",
            "Content-Length is required (no Transfer-Encoding)".into(),
        ));
    };
    if length > max {
        return Err(("413 Content Too Large", format!("larger than {max} bytes")));
    }
    let deadline = Instant::now() + body_time(length);
    let length = length as usize;
    body.truncate(length);
    let mut chunk = vec![0u8; 64 * 1024];
    while body.len() < length {
        let Some(left) = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
        else {
            return Err(("408 Request Timeout", "the body came too slowly".into()));
        };
        if stream
            .set_read_timeout(Some(left.min(Duration::from_secs(30))))
            .is_err()
        {
            return Err(("400 Bad Request", "the body cannot be read".into()));
        }
        match stream.read(&mut chunk[..(length - body.len()).min(64 * 1024)]) {
            Ok(0) | Err(_) => {
                return Err(("400 Bad Request", "the body ended early".into()));
            }
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    serde_json::from_slice(&body).map_err(|e| ("400 Bad Request", format!("not JSON: {e}")))
}

/// The longest a body may take to arrive: 30 s, and 1 s more per MiB (a trickling client
/// cannot hold a connection indefinitely).
fn body_time(length: u64) -> Duration {
    Duration::from_secs(30 + length / (1024 * 1024))
}

/// `PUT <token>/answers`, from the app's own origin: part `part`'s answers replaced when `rev`
/// is current.
fn put_answers(stream: &mut TcpStream, ctx: &Ctx, request: &Request, body: Vec<u8>) {
    let shared = ctx.shared;
    if shared.read_only {
        return refuse(
            stream,
            "409 Conflict",
            "opened read-only: no answer changes",
        );
    }
    let value = match json_body(stream, request, body, MAX_ANSWERS) {
        Ok(v) => v,
        Err((status, why)) => return refuse(stream, status, &why),
    };
    let (Some(rev), Some(part), Some(answers)) = (
        value["rev"].as_u64(),
        value["part"].as_u64(),
        value["answers"].as_object(),
    ) else {
        return refuse(
            stream,
            "400 Bad Request",
            "send {\"rev\": n, \"part\": k, \"answers\": {question id: answer}}",
        );
    };
    let part = part as usize;
    let stale = |s: &session::Session| {
        json!({"error": "the answers have changed since that rev: apply the change to these and try again",
               "rev": s.rev(), "parts": s.parts()})
    };
    let mut s = session::lock(&shared.session);
    if rev != s.rev() {
        let current = stale(&s);
        drop(s);
        return reply_json(stream, "409 Conflict", &current);
    }
    if part >= s.part_count() {
        let count = s.part_count();
        drop(s);
        return refuse(
            stream,
            "400 Bad Request",
            &format!("the file has {count} part(s): there is no part {part}"),
        );
    }
    // A part the agent has not opened is opened in the session on its first change: analysed
    // without the session's lock, then the rev checked again.
    if !s.is_open(part) {
        drop(s);
        if let Err(e) = session::analysis(&shared.session, part) {
            return refuse(stream, "400 Bad Request", &format!("part {part}: {e}"));
        }
        s = session::lock(&shared.session);
        if rev != s.rev() {
            let current = stale(&s);
            drop(s);
            return reply_json(stream, "409 Conflict", &current);
        }
    }
    s.set(part, answers.clone(), "page");
    let rev = s.rev();
    drop(s);
    reply_json(stream, "200 OK", &json!({ "rev": rev }));
}

/// `GET <token>/events`: the session's changes as server-sent events, the current state first;
/// a comment every [`KEEP_ALIVE`]. At most [`MAX_STREAMS`] at once.
fn events(stream: &mut TcpStream, ctx: &Ctx) {
    let shared = ctx.shared;
    if shared.streams.fetch_add(1, Ordering::SeqCst) >= MAX_STREAMS {
        shared.streams.fetch_sub(1, Ordering::SeqCst);
        let mut headers: Vec<(&str, &str)> = COMMON.to_vec();
        headers.push(("Content-Type", "application/json"));
        headers.push(("Retry-After", "1"));
        let body = json!({"error": format!("at most {MAX_STREAMS} event streams at once")});
        let _ = respond(
            stream,
            "503 Service Unavailable",
            &headers,
            body.to_string().as_bytes(),
        );
        return;
    }
    let received = session::lock(&shared.session).subscribe();
    let mut head = String::from("HTTP/1.1 200 OK\r\n");
    for (name, value) in COMMON {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Content-Type: text/event-stream\r\nConnection: close\r\n\r\n");
    let mut sent = stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.flush());
    let mut quiet_since = Instant::now();
    while sent.is_ok() && !shared.stop.load(Ordering::SeqCst) && !hung_up(stream) {
        match received.recv_timeout(Duration::from_millis(250)) {
            Ok(text) => {
                sent = stream
                    .write_all(text.as_bytes())
                    .and_then(|_| stream.flush());
                quiet_since = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if quiet_since.elapsed() >= KEEP_ALIVE {
                    sent = stream
                        .write_all(b": keep-alive\n\n")
                        .and_then(|_| stream.flush());
                    quiet_since = Instant::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
    shared.streams.fetch_sub(1, Ordering::SeqCst);
    // An open stream kept the server; its end counts as the last request.
    let mut a = lock(&shared.activity);
    a.last = Instant::now();
    if a.close_at.is_some() {
        a.close_at = Some(Instant::now() + ctx.close_delay);
    }
}

/// Whether the client has closed its side of an event stream (it sends nothing else).
fn hung_up(stream: &mut TcpStream) -> bool {
    if stream
        .set_read_timeout(Some(Duration::from_millis(1)))
        .is_err()
    {
        return true;
    }
    let mut byte = [0u8; 64];
    match stream.read(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => !matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
        ),
    }
}

/// `POST <token>/save`, from the app's own origin: the session's answers written by the
/// command (docs/open.md, `save`).
fn save(stream: &mut TcpStream, ctx: &Ctx, request: &Request, body: Vec<u8>) {
    let shared = ctx.shared;
    if shared.read_only {
        return refuse(stream, "409 Conflict", "opened read-only: nothing is saved");
    }
    let value = match json_body(stream, request, body, MAX_SAVE_BODY) {
        Ok(v) => v,
        Err((status, why)) => return refuse(stream, status, &why),
    };
    let Some(approved) = value.get("acceptDefaults").and_then(session::approvals) else {
        return refuse(
            stream,
            "400 Bad Request",
            "acceptDefaults is required: by part index, the question ids of the defaults the \
             person approved ({\"0\": [\"id\", ...]}; {} for none)",
        );
    };
    let Some(rev) = value.get("rev").and_then(|v| v.as_u64()) else {
        return refuse(
            stream,
            "400 Bad Request",
            "rev is required: the session revision the page last saw",
        );
    };
    match session::save(&shared.session, Some(rev), &approved, "page") {
        Ok(saved) => {
            let _ = ctx
                .tell
                .send(format!("Saved {} ({} bytes).", saved.path, saved.bytes));
            reply_json(stream, "200 OK", &saved.to_json());
        }
        Err(session::SaveError::Stale(current)) => reply_json(
            stream,
            "409 Conflict",
            &json!({"error": "the answers have changed since that rev: show them and save again", "rev": current}),
        ),
        Err(session::SaveError::Changed(why)) => {
            let current = session::lock(&shared.session).rev();
            reply_json(
                stream,
                "409 Conflict",
                &json!({"error": why, "rev": current}),
            )
        }
        Err(session::SaveError::Outside(why)) => refuse(stream, "403 Forbidden", &why),
        Err(session::SaveError::Question {
            message,
            part,
            question,
        }) => reply_json(
            stream,
            "400 Bad Request",
            &json!({"error": message, "part": part, "question": question}),
        ),
        Err(session::SaveError::Refused(why)) => refuse(stream, "422 Unprocessable Content", &why),
        Err(session::SaveError::Failed(why)) => refuse(stream, "500 Internal Server Error", &why),
    }
}

/// A file inside `root` (canonical) at the request path `rel`, and its Content-Type; None for
/// anything else: an encoded or literal `..`, `.`, an empty segment, a backslash or colon, a
/// directory, or a link that leads outside.
fn static_file(root: &Path, rel: &str) -> Option<(Vec<u8>, &'static str)> {
    let mut path = root.to_path_buf();
    for raw in rel.split('/') {
        let segment = percent_decode(raw)?;
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains(['/', '\\', ':', '\0'])
        {
            return None;
        }
        path.push(segment);
    }
    let real = fs::canonicalize(&path).ok()?;
    if !real.starts_with(root) || !real.is_file() {
        return None;
    }
    let bytes = fs::read(&real).ok()?;
    Some((bytes, content_type(&real)))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The Content-Type a file is served with, by its extension.
pub fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// A request's method, target and headers, as far as they are read.
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) headers: Vec<(String, String)>,
}

impl Request {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Reads a request's head (up to 16 KiB, all of it within 10 s), and the bytes read past it
/// (the start of a body); None for anything else.
pub(crate) fn read_head(stream: &mut TcpStream) -> Option<(Request, Vec<u8>)> {
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
    let end = head.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    let rest = head.split_off(end);
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
    Some((
        Request {
            method: method.to_string(),
            target: target.to_string(),
            headers,
        },
        rest,
    ))
}

/// Writes a response and closes the connection.
pub(crate) fn respond(
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
