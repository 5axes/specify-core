//! `specify_core._native`: the Rust engine behind the Python package `specify_core`.
//!
//! The package's `api.py` is a thin layer over these functions, with the names, signatures and
//! return shapes of specify-core's own `api.py`. Values cross as JSON text, as they do in the
//! WASM binding and `serve`: an analysis is a stored snapshot, read by the rules as JSON, and
//! serde_json's text round-trips every number through Python's `json` exactly.
//!
//! A part loaded once is a [`LoadedPart`] handle (Python's `LoadedPart`, opaque here): `analyse`,
//! `mesh`, `write`, `write_parts`, `read_existing` and `part_settings` take it (or a path), so a
//! caller that reuses a part, as `serve` does, reads the file once.
//!
//! The command line ([`cli`]) and a serving session ([`Server`], `serve.reply`) are the engine's
//! own (`specify_core::cli`, `specify_core::serve`).
//!
//! Errors are raised as Python raises them: answers that leave questions open are the
//! package's `IncompleteError` (a `ValueError`, with `missing` and `part`); a file or part that
//! cannot be loaded, an answer the rules refuse, or a write asked wrongly is a `ValueError`; a
//! file written that does not read back as written is the package's `VerificationError` (a
//! `RuntimeError`); anything else (a part that cannot be analysed honestly, a write haecceity
//! refuses) is a `RuntimeError`.

use std::path::PathBuf;

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use serde_json::{Map, Value, json};
use specify_core::api::{self, Error};
use specify_core::choices;
use specify_core::existing;
use specify_core::part;
use specify_core::rules::{self, Question};
use specify_core::writer::WriteError;

/// Python's exception for an engine error (see the module's documentation).
fn raised(error: Error) -> PyErr {
    if let Error::Rules(rules::Error::Incomplete(e)) = &error {
        return incomplete(e);
    }
    if let Error::Write(WriteError::Verification(e)) = &error {
        return package_error("specify_core.writer", "VerificationError", e.to_string());
    }
    match error {
        Error::Load(_)
        | Error::Rules(rules::Error::Invalid(_))
        | Error::Write(WriteError::Invalid(_)) => PyValueError::new_err(error.to_string()),
        _ => PyRuntimeError::new_err(error.to_string()),
    }
}

/// The package's `IncompleteError(missing, part)`, as specify-core's `rules` raises it.
fn incomplete(e: &rules::IncompleteError) -> PyErr {
    Python::attach(|py| {
        made(
            py,
            "specify_core.rules",
            "IncompleteError",
            (e.missing.clone(), e.part),
        )
    })
}

/// The package's exception `module.name(message)`.
fn package_error(module: &str, name: &str, message: String) -> PyErr {
    Python::attach(|py| made(py, module, name, (message,)))
}

fn made<'py>(
    py: Python<'py>,
    module: &str,
    name: &str,
    args: impl pyo3::call::PyCallArgs<'py>,
) -> PyErr {
    let error = py
        .import(module)
        .and_then(|m| m.getattr(name))
        .and_then(|class| class.call1(args));
    match error {
        Ok(error) => PyErr::from_value(error),
        Err(error) => error,
    }
}

fn parse(what: &str, text: &str) -> PyResult<Value> {
    serde_json::from_str(text).map_err(|e| PyValueError::new_err(format!("Invalid {what}: {e}")))
}

/// The distinct parts of `step`, as a JSON list (`api.parts`).
#[pyfunction]
fn parts(py: Python<'_>, step: PathBuf) -> PyResult<String> {
    let parts = py.detach(|| api::parts(&step)).map_err(raised)?;
    Ok(Value::Array(parts).to_string())
}

/// Assembly bill of materials, including each field's source.
#[pyfunction]
fn bom(py: Python<'_>, step: PathBuf) -> PyResult<String> {
    let rows = py
        .detach(|| specify_core::bom::read_path(&step))
        .map_err(PyValueError::new_err)?;
    serde_json::to_string(&rows).map_err(|e| PyRuntimeError::new_err(e.to_string()))
}

