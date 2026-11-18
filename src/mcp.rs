//! `specify-core mcp`: a Model Context Protocol server over the same steps as `serve`
//! ([`crate::api`]), for an LLM client such as Claude Code or Claude Desktop (docs/mcp.md).
//!
//! Protocol revision 2026-07-28 (stateless: every request carries its version and client
//! capabilities in `_meta`; `server/discover`), and the legacy revision 2025-11-25 (an
//! `initialize` handshake), on stdio (one JSON-RPC message per line) or on local Streamable
//! HTTP (`--http`: 127.0.0.1 only, a random bearer token, `Host` and `Origin` checked as
//! `open` checks them).
//!
//! State across calls is a server-minted document handle (`doc`, the SHA-256 of the part's
//! STEP text and its part index) passed as a tool argument. The analysis and the answers so far
//! are kept in this process's memory, by handle, and lost when it exits, unless the caller names
//! an `answers_file`, which is then kept up to date. Every path a call names must lie inside a
//! directory given with `--allow` (by default the working directory), after symbolic links are
//! resolved; nothing existing is overwritten unless the call says `overwrite: true`.
//!
//! Domain refusals (an answer refused, questions left open, a path outside `--allow`) are tool
//! results with `isError` and a structured `error`; protocol errors are JSON-RPC errors. A panic
//! inside a tool call is answered as a `failed` tool error, one anywhere else as a JSON-RPC
//! internal error, and the server carries on. In stdio mode
//! nothing but protocol messages is written to stdout.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::api::{self, Error};
use crate::choices;
use crate::part;
use crate::revision::{self, assembly};
use crate::rules;
use crate::writer::WriteError;

/// The current protocol revision ("modern": per-request metadata, no handshake).
pub const MODERN: &str = "2026-07-28";
/// The legacy revision served after `initialize`.
pub const LEGACY: &str = "2025-11-25";
/// Every revision served, newest first.
pub const SUPPORTED: [&str; 2] = [MODERN, LEGACY];

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const HEADER_MISMATCH: i64 = -32020;
const UNSUPPORTED_VERSION: i64 = -32022;
/// 2025-11-25's resource-not-found code (2026-07-28 uses -32602).
const LEGACY_NOT_FOUND: i64 = -32002;

/// How long tool and template lists may be cached: they change only with the binary.
const STATIC_TTL_MS: u64 = 3_600_000;

/// The largest HTTP request body accepted.
const MAX_BODY: usize = 64 * 1024 * 1024;

const INSTRUCTIONS: &str = "Specify adds AP242 semantic PMI (datums, tolerances, fits, threads, \
material, general tolerance) to a STEP part by asking questions about its recognised features. \
Typical flow: list_parts (for an assembly) -> open (returns a `doc` handle) -> questions -> \
answer (free text such as \"M6\" or \"H7\" is interpreted) -> review -> write_pmi. The analysis \
and the answers are held in this server's memory by `doc` and are lost when the server restarts \
unless an `answers_file` was given. Paths must lie inside the directories the server was started \
with (--allow). The analysis and a triangle mesh of an open document are resources: \
specify://doc/{doc}/analysis and specify://doc/{doc}/mesh.";

/// Which revision's semantics a request is served under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Era {
    Modern,
    Legacy,
}

/// A reply to one message: the HTTP status it goes with, and the JSON-RPC response (none for
/// a notification).
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Option<Value>,
}

/// A JSON-RPC error.
#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// The HTTP status it is answered with.
    fn status(&self) -> u16 {
        match self.code {
            METHOD_NOT_FOUND => 404,
            INTERNAL_ERROR => 500,
            _ => 400,
        }
    }
}

/// A tool's refusal: the `error` of an `isError` result.
#[derive(Debug)]
struct ToolError {
    /// `invalid`, `refused`, `unknown_document`, `stale`, `incomplete` or `failed`.
    code: &'static str,
    message: String,
    extra: Map<String, Value>,
}

impl ToolError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        ToolError {
            code,
            message: message.into(),
            extra: Map::new(),
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        ToolError::new("invalid", message)
    }

    fn refused(message: impl Into<String>) -> Self {
        ToolError::new("refused", message)
    }

    fn with(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.into(), value);
        self
    }

    fn to_value(&self) -> Value {
        let mut out = Map::new();
        out.insert("code".into(), json!(self.code));
        out.insert("message".into(), json!(self.message));
        out.extend(self.extra.clone());
        Value::Object(out)
    }
}

impl From<Error> for ToolError {
    /// `serve`'s mapping: questions left open are `incomplete` (with `missing`), an answer or a
    /// file refused is `invalid`, anything else `failed`.
    fn from(e: Error) -> Self {
        match e {
            Error::Rules(rules::Error::Incomplete(e)) => {
                let mut out =
                    ToolError::new("incomplete", e.to_string()).with("missing", json!(e.missing));
                if let Some(part) = e.part {
                    out = out.with("part", json!(part));
                }
                out
            }
            Error::Rules(rules::Error::Invalid(m)) => ToolError::invalid(m),
            Error::Load(e) => ToolError::invalid(e.to_string()),
            Error::Write(WriteError::Invalid(m)) => ToolError::invalid(m),
            other => ToolError::new("failed", other.to_string()),
        }
    }
}

impl From<rules::Error> for ToolError {
    fn from(e: rules::Error) -> Self {
        ToolError::from(Error::Rules(e))
    }
}

type ToolResult = Result<Value, ToolError>;

/// An open document: one part of a STEP file, its analysis and the answers so far.
struct Doc {
    /// The file, canonical.
    path: PathBuf,
    /// SHA-256 of the file's bytes as read (gzipped or not), to tell it changed.
    file_sha: String,
    part: usize,
    analysis: Value,
    answers: Map<String, Value>,
    answers_file: Option<PathBuf>,
    /// The mesh, made when first read.
    mesh: Option<Value>,
}

/// The MCP server's state: the directories calls may name, and the open documents.
pub struct Server {
    allow: Vec<PathBuf>,
    docs: BTreeMap<String, Doc>,
    /// Whether a stdio client has sent `initialize` (legacy semantics for the process).
    initialized: bool,
}

/// The HTTP headers of a request, looked up without regard to case.
pub struct Headers<'h>(pub &'h [(String, String)]);

impl Headers<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl Server {
    /// A server whose calls may name paths inside `allow` (each canonicalised; none: the
    /// working directory).
    pub fn new(allow: &[PathBuf]) -> io::Result<Server> {
        let dirs: Vec<PathBuf> = if allow.is_empty() {
            vec![std::env::current_dir()?]
        } else {
            allow.to_vec()
        };
        let mut canonical = Vec::with_capacity(dirs.len());
        for dir in dirs {
            let c = dir
                .canonicalize()
                .map_err(|e| io::Error::new(e.kind(), format!("--allow {}: {e}", dir.display())))?;
            if !c.is_dir() {
                return Err(io::Error::other(format!(
                    "--allow {}: not a directory",
                    dir.display()
                )));
            }
            canonical.push(c);
        }
        Ok(Server {
            allow: canonical,
            docs: BTreeMap::new(),
            initialized: false,
        })
    }

    /// The directories calls may name.
    pub fn allowed(&self) -> &[PathBuf] {
        &self.allow
    }

