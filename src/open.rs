//! `specify-core open`: a STEP file opened in the Specify app, served by this command on
//! 127.0.0.1 to the person's own browser, never uploaded, and saved back to disk.
//!
//! Everything is under `http://127.0.0.1:<port>/<token>/` (the token 32 lowercase hex from the
//! operating system's random source), see docs/open.md:
//!
//! - `GET <token>/…` the static app (`index.html` at `<token>/`), `GET <token>/wasm/…` the
//!   engine, `GET <token>/file` the STEP bytes, `GET <token>/config` the app's settings;
//! - `POST <token>/save` the specified STEP, written next to the input (never over a file,
//!   unless `--in-place`, which keeps the original once as `.bak`);
//! - `POST <token>/closed` the page went away: the server stops [`CLOSE_DELAY`] after the last
//!   request unless another arrives (a reload, or another open page's heartbeat).
//!
//! An open page sends `GET <token>/config` every [`HEARTBEAT`] (the engine runs in the page,
//! so editing alone makes no requests); any request resets the idle time and cancels a pending
//! close.
//!
//! A request whose `Host` is not exactly `127.0.0.1:<port>` is refused (a page that finds the
//! port through DNS rebinding), as is any other token, without saying why. `save` and `closed`
//! must carry `Origin: http://127.0.0.1:<port>`; a `GET` with any other `Origin` is refused. A
//! forgotten server stops after [`IDLE`] without a request.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;

/// How often an open page asks for `config` (the contract's heartbeat; the server does not
/// use it, it sizes [`CLOSE_DELAY`]).
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// How long after `closed` the server stops, unless another request arrives: longer than
/// [`HEARTBEAT`], so another open page keeps it.
pub const CLOSE_DELAY: Duration = Duration::from_secs(45);

/// How long the server waits without a request before it stops (a forgotten server: an open
/// page's heartbeat keeps it).
pub const IDLE: Duration = Duration::from_secs(30 * 60);

/// The largest save accepted, whatever the input's size.
pub const MAX_SAVE: u64 = 512 * 1024 * 1024;

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
    File::open("/dev/urandom")?.read_exact(buf)
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

/// How the file is opened.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Saves are refused (409).
    pub read_only: bool,
    /// A save replaces the input, which is kept as `.bak`.
    pub in_place: bool,
    /// The port on 127.0.0.1 (0: any free port).
    pub port: u16,
}

/// The input, its port open and its app found, ready to serve.
pub struct App {
    listener: TcpListener,
    port: u16,
    token: String,
    shared: Shared,
}