/// One part of a STEP file, loaded once (Python's `LoadedPart`): the part and the file it is
/// of, so a part of an assembly is analysed with the other parts its mates need, from the same
/// bytes. Its geometry stays in the engine; Python sees what identifies it.
#[pyclass(frozen, module = "specify_core._native")]
struct LoadedPart {
    file: part::File,
    loaded: part::LoadedPart,
}

#[pymethods]
impl LoadedPart {
    /// The file it was read from.
    #[getter]
    fn path(&self) -> PathBuf {
        self.loaded.path.clone()
    }

    /// What face indices are only valid against, as JSON (`Binding.to_dict()`).
    #[getter]
    fn binding(&self) -> PyResult<String> {
        serde_json::to_string(&self.loaded.binding)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// How many parts the file holds; this is part `binding.part` of them.
    #[getter]
    fn part_count(&self) -> usize {
        self.loaded.part_count
    }

    /// The part's name in the file, if it has one.
    #[getter]
    fn name(&self) -> &str {
        &self.loaded.name
    }
}

/// `step` as a [`LoadedPart`] handle, or None for a path. A loaded part takes no `part`.
fn loaded<'a>(step: &'a Bound<'_, PyAny>, part: Option<usize>) -> PyResult<Option<&'a LoadedPart>> {
    let Ok(handle) = step.cast::<LoadedPart>() else {
        return Ok(None);
    };
    if part.is_some() {
        return Err(PyTypeError::new_err("a loaded part takes no part"));
    }
    Ok(Some(handle.get()))
}

/// `step` as a handle, or the `part`-th part of the path `step`, loaded.
fn load_path(py: Python<'_>, step: &Bound<'_, PyAny>, part: Option<usize>) -> PyResult<LoadedPart> {
    let path: PathBuf = step.extract()?;
    load(py, path, part)
}

/// Triangles of `step` (its `part`-th part), or of a part already loaded, ranged by the face
/// indices its analysis uses, in the part's own frame, as JSON (`api.mesh`).
#[pyfunction]
#[pyo3(signature = (step, part=None))]
fn mesh(py: Python<'_>, step: &Bound<'_, PyAny>, part: Option<usize>) -> PyResult<String> {
    if let Some(handle) = loaded(step, part)? {
        return Ok(py.detach(|| api::mesh(&handle.loaded)).to_string());
    }
    let path: PathBuf = step.extract()?;
    let mesh = py.detach(|| api::mesh_file(&path, part)).map_err(raised)?;
    Ok(mesh.to_string())
}

/// Revision report and Markdown, including numbered semantic PMI changes.
#[pyfunction]
#[pyo3(signature = (old, new, answers="{}", title="Revision", part=None, scenes=false))]
fn diff(
    py: Python<'_>,
    old: &Bound<'_, PyAny>,
    new: &Bound<'_, PyAny>,
    answers: &str,
    title: &str,
    part: Option<usize>,
    scenes: bool,
) -> PyResult<String> {
    let answers = answers_of(answers)?;
    let old_owned;
    let old = match loaded(old, part)? {
        Some(handle) => handle,
        None => {
            old_owned = load_path(py, old, part)?;
            &old_owned
        }
    };
    let new_owned;
    let new = match loaded(new, part)? {
        Some(handle) => handle,
        None => {
            new_owned = load_path(py, new, part)?;
            &new_owned
        }
    };
    py.detach(|| {
        let report = specify_core::revision::diff_with_options(
            &old.loaded,
            &new.loaded,
            &answers,
            specify_core::revision::DiffOptions { scenes },
        )?;
        Ok(json!({"markdown": report.markdown(title, None), "report": report}).to_string())
    })
    .map_err(raised)
}

/// Haecceity PMI/presentation plus Specify metadata, from bytes or an open part.
#[pyfunction]
#[pyo3(signature = (step, answers=None, part=None))]
fn pmi_scene(
    py: Python<'_>,
    step: &Bound<'_, PyAny>,
    answers: Option<&str>,
    part: Option<usize>,
) -> PyResult<String> {
    let value = answers
        .map(|text| parse("answers or review state", text))
        .transpose()?;
    let state = value
        .as_ref()
        .filter(|v| v.get("answers").is_some_and(Value::is_object))
        .map(|v| {
            serde_json::from_value::<specify_core::tolerancing::State>(v.clone())
                .map_err(|e| PyValueError::new_err(e.to_string()))
        })
        .transpose()?;
    let answers = if state.is_some() {
        None
    } else {
        answers.map(answers_of).transpose()?
    };
    if let Ok(bytes) = step.extract::<Vec<u8>>() {
        if answers.is_some() || state.is_some() {
            return Err(PyTypeError::new_err(
                "bytes are view-only; load a part to supply answers",
            ));
        }
        return py
            .detach(|| specify_core::pmi_scene::pmi_scene(&bytes, part))
            .map(|v| v.to_string())
            .map_err(PyValueError::new_err);
    }
    let owned;
    let handle = match loaded(step, part)? {
        Some(handle) => handle,
        None => {
            owned = load_path(py, step, part)?;
            &owned
        }
    };
    py.detach(|| {
        let analysis = state
            .as_ref()
            .map(|s| &s.answers)
            .or(answers.as_ref())
            .map(|answers| {
                let analysis = api::analyse(&handle.loaded).map_err(|e| e.to_string())?;
                specify_core::input::answers(&analysis, answers).map_err(|e| e.to_string())?;
                Ok::<_, String>(analysis)
            })
            .transpose()?;
        let scene = specify_core::pmi_scene::Scene::new(&handle.loaded)?;
        if let Some(state) = &state {
            scene.scene_with_state(&handle.loaded, analysis.as_ref(), state)
        } else {
            scene.scene(&handle.loaded, analysis.as_ref(), answers.as_ref())
        }
    })
    .map(|v| v.to_string())
    .map_err(PyValueError::new_err)
}

/// Write `intent` (JSON) into `step` at `output`, on the part it was made for, or into a part
/// already loaded, verified by reading back; given the `answers` (JSON) it came from, they are
/// stored too. The report, as JSON (`api.write`).
#[pyfunction]
#[pyo3(signature = (step, intent, output, answers=None))]
fn write(
    py: Python<'_>,
    step: &Bound<'_, PyAny>,
    intent: &str,
    output: PathBuf,
    answers: Option<&str>,
) -> PyResult<String> {
    let intent = intent_of(intent)?;
    let answers = answers.map(answers_of).transpose()?;
    let report = match loaded(step, None)? {
        Some(handle) => {
            py.detach(|| api::write(&handle.loaded, &intent, &output, answers.as_ref()))
        }
        None => {
            let path: PathBuf = step.extract()?;
            py.detach(|| api::write_file(&path, &intent, &output, answers.as_ref()))
        }
    }
    .map_err(raised)?;
    Ok(report.to_dict().to_string())
}

/// Write several parts of `step` (a path, or any part of the file, loaded) together: each
/// `(intent, answers)` (JSON; answers may be None) on the part its intent was made for. The
/// report, as JSON (`api.write_parts`).
#[pyfunction]
fn write_parts(
    py: Python<'_>,
    step: &Bound<'_, PyAny>,
    intents: Vec<(String, Option<String>)>,
    output: PathBuf,
) -> PyResult<String> {
    let mut jobs = Vec::with_capacity(intents.len());
    for (intent, answers) in &intents {
        jobs.push((
            intent_of(intent)?,
            answers.as_deref().map(answers_of).transpose()?,
        ));
    }
    let report = match loaded(step, None)? {
        Some(handle) => py.detach(|| write_parts_of(&handle.file, &jobs, &output)),
        None => {
            let path: PathBuf = step.extract()?;
            py.detach(|| api::write_parts(&path, &jobs, &output))
        }
    }
    .map_err(raised)?;
    Ok(report.to_dict().to_string())
}

/// `api::write_parts` on a file already read.
fn write_parts_of(
    file: &part::File,
    jobs: &[(rules::Intent, Option<Map<String, Value>>)],
    output: &std::path::Path,
) -> api::Result<specify_core::writer::WriteReport> {
    let parts = file.load_all()?;
    let mut writes = Vec::with_capacity(jobs.len());
    for (intent, answers) in jobs {
        let k = api::intent_part(intent)?;
        let Some(part) = parts.get(k) else {
            return Err(Error::Write(WriteError::Invalid(format!(
                "{} has no part {k}",
                file.path().display()
            ))));
        };
        writes.push((part, intent, answers.as_ref()));
    }
    Ok(specify_core::writer::write_parts(&writes, output)?)
}

/// The semantic PMI the file states on the part (a handle, or the `part`-th part of a path):
/// every field of each `ExistingPmi`, as JSON (`existing.read_existing`).
#[pyfunction]
#[pyo3(signature = (step, part=None))]
fn read_existing(py: Python<'_>, step: &Bound<'_, PyAny>, part: Option<usize>) -> PyResult<String> {
    let owned;
    let handle = match loaded(step, part)? {
        Some(handle) => handle,
        None => {
            owned = load_path(py, step, part)?;
            &owned
        }
    };
    let items = py
        .detach(|| existing::read_existing(&handle.loaded))
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let records: Vec<Value> = items
        .iter()
        .map(|e| {
            json!({"kind": e.kind, "type": e.r#type, "faces": e.faces, "faces2": e.faces2,
                   "value": e.value, "upper": e.upper, "lower": e.lower, "limits": e.limits, "basic": e.basic, "datums": e.datums})
        })
        .collect();
    Ok(Value::Array(records).to_string())
}

/// The part's settings the file states (material, general tolerance, notes), as JSON
/// (`existing.part_settings`).
#[pyfunction]
#[pyo3(signature = (step, part=None))]
fn part_settings(py: Python<'_>, step: &Bound<'_, PyAny>, part: Option<usize>) -> PyResult<String> {
    let owned;
    let handle = match loaded(step, part)? {
        Some(handle) => handle,
        None => {
            owned = load_path(py, step, part)?;
            &owned
        }
    };
    let settings = py
        .detach(|| existing::part_settings(&handle.loaded))
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    Ok(Value::Object(settings).to_string())
}

/// The command line on `argv` (the arguments after the program's name), as the binary runs it:
/// replies on stdout, messages on stderr. The exit code.
#[pyfunction]
fn cli(py: Python<'_>, argv: Vec<String>) -> u8 {
    py.detach(|| specify_core::cli::main(&argv))
}

/// A serving session, as `specify-core serve` keeps one: the parts loaded for anchoring, kept
/// between requests (Python's `serve` module state).
#[pyclass(module = "specify_core._native")]
#[derive(Default)]
struct Server(specify_core::serve::Server);

#[pymethods]
impl Server {
    #[new]
    fn new() -> Self {
        Server::default()
    }