    /// The reply to one JSON-RPC message; `http` gives the request's headers when it came over
    /// HTTP (checked against the body as the Streamable HTTP binding requires).
    ///
    /// A panic anywhere in serving the message is answered as a JSON-RPC internal error, and
    /// the server carries on.
    pub fn handle(&mut self, message: &Value, http: Option<&Headers>) -> Reply {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let mut era = None;
        let outcome =
            panic::catch_unwind(AssertUnwindSafe(|| self.dispatch(message, http, &mut era)))
                .unwrap_or_else(|payload| {
                    Err(RpcError::new(
                        INTERNAL_ERROR,
                        format!("Internal error: panicked: {}", panic_message(&*payload)),
                    ))
                });
        match outcome {
            Ok(None) => Reply {
                status: 202,
                body: None,
            },
            Ok(Some((era, mut result))) => {
                if era == Era::Legacy {
                    // 2025-11-25 has no result type and no caching hints.
                    if let Value::Object(r) = &mut result {
                        r.remove("resultType");
                        r.remove("ttlMs");
                        r.remove("cacheScope");
                    }
                }
                Reply {
                    status: 200,
                    body: Some(json!({"jsonrpc": "2.0", "id": id, "result": result})),
                }
            }
            Err(e) => Reply {
                // 2025-11-25's Streamable HTTP answers a request with its JSON-RPC response,
                // error or not, in a 200 body; only a bad or unsupported MCP-Protocol-Version
                // (refused before an era is chosen) is 400. 2026-07-28 maps errors to statuses.
                status: if era == Some(Era::Legacy) {
                    200
                } else {
                    e.status()
                },
                body: Some(error_response(&id, &e)),
            },
        }
    }