impl App {
    /// Reads `step` (a `.step` or `.stp` name, or either gzipped) and opens its port on
    /// 127.0.0.1.
    pub fn new(step: &Path, assets: Assets, options: &Options) -> Result<App, Error> {
        if options.read_only && options.in_place {
            return Err(Error::Refused(
                "--read-only and --in-place cannot be combined".to_string(),
            ));
        }
        let name = step
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !is_step_name(&name) {
            return Err(Error::Refused(format!(
                "{} is not a STEP file: Specify opens .step or .stp, or either gzipped",
                step.display()
            )));
        }
        let read = |e: io::Error| Error::Io(format!("{}: {e}", step.display()));
        // The path as given, made absolute but not resolved: saves go next to it (beside a
        // link, not its target) and every path shown is this one, never a canonical form
        // (`\\?\C:\…` on Windows).
        let step = std::path::absolute(step).map_err(read)?;
        let bytes = fs::read(&step).map_err(read)?;
        let gz = name.to_ascii_lowercase().ends_with(".gz");
        // The cap counts the STEP text, so a gzipped input is measured unzipped (every member).
        let text_len = if gz {
            io::copy(
                &mut flate2::read::MultiGzDecoder::new(&bytes[..]),
                &mut io::sink(),
            )
            .map_err(read)?
        } else {
            bytes.len() as u64
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
            step,
            gz,
            bytes: Mutex::new(Arc::new(bytes)),
            cap: text_len.saturating_mul(4).min(MAX_SAVE),
            assets,
            read_only: options.read_only,
            in_place: options.in_place,
            backup: Mutex::new(None),
            save_lock: Mutex::new(()),
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

    /// The file's name.
    pub fn name(&self) -> &str {
        &self.shared.name
    }

    /// The loopback port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Answers requests, each connection on its own thread (at most [`MAX_CONNECTIONS`] at
    /// once), until `close_delay` after `closed` with no request since, or `idle` without a
    /// request. `notice` gets each save's and the stop's one-line notice.
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
    /// The input as given, made absolute (not resolved: a link stays the link).
    step: PathBuf,
    gz: bool,
    /// What `file` serves: the input, or the last in-place save.
    bytes: Mutex<Arc<Vec<u8>>>,
    /// The largest save accepted.
    cap: u64,
    assets: Assets,
    read_only: bool,
    in_place: bool,
    /// `--in-place`: where this session's first save kept the original; later saves keep no
    /// other copy.
    backup: Mutex<Option<PathBuf>>,
    /// One save at a time, so `saveTarget` and the name a save takes agree.
    save_lock: Mutex<()>,
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

fn refuse_save(stream: &mut TcpStream, status: &str, error: &str) {
    reply_json(stream, status, &json!({ "error": error }));
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
    match (request.method.as_str(), rest) {
        ("POST", "save") => {
            if !same_origin {
                return refuse_save(stream, "403 Forbidden", "save only from the app's own page");
            }
            save(stream, ctx, &request, body);
        }
        ("POST", "closed") => {
            if !same_origin {
                return refuse_save(stream, "403 Forbidden", "only from the app's own page");
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
        ("GET", "file") => {
            let bytes = lock(&shared.bytes).clone();
            reply(stream, "200 OK", Some("application/octet-stream"), &bytes);
        }
        ("GET", "config") => {
            let target = if shared.read_only {
                None
            } else {
                let _guard = lock(&shared.save_lock);
                save_target(shared).map(|p| p.display().to_string())
            };
            let config = json!({
                "name": shared.name,
                "readOnly": shared.read_only,
                "wasmUrl": WASM_URL,
                "saveTarget": target,
                "specifyCore": env!("CARGO_PKG_VERSION"),
            });
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

/// The input's name without `.gz` and `.step`/`.stp`.
fn stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    let mut end = name.len();
    if lower.ends_with(".gz") {
        end -= 3;
    }
    let base = &lower[..end];
    if base.ends_with(".step") {
        end -= 5;
    } else if base.ends_with(".stp") {
        end -= 4;
    }
    &name[..end]
}

/// The `n`th name a save may take: `<stem>.specified.step`, then `<stem>.specified-2.step`, …
fn candidate(dir: &Path, stem: &str, n: u32) -> PathBuf {
    if n == 1 {
        dir.join(format!("{stem}.specified.step"))
    } else {
        dir.join(format!("{stem}.specified-{n}.step"))
    }
}

/// The highest suffix tried before a save gives up.
const MAX_CANDIDATES: u32 = 10_000;

/// Where the next save goes: the input (`--in-place`), or the first free candidate.
fn save_target(shared: &Shared) -> Option<PathBuf> {
    if shared.in_place {
        return Some(shared.step.clone());
    }
    let dir = shared.step.parent()?;
    let stem = stem(&shared.name);
    (1..=MAX_CANDIDATES)
        .map(|n| candidate(dir, stem, n))
        .find(|p| fs::symlink_metadata(p).is_err())
}

/// Whether `body` reads as a STEP Part 21 file: `ISO-10303-21;` after an optional BOM and
/// whitespace.
fn looks_like_step(body: &[u8]) -> bool {
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    let start = body
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(body.len());
    body[start..].starts_with(b"ISO-10303-21;")
}

/// `POST <token>/save`, from the app's own origin.
fn save(stream: &mut TcpStream, ctx: &Ctx, request: &Request, mut body: Vec<u8>) {
    let shared = ctx.shared;
    if shared.read_only {
        return refuse_save(stream, "409 Conflict", "opened read-only: nothing is saved");
    }
    let octets = request
        .header("content-type")
        .and_then(|t| t.split(';').next())
        .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/octet-stream"));
    if !octets {
        return refuse_save(
            stream,
            "415 Unsupported Media Type",
            "send the STEP file as application/octet-stream",
        );
    }
    // The body is read by its Content-Length only: a chunked (or otherwise encoded) body is
    // refused rather than misread.
    let length = request
        .header("content-length")
        .and_then(|l| l.trim().parse::<u64>().ok());
    let (Some(length), None) = (length, request.header("transfer-encoding")) else {
        return refuse_save(
            stream,
            "411 Length Required",
            "Content-Length is required (no Transfer-Encoding)",
        );
    };
    if length > shared.cap {
        return refuse_save(
            stream,
            "413 Content Too Large",
            &format!("larger than {} bytes", shared.cap),
        );
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
            return refuse_save(stream, "408 Request Timeout", "the body came too slowly");
        };
        if stream
            .set_read_timeout(Some(left.min(Duration::from_secs(30))))
            .is_err()
        {
            return;
        }
        match stream.read(&mut chunk[..(length - body.len()).min(64 * 1024)]) {
            Ok(0) | Err(_) => {
                return refuse_save(stream, "400 Bad Request", "the body ended early");
            }
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    if !looks_like_step(&body) {
        return refuse_save(
            stream,
            "400 Bad Request",
            "not a STEP file: it must start with ISO-10303-21;",
        );
    }
    let _guard = lock(&shared.save_lock);
    let bytes = body.len();
    let written = if shared.in_place {
        replace_input(shared, body)
    } else {
        write_new(shared, &body)
    };
    match written {
        Ok(path) => {
            let shown = path.display().to_string();
            let _ = ctx.tell.send(format!("Saved {shown} ({bytes} bytes)."));
            reply_json(stream, "200 OK", &json!({ "path": shown, "bytes": bytes }));
        }
        Err(e) => refuse_save(
            stream,
            "500 Internal Server Error",
            &format!("cannot save: {e}"),
        ),
    }
}

/// The longest a save's body may take to arrive: 30 s, and 1 s more per MiB (a trickling
/// client cannot hold a connection indefinitely).
fn body_time(length: u64) -> Duration {
    Duration::from_secs(30 + length / (1024 * 1024))
}

/// `body` written to a new temporary file in `dir`, synced.
fn temp_file(dir: &Path, body: &[u8]) -> io::Result<PathBuf> {
    let tag = token().map_err(|e| io::Error::other(e.to_string()))?;
    let path = dir.join(format!(".specify-save-{}.tmp", &tag[..12]));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let written = file.write_all(body).and_then(|_| file.sync_all());
    if let Err(e) = written {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// A save next to the input under the first free candidate name. The name is taken with
/// `create_new` (so nothing is overwritten, even in a race), and then the written temporary
/// file renamed onto it.
fn write_new(shared: &Shared, body: &[u8]) -> io::Result<PathBuf> {
    let dir = shared
        .step
        .parent()
        .ok_or_else(|| io::Error::other("the input has no directory"))?;
    let stem = stem(&shared.name);
    let temp = temp_file(dir, body)?;
    for n in 1..=MAX_CANDIDATES {
        let target = candidate(dir, stem, n);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(_) => {
                if let Err(e) = fs::rename(&temp, &target) {
                    let _ = fs::remove_file(&temp);
                    let _ = fs::remove_file(&target);
                    return Err(e);
                }
                return Ok(target);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                let _ = fs::remove_file(&temp);
                return Err(e);
            }
        }
    }
    let _ = fs::remove_file(&temp);
    Err(io::Error::other("every candidate name is taken"))
}

/// `--in-place`: the input replaced by renaming a written temporary file onto it. The first
/// save of the session first copies the original to the first free `.bak` (`<file>.bak`, then
/// `<file>.bak-2`, …) beside the path as given; later saves keep no other copy. A link is
/// followed (its target is replaced, the link kept). The replacement keeps the original's
/// permissions, and a gzipped input is written gzipped, so the file stays what its name says.
fn replace_input(shared: &Shared, body: Vec<u8>) -> io::Result<PathBuf> {
    let step = &shared.step;
    // The file itself, through any link; renaming onto the link would replace the link.
    let real = fs::canonicalize(step)?;
    let dir = real
        .parent()
        .ok_or_else(|| io::Error::other("the input has no directory"))?;
    let stored = if shared.gz {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&body)?;
        gz.finish()?
    } else {
        body
    };
    let temp = temp_file(dir, &stored)?;
    let replaced = (|| {
        fs::set_permissions(&temp, fs::metadata(&real)?.permissions())?;
        let mut backup = lock(&shared.backup);
        if backup.is_none() {
            *backup = Some(back_up(step, &real)?);
        }
        fs::rename(&temp, &real)
    })();
    if let Err(e) = replaced {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    *lock(&shared.bytes) = Arc::new(stored);
    Ok(step.clone())
}

/// `real` (the input's file) copied to the first free name of `<step>.bak`, `<step>.bak-2`, …
/// (never over a file); the name taken.
fn back_up(step: &Path, real: &Path) -> io::Result<PathBuf> {
    for n in 1..=MAX_CANDIDATES {
        let mut name = step.as_os_str().to_os_string();
        if n == 1 {
            name.push(".bak");
        } else {
            name.push(format!(".bak-{n}"));
        }
        let bak = PathBuf::from(name);
        match OpenOptions::new().write(true).create_new(true).open(&bak) {
            Ok(mut out) => {
                let copied = File::open(real)
                    .and_then(|mut input| io::copy(&mut input, &mut out))
                    .and_then(|_| out.sync_all());
                if let Err(e) = copied {
                    let _ = fs::remove_file(&bak);
                    return Err(e);
                }
                return Ok(bak);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("every .bak name is taken"))
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
