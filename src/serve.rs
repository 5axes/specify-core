//! `specify-core serve`: a long-lived child process speaking JSON lines (port of `serve.py`).
//!
//! A host sends one request per line on stdin; one reply per line comes back on stdout, and
//! nothing else is ever written to stdout (diagnostics, such as a panic's message, go to
//! stderr). Requests carry everything they need (the analysis is passed back in, not kept
//! here), so a crashed or restarted child loses nothing.
//!
//! ```text
//! {"id": 1, "op": "analyse", "step": "/path/part.step"}
//! -> {"id": 1, "ok": true, "result": {"analysis": {...}, "mesh": {...}}}
//! {"id": 6, "op": "parts", "step": "/path/assembly.step"}
//! -> {"id": 6, "ok": true, "result": {"parts": [{"part": 0, "name": ..., "product_id": ...,
//!    ...}, ...]}} (a part is named on the wire by its index, "part")
//!    and "analyse" of an assembly says which: {..., "part": 1}
//! {"id": 2, "op": "questions", "analysis": {...}, "answers": {...}}
//! {"id": 3, "op": "preview", "analysis": {...}, "answers": {...}, "step": "/path/part.step"}
//! -> the intent, with "anchors" for its labels when the step is given
//! {"id": 5, "op": "interpret", "analysis": ..., "answers": ..., "question": id, "text": "M6"}
//! -> {"id": 5, "ok": true, "result": {"value": "clearance:M6", "code": ..., "callout": ...}}
//! {"id": 4, "op": "write", "step": ..., "analysis": ..., "answers": ...,
//!  "output": "/path/out.step", "accept_defaults": true}
//! -> the report and the intent; the answers are stored in the file
//! -> {"id": 4, "ok": false, "error": {"code": "incomplete", "message": ..., "missing": [...]}}
//! {"id": 7, "op": "write", "step": ..., "output": ..., "accept_defaults": true,
//!  "parts": [{"analysis": ..., "answers": ...}, ...]}
//! -> parts of an assembly, written together (one part may be written alone: the others and
//!    their stored answers are left as they are)
//! ```
//!
//! Errors are `{code, message}`: `incomplete` (with `missing`, and `part` in an assembly),
//! `invalid` (an answer specify-core refuses, or a malformed request: a missing field is
//! `missing field 'name'`, as Python's `KeyError` reads), or `failed` (anything else, with a
//! `trace`: the error's debug form, or a panic's message, location and backtrace). A line that
//! is not JSON, or JSON that is not an object, is answered as `invalid` and the child carries
//! on; a panic inside one request is caught and answered as `failed`, so the child outlives one
//! bad part.
//!
//! Python moves file descriptor 1 onto stderr because OpenCascade prints from C++; nothing in
//! this crate or its dependencies prints to stdout, so replies are simply the only writes to it.
//!
//! What differs from Python: an analyse reply carries `warnings` (a list of lines) when a
//! contact between parts could not be measured, which Python never reports; a field of the
//! wrong JSON type (an `answers` that is not an object, a `step` that is not a string) is
//! `invalid` where Python fails with a `TypeError` or `AttributeError` (`failed`); a JSON
//! syntax error's message is serde_json's, not Python's.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::api::{self, Error};
use crate::choices;
use crate::part::{self, LoadedPart};
use crate::problems::problems;
use crate::pyjson;
use crate::rules::{self, IncompleteError, repr, truthy};
use crate::writer::WriteError;

/// The protocol's version, answered to `hello`.
pub const PROTOCOL: i64 = 1;

/// How many parts loaded for anchoring are kept, most recent last (Python's `_KEEP`).
const KEEP: usize = 2;

/// Why a request failed: one of the protocol's error codes.
#[derive(Debug)]
enum Failure {
    Incomplete(IncompleteError),
    Invalid(String),
    Failed { message: String, trace: String },
}

impl From<Error> for Failure {
    /// Python's mapping: an `IncompleteError` is `incomplete`; a `ValueError` (a load refused,
    /// an answer refused, a write asked wrongly) is `invalid`; anything else is `failed`.
    fn from(e: Error) -> Self {
        match e {
            Error::Rules(rules::Error::Incomplete(e)) => Failure::Incomplete(e),
            Error::Rules(rules::Error::Invalid(m)) => Failure::Invalid(m),
            Error::Load(e) => Failure::Invalid(e.to_string()),
            Error::Write(WriteError::Invalid(m)) => Failure::Invalid(m),
            other => Failure::Failed {
                message: other.to_string(),
                trace: format!("{other:?}"),
            },
        }
    }
}

