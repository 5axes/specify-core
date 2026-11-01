//! Gap rules: which questions a part needs, and how answers become intent (port of
//! `rules.py`).
//!
//! Rules are deterministic and decide completeness. Defaults come from standards and simple
//! geometry; a part is only complete when every question here has an answer. Questions are
//! recomputed from the analysis and the answers so far, because some depend on earlier answers
//! (a hole's position tolerance depends on whether it is a clearance, tapped or fitted hole,
//! and on which datums exist).
//!
//! Everything here is a pure function of the analysis JSON and the answers JSON, read as
//! Python reads its dicts (`records`): no geometry. The analysis may come from either engine;
//! a record the rules need but cannot read is an [`Error::Invalid`] naming it, never a panic.
//! Messages, prompts and the JSON produced are specify-core's, byte for byte and key for key.

// Literal indices into LOCATING and EDGES are in bounds of their fixed-size constant arrays.
use std::collections::BTreeSet;
use std::fmt;

use serde_json::{Map, Value};

use crate::choices::{self, SpecError};
use crate::{extras, general, locations, materials, standards};

pub mod datums;
mod holes;
pub mod mates;
mod py;
pub mod records;
mod turned;

pub use datums::{Control, datum_control, datum_kind, default_datums};
pub use records::{Datum, DatumKind, Datums, Face, Faces};
pub use turned::RUNOUTS;

pub(crate) use holes::{bores, bores_of};
pub(crate) use py::{float_of, int_of, iter_of, object, repr, round, str_of, truthy};
pub(crate) use records::{Ctx, Feature};

pub const RULES_VERSION: i64 = 3;

pub const NONE: &str = choices::NONE;
pub const GENERAL: &str = choices::GENERAL;

/// How holes are located: by position tolerances from datums, or by ± dimensions on the
/// drawing under the general tolerance. Both use datums as measurement origins.
pub const LOCATING: [&str; 2] = ["position tolerances", "± dimensions"];

/// Surface roughness Ra in micrometres, finest first, and edge notes.
pub const FINISHES: [&str; 6] = ["Ra 0.4", "Ra 0.8", "Ra 1.6", "Ra 3.2", "Ra 6.3", "Ra 12.5"];
pub const EDGES: [&str; 4] = [
    "Break sharp edges 0.2 max",
    "Break sharp edges 0.5 max",
    "Deburr only",
    "No edge note",
];
/// The part's notes, beside its material and general tolerance. The four conventional notes
/// accept "nothing" answers; free-text `notes` is an ordered string list, empty by default.
pub const NOTES: [&str; 5] = [
    "surface_finish",
    "coating",
    "heat_treatment",
    "edges",
    "notes",
];
const NOTHING: [&str; 5] = [
    "",
    "none",
    "none (as machined)",
    "as machined",
    "no edge note",
];

/// Datums beyond A, B and C, asked only once the user adds them.
pub const EXTRA_DATUMS: &str = "DEFGH";

/// How much each question needs a person, so a part can be reviewed rather than
/// interrogated: `required` has no default and must be answered; `check` has a default the
/// rules doubt, with a `reason`; `routine` has a suggested default from a standard or the
/// geometry; `refine` is an optional tightening that defaults to none or
/// general.
pub const ATTENTION: [&str; 4] = ["required", "check", "routine", "refine"];

// ------------------------------------------------------------------------------------------
// Errors
// ------------------------------------------------------------------------------------------

/// Answers that leave questions open (Python's `IncompleteError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompleteError {
    pub missing: Vec<String>,
    /// In an assembly, which part's questions they are.
    pub part: Option<usize>,
}

impl IncompleteError {
    pub fn new(missing: Vec<String>, part: Option<usize>) -> Self {
        IncompleteError { missing, part }
    }
}

impl fmt::Display for IncompleteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(part) = self.part {
            write!(f, "part {part}: ")?;
        }
        write!(
            f,
            "{} question(s) unanswered: {}",
            self.missing.len(),
            self.missing.join(", ")
        )
    }
}

impl std::error::Error for IncompleteError {}

/// Why the rules refused: questions left open, or an answer (or a record of the analysis) that
/// cannot be used, with Python's `ValueError` message (a typed spec's `SpecError` among them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Incomplete(IncompleteError),
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Incomplete(e) => e.fmt(f),
            Error::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<SpecError> for Error {
    fn from(e: SpecError) -> Self {
        Error::Invalid(e.0)
    }
}

impl From<IncompleteError> for Error {
    fn from(e: IncompleteError) -> Self {
        Error::Incomplete(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(message.into()))
}

// ------------------------------------------------------------------------------------------
// Question and Intent
// ------------------------------------------------------------------------------------------

/// The geometric tolerance a question asks about, independent of its wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Characteristic {
    Flatness,
    Perpendicularity,
    Parallelism,
    Position,
    Runout,
}