    /// A message's result and the era it was served in; None for a notification. `chosen` is
    /// set to the era as soon as it is known, so an error (or a panic) after it is answered in
    /// that era's manner.
    fn dispatch(
        &mut self,
        message: &Value,
        http: Option<&Headers>,
        chosen: &mut Option<Era>,
    ) -> Result<Option<(Era, Value)>, RpcError> {
        let Value::Object(m) = message else {
            return Err(RpcError::new(
                INVALID_REQUEST,
                "a message is a JSON-RPC object (batches are not part of MCP)",
            ));
        };
        if m.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(RpcError::new(INVALID_REQUEST, "jsonrpc must be \"2.0\""));
        }
        let Some(method) = m.get("method").and_then(Value::as_str) else {
            return Err(RpcError::new(INVALID_REQUEST, "a request names its method"));
        };
        let params = match m.get("params") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(p)) => p.clone(),
            Some(_) => return Err(RpcError::new(INVALID_PARAMS, "params is an object")),
        };
        let Some(id) = m.get("id") else {
            // A notification (notifications/initialized, notifications/cancelled): accepted,
            // never answered. Calls run to completion one at a time, so there is nothing to cancel.
            return Ok(None);
        };
        if !(id.is_string() || id.is_i64() || id.is_u64()) {
            return Err(RpcError::new(
                INVALID_REQUEST,
                "a request's id is a string or an integer",
            ));
        }
        let era = self.era(method, &params, http)?;
        *chosen = Some(era);
        let result = self.method(era, method, &params)?;
        Ok(Some((era, result)))
    }

    /// The era a request is served in, after its metadata (and on HTTP its headers) are
    /// checked.
    fn era(
        &mut self,
        method: &str,
        params: &Map<String, Value>,
        http: Option<&Headers>,
    ) -> Result<Era, RpcError> {
        let meta = params.get("_meta").and_then(Value::as_object);
        let version = meta.and_then(|m| m.get(META_VERSION));
        let header_version = http.and_then(|h| h.get("mcp-protocol-version"));
        let unsupported = |requested: &str| {
            RpcError::new(UNSUPPORTED_VERSION, "Unsupported protocol version")
                .with_data(json!({"supported": SUPPORTED, "requested": requested}))
        };
        if let Some(version) = version {
            let Some(version) = version.as_str() else {
                return Err(RpcError::new(
                    INVALID_PARAMS,
                    format!("_meta {META_VERSION} is a string"),
                ));
            };
            if let Some(h) = http {
                // The body is the source of truth; the mirrored headers must match it.
                match header_version {
                    Some(v) if v == version => {}
                    Some(v) => {
                        return Err(RpcError::new(
                            HEADER_MISMATCH,
                            format!(
                                "Header mismatch: MCP-Protocol-Version header value '{v}' does not match body value '{version}'"
                            ),
                        ));
                    }
                    None => {
                        return Err(RpcError::new(
                            HEADER_MISMATCH,
                            "Header mismatch: the MCP-Protocol-Version header is missing",
                        ));
                    }
                }
                if version == MODERN {
                    check_mirrored(h, method, params)?;
                }
            }
            if !SUPPORTED.contains(&version) {
                return Err(unsupported(version));
            }
            if version == LEGACY {
                return Ok(Era::Legacy);
            }
            if !meta
                .and_then(|m| m.get(META_CAPABILITIES))
                .is_some_and(Value::is_object)
            {
                return Err(RpcError::new(
                    INVALID_PARAMS,
                    format!("a {MODERN} request carries _meta {META_CAPABILITIES}"),
                ));
            }
            return Ok(Era::Modern);
        }
        // No per-request version: a legacy client.
        if method == "initialize" {
            if http.is_none() {
                self.initialized = true;
            }
            return Ok(Era::Legacy);
        }
        match http {
            Some(_) => match header_version {
                Some(LEGACY) => Ok(Era::Legacy),
                // A version this server speaks in the header but none in the body: the header
                // does not match the body (2026-07-28 Streamable HTTP, Server Validation).
                Some(v) if SUPPORTED.contains(&v) => Err(RpcError::new(
                    HEADER_MISMATCH,
                    format!(
                        "Header mismatch: MCP-Protocol-Version header value '{v}' does not match the body, which carries no _meta {META_VERSION}"
                    ),
                )),
                Some(other) => Err(unsupported(other)),
                None => Err(RpcError::new(
                    INVALID_PARAMS,
                    format!(
                        "the request carries no _meta {META_VERSION} (send it, or initialize for {LEGACY})"
                    ),
                )),
            },
            None if self.initialized => Ok(Era::Legacy),
            None => Err(RpcError::new(
                INVALID_PARAMS,
                format!(
                    "the request carries no _meta {META_VERSION} (send it, or initialize for {LEGACY})"
                ),
            )),
        }
    }

    /// One method's result.
    fn method(
        &mut self,
        era: Era,
        method: &str,
        params: &Map<String, Value>,
    ) -> Result<Value, RpcError> {
        let server_info = json!({"name": "specify-core", "version": env!("CARGO_PKG_VERSION")});
        let capabilities = json!({"tools": {}, "resources": {}});
        let mut result = match (era, method) {
            (Era::Legacy, "initialize") => json!({
                // A client asking for any other version is offered this one, as 2025-11-25's
                // lifecycle says; it disconnects if it cannot use it.
                "protocolVersion": LEGACY,
                "capabilities": capabilities,
                "serverInfo": server_info,
                "instructions": INSTRUCTIONS,
            }),
            (Era::Legacy, "ping") => json!({}),
            (_, "server/discover") => json!({
                "supportedVersions": SUPPORTED,
                "capabilities": capabilities,
                "instructions": INSTRUCTIONS,
                "ttlMs": STATIC_TTL_MS,
                "cacheScope": "public",
            }),
            (_, "tools/list") => json!({
                "tools": tools(),
                "ttlMs": STATIC_TTL_MS,
                "cacheScope": "public",
            }),
            (_, "tools/call") => self.call(params)?,
            (_, "resources/list") => json!({
                "resources": self.resources(),
                "ttlMs": 0,
                "cacheScope": "private",
            }),
            (_, "resources/templates/list") => json!({
                "resourceTemplates": templates(),
                "ttlMs": STATIC_TTL_MS,
                "cacheScope": "public",
            }),
            (_, "resources/read") => self.read_resource(era, params)?,
            // Only in this module's unit tests: a method that panics outside any tool.
            #[cfg(test)]
            (_, "test/panic") => panic!("a panic outside a tool"),
            _ => {
                return Err(RpcError::new(
                    METHOD_NOT_FOUND,
                    format!("Method not found: {method}"),
                ));
            }
        };
        if let Value::Object(r) = &mut result {
            r.insert("resultType".into(), json!("complete"));
            let mut meta = Map::new();
            meta.insert(META_SERVER.into(), server_info);
            r.insert("_meta".into(), Value::Object(meta));
        }
        Ok(result)
    }

    /// `tools/call`: the tool's result, or its refusal as an `isError` result.
    fn call(&mut self, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err(RpcError::new(INVALID_PARAMS, "tools/call names its tool"));
        };
        let testing = cfg!(test) && name == "panic for a test";
        if !TOOL_NAMES.contains(&name) && !testing {
            return Err(RpcError::new(
                INVALID_PARAMS,
                format!("Unknown tool: {name}"),
            ));
        }
        let args = match params.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(a)) => a.clone(),
            Some(_) => return Err(RpcError::new(INVALID_PARAMS, "arguments is an object")),
        };
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| self.tool(name, &args)));
        let outcome = outcome.unwrap_or_else(|payload| {
            Err(ToolError::new(
                "failed",
                format!("panicked: {}", panic_message(&*payload)),
            ))
        });
        Ok(match outcome {
            Ok(value) => json!({
                "content": [{"type": "text", "text": value.to_string()}],
                "structuredContent": value,
                "isError": false,
            }),
            Err(e) => {
                let error = e.to_value();
                json!({
                    "content": [{"type": "text", "text": format!("{}: {}", e.code, e.message)}],
                    "structuredContent": {"error": error},
                    "isError": true,
                })
            }
        })
    }

    fn tool(&mut self, name: &str, args: &Map<String, Value>) -> ToolResult {
        match name {
            "list_parts" => self.list_parts(args),
            "open" => self.open(args),
            "questions" => self.questions(args),
            "answer" => self.answer(args),
            "review" => self.review(args),
            "characteristics" => self.characteristics(args),
            "read_pmi" => self.read_pmi(args),
            "write_pmi" => self.write_pmi(args),
            "export_answers" => self.export_answers(args),
            "diff" => self.diff(args),
            // Only in this module's unit tests: a call that panics.
            #[cfg(test)]
            "panic for a test" => panic!("a panic for a test"),
            _ => unreachable!("tool names are checked first"),
        }
    }

    // ----- paths -----

    /// An existing file named by a call, canonical, inside `--allow`.
    fn existing_path(&self, raw: &str) -> Result<PathBuf, ToolError> {
        let path = Path::new(raw)
            .canonicalize()
            .map_err(|e| ToolError::invalid(format!("{raw}: {e}")))?;
        self.inside(&path, raw)?;
        if !path.is_file() {
            return Err(ToolError::invalid(format!("{raw} is not a file")));
        }
        Ok(path)
    }

    /// A file a call will write, inside `--allow` (its directory must exist; a symbolic link is
    /// followed and its target checked); an existing file only with `overwrite`.
    fn output_path(&self, raw: &str, overwrite: bool) -> Result<PathBuf, ToolError> {
        let given = Path::new(raw);
        let name = given
            .file_name()
            .ok_or_else(|| ToolError::invalid(format!("{raw} names no file")))?;
        let parent = match given.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        let dir = parent
            .canonicalize()
            .map_err(|e| ToolError::invalid(format!("{}: {e}", parent.display())))?;
        let mut target = dir.join(name);
        self.inside(&target, raw)?;
        if let Ok(meta) = std::fs::symlink_metadata(&target) {
            if meta.file_type().is_symlink() {
                target = target
                    .canonicalize()
                    .map_err(|e| ToolError::invalid(format!("{raw}: {e}")))?;
                self.inside(&target, raw)?;
            }
            if target.is_dir() {
                return Err(ToolError::invalid(format!("{raw} is a directory")));
            }
            if !overwrite {
                return Err(ToolError::refused(format!(
                    "{raw} exists; pass overwrite: true to replace it"
                ))
                .with("path", json!(target)));
            }
        }
        Ok(target)
    }

    fn inside(&self, path: &Path, raw: &str) -> Result<(), ToolError> {
        if self.allow.iter().any(|dir| path.starts_with(dir)) {
            Ok(())
        } else {
            Err(ToolError::refused(format!(
                "{raw} is outside the directories this server may use (--allow: {})",
                self.allow
                    .iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .with("path", json!(path)))
        }
    }

    // ----- documents -----

    /// The open document `args.doc`, checked to be known and its file unchanged.
    fn doc(&self, args: &Map<String, Value>) -> Result<(&str, &Doc), ToolError> {
        let handle = string(args, "doc")?;
        let (key, doc) = self.docs.get_key_value(handle).ok_or_else(|| {
            ToolError::new(
                "unknown_document",
                format!(
                    "no open document {handle}: documents are held in this server's memory and \
                     forgotten when it restarts. Call open again with the file (answers given \
                     since are lost unless an answers_file was named)"
                ),
            )
            .with("doc", json!(handle))
        })?;
        fresh(doc)?;
        Ok((key.as_str(), doc))
    }

    fn doc_mut(&mut self, handle: &str) -> &mut Doc {
        self.docs.get_mut(handle).expect("checked by doc()")
    }

    // ----- tools -----

    fn list_parts(&self, args: &Map<String, Value>) -> ToolResult {
        let path = self.existing_path(string(args, "path")?)?;
        let parts = api::parts(&path)?;
        Ok(json!({"path": path, "parts": parts}))
    }

    fn open(&mut self, args: &Map<String, Value>) -> ToolResult {
        let path = self.existing_path(string(args, "path")?)?;
        let part = optional_index(args, "part")?;
        let (answers_file, from_file) = match optional_string(args, "answers_file")? {
            Some(raw) => {
                let (target, kept) = self.answers_path(raw, &path, flag(args, "overwrite")?)?;
                (Some(target), kept)
            }
            None => (None, None),
        };
        let bytes = std::fs::read(&path)
            .map_err(|e| ToolError::invalid(format!("{raw}: {e}", raw = path.display())))?;
        let file_sha = sha256(&bytes);
        let file = part::File::of_bytes(&path, bytes).map_err(Error::Load)?;
        if part.is_none() && file.definitions().len() > 1 {
            return Ok(json!({
                "doc": null,
                "path": path,
                "needs_part": true,
                "message": "this file holds several parts: call open again with `part`, one of \
                            the parts' indices below",
                "parts": api::parts_of(&file),
            }));
        }
        let loaded = file.load(part).map_err(Error::Load)?;
        let (analysis, warnings) = api::analyse_reporting_in(&file, &loaded)?;
        let index = loaded.binding.part;
        let handle = format!("{}-{index}", loaded.binding.source_sha256);
        // Answers: the file named, else those already held for this handle, else those a
        // file Specify wrote stores (resumed by the analysis).
        let mut answers = self
            .docs
            .get(&handle)
            .map(|d| d.answers.clone())
            .or_else(|| analysis["resume"]["answers"].as_object().cloned())
            .unwrap_or_default();
        if let Some(kept) = from_file {
            answers = kept;
        }
        api::questions(&analysis, &answers)?;
        let previous_file = self.docs.get(&handle).and_then(|d| d.answers_file.clone());
        let doc = Doc {
            path: path.clone(),
            file_sha,
            part: index,
            analysis,
            answers,
            answers_file: answers_file.or(previous_file),
            mesh: None,
        };
        if let Some(target) = &doc.answers_file {
            save_answers(target, &doc.answers)?;
        }
        let qs = api::questions(&doc.analysis, &doc.answers)?;
        let open: Vec<&str> = rules::open_questions(&qs, &doc.answers)
            .iter()
            .map(|q| q.id.as_str())
            .collect();
        let mut out = Map::new();
        out.insert("doc".into(), json!(handle));
        out.insert("path".into(), json!(path));
        out.insert("part".into(), json!(index));
        out.insert("name".into(), json!(loaded.name));
        out.insert("faces".into(), json!(loaded.binding.face_count));
        out.insert("features".into(), feature_counts(&doc.analysis));
        out.insert(
            "existing_pmi".into(),
            json!(doc.analysis["existing"].as_array().map_or(0, Vec::len)),
        );
        out.insert(
            "resumed".into(),
            json!(doc.analysis.get("resume").is_some()),
        );
        out.insert("answered".into(), json!(doc.answers.len()));
        out.insert("open_questions".into(), json!(open));
        if let Some(f) = &doc.answers_file {
            out.insert("answers_file".into(), json!(f));
        }
        if !warnings.is_empty() {
            out.insert("warnings".into(), json!(warnings));
        }
        out.insert(
            "resources".into(),
            json!([
                format!("specify://doc/{handle}/analysis"),
                format!("specify://doc/{handle}/mesh")
            ]),
        );
        self.docs.insert(handle, doc);
        Ok(Value::Object(out))
    }

    fn questions(&self, args: &Map<String, Value>) -> ToolResult {
        let (handle, doc) = self.doc(args)?;
        let wanted = optional_string(args, "state")?.unwrap_or("open");
        if !["open", "answered", "all"].contains(&wanted) {
            return Err(ToolError::invalid(format!(
                "state is open, answered or all, not {wanted}"
            )));
        }
        let qs = api::questions(&doc.analysis, &doc.answers)?;
        let open: Vec<&str> = rules::open_questions(&qs, &doc.answers)
            .iter()
            .map(|q| q.id.as_str())
            .collect();
        let mut shown = Vec::new();
        let mut counts = BTreeMap::from([("answered", 0), ("open", 0), ("optional", 0)]);
        for q in &qs {
            let state = state_of(&q.id, &open, &doc.answers);
            *counts.get_mut(state).expect("a state") += 1;
            if wanted != "all" && wanted != state {
                continue;
            }
            // As `api::review` presents it, with its state and any answer given.
            let mut value = q.to_presented_dict();
            if let Some(d) = q.diameter {
                value["diameter"] = json!(d);
            }
            if let Some(d) = q.depth {
                value["depth"] = json!(d);
            }
            value["state"] = json!(state);
            if let Some(a) = doc.answers.get(&q.id) {
                value["answer"] = a.clone();
            }
            shown.push(value);
        }
        Ok(json!({
            "doc": handle,
            "state": wanted,
            "questions": shown,
            "open": open,
            "counts": counts,
        }))
    }

    fn answer(&mut self, args: &Map<String, Value>) -> ToolResult {
        let (handle, doc) = self.doc(args)?;
        let handle = handle.to_string();
        let set = match args.get("set") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(s)) => s.clone(),
            Some(_) => {
                return Err(ToolError::invalid(
                    "set is an object of question id to answer",
                ));
            }
        };
        let clear: Vec<String> = match args.get("clear") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| ToolError::invalid("clear is a list of question ids"))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(ToolError::invalid("clear is a list of question ids")),
        };
        let answers_file = match optional_string(args, "answers_file")? {
            Some(raw) => Some(
                self.answers_path(raw, &doc.path, flag(args, "overwrite")?)?
                    .0,
            ),
            None => None,
        };
        let mut trial = doc.answers.clone();
        for id in &clear {
            trial.remove(id);
        }
        let mut changed = Vec::new();
        for (id, given) in &set {
            // Earlier answers can ask new questions, so each is read against the ones before.
            let qs = api::questions(&doc.analysis, &trial)?;
            let q = qs.iter().find(|q| q.id == *id);
            let mut entry = Map::new();
            entry.insert("id".into(), json!(id));
            let value = match (q, given) {
                (Some(q), Value::String(text))
                    if q.kind == "choice" && !q.options.iter().any(|o| o == text) =>
                {
                    let value = choices::interpret(&q.id, text, &q.options, q.diameter, q.depth)
                        .map_err(|e| {
                            ToolError::invalid(format!("{id}: {}", e.0)).with("question", json!(id))
                        })?;
                    entry.insert("interpreted_from".into(), json!(text));
                    json!(value)
                }
                _ => given.clone(),
            };
            trial.insert(id.clone(), value.clone());
            api::review(&doc.analysis, &trial).map_err(|e| {
                let e = ToolError::from(e);
                ToolError {
                    message: format!("{id}: {}", e.message),
                    ..e
                }
                .with("question", json!(id))
            })?;
            entry.insert("value".into(), value);
            changed.push(entry);
        }
        if !clear.is_empty() {
            api::review(&doc.analysis, &trial)?;
        }
        let qs = api::questions(&doc.analysis, &trial)?;
        let open: Vec<&str> = rules::open_questions(&qs, &trial)
            .iter()
            .map(|q| q.id.as_str())
            .collect();
        for entry in &mut changed {
            let id = entry["id"].as_str().unwrap_or_default().to_string();
            let state = if qs.iter().any(|q| q.id == id) {
                state_of(&id, &open, &trial)
            } else {
                // Valid, but not asked given the other answers: kept, writes nothing.
                "not_asked"
            };
            entry.insert("state".into(), json!(state));
        }
        let open: Vec<String> = open.into_iter().map(str::to_string).collect();
        let doc = self.doc_mut(&handle);
        if let Some(target) = answers_file {
            doc.answers_file = Some(target);
        }
        doc.answers = trial;
        if let Some(target) = &doc.answers_file {
            save_answers(target, &doc.answers)?;
        }
        let mut out = Map::new();
        out.insert("doc".into(), json!(handle));
        out.insert("set".into(), json!(changed));
        out.insert("cleared".into(), json!(clear));
        out.insert("open".into(), json!(open));
        if let Some(f) = &doc.answers_file {
            out.insert("answers_file".into(), json!(f));
        }
        Ok(Value::Object(out))
    }

    fn review(&self, args: &Map<String, Value>) -> ToolResult {
        let (handle, doc) = self.doc(args)?;
        let review = api::review(&doc.analysis, &doc.answers)?;
        let mut out = Map::new();
        out.insert("doc".into(), json!(handle));
        out.insert("open".into(), review["open"].clone());
        out.insert("problems".into(), review["problems"].clone());
        for (defaults, ok, missing) in [
            (false, "writable", "missing"),
            (true, "writable_with_defaults", "missing_with_defaults"),
        ] {
            match api::apply(&doc.analysis, &doc.answers, defaults) {
                Ok(_) => {
                    out.insert(ok.into(), json!(true));
                    out.insert(missing.into(), json!([]));
                }
                Err(Error::Rules(rules::Error::Incomplete(e))) => {
                    out.insert(ok.into(), json!(false));
                    out.insert(missing.into(), json!(e.missing));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(Value::Object(out))
    }

    fn characteristics(&self, args: &Map<String, Value>) -> ToolResult {
        let (handle, doc) = self.doc(args)?;
        let intent = api::preview(&doc.analysis, &doc.answers)?;
        let sources = api::preview_sources(&doc.analysis, &doc.answers)?;
        Ok(json!({"doc": handle, "intent": intent.to_dict(), "sources": sources}))
    }

    fn read_pmi(&self, args: &Map<String, Value>) -> ToolResult {
        let (handle, doc) = self.doc(args)?;
        let a = &doc.analysis;
        let mut out = Map::new();
        out.insert("doc".into(), json!(handle));
        out.insert("existing".into(), a["existing"].clone());
        out.insert(
            "part_settings".into(),
            a.get("part").cloned().unwrap_or(json!({})),
        );
        out.insert("resumed".into(), json!(a.get("resume").is_some()));
        if let Some(saved) = a["resume"].get("answers") {
            out.insert("saved_answers".into(), saved.clone());
        }
        for key in ["dimensioning_stated", "characteristics"] {
            if let Some(v) = a.get(key) {
                out.insert(key.into(), v.clone());
            }
        }
        Ok(Value::Object(out))
    }

    fn write_pmi(&self, args: &Map<String, Value>) -> ToolResult {
        let (handle, doc) = self.doc(args)?;
        let overwrite = flag(args, "overwrite")?;
        let output = self.output_path(string(args, "output")?, overwrite)?;
        let defaults = flag(args, "accept_defaults")?;
        let intent = api::apply(&doc.analysis, &doc.answers, defaults)?;
        let report = api::write_file(&doc.path, &intent, &output, Some(&doc.answers))?;
        Ok(json!({"doc": handle, "output": output, "report": report.to_dict()}))
    }

    fn export_answers(&self, args: &Map<String, Value>) -> ToolResult {
        let (handle, doc) = self.doc(args)?;
        let output = match optional_string(args, "output")? {
            Some(raw) => Some(self.output_path(raw, flag(args, "overwrite")?)?),
            None => None,
        };
        let loaded = part::load(&doc.path, Some(doc.part)).map_err(Error::Load)?;
        let complete = api::export_answers(&loaded, &doc.analysis, &doc.answers)?;
        let mut out = Map::new();
        out.insert("doc".into(), json!(handle));
        if let Some(target) = output {
            save_answers(&target, &complete)?;
            out.insert("output".into(), json!(target));
        }
        out.insert("answers".into(), Value::Object(complete));
        Ok(Value::Object(out))
    }

    fn diff(&self, args: &Map<String, Value>) -> ToolResult {
        let old = self.existing_path(string(args, "old")?)?;
        let new = self.existing_path(string(args, "new")?)?;
        let part = optional_index(args, "part")?;
        let answers = match optional_string(args, "doc")? {
            Some(_) => {
                let (_, doc) = self.doc(args)?;
                let bytes = std::fs::read(&old)
                    .map_err(|e| ToolError::invalid(format!("{}: {e}", old.display())))?;
                if sha256(&bytes) != doc.file_sha {
                    return Err(ToolError::invalid(
                        "doc is not a document of OLD: its answers belong to another file",
                    ));
                }
                doc.answers.clone()
            }
            None => Map::new(),
        };
        let title = new
            .file_name()
            .map_or("part".into(), |n| n.to_string_lossy().into_owned());
        let options = revision::DiffOptions::default();
        let (before, after) = match part {
            Some(k) => {
                for path in [&old, &new] {
                    let count = part::parts(path).map_err(Error::Load)?.len();
                    if k >= count {
                        return Err(ToolError::invalid(format!(
                            "{} has {count} part(s); there is no part {k}",
                            path.display()
                        )));
                    }
                }
                let load = |p: &Path| part::load(p, part).map_err(Error::Load);
                (load(&old)?, load(&new)?)
            }
            None => {
                let read = |p: &Path| part::File::read(p).map_err(Error::Load);
                let (old_file, new_file) = (read(&old)?, read(&new)?);
                if assembly::is_assembly(&old_file) || assembly::is_assembly(&new_file) {
                    let all = |f: &part::File| f.load_all().map_err(Error::Load);
                    let compared = assembly::diff_with_options(
                        &all(&old_file)?,
                        &all(&new_file)?,
                        &answers,
                        options,
                    )?;
                    let report = serde_json::to_value(&compared)
                        .map_err(|e| ToolError::new("failed", e.to_string()))?;
                    return Ok(json!({
                        "kind": "assembly",
                        "verdict": compared.verdict,
                        "summary": compared.summary,
                        "answers": compared.answers(),
                        "report": report,
                        "markdown": compared.markdown(&title, None, None),
                    }));
                }
                let load = |f: &part::File| f.load(None).map_err(Error::Load);
                (load(&old_file)?, load(&new_file)?)
            }
        };
        let revised = revision::diff_with_options(&before, &after, &answers, options)?;
        let report =
            serde_json::to_value(&revised).map_err(|e| ToolError::new("failed", e.to_string()))?;
        Ok(json!({
            "kind": "part",
            "verdict": revised.verdict,
            "summary": revised.summary,
            "answers": revised.answers,
            "report": report,
            "markdown": revised.markdown(&title, None),
        }))
    }

    /// An `answers_file` argument, never the STEP file itself or another STEP file, and the
    /// answers it already holds. An existing file is kept up to date only if it is an answer
    /// map (what Specify writes); any other existing file is replaced only with `overwrite`.
    fn answers_path(
        &self,
        raw: &str,
        step: &Path,
        overwrite: bool,
    ) -> Result<(PathBuf, Option<Map<String, Value>>), ToolError> {
        let target = self.output_path(raw, true)?;
        if target == step || crate::open::is_step_name(&raw.to_ascii_lowercase()) {
            return Err(ToolError::refused(format!(
                "answers_file {raw} would replace a STEP file; name a .json file"
            )));
        }
        if !target.exists() {
            return Ok((target, None));
        }
        match read_answers(&target) {
            Ok(kept) => Ok((target, Some(kept))),
            Err(_) if overwrite => Ok((target, None)),
            Err(why) => Err(ToolError::refused(format!(
                "answers_file {raw} exists and is not an answers file ({}); name another file, \
                 or pass overwrite: true to replace it",
                why.message
            ))
            .with("path", json!(target))),
        }
    }

    // ----- resources -----

    fn resources(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for (handle, doc) in &self.docs {
            let name = doc
                .path
                .file_name()
                .map_or(String::new(), |n| n.to_string_lossy().into_owned());
            out.push(json!({
                "uri": format!("specify://doc/{handle}/analysis"),
                "name": format!("{name} part {} analysis", doc.part),
                "description": "Recognised features, faces and existing PMI of the open document",
                "mimeType": "application/json",
            }));
            out.push(json!({
                "uri": format!("specify://doc/{handle}/mesh"),
                "name": format!("{name} part {} mesh", doc.part),
                "description": "Triangles of the open document, ranged by face index",
                "mimeType": "application/json",
            }));
        }
        out
    }

    fn read_resource(&mut self, era: Era, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let Some(uri) = params.get("uri").and_then(Value::as_str) else {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "resources/read names its uri",
            ));
        };
        let not_found = |why: &str| {
            let code = match era {
                Era::Modern => INVALID_PARAMS,
                Era::Legacy => LEGACY_NOT_FOUND,
            };
            RpcError::new(code, format!("Resource not found: {why}")).with_data(json!({"uri": uri}))
        };
        let Some((handle, kind)) = uri
            .strip_prefix("specify://doc/")
            .and_then(|rest| rest.split_once('/'))
        else {
            return Err(not_found(
                "not a specify://doc/{doc}/analysis or /mesh address",
            ));
        };
        let Some(doc) = self.docs.get_mut(handle) else {
            return Err(not_found(
                "no such open document (documents are forgotten when the server restarts; call open again)",
            ));
        };
        fresh(doc).map_err(|e| not_found(&e.message))?;
        let text = match kind {
            "analysis" => doc.analysis.to_string(),
            "mesh" => {
                if doc.mesh.is_none() {
                    let loaded = part::load(&doc.path, Some(doc.part))
                        .map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))?;
                    doc.mesh = Some(api::mesh(&loaded));
                }
                doc.mesh.as_ref().expect("just made").to_string()
            }
            _ => return Err(not_found("a document's resources are analysis and mesh")),
        };
        Ok(json!({
            "contents": [{"uri": uri, "mimeType": "application/json", "text": text}],
            "ttlMs": 0,
            "cacheScope": "private",
        }))
    }
}