impl From<rules::Error> for Failure {
    fn from(e: rules::Error) -> Self {
        Failure::from(Error::Rules(e))
    }
}

type Handled = Result<Value, Failure>;

/// A serving session: the parts loaded for anchoring and their anchors, kept between requests
/// (Python's module-level `_PARTS` and `_ANCHORS`). Everything else comes with each request.
#[derive(Default)]
pub struct Server {
    /// Parts loaded for anchoring, by `step#part`, most recent last.
    parts: VecDeque<(String, LoadedPart)>,
    /// Anchors found, by (`step#part`, group key).
    anchors: HashMap<(String, String), Value>,
}

thread_local! {
    /// The last panic's message, location and backtrace on this thread, recorded by the hook
    /// [`serve`] installs, for the `failed` reply's trace.
    static PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Answers requests from `input` until it closes, one reply line each on `replies`.
pub fn serve(input: impl BufRead, mut replies: impl Write) -> io::Result<()> {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|info| {
        let text = format!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        // The panic goes to stderr as well, never to stdout.
        eprintln!("specify-core serve: {info}");
        PANIC.with(|p| *p.borrow_mut() = Some(text));
    }));
    let mut server = Server::default();
    let result = (|| {
        for line in input.lines() {
            let line = line?;
            let Some(out) = server.line(&line) else {
                continue;
            };
            replies.write_all(pyjson::dumps_compact(&out).as_bytes())?;
            replies.write_all(b"\n")?;
            replies.flush()?;
        }
        Ok(())
    })();
    panic::set_hook(previous);
    result
}

impl Server {
    /// The reply to one line of input; None for a blank line, which is passed over.
    pub fn line(&mut self, line: &str) -> Option<Value> {
        if line.trim().is_empty() {
            return None;
        }
        Some(match serde_json::from_str::<Value>(line) {
            Ok(request) => self.reply(&request),
            Err(e) => json!({
                "id": null, "ok": false,
                "error": {"code": "invalid", "message": e.to_string()},
            }),
        })
    }

    /// A request's reply: its result, or a structured error.
    pub fn reply(&mut self, request: &Value) -> Value {
        let Value::Object(request) = request else {
            // Valid JSON, but not a request: answered, and the child carries on.
            let message = format!("a request is a JSON object, not {}", type_name(request));
            return json!({"id": null, "ok": false, "error": {"code": "invalid", "message": message}});
        };
        let mut out = Map::new();
        out.insert(
            "id".into(),
            request.get("id").cloned().unwrap_or(Value::Null),
        );
        PANIC.with(|p| p.borrow_mut().take());
        let handled = panic::catch_unwind(AssertUnwindSafe(|| self.handle(request)));
        let handled = handled.unwrap_or_else(|payload| {
            // A panic half way through may have left the anchoring caches inconsistent.
            self.parts.clear();
            self.anchors.clear();
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic with no message".into());
            let trace = PANIC
                .with(|p| p.borrow_mut().take())
                .unwrap_or_else(|| message.clone());
            Err(Failure::Failed {
                message: format!("panicked: {message}"),
                trace,
            })
        });
        match handled {
            Ok(result) => {
                out.insert("ok".into(), Value::Bool(true));
                out.insert("result".into(), result);
            }
            Err(failure) => {
                out.insert("ok".into(), Value::Bool(false));
                out.insert("error".into(), error(failure));
            }
        }
        Value::Object(out)
    }