/// One question: what it asks, of which faces, its default and options, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub id: String,
    /// `choice`, `text`, `text_list` (ordered free-text strings) or `faces`.
    pub kind: &'static str,
    pub prompt: String,
    /// A feature row's title, resolved where its geometry is known.
    pub title: Option<String>,
    pub characteristic: Option<Characteristic>,
    /// What to highlight.
    pub faces: Vec<i64>,
    /// None: no default (Python's `None`); a datum's is a list of faces, possibly empty.
    pub default: Option<Value>,
    pub options: Vec<String>,
    pub why: String,
    pub feature: Option<String>,
    /// For a position or runout: the datum frame it is measured from, e.g. `A|B|C`.
    pub frame: Option<String>,
    /// How much a person needs to look at it; see [`ATTENTION`].
    pub attention: &'static str,
    /// Why a `check` question was flagged.
    pub reason: String,
    /// When identical features are asked about once: each one's id and faces. Each still gets
    /// its own requirements.
    pub parts: Vec<(String, Vec<i64>)>,
    /// For a text question: common answers to offer as it is typed.
    pub suggestions: Vec<Value>,
    /// For a hole or turned diameter: each option's purpose, label, code and callout.
    pub choices: Vec<Value>,
    /// The feature's diameter, against which a typed spec is checked (not serialised).
    pub diameter: Option<f64>,
    /// For a blind hole: its drill depth, against which a typed thread depth is checked (not
    /// serialised).
    pub depth: Option<f64>,
    /// Why the default is what it is, in a sentence.
    pub basis: String,
    /// When the default comes from how the part fits another in its assembly: why (not
    /// serialised; it becomes the `reason` of a `check`).
    pub mate: String,
}

impl Question {
    /// A question with no default, options or explanation yet, `routine`.
    pub fn new(id: String, kind: &'static str, prompt: String, faces: Vec<i64>) -> Question {
        Question {
            id,
            kind,
            prompt,
            title: None,
            characteristic: None,
            faces,
            default: None,
            options: Vec::new(),
            why: String::new(),
            feature: None,
            frame: None,
            attention: "routine",
            reason: String::new(),
            parts: Vec::new(),
            suggestions: Vec::new(),
            choices: Vec::new(),
            diameter: None,
            depth: None,
            basis: String::new(),
            mate: String::new(),
        }
    }

    /// Additive presentation record for clients that must not parse the prompt.
    /// `to_dict` retains the Python-compatible document for existing callers.
    pub fn to_presented_dict(&self) -> Value {
        let mut out = self.to_dict();
        if let Some(title) = &self.title {
            // to_dict() constructs an object; Value writes add string keys without bounds checks.
            out["title"] = Value::from(title.as_str());
        }
        if let Some(characteristic) = self.characteristic {
            // Characteristic is a fieldless Serialize enum, which always serializes as a string.
            out["characteristic"] =
                serde_json::to_value(characteristic).expect("characteristic serialises");
        }
        out
    }

    /// `Question.to_dict()`: Python's keys, order and omissions.
    pub fn to_dict(&self) -> Value {
        let mut out = object([
            ("id", Value::from(self.id.as_str())),
            ("kind", Value::from(self.kind)),
            ("prompt", Value::from(self.prompt.as_str())),
            ("faces", Value::from(self.faces.clone())),
            ("default", self.default.clone().unwrap_or(Value::Null)),
            ("why", Value::from(self.why.as_str())),
        ]);
        if !self.options.is_empty() {
            out.insert("options".into(), Value::from(self.options.clone()));
        }
        if let Some(feature) = self.feature.as_ref().filter(|f| !f.is_empty()) {
            out.insert("feature".into(), Value::from(feature.as_str()));
        }
        if let Some(frame) = self.frame.as_ref().filter(|f| !f.is_empty()) {
            out.insert("frame".into(), Value::from(frame.as_str()));
        }
        out.insert("attention".into(), Value::from(self.attention));
        if !self.reason.is_empty() {
            out.insert("reason".into(), Value::from(self.reason.as_str()));
        }
        if !self.parts.is_empty() {
            let ids: Vec<Value> = self
                .parts
                .iter()
                .map(|(id, _)| Value::from(id.as_str()))
                .collect();
            out.insert("features".into(), Value::Array(ids));
        }
        if !self.suggestions.is_empty() {
            out.insert("suggestions".into(), Value::from(self.suggestions.clone()));
        }
        if !self.choices.is_empty() {
            out.insert("choices".into(), Value::from(self.choices.clone()));
        }
        if !self.basis.is_empty() {
            out.insert("basis".into(), Value::from(self.basis.as_str()));
        }
        Value::Object(out)
    }
}

/// What the answers amount to: the part's settings, its datums and its requirements, each
/// a JSON object with specify-core's keys in its order; and the dimensions the general
/// tolerance covers (listed for inspection, and written with their nominal alone).
#[derive(Debug, Clone, PartialEq)]
pub struct Intent {
    pub schema: i64,
    pub rules_version: i64,
    pub binding: Value,
    pub part: Map<String, Value>,
    pub datums: Vec<Map<String, Value>>,
    pub requirements: Vec<Map<String, Value>>,
    pub general: Vec<Map<String, Value>>,
}

impl Intent {
    /// An empty intent for the part `binding` names.
    pub fn new(binding: Value) -> Intent {
        Intent {
            schema: 1,
            rules_version: RULES_VERSION,
            binding,
            part: Map::new(),
            datums: Vec::new(),
            requirements: Vec::new(),
            general: Vec::new(),
        }
    }

    pub fn to_dict(&self) -> Value {
        let maps = |items: &[Map<String, Value>]| {
            Value::Array(items.iter().cloned().map(Value::Object).collect())
        };
        Value::Object(object([
            ("schema", Value::from(self.schema)),
            ("rules_version", Value::from(self.rules_version)),
            ("binding", self.binding.clone()),
            ("part", Value::Object(self.part.clone())),
            ("datums", maps(&self.datums)),
            ("requirements", maps(&self.requirements)),
            ("general", maps(&self.general)),
        ]))
    }