/// On HTTP, a 2026-07-28 request's `Mcp-Method` and (for `tools/call` and `resources/read`)
/// `Mcp-Name` headers must be present and match its body.
fn check_mirrored(h: &Headers, method: &str, params: &Map<String, Value>) -> Result<(), RpcError> {
    let mismatch = |m: String| RpcError::new(HEADER_MISMATCH, format!("Header mismatch: {m}"));
    match h.get("mcp-method") {
        Some(v) if v == method => {}
        Some(v) => {
            return Err(mismatch(format!(
                "Mcp-Method header value '{v}' does not match body value '{method}'"
            )));
        }
        None => return Err(mismatch("the Mcp-Method header is missing".into())),
    }
    let field = match method {
        "tools/call" | "prompts/get" => "name",
        "resources/read" => "uri",
        _ => return Ok(()),
    };
    let body = params
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(raw) = h.get("mcp-name") else {
        return Err(mismatch("the Mcp-Name header is missing".into()));
    };
    let header = decode_sentinel(raw)
        .ok_or_else(|| mismatch(format!("Mcp-Name header value '{raw}' is malformed")))?;
    if header != body {
        return Err(mismatch(format!(
            "Mcp-Name header value '{header}' does not match body value '{body}'"
        )));
    }
    Ok(())
}