    /// One request to its result.
    fn handle(&mut self, request: &Map<String, Value>) -> Handled {
        let op = request.get("op").unwrap_or(&Value::Null);
        match op.as_str() {
            Some("hello") => return Ok(json!({ "protocol": PROTOCOL })),
            Some("parts") => {
                let parts = api::parts(&path(request, "step")?)?;
                return Ok(json!({ "parts": parts }));
            }
            Some("analyse") => {
                let step = path(request, "step")?;
                let part = match request.get("part") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(index(v, "part")?),
                };
                // The file is read once, for the part and the other parts its mates need.
                let file = part::File::read(&step).map_err(Error::Load)?;
                let loaded = file.load(part).map_err(Error::Load)?;
                let (analysis, warnings) = api::analyse_reporting_in(&file, &loaded)?;
                let mut result = Map::new();
                result.insert("analysis".into(), analysis);
                result.insert("mesh".into(), api::mesh(&loaded));
                if !warnings.is_empty() {
                    result.insert("warnings".into(), json!(warnings));
                }
                return Ok(Value::Object(result));
            }
            Some("write") if request.contains_key("parts") => return write_parts(request),
            // Only in this module's unit tests: a request that panics.
            #[cfg(test)]
            Some("panic for a test") => panic!("a panic for a test"),
            _ => {}
        }
        // Every other op works on an analysis passed back in (asked for before the op is
        // known, as Python's `request["analysis"]` is).
        let analysis = field(request, "analysis")?;
        let answers = answers(request.get("answers"))?;
        match op.as_str() {
            Some("questions") => {
                let qs = api::questions(analysis, &answers)?;
                let open: Vec<&str> = rules::open_questions(&qs, &answers)
                    .iter()
                    .map(|q| q.id.as_str())
                    .collect();
                Ok(json!({
                    "questions": qs.iter().map(|q| q.to_dict()).collect::<Vec<_>>(),
                    "open": open,
                    "problems": problems(analysis, &answers)?,
                }))
            }
            Some("interpret") => interpret(request, analysis, &answers),
            Some("preview") => {
                let mut intent = api::preview(analysis, &answers)?.to_dict();
                if request.get("step").is_some_and(truthy) {
                    // Where each label lands, worked out on the solid; the page falls back to
                    // its own guess for any face without one.
                    let step = path(request, "step")?;
                    let anchors = self.label_anchors(&step, analysis, &intent)?;
                    intent["anchors"] = Value::Object(anchors);
                }
                Ok(json!({ "intent": intent }))
            }
            Some("write") => {
                let defaults = request.get("accept_defaults").is_some_and(truthy);
                let mut intent = api::apply(analysis, &answers, defaults)?;
                if request.get("bom").is_some_and(truthy) {
                    crate::bom::accept_suggestions(analysis, &answers, &mut intent)
                        .map_err(Failure::Invalid)?;
                }
                // The answers as given, defaults not filled in: resumed, a default not chosen
                // stays a default and follows the rules as they then are.
                let step = path(request, "step")?;
                let output = path(request, "output")?;
                let report = api::write_file(&step, &intent, &output, Some(&answers))?;
                Ok(json!({"report": report.to_dict(), "intent": intent.to_dict()}))
            }
            _ => Err(Failure::Invalid(format!("unknown op {}", repr(op)))),
        }
    }

    /// The anchors of the intent's labels and the analysis's existing PMI on `step` (Python's
    /// `_label_anchors`): none when the analysis was not made of this file's part.
    fn label_anchors(
        &mut self,
        step: &Path,
        analysis: &Value,
        intent: &Value,
    ) -> Result<Map<String, Value>, Failure> {
        let binding = analysis.get("binding").cloned().unwrap_or(json!({}));
        let Value::Object(binding) = binding else {
            return Err(Failure::Invalid(
                "the analysis's binding is not an object".into(),
            ));
        };
        let part = match binding.get("part") {
            None => 0,
            Some(v) => index(v, "binding.part")?,
        };
        let key = format!("{}#{part}", step.display());
        match self.parts.iter().position(|(k, _)| *k == key) {
            // Most recent last.
            Some(i) => {
                let entry = self.parts.remove(i).expect("found");
                self.parts.push_back(entry);
            }
            None => {
                let loaded = part::load(step, Some(part)).map_err(Error::Load)?;
                self.parts.push_back((key.clone(), loaded));
                while self.parts.len() > KEEP {
                    let (gone, _) = self.parts.pop_front().expect("more than KEEP");
                    self.anchors.retain(|(k, _), _| *k != gone);
                }
            }
        }
        let loaded = &self.parts.back().expect("just pushed").1;
        // An analysis made before parts were told apart is of part 0.
        let mut expected = Map::new();
        expected.insert("part".into(), json!(0));
        expected.extend(binding);
        let actual = serde_json::to_value(&loaded.binding).expect("a binding serialises");
        if actual != Value::Object(expected) {
            return Ok(Map::new());
        }
        let mut groups: Vec<&Value> = Vec::new();
        for list in ["datums", "requirements"] {
            for item in intent[list].as_array().into_iter().flatten() {
                groups.push(field_of(item, "faces")?);
            }
        }
        for item in analysis
            .get("existing")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(faces) = item.get("faces") {
                groups.push(faces);
            }
        }
        // Each group once, by key, in the order first met; empty groups have no anchor.
        let mut wanted: Vec<(String, Vec<usize>)> = Vec::new();
        for faces in groups {
            let faces = face_list(faces)?;
            if faces.is_empty() {
                continue;
            }
            let k = crate::anchors::key(&faces);
            if !wanted.iter().any(|(w, _)| *w == k) {
                wanted.push((k, faces));
            }
        }
        let missing: Vec<Vec<usize>> = wanted
            .iter()
            .filter(|(k, _)| !self.anchors.contains_key(&(key.clone(), k.clone())))
            .map(|(_, g)| g.clone())
            .collect();
        if !missing.is_empty() {
            for (k, found) in api::anchors(loaded, &missing) {
                self.anchors.insert((key.clone(), k), found);
            }
        }
        Ok(wanted
            .into_iter()
            .filter_map(|(k, _)| {
                let found = self.anchors.get(&(key.clone(), k.clone()))?;
                Some((k, found.clone()))
            })
            .collect())
    }
}