    /// An intent from its JSON, refusing any schema but 1 with Python's message. Absent parts
    /// default as Python's do (`rules_version` 0); a part of the wrong JSON type is refused
    /// (Python would carry it along and fail later).
    pub fn from_dict(data: &Value) -> Result<Intent> {
        let schema = data.get("schema");
        // Python compares with ==, so 1.0 and True are 1 too.
        let one = match schema {
            Some(Value::Number(n)) => n.as_f64() == Some(1.0),
            Some(Value::Bool(b)) => *b,
            _ => false,
        };
        if !one {
            let shown = schema.map_or_else(|| "None".to_string(), repr);
            return invalid(format!("unsupported intent schema {shown}"));
        }
        let Some(binding) = data.get("binding") else {
            return invalid("an intent has no 'binding'");
        };
        let rules_version = match data.get("rules_version") {
            None => 0,
            Some(v) => v
                .as_i64()
                .ok_or_else(|| Error::Invalid(format!("unsupported rules_version {}", repr(v))))?,
        };
        let part = match data.get("part") {
            None => Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(_) => return invalid("an intent's 'part' is not an object"),
        };
        let list = |key: &str| -> Result<Vec<Map<String, Value>>> {
            match data.get(key) {
                None => Ok(Vec::new()),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        item.as_object().cloned().ok_or_else(|| {
                            Error::Invalid(format!("an intent's '{key}' holds a non-object"))
                        })
                    })
                    .collect(),
                Some(_) => invalid(format!("an intent's '{key}' is not a list")),
            }
        };
        Ok(Intent {
            schema: 1,
            rules_version,
            binding: binding.clone(),
            part,
            datums: list("datums")?,
            requirements: list("requirements")?,
            general: list("general")?,
        })
    }
}

// ------------------------------------------------------------------------------------------
// Questions
// ------------------------------------------------------------------------------------------

/// Every question the part needs, given the answers so far.
///
/// Propose the measurement frame before choosing how closely features are located, including
/// in ± mode and when every feature uses the general tolerance. Datums the person changes
/// or removes remain questions; a file's own frame is retained.
pub fn questions(analysis: &Value, answers: &Map<String, Value>) -> Result<Vec<Question>> {
    questions_in(&Ctx::new(analysis)?, answers)
}

/// `_questions` with the letters to `propose` given, for tests of the proposal itself.
pub fn questions_proposing(
    analysis: &Value,
    answers: &Map<String, Value>,
    propose: &str,
) -> Result<Vec<Question>> {
    all_questions(&Ctx::new(analysis)?, answers, propose)
}

pub(crate) fn questions_in(ctx: &Ctx<'_>, answers: &Map<String, Value>) -> Result<Vec<Question>> {
    let cited: String = extras::cited(answers)?.into_iter().collect();
    all_questions(ctx, answers, &format!("ABC{cited}"))
}

/// `_answer`: the answer to `qid`, else the default of the question `qid` among `qs`.
pub(crate) fn answer<'v>(
    answers: &'v Map<String, Value>,
    qs: &'v [Question],
    qid: &str,
) -> Option<&'v Value> {
    if let Some(v) = answers.get(qid) {
        return Some(v);
    }
    qs.iter()
        .find(|q| q.id == qid)
        .and_then(|q| q.default.as_ref())
}