/// A header value, its `=?base64?...?=` form decoded.
fn decode_sentinel(raw: &str) -> Option<String> {
    match raw
        .strip_prefix("=?base64?")
        .and_then(|r| r.strip_suffix("?="))
    {
        Some(encoded) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()?;
            String::from_utf8(bytes).ok()
        }
        None => Some(raw.to_string()),
    }
}

/// A panic's message, when it has one.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".into())
}

fn error_response(id: &Value, e: &RpcError) -> Value {
    let mut error = Map::new();
    error.insert("code".into(), json!(e.code));
    error.insert("message".into(), json!(e.message));
    if let Some(data) = &e.data {
        error.insert("data".into(), data.clone());
    }
    json!({"jsonrpc": "2.0", "id": id, "error": error})
}

/// Refuses a document whose file has changed since it was opened.
fn fresh(doc: &Doc) -> Result<(), ToolError> {
    let now = std::fs::read(&doc.path).map(|b| sha256(&b));
    if now.as_deref().ok() != Some(doc.file_sha.as_str()) {
        return Err(ToolError::new(
            "stale",
            format!(
                "{} has changed (or gone) since it was opened: open it again, or use diff to carry \
                 the answers to the new revision",
                doc.path.display()
            ),
        ));
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `answered`, `open` (asked, unanswered, no default taken for it) or `optional` (unanswered
/// but never counted open: BOM suggestions, the process, a full-thread depth).
fn state_of(id: &str, open: &[&str], answers: &Map<String, Value>) -> &'static str {
    if answers.contains_key(id) {
        "answered"
    } else if open.contains(&id) {
        "open"
    } else {
        "optional"
    }
}

/// Features counted by family, in the order families first appear (the CLI's summary).
fn feature_counts(analysis: &Value) -> Value {
    let mut counts = Map::new();
    for f in analysis["features"].as_array().into_iter().flatten() {
        let family = f["family"].as_str().unwrap_or_default().to_string();
        let n = counts.get(&family).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(family, json!(n + 1));
    }
    Value::Object(counts)
}

fn read_answers(path: &Path) -> Result<Map<String, Value>, ToolError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ToolError::invalid(format!("{}: {e}", path.display())))?;
    match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => {
            crate::input::answers_shape(&map)
                .map_err(|e| ToolError::invalid(format!("{}: {e}", path.display())))?;
            Ok(map)
        }
        Ok(_) => Err(ToolError::invalid(format!(
            "{}: an answers file is a JSON object of question id to answer",
            path.display()
        ))),
        Err(e) => Err(ToolError::invalid(format!("{}: {e}", path.display()))),
    }
}