/// `write` with `parts`: several parts of an assembly, each applied with its answers (a
/// refusal naming the part), then written together.
fn write_parts(request: &Map<String, Value>) -> Handled {
    let defaults = request.get("accept_defaults").is_some_and(truthy);
    let Some(parts) = field(request, "parts")?.as_array() else {
        return Err(Failure::Invalid("'parts' is not a list".into()));
    };
    let mut written = Vec::with_capacity(parts.len());
    for p in parts {
        let analysis = field_of(p, "analysis")?;
        let part = analysis
            .get("binding")
            .and_then(|b| b.get("part"))
            .unwrap_or(&json!(0))
            .clone();
        let answers = answers(p.get("answers"))?;
        let mut intent = api::apply(analysis, &answers, defaults).map_err(|e| match e {
            Error::Rules(rules::Error::Incomplete(e)) => Failure::Incomplete(IncompleteError::new(
                e.missing,
                Some(part.as_u64().unwrap_or(0) as usize),
            )),
            Error::Rules(rules::Error::Invalid(m)) => {
                Failure::Invalid(format!("part {}: {m}", python_str(&part)))
            }
            other => Failure::from(other),
        })?;
        if request.get("bom").is_some_and(truthy) {
            crate::bom::accept_suggestions(analysis, &answers, &mut intent)
                .map_err(Failure::Invalid)?;
        }
        written.push((intent, Some(answers)));
    }
    let step = path(request, "step")?;
    let output = path(request, "output")?;
    let report = api::write_parts(&step, &written, &output)?;
    Ok(json!({
        "report": report.to_dict(),
        "intents": written.iter().map(|(i, _)| i.to_dict()).collect::<Vec<_>>(),
    }))
}

/// What someone typed for a question, read as the answer it means.
fn interpret(
    request: &Map<String, Value>,
    analysis: &Value,
    answers: &Map<String, Value>,
) -> Handled {
    let qs = api::questions(analysis, answers)?;
    let wanted = field(request, "question")?;
    let q = qs
        .iter()
        .find(|q| wanted.as_str() == Some(q.id.as_str()))
        .filter(|q| q.kind == "choice");
    let Some(q) = q else {
        return Err(Failure::Invalid(format!(
            "no choice question {}",
            repr(wanted)
        )));
    };
    let text = python_str(field(request, "text")?);
    let value = choices::interpret(&q.id, &text, &q.options, q.diameter, q.depth)
        .map_err(|e| Failure::Invalid(e.0))?;
    let mut out = Map::new();
    out.insert("value".into(), json!(value));
    let shown = if q.id.starts_with("hole.function:") {
        choices::with_default_full_thread(&value, q.depth)
    } else {
        value.clone()
    };
    out.insert("code".into(), json!(choices::code(&shown)));
    if let Some(d) = q.diameter
        && (q.id.starts_with("hole.function:") || q.id.starts_with("diameter.fit:"))
    {
        let callout =
            choices::callout(&shown, d, q.id.starts_with("hole.")).map_err(Failure::Invalid)?;
        out.insert("callout".into(), json!(callout));
    }
    Ok(Value::Object(out))
}