/// `_questions`.
fn all_questions(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
    propose: &str,
) -> Result<Vec<Question>> {
    if let Some(notes) = answers.get("part.notes") {
        note_strings(notes)?;
    }
    let mut out = part_questions();
    // A file with its own datums locates by them; the choice is for a bare part.
    if ctx.in_file.is_empty()
        && ctx
            .features
            .iter()
            .any(|f| f.family == "holes" || f.family == "hole_patterns")
    {
        out.push(Question {
            default: Some(Value::from(LOCATING[0])),
            options: LOCATING.map(String::from).to_vec(),
            why: "Position tolerances locate each hole from datums, for parts that must fit \
                  together. ± dimensions locate them on the drawing under the general \
                  tolerance. In both modes, datums say where the part is measured from."
                .to_string(),
            basis: "Position tolerances, as holes usually take parts that must line up."
                .to_string(),
            ..Question::new(
                "part.locating".into(),
                "choice",
                "How are holes located?".into(),
                Vec::new(),
            )
        });
    }
    // What the file already states of the part is not asked again.
    if let Some(stated) = ctx.stated {
        out.retain(|q| {
            let key = q.id.strip_prefix("part.").unwrap_or(&q.id);
            !py::truthy_opt(stated.get(key))
        });
    }
    // An added requirement that is not one is refused, not silently left out.
    for (key, value) in answers {
        if key.starts_with(extras::PREFIX) && !value.is_object() {
            return invalid(format!(
                "{key}: an added requirement is an object, not {}",
                choices::short_repr(value, 6)
            ));
        }
    }
    for (key, extra) in extras::given(answers) {
        extras::check(key, extra, &ctx.faces)?;
    }
    let asked_datums = datums::datum_questions(ctx, answers, propose)?;
    let datums = datums::chosen_datums(ctx, answers, &asked_datums)?;
    let mut controls = Vec::new();
    for letter in datums.letters() {
        if datum_control(letter, &datums, &ctx.faces)?.is_some() {
            controls.push(letter);
        }
    }
    let by_position = !py::is_str(answer(answers, &out, "part.locating"), LOCATING[1]);
    let frames = if by_position {
        datums::frames(&datums.letters())
    } else {
        Vec::new()
    };
    let located = |hole: &Feature<'_>| -> Result<Vec<String>> {
        let bores = bores_of(hole, &ctx.faces)?;
        if !by_position || bores.iter().any(|i| ctx.positioned.contains(i)) {
            return Ok(Vec::new());
        }
        // ISO 1101 diametral position with zero nominal offset controls the hole's
        // median line in a cylindrical zone coaxial with A. The shoulder does not
        // constrain that zone; keep B for axial dimensions and other features.
        if datums::datum_of(&bores, &datums).is_none()
            && datums::turned_axis(ctx, answers)?.is_some()
            && datums::axis_locates(&bores, &datums, &ctx.faces)?
        {
            return Ok(vec!["A".to_string()]);
        }
        Ok(datums::hole_frames(&bores, &datums, &controls, &frames))
    };
    let covered = |ids: &[i64]| ids.iter().any(|i| ctx.covered.contains(i));

    let pattern_members: BTreeSet<&str> = ctx
        .features
        .iter()
        .filter(|f| f.family == "hole_patterns")
        .flat_map(|f| f.members.iter().copied())
        .collect();
    // (target, hole, frames) for every hole or group asked about, then merged where identical:
    // two equal bolt circles are one question.
    let mut holes: Vec<(holes::Target<'_>, &Feature<'_>, Vec<String>)> = Vec::new();
    let mut turned: Vec<&Feature<'_>> = Vec::new();
    for feature in &ctx.features {
        match feature.family {
            "holes" if !pattern_members.contains(feature.id) => {
                if covered(&bores_of(feature, &ctx.faces)?) {
                    continue;
                }
                holes.push((holes::Target::of(feature)?, feature, located(feature)?));
            }
            "hole_patterns" if !feature.members.is_empty() => {
                let members: Vec<&Feature<'_>> = feature
                    .members
                    .iter()
                    .map(|m| ctx.feature(m))
                    .collect::<Result<_>>()?;
                // Sizing one of a set of identical holes sizes the set: NIST CTC-01 attaches
                // its "4X Ø25" to one hole.
                let mut any_covered = false;
                for m in &members {
                    if covered(&bores_of(m, &ctx.faces)?) {
                        any_covered = true;
                        break;
                    }
                }
                if any_covered {
                    continue;
                }
                // A member chosen as a datum leaves its set and is asked about on its own.
                let mut rest = Vec::new();
                for &m in &members {
                    if datums::datum_of(&bores_of(m, &ctx.faces)?, &datums).is_some() {
                        holes.push((holes::Target::of(m)?, m, located(m)?));
                    } else {
                        rest.push(m);
                    }
                }
                let Some(&first) = rest.first() else {
                    continue;
                };
                let mut faces = Vec::new();
                for m in &rest {
                    faces.extend_from_slice(m.faces()?);
                }
                let target = holes::Target {
                    members: rest.iter().map(|m| m.id).collect(),
                    faces,
                    ..holes::Target::of_id(feature)
                };
                let mut positions = false;
                for m in &rest {
                    if bores_of(m, &ctx.faces)?
                        .iter()
                        .any(|i| ctx.positioned.contains(i))
                    {
                        positions = true;
                        break;
                    }
                }
                let frames = if positions {
                    Vec::new()
                } else {
                    frames.clone()
                };
                holes.push((target, first, frames));
            }
            "turned_steps" if !covered(feature.faces()?) => turned.push(feature),
            _ => {}
        }
    }

    let mut keys = Vec::with_capacity(holes.len());
    for (target, hole, frames) in &holes {
        keys.push(holes::hole_key(target, hole, frames, &ctx.faces, &datums)?);
    }
    for group in same(&holes, &keys) {
        // same() creates groups with one item and only appends, so every group is nonempty.
        let (target, hole, located_frames) = group[0];
        let mut target = target.clone();
        if group.len() > 1 {
            let diameter = hole.number("diameter")?;
            let mut faces = Vec::new();
            let mut members = Vec::new();
            let mut parts = Vec::new();
            for (t, _, _) in &group {
                faces.extend_from_slice(&t.faces);
                if t.members.is_empty() {
                    members.push(t.id);
                } else {
                    members.extend_from_slice(&t.members);
                }
                parts.push((t.id.to_string(), bores(&t.faces, diameter, &ctx.faces)?));
            }
            target = holes::Target {
                faces,
                members,
                groups: group.len(),
                parts,
                ..target
            };
        }
        let planes = if by_position {
            None
        } else {
            Some(datums::planes(&datums))
        };
        out.extend(holes::hole_questions(
            ctx,
            &target,
            hole,
            answers,
            located_frames,
            planes.as_deref(),
        )?);
    }

    let axial = if by_position {
        turned::runout_frame(ctx, &datums, answers)?
    } else {
        None
    };
    let mut diameters = Vec::with_capacity(turned.len());
    for f in &turned {
        diameters.push(py::round(f.number("diameter")?, 6));
    }
    for group in same(&turned, &diameters) {
        let group: Vec<&Feature<'_>> = group.into_iter().copied().collect();
        out.push(turned::turned_question(&group, ctx)?);
        // Not the diameter that is the axis itself: it would run out from itself.
        if let Some((frame, axis_faces)) = &axial {
            let mut on_axis = false;
            for f in &group {
                if f.faces()?.iter().any(|i| axis_faces.contains(i)) {
                    on_axis = true;
                    break;
                }
            }
            if !on_axis {
                out.push(turned::runout_question(&group, ctx, frame)?);
            }
        }
    }

    out.extend(asked_datums);
    if by_position {
        out.extend(datums::datum_control_questions(ctx, &datums)?);
    }
    // A typed answer is refused here, where it is given, not when the file is written.
    for q in &out {
        if q.kind == "choice"
            && let Some(value) = answers.get(&q.id)
        {
            choices::check(&q.id, value, &q.options, q.diameter, q.depth)?;
        }
    }
    // Keep dependent depth edits after the existing questions, preserving their order.
    out.sort_by_key(|q| q.id.starts_with("hole.full_thread:"));
    out.into_iter().map(|q| reviewed(q, &ctx.faces)).collect()
}