    /// The reply to one request (JSON), as JSON: its result, or a structured error.
    fn reply(&mut self, py: Python<'_>, request: &str) -> PyResult<String> {
        let request = parse("request", request)?;
        let server = &mut self.0;
        Ok(py.detach(|| server.reply(&request)).to_string())
    }
}

/// What may be wrong with the answers so far, as JSON (`problems.problems`).
#[pyfunction]
fn problems(py: Python<'_>, analysis: &str, answers: &str) -> PyResult<String> {
    let (analysis, answers) = parse_both(analysis, answers)?;
    let found = py
        .detach(|| specify_core::problems::problems(&analysis, &answers))
        .map_err(|e| raised(Error::Rules(e)))?;
    Ok(Value::Array(found).to_string())
}

/// The package's `choices.SpecError(message)`.
fn spec_error(e: choices::SpecError) -> PyErr {
    package_error("specify_core.choices", "SpecError", e.0)
}

/// The answer `text` means for question `qid` (`choices.interpret`).
#[pyfunction]
#[pyo3(signature = (qid, text, options, diameter, depth=None))]
fn choices_interpret(
    qid: &str,
    text: &str,
    options: Vec<String>,
    diameter: Option<f64>,
    depth: Option<f64>,
) -> PyResult<String> {
    choices::interpret(qid, text, &options, diameter, depth).map_err(spec_error)
}

/// Refuses an answer (JSON) that is neither offered nor a spec that fits (`choices.check`).
#[pyfunction]
#[pyo3(signature = (qid, value, options, diameter, depth=None))]
fn choices_check(
    qid: &str,
    value: &str,
    options: Vec<String>,
    diameter: Option<f64>,
    depth: Option<f64>,
) -> PyResult<()> {
    let value = parse("value", value)?;
    choices::check(qid, &value, &options, diameter, depth).map_err(spec_error)
}

/// Each option of a hole or turned-diameter question, as JSON (`choices.describe`).
#[pyfunction]
fn choices_describe(qid: &str, options: Vec<String>, diameter: Option<f64>) -> PyResult<String> {
    let described = choices::describe(qid, &options, diameter).map_err(PyValueError::new_err)?;
    Ok(Value::Array(described).to_string())
}

/// An answer as an engineer would name it (`choices.code`).
#[pyfunction]
fn choices_code(value: &str) -> String {
    choices::code(value)
}

/// What the answer puts on the drawing for a feature of diameter `d` (`choices.callout`).
#[pyfunction]
fn choices_callout(value: &str, d: f64, hole: bool) -> PyResult<String> {
    choices::callout(value, d, hole).map_err(PyValueError::new_err)
}

fn intent_of(text: &str) -> PyResult<rules::Intent> {
    rules::Intent::from_dict(&parse("intent", text)?).map_err(|e| raised(Error::Rules(e)))
}

fn answers_of(text: &str) -> PyResult<Map<String, Value>> {
    match parse("answers", text)? {
        Value::Object(answers) => Ok(answers),
        _ => Err(PyValueError::new_err("Invalid answers: not an object")),
    }
}

/// The `part`-th part of `step` (any part of a file of one part; a file of several must say
/// which), loaded (`load.load`).
#[pyfunction]
#[pyo3(signature = (step, part=None))]
fn load(py: Python<'_>, step: PathBuf, part: Option<usize>) -> PyResult<LoadedPart> {
    let (file, loaded) = py
        .detach(|| {
            let file = part::File::read(&step)?;
            let loaded = file.load(part)?;
            Ok::<_, part::LoadError>((file, loaded))
        })
        .map_err(|e| raised(Error::Load(e)))?;
    Ok(LoadedPart { file, loaded })
}

/// The analysis of `step` (its `part`-th part, in an assembly), or of a part already loaded
/// (`part` must then be None), as JSON (`api.analyse`).
#[pyfunction]
#[pyo3(signature = (step, part=None))]
fn analyse(py: Python<'_>, step: &Bound<'_, PyAny>, part: Option<usize>) -> PyResult<String> {
    if let Ok(handle) = step.cast::<LoadedPart>() {
        if part.is_some() {
            return Err(PyTypeError::new_err(
                "analyse of a loaded part takes no part",
            ));
        }
        let handle = handle.get();
        let (analysis, _) = py
            .detach(|| api::analyse_reporting_in(&handle.file, &handle.loaded))
            .map_err(raised)?;
        return Ok(analysis.to_string());
    }
    let path: PathBuf = step.extract()?;
    let analysis = py
        .detach(|| api::analyse_file(&path, part))
        .map_err(raised)?;
    Ok(analysis.to_string())
}

/// Every question the part needs, given the answers so far (`api.questions`): a JSON list of
/// every field of each `Question`, for the package to build Python's `Question` from. Not
/// `to_dict`, which leaves out what Python's dataclass carries beside it (each part's faces,
/// `diameter`, `depth`, `mate`).
#[pyfunction]
fn questions(py: Python<'_>, analysis: &str, answers: &str) -> PyResult<String> {
    let (analysis, answers) = parse_both(analysis, answers)?;
    let questions = py
        .detach(|| api::questions(&analysis, &answers))
        .map_err(raised)?;
    Ok(Value::Array(questions.iter().map(record).collect()).to_string())
}

/// The intent `answers` amount to, as JSON (`Intent.to_dict()`); with `accept_defaults`,
/// unanswered defaults are taken first, to a fixed point (`api.apply`).
#[pyfunction]
#[pyo3(signature = (analysis, answers, accept_defaults=false, bom=false))]
fn apply(
    py: Python<'_>,
    analysis: &str,
    answers: &str,
    accept_defaults: bool,
    bom: bool,
) -> PyResult<String> {
    let (analysis, answers) = parse_both(analysis, answers)?;
    let mut intent = py
        .detach(|| api::apply(&analysis, &answers, accept_defaults))
        .map_err(raised)?;
    if bom {
        specify_core::bom::accept_suggestions(&analysis, &answers, &mut intent)
            .map_err(pyo3::exceptions::PyValueError::new_err)?;
    }
    Ok(intent.to_dict().to_string())
}

/// What the answers so far amount to, as JSON (`api.preview`).
#[pyfunction]
fn preview(py: Python<'_>, analysis: &str, answers: &str) -> PyResult<String> {
    let (analysis, answers) = parse_both(analysis, answers)?;
    let intent = py
        .detach(|| api::preview(&analysis, &answers))
        .map_err(raised)?;
    Ok(intent.to_dict().to_string())
}

/// `answers` plus every default, to a fixed point, as JSON (`rules.with_all_defaults`).
#[pyfunction]
fn with_all_defaults(py: Python<'_>, analysis: &str, answers: &str) -> PyResult<String> {
    let (analysis, answers) = parse_both(analysis, answers)?;
    let answers = py
        .detach(|| rules::with_all_defaults(&analysis, &answers))
        .map_err(|e| raised(Error::Rules(e)))?;
    Ok(Value::Object(answers).to_string())
}

fn parse_both(analysis: &str, answers: &str) -> PyResult<(Value, Map<String, Value>)> {
    let analysis = parse("analysis", analysis)?;
    let Value::Object(answers) = parse("answers", answers)? else {
        return Err(PyValueError::new_err("Invalid answers: not an object"));
    };
    Ok((analysis, answers))
}

/// Every field of `question`, under Python's field names.
fn record(question: &Question) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(question.id));
    out.insert("optional".into(), json!(question.id.starts_with("bom.")));
    out.insert("kind".into(), json!(question.kind));
    out.insert("prompt".into(), json!(question.prompt));
    out.insert("faces".into(), json!(question.faces));
    out.insert(
        "default".into(),
        question.default.clone().unwrap_or(Value::Null),
    );
    out.insert("options".into(), json!(question.options));
    out.insert("why".into(), json!(question.why));
    out.insert("feature".into(), json!(question.feature));
    out.insert("frame".into(), json!(question.frame));
    out.insert("attention".into(), json!(question.attention));
    out.insert("reason".into(), json!(question.reason));
    out.insert("parts".into(), json!(question.parts));
    out.insert("suggestions".into(), json!(question.suggestions));
    out.insert("choices".into(), json!(question.choices));
    out.insert("diameter".into(), json!(question.diameter));
    out.insert("depth".into(), json!(question.depth));
    out.insert("basis".into(), json!(question.basis));
    out.insert("mate".into(), json!(question.mate));
    Value::Object(out)
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LoadedPart>()?;
    m.add_class::<Server>()?;
    m.add_function(wrap_pyfunction!(cli, m)?)?;
    m.add_function(wrap_pyfunction!(problems, m)?)?;
    m.add_function(wrap_pyfunction!(choices_interpret, m)?)?;
    m.add_function(wrap_pyfunction!(choices_check, m)?)?;
    m.add_function(wrap_pyfunction!(choices_describe, m)?)?;
    m.add_function(wrap_pyfunction!(choices_code, m)?)?;
    m.add_function(wrap_pyfunction!(choices_callout, m)?)?;
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(parts, m)?)?;
    m.add_function(wrap_pyfunction!(bom, m)?)?;
    m.add_function(wrap_pyfunction!(analyse, m)?)?;
    m.add_function(wrap_pyfunction!(questions, m)?)?;
    m.add_function(wrap_pyfunction!(apply, m)?)?;
    m.add_function(wrap_pyfunction!(preview, m)?)?;
    m.add_function(wrap_pyfunction!(with_all_defaults, m)?)?;
    m.add_function(wrap_pyfunction!(mesh, m)?)?;
    m.add_function(wrap_pyfunction!(pmi_scene, m)?)?;
    m.add_function(wrap_pyfunction!(diff, m)?)?;
    m.add_function(wrap_pyfunction!(write, m)?)?;
    m.add_function(wrap_pyfunction!(write_parts, m)?)?;
    m.add_function(wrap_pyfunction!(read_existing, m)?)?;
    m.add_function(wrap_pyfunction!(part_settings, m)?)?;
    m.add("ANALYSIS_SCHEMA", api::ANALYSIS_SCHEMA)?;
    m.add("LOADER_VERSION", part::LOADER_VERSION)?;
    m.add("RULES_VERSION", rules::RULES_VERSION)?;
    Ok(())
}