/// An answer map as a sidecar: two-space indentation and a final newline (docs/answers.md).
fn save_answers(path: &Path, answers: &Map<String, Value>) -> Result<(), ToolError> {
    let text = serde_json::to_string_pretty(answers)
        .map_err(|e| ToolError::new("failed", e.to_string()))?;
    std::fs::write(path, text + "\n")
        .map_err(|e| ToolError::new("failed", format!("{}: {e}", path.display())))
}

fn string<'a>(args: &'a Map<String, Value>, name: &str) -> Result<&'a str, ToolError> {
    optional_string(args, name)?
        .ok_or_else(|| ToolError::invalid(format!("missing argument '{name}'")))
}

fn optional_string<'a>(
    args: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, ToolError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ToolError::invalid(format!("'{name}' is a string"))),
    }
}

fn optional_index(args: &Map<String, Value>, name: &str) -> Result<Option<usize>, ToolError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(|k| Some(k as usize))
            .ok_or_else(|| ToolError::invalid(format!("'{name}' is an index (0, 1, ...)"))),
    }
}

fn flag(args: &Map<String, Value>, name: &str) -> Result<bool, ToolError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(ToolError::invalid(format!("'{name}' is true or false"))),
    }
}

// ----- tool and resource definitions -----

const TOOL_NAMES: [&str; 10] = [
    "list_parts",
    "open",
    "questions",
    "answer",
    "review",
    "characteristics",
    "read_pmi",
    "write_pmi",
    "export_answers",
    "diff",
];

const MEMORY_RULE: &str = "Documents and their answers are held in this server's memory and \
are lost when the server restarts, unless an answers_file was given; an unknown doc means open \
the file again.";

/// An output schema: the success properties (with `required`) or a refusal's `error`.
fn output(properties: Value, required: &[&str]) -> Value {
    let mut props = properties.as_object().cloned().unwrap_or_default();
    props.insert(
        "error".into(),
        json!({
            "type": "object",
            "description": "Present only when isError is true: why the call was refused.",
            "properties": {
                "code": {"type": "string", "enum": ["invalid", "refused", "unknown_document", "stale", "incomplete", "failed"]},
                "message": {"type": "string"},
                "missing": {"type": "array", "items": {"type": "string"}},
                "question": {"type": "string"},
                "path": {"type": "string"},
                "doc": {"type": "string"}
            },
            "required": ["code", "message"]
        }),
    );
    json!({
        "type": "object",
        "properties": props,
        "anyOf": [{"required": required}, {"required": ["error"]}],
    })
}

/// A default's `source` (`rule_sources::Rule::to_json`).
fn source_schema() -> Value {
    json!({
        "type": "object",
        "description": "The rule that proposed the default and what it rests on.",
        "properties": {
            "rule": {"type": "string"},
            "kind": {"type": "string", "enum": ["standard", "convention", "vendor", "assumption"]},
            "refs": {"type": "array", "items": {"type": "object", "properties": {
                "standard": {"type": "string"},
                "edition": {"type": "string"},
                "clause": {"type": "string"}
            }, "required": ["standard"]}}
        },
        "required": ["rule", "kind", "refs"]
    })
}