/// The material, general tolerance and notes questions.
pub(crate) fn part_questions() -> Vec<Question> {
    let choice =
        |id: &str, prompt: &str, default: &str, options: &[&str], why: &str, basis: &str| {
            Question {
                default: Some(Value::from(default)),
                options: options.iter().map(|o| o.to_string()).collect(),
                why: why.to_string(),
                basis: basis.to_string(),
                ..Question::new(id.into(), "choice", prompt.into(), Vec::new())
            }
        };
    let text = |id: &str, prompt: &str, default: Option<&str>, why: &str, suggestions| Question {
        default: default.map(Value::from),
        why: why.to_string(),
        suggestions,
        ..Question::new(id.into(), "text", prompt.into(), Vec::new())
    };
    vec![
        text(
            "part.material",
            "What material is the part made from?",
            None,
            "Every drawing states the material; it also decides what tolerances are sensible.",
            materials::suggestions(),
        ),
        choice(
            "part.general_tolerance",
            "Which general tolerance applies to dimensions without their own?",
            "ISO 2768-m",
            &standards::GENERAL_TOLERANCES,
            "Covers every dimension without its own tolerance, so most features need no \
             question.",
            "ISO 2768-m (medium) is the usual general tolerance for machined parts.",
        ),
        choice(
            "part.surface_finish",
            "What surface finish applies where none is given?",
            "Ra 3.2",
            &FINISHES,
            "Ra 3.2 is ordinary machining; finer costs more. Faces that need better, such as \
             bores and seals, can be given their own.",
            "Ra 3.2 is what ordinary machining gives without extra passes.",
        ),
        text(
            "part.coating",
            "Is the part coated or treated?",
            Some("None (as machined)"),
            "A coating changes the size of every face it covers, and the price.",
            materials::suggested(materials::COATINGS),
        ),
        text(
            "part.heat_treatment",
            "Is the part heat treated?",
            Some("None"),
            "Hardening is done after machining, and is written on the drawing.",
            materials::suggested(materials::HEAT_TREATMENTS),
        ),
        choice(
            "part.edges",
            "What should be done to sharp edges?",
            EDGES[0],
            &EDGES,
            "Sharp edges cut hands and chip; a standard note says how far to break them.",
            "Breaking edges to 0.2 is the common standard note.",
        ),
        Question {
            default: Some(Value::Array(Vec::new())),
            why: "Notes about making the part are stored with the model. Drawing-only notes \
                  belong in the drawing specification."
                .into(),
            ..Question::new(
                "part.notes".into(),
                "text_list",
                "What other notes apply to making the part, in order?".into(),
                Vec::new(),
            )
        },
    ]
}

/// `_same`: `items` grouped by their `keys` (equal by `==`), groups in order of first
/// appearance.
fn same<'i, T, K: PartialEq>(items: &'i [T], keys: &[K]) -> Vec<Vec<&'i T>> {
    let mut firsts: Vec<&K> = Vec::new();
    let mut groups: Vec<Vec<&T>> = Vec::new();
    for (item, key) in items.iter().zip(keys) {
        match firsts.iter().position(|k| *k == key) {
            // keys and groups are appended together; i comes from the keys position.
            Some(i) => groups[i].push(item),
            None => {
                firsts.push(key);
                groups.push(vec![item]);
            }
        }
    }
    groups
}

/// `_reviewed`: the question with its attention level; the policy for all questions is here.
fn reviewed(mut q: Question, faces: &Faces) -> Result<Question> {
    let Some(default) = q.default.as_ref() else {
        q.attention = "required";
        return Ok(q);
    };
    let text = str_of(default);
    if !q.mate.is_empty() {
        // Taken from the part it fits in its assembly: likely, but the fit or thread is
        // still the person's to confirm.
        q.attention = "check";
        q.reason = q.mate.clone();
        return Ok(q);
    }
    if q.id.starts_with("hole.function:hole_patterns") && py::is_str(Some(default), GENERAL) {
        // A single odd hole is usually just a hole. A group of identical ones is usually for
        // fasteners, and left general it gets no position tolerance.
        q.attention = "check";
        q.reason = "a group of identical holes matching no standard clearance or tap-drill \
                    size; if they take fasteners they need a position tolerance"
            .to_string();
        return Ok(q);
    }
    if q.id.starts_with("hole.function:") && text.starts_with("tapped:") {
        // A thread is a strong claim to make from a diameter alone: the size matches a tap
        // drill, but a plain hole can be that size too.
        let size = text.split_once(':').map_or("", |(_, s)| s);
        q.reason = format!(
            "⌀{} is the tap-drill size for {size}, so it is assumed tapped {size}; if it is not \
             threaded, choose what it is for",
            py::fmt(q.diameter.unwrap_or(0.0))?
        );
        q.attention = "check";
        return Ok(q);
    }
    if q.id == "datum.A"
        && !q
            .faces
            .iter()
            .any(|&i| faces.get(i).is_some_and(|f| f.is_cylinder()))
    {
        let doubt = datums::datum_a_doubt(faces)?;
        if !doubt.is_empty() {
            q.attention = "check";
            q.reason = doubt;
            return Ok(q);
        }
    }
    if q.id.starts_with("diameter.fit:")
        || q.id.starts_with("diameter.runout:")
        || q.id.starts_with("hole.full_thread:")
        || q.id == "datum.A.flatness"
        || q.id.ends_with(".control")
    {
        q.attention = "refine";
    }
    Ok(q)
}