/// A failure as the reply's `error`.
fn error(failure: Failure) -> Value {
    match failure {
        Failure::Incomplete(e) => {
            let mut out = Map::new();
            out.insert("code".into(), json!("incomplete"));
            out.insert("message".into(), json!(e.to_string()));
            out.insert("missing".into(), json!(e.missing));
            if let Some(part) = e.part {
                out.insert("part".into(), json!(part));
            }
            Value::Object(out)
        }
        Failure::Invalid(message) => json!({"code": "invalid", "message": message}),
        Failure::Failed { message, trace } => {
            json!({"code": "failed", "message": message, "trace": trace})
        }
    }
}

/// A request's field, or Python's `KeyError` as `invalid`: `missing field 'name'`.
fn field<'r>(request: &'r Map<String, Value>, name: &str) -> Result<&'r Value, Failure> {
    request
        .get(name)
        .ok_or_else(|| Failure::Invalid(format!("missing field '{name}'")))
}

/// [`field`] of a value that should be an object.
fn field_of<'v>(value: &'v Value, name: &str) -> Result<&'v Value, Failure> {
    match value {
        Value::Object(map) => field(map, name),
        other => Err(Failure::Invalid(format!(
            "expected an object with '{name}', not {}",
            type_name(other)
        ))),
    }
}

/// A field naming a file.
fn path(request: &Map<String, Value>, name: &str) -> Result<PathBuf, Failure> {
    match field(request, name)? {
        Value::String(s) => Ok(PathBuf::from(s)),
        other => Err(Failure::Invalid(format!(
            "'{name}' is a path, not {}",
            type_name(other)
        ))),
    }
}

/// The answers given (none: `{}`), which must be an object.
fn answers(value: Option<&Value>) -> Result<Map<String, Value>, Failure> {
    match value {
        None => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(other) => Err(Failure::Invalid(format!(
            "answers are a JSON object, not {}",
            type_name(other)
        ))),
    }
}

/// A part or face index.
fn index(value: &Value, name: &str) -> Result<usize, Failure> {
    value
        .as_u64()
        .map(|k| k as usize)
        .ok_or_else(|| Failure::Invalid(format!("{name} is an index, not {}", repr(value))))
}

/// A group of face indices.
fn face_list(value: &Value) -> Result<Vec<usize>, Failure> {
    let Some(items) = value.as_array() else {
        return Err(Failure::Invalid(format!(
            "faces are a list, not {}",
            repr(value)
        )));
    };
    items.iter().map(|v| index(v, "a face")).collect()
}

/// Python's `type(value).__name__` for a JSON value.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python's `str(value)`: a string as it is, anything else as its `repr`.
fn python_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => repr(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panic inside one request is answered as `failed`, with the panic's message, location
    /// and backtrace as its trace, and the session carries on.
    #[test]
    fn a_panic_in_one_request_is_answered_and_the_session_carries_on() {
        let input = "{\"id\": 1, \"op\": \"panic for a test\"}\n{\"id\": 2, \"op\": \"hello\"}\n";
        let mut out = Vec::new();
        serve(input.as_bytes(), &mut out).unwrap();
        let replies: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(replies.len(), 2);
        let error = &replies[0]["error"];
        assert_eq!(replies[0]["id"], json!(1));
        assert_eq!(error["code"], json!("failed"));
        assert_eq!(error["message"], json!("panicked: a panic for a test"));
        let trace = error["trace"].as_str().unwrap();
        assert!(
            trace.contains("a panic for a test") && trace.contains("serve.rs"),
            "{trace}"
        );
        assert_eq!(
            replies[1],
            json!({"id": 2, "ok": true, "result": {"protocol": 1}})
        );
    }
}