fn input(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn doc_arg() -> Value {
    json!({"type": "string", "description": "The document handle `open` returned."})
}

/// The tools, in a fixed order.
pub fn tools() -> Vec<Value> {
    let doc = doc_arg;
    let strings = || json!({"type": "array", "items": {"type": "string"}});
    vec![
        json!({
            "name": "list_parts",
            "title": "List a STEP file's parts",
            "description": "List the distinct parts of a STEP file (a single part, or an assembly's parts), each with its index `part`, name, product id, face count and instances. Use the index with open.",
            "inputSchema": input(json!({"path": {"type": "string", "description": "A .step/.stp file (or gzipped) inside the server's allowed directories."}}), &["path"]),
            "outputSchema": output(json!({"path": {"type": "string"}, "parts": {"type": "array", "items": {"type": "object"}}}), &["parts"]),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "open",
            "title": "Open a part to specify",
            "description": format!("Analyse one part of a STEP file (recognise its features, read its existing PMI) and return a document handle `doc` for the other tools, with the open questions. An assembly opened without `part` returns its parts and needs_part: true; call again with `part`. A file Specify wrote resumes its stored answers. {MEMORY_RULE} With answers_file, answers are read from that JSON file if it exists and every change is saved to it."),
            "inputSchema": input(json!({
                "path": {"type": "string", "description": "A .step/.stp file (or gzipped) inside the server's allowed directories."},
                "part": {"type": "integer", "minimum": 0, "description": "Which part of an assembly (see list_parts)."},
                "answers_file": {"type": "string", "description": "Optional .json file to load answers from (if it exists) and to keep the answers in. An existing file that is not an answers map is refused unless overwrite is true."},
                "overwrite": {"type": "boolean", "description": "Replace an existing answers_file that is not an answers map."}
            }), &["path"]),
            "outputSchema": output(json!({
                "doc": {"type": ["string", "null"]},
                "path": {"type": "string"},
                "part": {"type": "integer"},
                "name": {"type": "string"},
                "faces": {"type": "integer"},
                "features": {"type": "object", "description": "Recognised features counted by family."},
                "existing_pmi": {"type": "integer"},
                "resumed": {"type": "boolean"},
                "answered": {"type": "integer"},
                "open_questions": strings(),
                "answers_file": {"type": "string"},
                "warnings": strings(),
                "resources": strings(),
                "needs_part": {"type": "boolean"},
                "message": {"type": "string"},
                "parts": {"type": "array", "items": {"type": "object"}}
            }), &["doc"]),
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "questions",
            "title": "List the questions",
            "description": format!("The questions Specify asks about the open document, given the answers so far: id, kind (choice, text, text_list, faces), prompt, default, why, options/choices, faces, attention, reason, basis and source (the rule behind the default: rule id, kind standard/convention/vendor/assumption, and the standards it cites), and state (open, answered, or optional: a suggestion never required). state selects which (default open). {MEMORY_RULE}"),
            "inputSchema": input(json!({
                "doc": doc(),
                "state": {"type": "string", "enum": ["open", "answered", "all"], "description": "Which questions (default open)."}
            }), &["doc"]),
            "outputSchema": output(json!({
                "doc": {"type": "string"},
                "state": {"type": "string"},
                "questions": {"type": "array", "items": {"type": "object", "properties": {"source": source_schema()}}},
                "open": strings(),
                "counts": {"type": "object"}
            }), &["doc", "questions", "open"]),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "answer",
            "title": "Answer questions",
            "description": format!("Set or clear answers on the open document. set maps question id to answer: a choice question takes one of its options or free text Specify interprets (\"M6\" for a hole, \"H7\" for a bore, \"6061-T6\" for material); a faces question takes a list of face indices; text questions take text. Answers are applied in order and all-or-nothing: a refused answer is an error naming the question and nothing changes. {MEMORY_RULE} answers_file also saves them to that JSON file from now on."),
            "inputSchema": input(json!({
                "doc": doc(),
                "set": {"type": "object", "description": "Question id to answer.", "additionalProperties": true},
                "clear": {"type": "array", "items": {"type": "string"}, "description": "Question ids whose answers to remove."},
                "answers_file": {"type": "string", "description": "Optional .json file to keep the answers in. An existing file that is not an answers map is refused unless overwrite is true."},
                "overwrite": {"type": "boolean", "description": "Replace an existing answers_file that is not an answers map."}
            }), &["doc"]),
            "outputSchema": output(json!({
                "doc": {"type": "string"},
                "set": {"type": "array", "items": {"type": "object"}},
                "cleared": strings(),
                "open": strings(),
                "answers_file": {"type": "string"}
            }), &["doc", "set", "open"]),
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
        }),
        json!({
            "name": "review",
            "title": "Review before writing",
            "description": format!("What stands between the open document and its PMI: open questions, problems review finds, whether it can be written as answered (writable, missing) and accepting the suggested defaults for the rest (writable_with_defaults, missing_with_defaults). {MEMORY_RULE}"),
            "inputSchema": input(json!({"doc": doc()}), &["doc"]),
            "outputSchema": output(json!({
                "doc": {"type": "string"},
                "open": strings(),
                "problems": {"type": "array"},
                "writable": {"type": "boolean"},
                "missing": strings(),
                "writable_with_defaults": {"type": "boolean"},
                "missing_with_defaults": strings()
            }), &["doc", "writable", "writable_with_defaults"]),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "characteristics",
            "title": "Preview the specification",
            "description": format!("The intent the answers so far amount to (part settings, datums, requirements with their tolerances, and the dimensions the general tolerance covers), as write_pmi would write it; unanswered defaults stay suggestions and are not included. sources maps each answer that is its question's default to the rule behind it. {MEMORY_RULE}"),
            "inputSchema": input(json!({"doc": doc()}), &["doc"]),
            "outputSchema": output(json!({"doc": {"type": "string"}, "intent": {"type": "object"}, "sources": {"type": "object", "additionalProperties": source_schema()}}), &["doc", "intent"]),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "read_pmi",
            "title": "Read the file's own PMI",
            "description": format!("The PMI the STEP file already states for this part (dimensions, tolerances, datums, notes, by face) and its part settings. Specify never asks for what the file states and keeps the file's own PMI when writing; a file Specify wrote instead resumes its answers (resumed, saved_answers). {MEMORY_RULE}"),
            "inputSchema": input(json!({"doc": doc()}), &["doc"]),
            "outputSchema": output(json!({
                "doc": {"type": "string"},
                "existing": {"type": "array"},
                "part_settings": {"type": "object"},
                "resumed": {"type": "boolean"},
                "saved_answers": {"type": "object"},
                "dimensioning_stated": {},
                "characteristics": {}
            }), &["doc", "existing", "part_settings", "resumed"]),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "write_pmi",
            "title": "Write the PMI",
            "description": format!("Write the open document's part with its PMI as AP242 to output (inside the allowed directories), verified by reading back; the answers are stored in the file so opening it again resumes them. accept_defaults takes the suggested default for every unanswered question; without it, unanswered required questions refuse the write (error incomplete with missing). An existing output is replaced only with overwrite: true. {MEMORY_RULE}"),
            "inputSchema": input(json!({
                "doc": doc(),
                "output": {"type": "string", "description": "The STEP file to write."},
                "accept_defaults": {"type": "boolean", "description": "Take the default for every unanswered question."},
                "overwrite": {"type": "boolean", "description": "Replace output if it exists."}
            }), &["doc", "output"]),
            "outputSchema": output(json!({
                "doc": {"type": "string"},
                "output": {"type": "string"},
                "report": {"type": "object", "description": "What was written and what was not, and why."}
            }), &["doc", "output", "report"]),
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "export_answers",
            "title": "Export a complete answers file",
            "description": format!("A complete, editable answer map for the open document, defaults accepted, with face evidence (_specify) and the rule behind each accepted default (_specify.sources), without writing STEP; refused (incomplete) if a required question has no answer or default. Optionally saved to output (.json; overwrite: true to replace). {MEMORY_RULE}"),
            "inputSchema": input(json!({
                "doc": doc(),
                "output": {"type": "string", "description": "Optional .json file to save the map to."},
                "overwrite": {"type": "boolean", "description": "Replace output if it exists."}
            }), &["doc"]),
            "outputSchema": output(json!({
                "doc": {"type": "string"},
                "answers": {"type": "object"},
                "output": {"type": "string"}
            }), &["doc", "answers"]),
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
        }),
        json!({
            "name": "diff",
            "title": "Compare two revisions",
            "description": "What changed between two revisions of a part (or of an assembly, part by part): verdict (ok, decide, problems), summary, the answers carried to the new revision, and the full report with a Markdown rendering. Give doc (an open document of OLD) to carry its answers. Writes nothing.",
            "inputSchema": input(json!({
                "old": {"type": "string", "description": "The STEP file as it was."},
                "new": {"type": "string", "description": "The STEP file as it is now."},
                "part": {"type": "integer", "minimum": 0, "description": "Compare one part of an assembly."},
                "doc": {"type": "string", "description": "Optional open document of OLD whose answers to carry."}
            }), &["old", "new"]),
            "outputSchema": output(json!({
                "kind": {"type": "string", "enum": ["part", "assembly"]},
                "verdict": {"type": "string"},
                "summary": {"type": "string"},
                "answers": {"type": "object"},
                "report": {"type": "object"},
                "markdown": {"type": "string"}
            }), &["kind", "verdict", "summary", "report"]),
            "annotations": {"readOnlyHint": true, "openWorldHint": false},
        }),
    ]
}

fn templates() -> Value {
    json!([
        {
            "uriTemplate": "specify://doc/{doc}/analysis",
            "name": "Document analysis",
            "description": "The analysis of an open document (features, faces, existing PMI), by the handle open returned.",
            "mimeType": "application/json",
        },
        {
            "uriTemplate": "specify://doc/{doc}/mesh",
            "name": "Document mesh",
            "description": "Triangles of an open document, ranged by face index (the pmi-assist-mesh format).",
            "mimeType": "application/json",
        }
    ])
}

// ----- transports -----

/// Serves stdio: one JSON-RPC message per line on `input`, one response per line on `out`
/// (none for a notification), until `input` closes.
pub fn serve_stdio(
    input: impl BufRead,
    mut out: impl Write,
    server: &mut Server,
) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let body = match serde_json::from_str::<Value>(&line) {
            Ok(message) => server.handle(&message, None).body,
            Err(e) => Some(error_response(
                &Value::Null,
                &RpcError::new(PARSE_ERROR, format!("Parse error: {e}")),
            )),
        };
        if let Some(body) = body {
            out.write_all(body.to_string().as_bytes())?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// A port on 127.0.0.1 for [`serve_http`] (0: any free port).
pub fn bind(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
}

/// A bearer token from the operating system's random source.
pub fn new_token() -> io::Result<String> {
    crate::open::token().map_err(|e| io::Error::other(e.to_string()))
}

/// The MCP endpoint's path.
pub const ENDPOINT: &str = "/mcp";

/// The most HTTP connections served at once; another is answered 503 at once and closed, so
/// stalled connections cannot pile up threads.
pub const MAX_CONNECTIONS: usize = 16;

/// A served connection, counted until its thread ends.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Serves Streamable HTTP on `listener` (bound to 127.0.0.1) at [`ENDPOINT`], each connection
/// on its own thread (at most [`MAX_CONNECTIONS`] at once), calls one at a time. A request must carry `Authorization: Bearer
/// <token>`, a `Host` of `127.0.0.1:<port>` or `localhost:<port>`, and no `Origin` (no
/// browser page may call it). Runs until the listener fails.
pub fn serve_http(listener: TcpListener, token: String, server: Server) -> io::Result<()> {
    let port = listener.local_addr()?.port();
    let rules = Arc::new(HttpRules {
        hosts: [format!("127.0.0.1:{port}"), format!("localhost:{port}")],
        token,
    });
    let server = Arc::new(Mutex::new(server));
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            // Answered here without reading the request; a short timeout keeps a client that
            // never reads from holding up the accept loop.
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            let _ = crate::open::respond(
                &mut stream,
                "503 Service Unavailable",
                &[("Retry-After", "1")],
                b"",
            );
            continue;
        }
        let slot = Slot(active.clone());
        let (rules, server) = (rules.clone(), server.clone());
        // A thread that cannot be started drops the connection (and its slot), which the
        // client can retry.
        let _ = thread::Builder::new().spawn(move || {
            let _slot = slot;
            answer_http(stream, &rules, &server);
        });
    }
    Ok(())
}