/// Unanswered questions that need answers. Full-thread depth is an optional override of
/// the manufacturing depth already implied by an answered tap, so it never blocks saving.
pub fn open_questions<'q>(qs: &'q [Question], answers: &Map<String, Value>) -> Vec<&'q Question> {
    qs.iter()
        .filter(|q| !q.id.starts_with("hole.full_thread:") && !answers.contains_key(&q.id))
        .collect()
}

/// `answers` plus the default of every unanswered question that has one.
pub fn with_defaults(qs: &[Question], answers: &Map<String, Value>) -> Map<String, Value> {
    let mut merged = answers.clone();
    for q in qs {
        if let Some(default) = &q.default
            && !merged.contains_key(&q.id)
        {
            merged.insert(q.id.clone(), default.clone());
        }
    }
    merged
}

/// `answers` plus every default, to a fixed point: some questions only exist once an earlier
/// default is taken.
pub fn with_all_defaults(
    analysis: &Value,
    answers: &Map<String, Value>,
) -> Result<Map<String, Value>> {
    Ok(settled(&Ctx::new(analysis)?, answers)?.0)
}

/// `with_all_defaults`, with the questions of the answers it settles on (which `preview` and
/// `problems` would otherwise ask for again: the same call, the same result).
fn settled(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
) -> Result<(Map<String, Value>, Vec<Question>)> {
    let mut answers = answers.clone();
    loop {
        let qs = questions_in(ctx, &answers)?;
        let merged = with_defaults(&qs, &answers);
        // Defaults are only ever added, so nothing added is the fixed point.
        if merged.len() == answers.len() {
            return Ok((answers, qs));
        }
        answers = merged;
    }
}

/// Turn a complete set of answers into intent. Refuses while any question is open.
pub fn apply(analysis: &Value, answers: &Map<String, Value>) -> Result<Intent> {
    let ctx = Ctx::new(analysis)?;
    let qs = questions_in(&ctx, answers)?;
    let missing: Vec<String> = open_questions(&qs, answers)
        .iter()
        .map(|q| q.id.clone())
        .collect();
    if !missing.is_empty() {
        return Err(IncompleteError::new(missing, None).into());
    }
    intent(&ctx, answers, &qs, IntentMode::Apply)
}

/// `api.apply`: intent from `answers`; with `accept_defaults`, unanswered defaults are taken
/// (to a fixed point) first.
pub fn apply_with_defaults(
    analysis: &Value,
    answers: &Map<String, Value>,
    accept_defaults: bool,
) -> Result<Intent> {
    if accept_defaults {
        let merged = with_all_defaults(analysis, answers)?;
        apply(analysis, &merged)
    } else {
        apply(analysis, answers)
    }
}

/// The intent the answers so far amount to, for showing while they are given.
///
/// Unanswered defaults remain suggestions on the questions; they contribute no PMI. Existing
/// file datums are retained, and answered tolerances need an answered or existing datum frame.
/// An answered tap includes its established inline/computed manufacturing depth; the optional
/// full-thread question overrides that depth only when answered.
/// Items keep the `default` field for front ends, always false for an answered question.
pub fn preview(analysis: &Value, answers: &Map<String, Value>) -> Result<Intent> {
    let ctx = Ctx::new(analysis)?;
    let qs = questions_in(&ctx, answers)?;
    let mut intent = intent(&ctx, answers, &qs, IntentMode::Preview)?;
    mark_defaults(&mut intent, answers);
    Ok(intent)
}

/// Prospective diagnostics still examine the suggested scheme, without accepting it. This is
/// private to `problems`, never the intent returned to a front end or passed to a writer.
pub(crate) fn proposed_in(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
) -> Result<(Intent, Map<String, Value>, Vec<Question>)> {
    let (merged, qs) = settled(ctx, answers)?;
    let mut intent = intent(ctx, &merged, &qs, IntentMode::Apply)?;
    mark_defaults(&mut intent, answers);
    Ok((intent, merged, qs))
}

fn mark_defaults(intent: &mut Intent, answers: &Map<String, Value>) {
    for item in intent
        .datums
        .iter_mut()
        .chain(intent.requirements.iter_mut())
    {
        if let Some(question) = item.get("question") {
            // A question that is not text cannot be among the answers' keys.
            let default = question.as_str().is_none_or(|q| !answers.contains_key(q));
            item.insert("default".into(), Value::from(default));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IntentMode {
    Preview,
    Apply,
}

/// `_intent`: intent from the answered questions among `qs`; each item names its question.
fn intent(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
    qs: &[Question],
    mode: IntentMode,
) -> Result<Intent> {
    let Some(binding) = ctx.analysis.get("binding") else {
        return invalid("the analysis has no 'binding'");
    };
    let mut intent = Intent::new(binding.clone());
    for key in ["material", "general_tolerance"].into_iter().chain(NOTES) {
        let id = format!("part.{key}");
        if key == "notes" {
            if let Some(value) = answers.get(&id)
                && !note_strings(value)?.is_empty()
            {
                intent.part.insert(key.into(), value.clone());
            }
            continue;
        }
        let given = answers.get(&id).filter(|v| truthy(v));
        let text = given.map(str_of).unwrap_or_default();
        let text = crate::pyfmt::strip(&text).to_lowercase();
        if !NOTHING.contains(&text.as_str()) {
            intent.part.insert(
                key.to_string(),
                answers.get(&id).cloned().unwrap_or_default(),
            );
        }
    }
    // Questions may use suggested datums to propose dependent tolerances. Only existing or
    // answered datums belong in PMI, so do not supply question defaults here.
    let datums = datums::chosen_datums(ctx, answers, &[])?;
    intent.datums = datums
        .iter()
        .map(|(x, d)| {
            let mut item = object([
                ("letter", Value::from(x.to_string())),
                ("faces", Value::from(d.faces.clone())),
            ]);
            if d.existing {
                item.insert("existing".into(), Value::from(true));
            } else {
                item.insert("question".into(), Value::from(format!("datum.{x}")));
            }
            item
        })
        .collect();

    for q in qs {
        let Some(value) = answers.get(&q.id) else {
            continue;
        };
        let before = intent.requirements.len();
        let parts: Vec<(String, Vec<i64>)> = if q.parts.is_empty() {
            vec![(q.feature.clone().unwrap_or_default(), q.faces.clone())]
        } else {
            q.parts.clone()
        };
        let reqs = &mut intent.requirements;
        if q.id.starts_with("hole.function:") {
            for (fid, bores) in &parts {
                let feature = ctx.feature(fid)?;
                let hole = if feature.family == "holes" {
                    feature
                } else {
                    let Some(first) = feature.members.first() else {
                        return invalid(format!("feature {fid} has no 'members'"));
                    };
                    ctx.feature(first)?
                };
                for mut req in holes::hole_requirements(value, bores, feature, hole)? {
                    if req.get("kind").and_then(Value::as_str) == Some("thread")
                        && !py::is_str(hole.get("bottom")?, "through")
                        && let Some(drill) = hole.optional_number("depth")?
                    {
                        let id = format!("hole.full_thread:{}", q.feature.as_deref().unwrap_or(""));
                        if let Some(text) = answers.get(&id).and_then(Value::as_str) {
                            let size = choices::split(value.as_str().unwrap_or("")).1;
                            req.insert(
                                "full_thread".into(),
                                Value::from(choices::full_thread_depth(text, size, drill)?),
                            );
                        }
                    }
                    reqs.push(req);
                }
            }
        } else if q.id.starts_with("hole.location:") {
            // A tolerance of 0 is false to Python, and makes no requirement either.
            if let Some(tolerance) = locations::parse(value)?.filter(|&t| t != 0.0) {
                let planes = datums::planes(&datums);
                for (fid, _) in &parts {
                    let members = members(ctx.feature(fid)?, ctx)?;
                    reqs.extend(locations::dimensions(
                        &members,
                        &planes,
                        &ctx.faces,
                        Some(tolerance),
                    )?);
                }
            }
        } else if q.id.starts_with("hole.position:") && !py::is_str(Some(value), NONE) {
            let feature = q.feature.clone().unwrap_or_default();
            let function = answer(answers, qs, &format!("hole.function:{feature}"))
                .map_or_else(|| "None".to_string(), str_of);
            let frame = q.frame.clone().unwrap_or_default();
            let letters: Vec<&str> = frame.split('|').collect();
            if !letters
                .iter()
                .all(|x| one_letter(x).is_some_and(|x| datums.contains(x)))
            {
                continue;
            }
            // Basic dimensions from the frame's planes say where "true position" is.
            let mut planes = Vec::new();
            for x in &letters {
                if let Some(d) = one_letter(x).and_then(|x| datums.get(x))
                    && d.kind == DatumKind::Plane
                {
                    planes.push(d.faces.clone());
                }
            }
            for (fid, _) in &parts {
                let members = members(ctx.feature(fid)?, ctx)?;
                reqs.extend(locations::dimensions(&members, &planes, &ctx.faces, None)?);
            }
            let tolerance = number(value)?;
            for (fid, bores) in &parts {
                let mut req = object([
                    ("kind", Value::from("position")),
                    ("feature", Value::from(fid.as_str())),
                    ("faces", Value::from(bores.clone())),
                    ("tolerance", Value::from(tolerance)),
                    ("diametral", Value::from(true)),
                    ("datums", Value::from(letters.clone())),
                ]);
                // A clearance hole's position at maximum material: bonus tolerance as the
                // hole grows, and a gauge pin checks it.
                if function.starts_with("clearance") {
                    req.insert("mmc".into(), Value::from(true));
                }
                reqs.push(req);
            }
        } else if q.id.starts_with("diameter.runout:") && !py::is_str(Some(value), NONE) {
            let frame = q.frame.clone().unwrap_or_default();
            let letters: Vec<&str> = frame.split('|').collect();
            if letters
                .iter()
                .all(|x| one_letter(x).is_some_and(|x| datums.contains(x)))
            {
                let tolerance = number(value)?;
                for (fid, cylinders) in &parts {
                    reqs.push(object([
                        ("kind", Value::from("runout")),
                        ("feature", Value::from(fid.as_str())),
                        ("faces", Value::from(cylinders.clone())),
                        ("tolerance", Value::from(tolerance)),
                        ("datums", Value::from(letters.clone())),
                    ]));
                }
            }
        } else if q.id.starts_with("diameter.fit:") && !py::is_str(Some(value), GENERAL) {
            let feature = q.feature.clone().unwrap_or_default();
            let d = ctx.feature(&feature)?.number("diameter")?;
            for (fid, cylinders) in &parts {
                reqs.push(turned::turned_requirement(value, d, fid, cylinders, ctx)?);
            }
        } else if q.id == "datum.A.flatness" && !py::is_str(Some(value), NONE) {
            if !datums.contains('A') {
                continue;
            }
            reqs.push(object([
                ("kind", Value::from("flatness")),
                ("faces", Value::from(q.faces.clone())),
                ("tolerance", Value::from(number(value)?)),
            ]));
        } else if q.id.ends_with(".control") && !py::is_str(Some(value), NONE) {
            let letter = q.id.split('.').nth(1).and_then(one_letter);
            let control = match letter {
                Some(x) => datum_control(x, &datums, &ctx.faces)?,
                None => None,
            };
            let (Some(letter), Some(control)) = (letter, control) else {
                // The question can propose a control before its datum frame is accepted.
                if mode == IntentMode::Preview {
                    continue;
                }
                return invalid("there is no datum control to give");
            };
            reqs.push(object([
                ("kind", Value::from(control.kind)),
                ("feature", Value::from(format!("datum {letter}"))),
                ("faces", Value::from(q.faces.clone())),
                ("tolerance", Value::from(number(value)?)),
                ("diametral", Value::from(control.diametral)),
                ("datums", Value::from(control.datums.clone())),
            ]));
        }
        // before was the length before this iteration appended requirements; no items are removed.
        for req in &mut intent.requirements[before..] {
            req.insert("question".into(), Value::from(q.id.as_str()));
        }
    }
    for (key, extra) in extras::given(answers) {
        if let Some(req) = extras::requirement(key, extra, &ctx.faces, &datums)? {
            intent.requirements.push(req);
        }
    }
    let tolerance = answers
        .get("part.general_tolerance")
        .filter(|v| truthy(v))
        .or_else(|| {
            ctx.stated
                .and_then(|p| p.get("general_tolerance"))
                .filter(|v| truthy(v))
        });
    if let Some(tolerance) = tolerance {
        let Value::String(tolerance) = tolerance else {
            return invalid(format!(
                "part.general_tolerance: {} is not a tolerance class",
                repr(tolerance)
            ));
        };
        intent.general = general::dimensions(ctx, &datums, &intent.requirements, tolerance)?;
    }
    if let Some(input) = ctx.analysis.get("dimensioning") {
        let input = serde_json::from_value(input.clone())
            .map_err(|e| Error::Invalid(format!("dimensioning input: {e}")))?;
        if let Some(stated) = ctx.analysis.get("dimensioning_stated") {
            intent.requirements.extend(
                serde_json::from_value::<Vec<Map<String, Value>>>(stated.clone())
                    .map_err(|e| Error::Invalid(format!("stored dimensions: {e}")))?,
            );
        }
        crate::dimensioning::apply(input, &mut intent).map_err(Error::Invalid)?;
    }
    Ok(intent)
}

/// A one-character datum letter, or None.
fn one_letter(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

/// `float(value)` of a tolerance answer (checked already), refused with Python's message
/// where it is not a number.
fn number(value: &Value) -> Result<f64> {
    float_of(value).map_err(Error::Invalid)
}

/// `_members`: each hole of a hole or pattern feature, with its record and bore faces.
pub(crate) fn members<'a>(
    feature: &Feature<'a>,
    ctx: &Ctx<'a>,
) -> Result<Vec<(String, Feature<'a>, Vec<i64>)>> {
    let holes: Vec<&Feature<'a>> = if feature.members.is_empty() {
        vec![feature]
    } else {
        feature
            .members
            .iter()
            .map(|m| ctx.feature(m))
            .collect::<Result<_>>()?
    };
    holes
        .into_iter()
        .map(|h| Ok((h.id.to_string(), h.clone(), bores_of(h, &ctx.faces)?)))
        .collect()
}

/// Free-text notes are literal strings: no trimming, sentinel words or deduplication.
/// Shared with the writer, since callers can supply intent without going through questions.
pub(crate) fn note_strings(value: &Value) -> Result<Vec<&str>> {
    value
        .as_array()
        .and_then(|items| items.iter().map(Value::as_str).collect())
        .ok_or_else(|| Error::Invalid("part.notes: expected an ordered list of strings".into()))
}

#[cfg(test)]
mod intent_mode_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_preview_may_skip_a_control_without_its_frame() {
        let analysis = json!({"binding": {}, "faces": [], "features": []});
        let ctx = Ctx::new(&analysis).unwrap();
        let given = json!({"datum.B.control": "0.05"})
            .as_object()
            .unwrap()
            .clone();
        // Public apply regenerates questions from the complete accepted frame: normally
        // datum_control_questions excludes this impossible control. Pin the shared intent
        // boundary with a stale question so apply/accept-defaults cannot silently skip it.
        let qs = [Question::new(
            "datum.B.control".into(),
            "choice",
            String::new(),
            vec![],
        )];
        assert!(
            intent(&ctx, &given, &qs, IntentMode::Preview)
                .unwrap()
                .requirements
                .is_empty()
        );
        assert!(matches!(
            intent(&ctx, &given, &qs, IntentMode::Apply),
            Err(Error::Invalid(message)) if message == "there is no datum control to give"
        ));
    }
}