struct HttpRules {
    hosts: [String; 2],
    token: String,
}

/// Compares without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn answer_http(mut stream: TcpStream, rules: &HttpRules, server: &Mutex<Server>) {
    exchange(&mut stream, rules, server);
    // The response is sent and the write side shut. Read what the client still sends (a body
    // a refusal never read) until it closes, briefly, so closing does not reset the
    // connection and lose the response.
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

/// The most read from a connection after its response before it is closed anyway.
const MAX_DRAIN: usize = 1024 * 1024;

fn exchange(stream: &mut TcpStream, rules: &HttpRules, server: &Mutex<Server>) {
    use crate::open::{read_head, respond};
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let json_error = |e: RpcError| error_response(&Value::Null, &e).to_string();
    let Some((request, mut body)) = read_head(stream) else {
        let _ = respond(stream, "400 Bad Request", &[], b"");
        return;
    };
    // This address only: a page that finds the port through DNS rebinding is refused.
    if !request
        .header("host")
        .is_some_and(|h| rules.hosts.iter().any(|a| a.as_str() == h))
    {
        let _ = respond(stream, "403 Forbidden", &[], b"");
        return;
    }
    // No browser page may call it: any Origin is refused.
    if request.header("origin").is_some() {
        let body = json_error(RpcError::new(
            INVALID_REQUEST,
            "Forbidden: a browser Origin is refused",
        ));
        let _ = respond(
            stream,
            "403 Forbidden",
            &[("Content-Type", "application/json")],
            body.as_bytes(),
        );
        return;
    }
    if request.target != ENDPOINT {
        let _ = respond(stream, "404 Not Found", &[], b"");
        return;
    }
    // The auth scheme's name is case-insensitive (RFC 9110 section 11.1).
    let authorised = request
        .header("authorization")
        .and_then(|a| a.trim().split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .is_some_and(|(_, t)| same(t.trim(), &rules.token));
    if !authorised {
        let _ = respond(
            stream,
            "401 Unauthorized",
            &[("WWW-Authenticate", "Bearer")],
            b"",
        );
        return;
    }
    if request.method != "POST" {
        let _ = respond(stream, "405 Method Not Allowed", &[("Allow", "POST")], b"");
        return;
    }
    let Some(length) = request
        .header("content-length")
        .and_then(|l| l.trim().parse::<usize>().ok())
    else {
        let _ = respond(stream, "411 Length Required", &[], b"");
        return;
    };
    if length > MAX_BODY {
        let _ = respond(stream, "413 Content Too Large", &[], b"");
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut chunk = vec![0u8; 64 * 1024];
    while body.len() < length {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return;
        };
        if stream.set_read_timeout(Some(left)).is_err() {
            return;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    let message = match serde_json::from_slice::<Value>(&body) {
        Ok(m) => m,
        Err(e) => {
            let body = json_error(RpcError::new(PARSE_ERROR, format!("Parse error: {e}")));
            let _ = respond(
                stream,
                "400 Bad Request",
                &[("Content-Type", "application/json")],
                body.as_bytes(),
            );
            return;
        }
    };
    let reply = {
        let mut server = server.lock().unwrap_or_else(|p| p.into_inner());
        server.handle(&message, Some(&Headers(&request.headers)))
    };
    let status = match reply.status {
        200 => "200 OK",
        202 => "202 Accepted",
        400 => "400 Bad Request",
        404 => "404 Not Found",
        _ => "500 Internal Server Error",
    };
    match reply.body {
        Some(body) => {
            let _ = respond(
                stream,
                status,
                &[
                    ("Content-Type", "application/json"),
                    ("Cache-Control", "no-store"),
                ],
                body.to_string().as_bytes(),
            );
        }
        None => {
            let _ = respond(stream, status, &[], b"");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modern(id: i64, method: &str, params: Value) -> Value {
        let mut params = params.as_object().cloned().unwrap_or_default();
        params.insert(
            "_meta".into(),
            json!({META_VERSION: MODERN, META_CAPABILITIES: {}}),
        );
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    /// A panic inside one call is a `failed` tool error and the server carries on.
    #[test]
    fn a_panic_in_one_call_is_answered_and_the_server_carries_on() {
        let mut server = Server::new(&[std::env::temp_dir()]).unwrap();
        let call = modern(1, "tools/call", json!({"name": "panic for a test"}));
        let reply = server.handle(&call, None).body.unwrap();
        let result = &reply["result"];
        assert_eq!(result["isError"], json!(true), "{reply}");
        assert_eq!(
            result["structuredContent"]["error"]["code"],
            json!("failed")
        );
        assert_eq!(
            result["structuredContent"]["error"]["message"],
            json!("panicked: a panic for a test")
        );
        let reply = server.handle(&modern(2, "server/discover", json!({})), None);
        assert_eq!(
            reply.body.unwrap()["result"]["resultType"],
            json!("complete")
        );
    }

    /// A panic outside any tool (resources/read making a mesh, say) is a JSON-RPC internal
    /// error, and the server carries on.
    #[test]
    fn a_panic_outside_a_tool_is_an_internal_error_and_the_server_carries_on() {
        let mut server = Server::new(&[std::env::temp_dir()]).unwrap();
        let reply = server.handle(&modern(1, "test/panic", json!({})), None);
        assert_eq!(reply.status, 500);
        let body = reply.body.unwrap();
        assert_eq!(body["id"], json!(1));
        assert_eq!(body["error"]["code"], json!(INTERNAL_ERROR), "{body}");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("a panic outside a tool")
        );
        let reply = server.handle(&modern(2, "tools/list", json!({})), None);
        assert_eq!(reply.status, 200);
        assert!(reply.body.unwrap()["result"]["tools"].is_array());
    }

    #[test]
    fn sentinel_values_are_decoded() {
        assert_eq!(decode_sentinel("plain").as_deref(), Some("plain"));
        assert_eq!(
            decode_sentinel("=?base64?SGVsbG8sIOS4lueVjA==?=").as_deref(),
            Some("Hello, 世界")
        );
        assert_eq!(decode_sentinel("=?base64?***?="), None);
    }
}
