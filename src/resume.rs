//! Answers stored in a written STEP and resumed from it (port of `resume.py`).
//!
//! Writing a part, specify-core also stores the answers it was given in the file, so that
//! opening the file again resumes them. Replace-mode files analyse the geometry alone;
//! scoped Add-mode files also retain foreign PMI as existing interview answers. A mixed
//! rewrite removes only provenance-marked Specify requirements before adding replacements.
//! Add payloads carry `pmi_scope: "specify-core:written:v1"`; legacy payloads without it
//! retain their previous behavior. The payload is
//! Python's: `{schema, answers, part, face_count, faces}` as compact JSON, base64-encoded (a
//! Part 21 string then needs no escapes), `faces` being a [`fingerprint`] of the part's faces
//! that is checked before unnamed face indices are trusted. Named parts write schema 3 with
//! authored STEP names; wholly unnamed parts keep schema 2 and its bytes. Schemas 1/2 still read.
//!
//! **Where it is stored.** Python appends its own text: `PROPERTY_DEFINITION('pmi-assist
//! answers','schema 1',#pd)` with a representation holding one
//! `DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers','<base64>')`. This port writes the
//! same item through haecceity, as a CAx-IF user defined attribute on the part
//! ([`attribute_set`]: `AttributeSet` 'pmi-assist answers' with that one string item), within
//! the same `pmi::write` edit as the PMI: a standard construct that haecceity's reader consumes
//! as the part's, so a replace removes it with the PMI it describes. Both forms carry the same
//! `DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers','<base64>')`, which is all Python's
//! reader looks for, so each engine resumes the other's files; [`stored`] reads both.
//!
//! In an assembly each part's answers are stored on that part and say which part they are for;
//! a part with none stored has no resumable interview. Older Add-mode parts can carry
//! Specify provenance without saved answers and still open as existing PMI.

pub(crate) mod names;

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use haecceity::p21::{Attribute, Document, RawEntity};
use haecceity::pmi::{AttributeSet, AttributeValue};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::features::{FeatureError, describe_faces};
use crate::part::LoadedPart;
use crate::pyfmt::round_half_even;
use crate::pyjson;

/// Latest stored-answer schema: 3 adds indexed `face_names` (null for unnamed faces),
/// with geometric fingerprints only for mixed parts. Wholly unnamed parts still write 2.
/// Readers unaware of 3 ignore the entire stored-answer item, keeping standard STEP PMI.
pub const SCHEMA: i64 = 3;

/// Accepted schemas: signed-zero legacy 1, unsigned-zero 2, and authored-name 3.
pub const SCHEMAS: [i64; 3] = [1, 2, SCHEMA];

/// The name of the stored item (and of the attribute set holding it): Python's, kept from
/// before specify-core was renamed so that older files still resume.
pub const NAME: &str = "pmi-assist answers";

/// The payload member holding carry blocks on questions not asked when written (#126):
/// `{question ID: diff reason}`, beside `answers` and never inside it, as older builds refuse
/// `_specify` among answers (and ignore unknown payload members). On resume they return as the
/// answers' `_specify.unresolved`, so a block applies again once its question is asked.
pub const UNRESOLVED: &str = "unresolved";

/// Stored answers whose ID no question can have (a STEP written before such IDs were
/// refused, #126) are left open with a reason rather than resumed, so the rules, which refuse
/// them, can still open the part.
fn without_unknown(out: &mut Resumed) {
    let unknown: Vec<(String, Value)> = out
        .answers
        .iter()
        .filter(|(id, _)| !crate::input::known_id(id))
        .map(|(id, answer)| (id.clone(), answer.clone()))
        .collect();
    out.answers.retain(|id, _| crate::input::known_id(id));
    for (id, answer) in unknown {
        out.unresolved.push(UnresolvedAnswer {
            id,
            feature: None,
            answer,
            reason: "unknown answer ID: no question has it, so it was not resumed".into(),
            automatic: false,
        });
    }
}

/// The payload's carry blocks ([`UNRESOLVED`]) as answers' metadata, when well formed.
fn restore_blocks(payload: &Map<String, Value>, answers: &mut Map<String, Value>) {
    if let Some(blocks) = payload.get(UNRESOLVED).and_then(Value::as_object)
        && !blocks.is_empty()
        && blocks.values().all(Value::is_string)
        && !answers.contains_key(crate::answer_file::META)
    {
        answers.insert(
            crate::answer_file::META.into(),
            json!({ UNRESOLVED: blocks }),
        );
    }
}

/// Add-mode payloads scope replacement to provenance-marked requirements. Absence keeps
/// legacy resume semantics, including 0.1.7 Add-mode files with no stored answers.
pub(crate) fn scoped(doc: &Document, part: usize) -> bool {
    stored_with_form(doc).get(&part).is_some_and(|(p, _)| {
        p.get("pmi_scope").and_then(Value::as_str) == Some(crate::provenance::MARKER)
    })
}

/// The faces in order, by kind and centroid rounded to 3 decimals: what answers refer to, as
/// schema 2 hashes them ([`fingerprint_of`]). The fingerprint written.
pub fn fingerprint(loaded: &LoadedPart) -> Result<String, FeatureError> {
    Ok(fingerprint_of(&rounded_faces(loaded)?, SCHEMA))
}

/// Schema 1's fingerprint of the faces (Python's `resume.fingerprint` before schema 2): the
/// one a schema 1 payload is checked against.
pub fn fingerprint_v1(loaded: &LoadedPart) -> Result<String, FeatureError> {
    Ok(fingerprint_of(&rounded_faces(loaded)?, 1))
}

/// The faces as `[kind, [x, y, z]]`, each centroid component rounded to 3 decimals (Python's
/// `round(c, 3)`, half to even), in face order.
fn rounded_faces(loaded: &LoadedPart) -> Result<Value, FeatureError> {
    Ok(Value::Array(
        describe_faces(loaded)?
            .iter()
            .map(|f| {
                let centroid: Vec<Value> = f
                    .centroid
                    .iter()
                    .map(|&c| json!(round_half_even(c, 3)))
                    .collect();
                json!([f.kind, centroid])
            })
            .collect(),
    ))
}

/// The fingerprint, of `schema`, of `faces` given as `[kind, [x, y, z]]` with each component
/// already rounded to 3 decimals: the lowercase hex sha256 of the UTF-8 text Python's
/// `json.dumps(faces)` writes (`pyjson::dumps`: `", "` and `": "` separators, floats by
/// `float.__repr__`). Schema 1 hashes the components as they are, so a zero keeps its sign
/// (`-0.0`), which is integration noise; schema 2 first writes every zero unsigned (`0.0`), so
/// either engine's fingerprint of the same faces is the same (docs/design.md, "The
/// fingerprint").
pub fn fingerprint_of(faces: &Value, schema: i64) -> String {
    let unsigned = |v: &Value| match v.as_f64() {
        // A float pattern compares by `==`, so -0.0 matches too.
        Some(0.0) => json!(0.0),
        _ => v.clone(),
    };
    let faces: Value = if schema >= 2 {
        faces
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                // f is a serde_json::Value: missing or ill-typed reads return Null rather than panicking.
                let centroid: Vec<Value> = f[1]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(unsigned)
                    .collect();
                json!([f[0], centroid])
            })
            .collect()
    } else {
        faces.clone()
    };
    Sha256::digest(pyjson::dumps(&faces).as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The payload Python's `embed` stores for `answers` given for `loaded`, in its key order.
pub fn payload(loaded: &LoadedPart, answers: &Map<String, Value>) -> Result<Value, FeatureError> {
    let evidence = evidence(loaded)?;
    let mut payload = json!({
        "schema": evidence["schema"],
        "answers": answers,
        "part": loaded.binding.part,
        "face_count": loaded.binding.face_count,
        "faces": evidence["faces"],
    });
    for key in ["face_names", "geometry"] {
        if let Some(value) = evidence.get(key) {
            payload[key] = value.clone();
        }
    }
    Ok(payload)
}

/// `payload` as stored: compact JSON (`separators=(",", ":")`), base64.
pub fn encode(payload: &Value) -> String {
    STANDARD.encode(pyjson::dumps_compact(payload))
}

/// The user defined attribute that stores `payload` on its part, written by the writer in the
/// same edit as the part's PMI.
pub fn attribute_set(payload: &Value) -> AttributeSet {
    AttributeSet {
        description: None,
        general_property: None,
        name: NAME.into(),
        on: None,
        items: vec![(NAME.into(), AttributeValue::Text(encode(payload)))],
    }
}

/// Whether `set` is stored answers (this port's form), not PMI.
pub fn is_answers(set: &AttributeSet) -> bool {
    set.name == NAME
        && set.on.is_none()
        && matches!(set.items.as_slice(), [(name, AttributeValue::Text(_))] if name == NAME)
}

/// Every part's stored answers in the STEP text `bytes`, by part (Python's `stored`): each
/// `DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers', ...)`, in file order, whose string
/// is base64 of a JSON object of one of [`SCHEMAS`] with an `answers` object. The first legacy
/// frame for a part is kept; a later provenance-scoped frame supersedes older frames without
/// removing their entities. Answers saved before parts were told apart are part 0's. Anything else (not
/// base64, not JSON, another schema, a part that is not an integer) is passed over, as Python
/// passes it over. A file that does not parse as Part 21 stores nothing.
pub fn stored(bytes: &[u8]) -> BTreeMap<usize, Map<String, Value>> {
    match Document::parse(bytes.to_vec()) {
        Ok(doc) => stored_in(&doc),
        Err(_) => BTreeMap::new(),
    }
}

/// [`stored`] of the file already parsed as `doc`.
pub fn stored_in(doc: &Document) -> BTreeMap<usize, Map<String, Value>> {
    stored_with_form(doc)
        .into_iter()
        .map(|(part, (payload, _))| (part, payload))
        .collect()
}

/// [`stored_in`], with whether each payload is stored as this port writes it (a user defined
/// attribute) rather than in Python's own form: only the former is this port's to resume when
/// its schema 1 fingerprint cannot be reproduced ([`Check::Legacy`]).
fn stored_with_form(doc: &Document) -> BTreeMap<usize, (Map<String, Value>, bool)> {
    let mut out = BTreeMap::new();
    for id in items_in_file_order(doc, NAME) {
        let Some(text) = descriptive_text(doc, id) else {
            continue;
        };
        let Some(payload) = decode(&text) else {
            continue;
        };
        let part = match payload.get("part") {
            None => Some(0),
            // Python's isinstance(part, int): an integer (a bool is one too, as True is 1).
            Some(Value::Number(n)) => n.as_u64().map(|k| k as usize),
            Some(Value::Bool(b)) => Some(usize::from(*b)),
            Some(_) => None,
        };
        if let Some(part) = part {
            if payload.get("pmi_scope").and_then(Value::as_str) == Some(crate::provenance::MARKER) {
                // Add must retain unmarked legacy frames, including stale fingerprints.
                // Its newly saved frame is authoritative; legacy-only files keep first-wins.
                out.insert(part, (payload, is_attribute_item(doc, id)));
            } else {
                out.entry(part)
                    .or_insert_with(|| (payload, is_attribute_item(doc, id)));
            }
        }
    }
    out
}

/// Whether the item `id` is held by a representation of a `PROPERTY_DEFINITION` described as a
/// `'user defined attribute'`: the form haecceity writes ([`attribute_set`]), not Python's
/// `PROPERTY_DEFINITION('pmi-assist answers','schema 1',...)`.
pub(crate) fn is_attribute_item(doc: &Document, id: u64) -> bool {
    let simple = |id: u64| match doc.get(id) {
        Some(RawEntity::Simple {
            name, attributes, ..
        }) => Some((name.as_str(), attributes.as_slice())),
        _ => None,
    };
    doc.referrers(id).iter().any(|&rep| {
        doc.referrers(rep).iter().any(|&pdr| {
            let Some(("PROPERTY_DEFINITION_REPRESENTATION", [Attribute::EntityRef(pd), ..])) =
                simple(pdr)
            else {
                return false;
            };
            matches!(simple(*pd), Some(("PROPERTY_DEFINITION", [_, Attribute::String(d), ..])) if d == "user defined attribute")
        })
    })
}

/// Legacy descriptive items and haecceity's ordered text compounds, in file order.
pub(crate) fn items_in_file_order(doc: &Document, item: &str) -> Vec<u64> {
    let mut found: Vec<(usize, u64)> = doc
        .ids()
        .filter(|&id| match doc.get(id) {
            Some(RawEntity::Simple {
                name, attributes, ..
            }) => {
                (name == "DESCRIPTIVE_REPRESENTATION_ITEM"
                    || (name == "COMPOUND_REPRESENTATION_ITEM"
                        && descriptive_text(doc, id).is_some()))
                    && matches!(attributes.first(), Some(Attribute::String(s)) if s == item)
            }
            _ => false,
        })
        .map(|id| (doc.span(id).map_or(usize::MAX, |s| s.start), id))
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, id)| id).collect()
}

/// haecceity #172's bounded text representation. Only its marked, ordered list
/// means concatenated text; arbitrary compounds and unordered sets do not.
pub(crate) fn descriptive_text(doc: &Document, id: u64) -> Option<String> {
    let RawEntity::Simple {
        name, attributes, ..
    } = doc.get(id)?
    else {
        return None;
    };
    if name == "DESCRIPTIVE_REPRESENTATION_ITEM" {
        return match attributes.get(1)? {
            Attribute::String(s) => Some(s.clone()),
            _ => None,
        };
    }
    if name != "COMPOUND_REPRESENTATION_ITEM" {
        return None;
    }
    let Attribute::Typed { type_name, value } = attributes.get(1)? else {
        return None;
    };
    if type_name != "LIST_REPRESENTATION_ITEM" {
        return None;
    }
    let Attribute::List(items) = value.as_ref() else {
        return None;
    };
    if items.is_empty() {
        return None;
    }
    let mut text = String::new();
    for item in items {
        let Attribute::EntityRef(child) = item else {
            return None;
        };
        let RawEntity::Simple {
            name, attributes, ..
        } = doc.get(*child)?
        else {
            return None;
        };
        if name != "DESCRIPTIVE_REPRESENTATION_ITEM" {
            return None;
        }
        let [Attribute::String(marker), Attribute::String(chunk)] = attributes.as_slice() else {
            return None;
        };
        if marker != "haecceity text chunk/1" {
            return None;
        }
        text.push_str(chunk);
    }
    Some(text)
}

/// A stored string's payload, if it is one: base64 (the characters Python's pattern allows,
/// strictly decoded) of a JSON object of one of [`SCHEMAS`] whose `answers` is an object.
fn decode(text: &str) -> Option<Map<String, Value>> {
    if !text
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        return None;
    }
    let raw = STANDARD.decode(text).ok()?;
    let Value::Object(payload) = serde_json::from_slice::<Value>(&raw).ok()? else {
        return None;
    };
    (schema_of(&payload).is_some() && payload.get("answers").is_some_and(Value::is_object))
        .then_some(payload)
}

/// Which of [`SCHEMAS`] a payload is, compared as Python's `==` compares (so `1.0`, and `true`
/// as 1, are schema 1); `None` for any other.
fn schema_of(payload: &Map<String, Value>) -> Option<i64> {
    let n = match payload.get("schema") {
        Some(Value::Number(n)) => n.as_f64()?,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => return None,
    };
    SCHEMAS.into_iter().find(|&s| s as f64 == n)
}

/// Stored answers for this part. Schemas 1/2 require their existing face-count and
/// fingerprint checks (including [`Check::Legacy`]). Verified supports are rematched to
/// current feature IDs; legacy unverified answers are matched by ID.
/// Schema 3 rematches authored names and unnamed evidence to the current geometry and
/// returns only applicable answers, even across a rebuild. Missing or ambiguous supports
/// are excluded. Use [`report_for_parsed`] or [`crate::api::analyse`] to retain those
/// unresolved values and their diagnostics. [`stored`] exposes the original raw payload.
/// An error describing faces or recognising named supports is returned to the caller.
pub fn answers_for(loaded: &LoadedPart) -> Result<Option<Map<String, Value>>, FeatureError> {
    stored_answers(loaded, Need::Answers(None))
}

/// [`answers_for`], matched against `features`, `loaded`'s features already recognised.
pub(crate) fn answers_with(
    loaded: &LoadedPart,
    features: &[crate::features::Feature],
) -> Result<Option<Map<String, Value>>, FeatureError> {
    stored_answers(loaded, Need::Answers(Some(features)))
}

/// Whether [`answers_for`] returns answers, without rematching schema-1/2 feature answers.
pub(crate) fn has_answers(loaded: &LoadedPart) -> Result<bool, FeatureError> {
    Ok(stored_answers(loaded, Need::Presence)?.is_some())
}

/// What [`stored_answers`] is asked for.
#[derive(Clone, Copy)]
enum Need<'a> {
    /// Only whether there are answers.
    Presence,
    /// The answers, matched to the part's features (recognised here when not given).
    Answers(Option<&'a [crate::features::Feature]>),
}

/// `known`, or `loaded`'s features recognised now.
fn recognised<'a>(
    loaded: &LoadedPart,
    known: Option<&'a [crate::features::Feature]>,
) -> Result<std::borrow::Cow<'a, [crate::features::Feature]>, FeatureError> {
    Ok(match known {
        Some(features) => std::borrow::Cow::Borrowed(features),
        None => std::borrow::Cow::Owned(crate::features::recognise(loaded)?),
    })
}

fn stored_answers(
    loaded: &LoadedPart,
    need: Need,
) -> Result<Option<Map<String, Value>>, FeatureError> {
    let Ok(doc) = Document::parse(loaded.bytes.to_vec()) else {
        return Ok(None);
    };
    let Some((payload, form)) = stored_with_form(&doc).remove(&loaded.binding.part) else {
        return Ok(None);
    };
    if schema_of(&payload) == Some(3) {
        let known = match need {
            Need::Answers(known) => known,
            Need::Presence => None,
        };
        let features = recognised(loaded, known)?;
        return Ok(report_for_parsed(loaded, Some(&doc), &features)?.map(|r| r.answers));
    }
    let mut answers = match check(loaded, &payload, form)? {
        Check::Mismatch => None,
        state @ (Check::Verified | Check::Legacy) => {
            if let Some(answers) = payload.get("answers").and_then(Value::as_object) {
                // Part/datum-only payloads need no feature recognition. Keep
                // geometry work off the common settings-only reopen/write path.
                let Need::Answers(known) = need else {
                    return Ok(Some(answers.clone()));
                };
                let features = if answers.keys().any(|id| answer_feature(id).is_some()) {
                    recognised(loaded, known)?
                } else {
                    std::borrow::Cow::Owned(Vec::new())
                };
                Some(if state == Check::Verified {
                    rematch(answers, &features).answers
                } else {
                    by_id(answers, &features, loaded, NO_EVIDENCE).answers
                })
            } else {
                None
            }
        }
    };
    if let Some(answers) = &mut answers {
        answers.retain(|id, _| crate::input::known_id(id));
        restore_blocks(&payload, answers);
        answers.extend(crate::bom::answered_in(&doc, &loaded.definition));
    }
    Ok(answers)
}

/// What a payload's face evidence (`face_count`, and `faces` of its `schema`) says of a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Check {
    /// Same face count and fingerprint: the saved face indices denote this part's faces.
    Verified,
    /// A schema 1 payload stored as this port stores it, with the same face count, whose
    /// fingerprint is not reproduced. Schema 1 hashes the sign of each zero centroid component,
    /// which is integration noise that a kernel upgrade changes (quiddity v0.1.1 gives the
    /// mounting plate's face 1 `[0.0, -30.0, 0.0]` where alpha.6 gave `[-0.0, -30.0, 0.0]`),
    /// and the stored hash cannot be searched for every sign. Its answers are read as answers
    /// without face evidence ([`rematch_answers`] without evidence): no face correspondence is
    /// inferred, answers are matched by feature ID, and `Resumed::unverified` says so.
    Legacy,
    /// Anything else: no correspondence.
    Mismatch,
}

fn check(
    loaded: &LoadedPart,
    payload: &Map<String, Value>,
    attribute_form: bool,
) -> Result<Check, FeatureError> {
    if payload.get("face_count") != Some(&json!(loaded.binding.face_count)) {
        return Ok(Check::Mismatch);
    }
    let schema = schema_of(payload).unwrap_or(SCHEMA);
    let faces = fingerprint_of(&rounded_faces(loaded)?, schema);
    Ok(if payload.get("faces") == Some(&Value::String(faces)) {
        Check::Verified
    } else if schema == 1 && attribute_form {
        Check::Legacy
    } else {
        Check::Mismatch
    })
}

/// One stored answer that needs a human decision. Keep the original value for recovery.
#[derive(Clone, Debug, serde::Serialize)]
pub struct UnresolvedAnswer {
    pub id: String,
    pub feature: Option<String>,
    pub answer: Value,
    pub reason: String,
    /// The default path gave it ([`rematch_answers`]' `automatic`): a caller need not ask the
    /// person about it, as the default path gives it again on the feature's current ID.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub automatic: bool,
}

/// The result shown in the resume/session report. Only `answers` are applied; unresolved
/// values remain available for review and their current questions remain unanswered.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Resumed {
    pub answers: Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<UnresolvedAnswer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub migrated: Vec<(String, String)>,
    /// Why no face correspondence was inferred, when the answers were matched without verified
    /// face evidence: without evidence (or for a [`Check::Legacy`] payload) answers were matched
    /// by feature ID; with evidence that does not match, every answer on a feature or a datum is
    /// in `unresolved`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unverified: Option<String>,
    /// The automatic answers' IDs given to [`rematch_answers`], as they now are: migrated ones
    /// renamed, unresolved ones left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic: Option<Vec<String>>,
}

// Why answers have no verified face evidence. Without evidence (LEGACY, NO_EVIDENCE) answers are
// trusted by ID ([`by_id`]). With contradicting evidence (MISMATCH) `open` adds UNVERIFIED_ALL to
// the whole and UNVERIFIED_EACH to each answer it leaves open.
const LEGACY: &str = "the stored schema 1 face fingerprint keeps the signs of zero, which this \
     kernel computes differently, so it cannot be checked; answers were matched by feature ID";
const NO_EVIDENCE: &str = "no saved face fingerprint was given; answers were matched by feature ID";
const MISMATCH: &str = "the saved face fingerprint does not match this part's faces";
const UNVERIFIED_ALL: &str = "answers on features and datums are left open";
const UNVERIFIED_EACH: &str = "the faces it was given for cannot be matched to this part's, even \
     where the feature ID or face is unchanged";

/// Read saved answers with correspondence and diagnostics, using the already recognised
/// features. Legacy schemas contain no quiddity fingerprint: their ordered whole-part
/// geometry fingerprint is the evidence tying each old face index to this geometry.
pub fn report_for_parsed(
    loaded: &LoadedPart,
    doc: Option<&Document>,
    features: &[crate::features::Feature],
) -> Result<Option<Resumed>, FeatureError> {
    let mut report = ordinary_report_for_parsed(loaded, doc, features)?;
    if let Some(report) = &mut report
        && let Some(doc) = doc
    {
        report
            .answers
            .extend(crate::bom::answered_in(doc, &loaded.definition));
    }
    Ok(report)
}

fn ordinary_report_for_parsed(
    loaded: &LoadedPart,
    doc: Option<&Document>,
    features: &[crate::features::Feature],
) -> Result<Option<Resumed>, FeatureError> {
    let Some((payload, form)) = doc
        .map(stored_with_form)
        .unwrap_or_default()
        .remove(&loaded.binding.part)
    else {
        return Ok(None);
    };
    let Some(answers) = payload.get("answers").and_then(Value::as_object) else {
        return Ok(None);
    };
    let mut out = if schema_of(&payload) == Some(3) {
        Some(
            rematch_answers(
                loaded,
                features,
                answers,
                Some(&Value::Object(payload.clone())),
                None,
            )
            .unwrap_or_else(|error| open(answers, &error.to_string(), true)),
        )
    } else {
        match check(loaded, &payload, form)? {
            Check::Verified => Some(rematch(answers, features)),
            Check::Legacy => Some(by_id(answers, features, loaded, LEGACY)),
            Check::Mismatch => None,
        }
    };
    if let Some(out) = &mut out {
        without_unknown(out);
        restore_blocks(&payload, &mut out.answers);
    }
    Ok(out)
}

/// Evidence kept beside external answers. Unnamed parts keep `{schema:2, face_count, faces}`.
/// Unique authored `f:` identities use schema 3 and add indexed `face_names`; mixed
/// parts also carry `geometry` for unnamed, shared or generic-labelled faces. The writer
/// embeds these same fields. Names are opaque STEP strings, never inferred from geometry.
pub fn evidence(loaded: &LoadedPart) -> Result<Value, FeatureError> {
    let names = names::identities(&loaded.face_names);
    let named = names.iter().any(Option::is_some);
    let mut evidence = json!({
        "schema": if named { SCHEMA } else { 2 },
        "face_count": loaded.binding.face_count,
        "faces": fingerprint(loaded)?,
    });
    if named {
        evidence["face_names"] = json!(names);
        // Only mixed parts need geometric evidence across revisions. All-named parts
        // use names alone; unnamed parts keep their existing schema and bytes.
        if names.iter().any(Option::is_none) {
            evidence["geometry"] =
                json!(quiddity::correspondence::recognise(&loaded.part).fingerprints);
        }
    }
    Ok(evidence)
}

/// Why [`rematch_answers`] refused.
#[derive(Debug)]
pub enum RematchError {
    /// The evidence is not `{schema, face_count, faces}` of a schema read.
    Evidence(String),
    Faces(FeatureError),
}
impl std::fmt::Display for RematchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Evidence(why) => write!(f, "Invalid evidence: {why}"),
            Self::Faces(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for RematchError {}

/// Validate the evidence shared by STEP payloads and external answer maps.
pub fn validate_evidence(evidence: &Value) -> Result<&Map<String, Value>, RematchError> {
    let payload = evidence
        .as_object()
        .ok_or_else(|| RematchError::Evidence("not an object".into()))?;
    if schema_of(payload).is_none() {
        return Err(RematchError::Evidence(format!(
            "schema must be one of {SCHEMAS:?}"
        )));
    }
    if !payload.get("face_count").is_some_and(Value::is_u64)
        || !payload.get("faces").is_some_and(Value::is_string)
    {
        return Err(RematchError::Evidence(
            "face_count must be a count and faces a fingerprint".into(),
        ));
    }
    Ok(payload)
}

/// Whether saved face indices still denote this part's ordered faces. Unlike rematching,
/// a write never migrates indices by name, nor trusts a legacy mismatching fingerprint.
pub fn evidence_matches(loaded: &LoadedPart, evidence: &Value) -> Result<bool, RematchError> {
    let payload = validate_evidence(evidence)?;
    Ok(
        check(loaded, payload, false).map_err(RematchError::Faces)? == Check::Verified
            && (schema_of(payload) != Some(3)
                || payload.get("face_names")
                    == Some(&json!(names::identities(&loaded.face_names)))),
    )
}

/// Answers held outside the STEP (a saved project's), matched to `features`, this part's as
/// now recognised, as answers stored in the file are on opening ([`report_for_parsed`]).
///
/// Schema 3 resolves stored names exactly first, refusing missing/duplicate names without
/// geometric fallback. Stored nulls use verified indices or saved geometric correspondence.
/// Datum lists and extras (including distance references) follow the same per-face mapping.
///
/// With schema 1/2 `evidence` ([`evidence`] of the part when the answers were saved, or a stored
/// payload) whose face count and fingerprint match this part, feature answers move to a
/// unique same-family feature containing their saved faces ([`rematch`]). Without evidence
/// (a project saved before it kept evidence), the answers are trusted by ID: a feature answer
/// is kept when this part has a feature with that ID, a datum when its faces exist; the rest
/// are `unresolved` with their values, never dropped or guessed, and `unverified` says the
/// match was by ID. With evidence that does not match (another part's, or the part before an
/// edit), no face correspondence is inferred and an ID that still exists is no evidence of the
/// same feature, so every answer on a feature or a datum (`datum.X` and its settings) is
/// `unresolved` with its value and a reason, never applied, and `unverified` says why. Answers
/// on the part as a whole are kept as they are.
///
/// `automatic` are the IDs the default path answered (the editor's `auto`): unresolved ones
/// are marked `automatic`, and `Resumed::automatic` gives the list with migrated IDs renamed.
pub fn rematch_answers(
    loaded: &LoadedPart,
    features: &[crate::features::Feature],
    answers: &Map<String, Value>,
    evidence: Option<&Value>,
    automatic: Option<&[String]>,
) -> Result<Resumed, RematchError> {
    crate::answer_file::validate(answers).map_err(RematchError::Evidence)?;
    let metadata = answers.get(crate::answer_file::META);
    let evidence = evidence.or_else(|| metadata.and_then(|m| m.get("evidence")));
    let mut ordinary;
    let given = answers;
    let answers = if metadata.is_some() {
        ordinary = answers.clone();
        ordinary.remove(crate::answer_file::META);
        &ordinary
    } else {
        answers
    };
    let mut out = match evidence {
        None => by_id(answers, features, loaded, NO_EVIDENCE),
        Some(evidence) => {
            let payload = validate_evidence(evidence)?;
            if schema_of(payload) == Some(3) {
                rematch_named(loaded, features, answers, payload)?
            } else {
                match check(loaded, payload, false).map_err(RematchError::Faces)? {
                    Check::Verified => rematch(answers, features),
                    _ => open(answers, MISMATCH, false),
                }
            }
        }
    };
    if let Some(automatic) = automatic {
        for unresolved in &mut out.unresolved {
            unresolved.automatic = automatic.contains(&unresolved.id);
        }
        out.automatic = Some(
            automatic
                .iter()
                .filter_map(|id| {
                    let id = out
                        .migrated
                        .iter()
                        .find(|(old, _)| old == id)
                        .map_or(id, |(_, new)| new);
                    out.answers.contains_key(id).then(|| id.clone())
                })
                .collect(),
        );
    }
    if metadata.is_some() {
        crate::answer_file::attach(
            loaded,
            &mut out.answers,
            given,
            out.unresolved
                .iter()
                .map(|u| (u.id.clone(), u.reason.clone())),
        )
        .map_err(RematchError::Evidence)?;
    }
    Ok(out)
}

/// Schema 3: exact names override every geometric proposal, including a verified
/// same-index fingerprint. Only stored `None` entries may use geometry.
fn rematch_named(
    loaded: &LoadedPart,
    features: &[crate::features::Feature],
    answers: &Map<String, Value>,
    payload: &Map<String, Value>,
) -> Result<Resumed, RematchError> {
    let invalid = |why: &str| RematchError::Evidence(why.into());
    let old_names: Vec<Option<String>> = serde_json::from_value(
        payload
            .get("face_names")
            .cloned()
            .ok_or_else(|| invalid("schema 3 requires face_names"))?,
    )
    .map_err(|_| invalid("face_names must be an array of names or null"))?;
    if Some(old_names.len() as u64) != payload.get("face_count").and_then(Value::as_u64)
        || old_names.iter().flatten().any(String::is_empty)
    {
        return Err(invalid(
            "face_names must have one nonempty name or null per saved face",
        ));
    }
    let mut faces: BTreeMap<usize, Result<usize, String>> =
        names::match_names(&old_names, &loaded.face_names)
            .into_iter()
            .map(|(i, binding)| (i, binding.result()))
            .collect();
    if old_names.iter().all(Option::is_some) {
        return Ok(rematch_by(answers, features, Matching::Mapped(&faces)));
    }
    if check(loaded, payload, false).map_err(RematchError::Faces)? == Check::Verified {
        faces.extend(
            old_names
                .iter()
                .enumerate()
                .filter(|(_, name)| name.is_none())
                .map(|(i, _)| (i, Ok(i))),
        );
    } else if old_names.iter().any(Option::is_none)
        && let Some(geometry) = payload.get("geometry")
    {
        let old: quiddity::correspondence::Fingerprints = serde_json::from_value(geometry.clone())
            .map_err(|_| invalid("invalid saved geometry"))?;
        if old.faces.len() != old_names.len()
            || old.faces.iter().enumerate().any(|(i, f)| f.index != i)
        {
            return Err(invalid(
                "saved geometry must have one face in index order per saved face",
            ));
        }
        let new = quiddity::correspondence::recognise(&loaded.part).fingerprints;
        // A future incompatible fingerprint format must leave unnamed faces open. Names
        // remain usable independently of the geometry format and kernel version.
        if let Ok(correspondence) = quiddity::correspondence::correspond(&old, &new) {
            for face in correspondence.faces {
                use quiddity::correspondence::Class;
                let binding = match face.class {
                    Class::Carried | Class::Adapted => face
                        .new
                        .ok_or_else(|| format!("face {} has no geometric match", face.old)),
                    Class::Ambiguous => {
                        Err(format!("ambiguous geometric support for face {}", face.old))
                    }
                    _ => Err(format!("face {} has no geometric match", face.old)),
                };
                if old_names.get(face.old).is_some_and(Option::is_none) {
                    faces.insert(face.old, binding);
                }
            }
        }
    }
    Ok(rematch_by(answers, features, Matching::Mapped(&faces)))
}

/// Answers without face evidence, `why`, trusted by ID: a feature answer is kept on the
/// feature with the same ID or a unique same-family strict ID-face superset,
/// a datum when its faces exist ([`Matching::Exact`]).
fn by_id(
    answers: &Map<String, Value>,
    features: &[crate::features::Feature],
    loaded: &LoadedPart,
    why: &str,
) -> Resumed {
    let mut out = rematch_by(
        answers,
        features,
        Matching::Exact {
            face_count: loaded.binding.face_count,
        },
    );
    out.unverified = Some(why.into());
    out
}

/// Answers whose evidence contradicts this part, `why`: those on features or datums
/// ([`is_geometric`]) are left open with their values, the rest kept.
fn open(answers: &Map<String, Value>, why: &str, extras: bool) -> Resumed {
    let mut out = Resumed {
        unverified: Some(format!("{why}; {UNVERIFIED_ALL}")),
        ..Resumed::default()
    };
    for (id, answer) in answers {
        if is_geometric(id) || (extras && id.starts_with("extra.")) {
            out.unresolved.push(UnresolvedAnswer {
                id: id.clone(),
                feature: answer_feature(id).map(|(_, f)| f.into()),
                answer: answer.clone(),
                reason: format!("{why}; {UNVERIFIED_EACH}"),
                automatic: false,
            });
        } else {
            out.answers.insert(id.clone(), answer.clone());
        }
    }
    out
}

/// Whether the answer `id` is given for faces of the part: a feature question's, or a datum's
/// faces and the settings that follow its datum (`datum.A.flatness`, `datum.B.control`).
fn is_geometric(id: &str) -> bool {
    answer_feature(id).is_some() || id.starts_with("datum.")
}

fn answer_feature(id: &str) -> Option<(&str, &str)> {
    let (question, feature) = id.split_once(':')?;
    // Only feature-question keys, never colon-containing literal text or part notes.
    matches!(
        question,
        "hole.function"
            | "hole.full_thread"
            | "hole.frame"
            | "hole.position"
            | "hole.location"
            | "diameter.fit"
            | "diameter.runout"
    )
    .then_some((question, feature))
}

/// How saved feature IDs are matched to this part's features.
#[derive(Clone, Copy)]
enum Matching<'a> {
    /// By geometric face support: the saved face indices are proven to be this part's.
    Support,
    Mapped(&'a BTreeMap<usize, Result<usize, String>>),
    /// Prefer the same ID, then a unique same-family strict ID-face superset.
    /// Face indices are not proven, so a datum's faces must at least exist on the part.
    Exact {
        face_count: usize,
    },
}

/// Match geometric supports, never feature list positions or dimensions. Defining faces
/// can grow (e.g. a bore gaining its floor): allow a unique same-family superset, but never
/// partial overlap, a family change or a choice between candidates. Collisions, even with
/// equal values, require review rather than silently merging independently saved answers.
pub fn rematch(answers: &Map<String, Value>, features: &[crate::features::Feature]) -> Resumed {
    rematch_by(answers, features, Matching::Support)
}

fn rematch_by(
    answers: &Map<String, Value>,
    features: &[crate::features::Feature],
    matching: Matching<'_>,
) -> Resumed {
    let mut out = Resumed::default();
    let mut proposals: BTreeMap<String, Vec<(&str, Value)>> = BTreeMap::new();
    let mut sources: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (id, answer) in answers {
        let refuse = |reason: String| UnresolvedAnswer {
            id: id.clone(),
            feature: answer_feature(id).map(|(_, f)| f.into()),
            answer: answer.clone(),
            reason,
            automatic: false,
        };
        let mapped = |i: usize| match matching {
            Matching::Mapped(faces) => faces
                .get(&i)
                .cloned()
                .unwrap_or_else(|| Err(format!("face {i} has no verified geometric match"))),
            _ => Ok(i),
        };
        let value = match if matches!(matching, Matching::Mapped(_)) {
            names::map_value(id, answer, mapped)
        } else {
            Ok(answer.clone())
        } {
            Ok(value) => value,
            Err(reason) => {
                out.unresolved.push(refuse(reason));
                continue;
            }
        };
        let Some((question, old)) = answer_feature(id) else {
            if let Matching::Exact { face_count } = matching
                && id.starts_with("datum.")
                && let Some(faces) = answer.as_array()
                && faces
                    .iter()
                    .any(|face| face.as_u64().is_none_or(|i| i as usize >= face_count))
            {
                out.unresolved.push(UnresolvedAnswer {
                    id: id.clone(),
                    feature: None,
                    answer: answer.clone(),
                    reason: format!("names a face this part does not have ({face_count} faces)"),
                    automatic: false,
                });
                continue;
            }
            proposals.entry(id.clone()).or_default().push((id, value));
            continue;
        };
        let Some((family, support)) = old.split_once(':') else {
            out.unresolved.push(UnresolvedAnswer {
                id: id.clone(),
                feature: Some(old.into()),
                answer: answer.clone(),
                reason: "malformed saved feature ID; expected family:faces".into(),
                automatic: false,
            });
            continue;
        };
        let support: Option<Vec<usize>> = support.split('.').map(|s| s.parse().ok()).collect();
        let support = match support
            .map(|s| s.into_iter().map(mapped).collect::<Result<Vec<_>, _>>())
            .transpose()
        {
            Ok(support) => support,
            Err(reason) => {
                out.unresolved.push(refuse(reason));
                continue;
            }
        };
        let candidates: Vec<_> = support
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|support| {
                features
                    .iter()
                    .filter(|f| {
                        let id_family = f.id.split_once(':').map(|(family, _)| family);
                        id_family == Some(family)
                            && family.split('.').next() == Some(f.family.as_str())
                            && support.iter().all(|i| f.faces.contains(i))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let exact: Vec<_> = candidates.iter().copied().filter(|f| f.id == old).collect();
        let candidates = match matching {
            Matching::Exact { .. } if exact.is_empty() => candidates
                .into_iter()
                .filter(|f| {
                    let Some((_, faces)) = f.id.split_once(':') else {
                        return false;
                    };
                    let Some(faces) = faces
                        .split('.')
                        .map(|i| i.parse::<usize>().ok())
                        .collect::<Option<BTreeSet<_>>>()
                    else {
                        return false;
                    };
                    let saved: BTreeSet<_> = support.as_ref().unwrap().iter().copied().collect();
                    saved.len() < faces.len() && saved.is_subset(&faces)
                })
                .collect(),
            Matching::Exact { .. } => exact,
            Matching::Support if exact.is_empty() => candidates,
            Matching::Support => exact,
            Matching::Mapped(_) => candidates,
        };
        if let [feature] = candidates.as_slice() {
            sources.entry(&feature.id).or_default().insert(old);
            proposals
                .entry(format!("{question}:{}", feature.id))
                .or_default()
                .push((id, value));
        } else {
            out.unresolved.push(UnresolvedAnswer {
                id: id.clone(),
                feature: Some(old.into()),
                answer: answer.clone(),
                reason: if candidates.is_empty() {
                    "no same-family feature contains the saved geometric faces".into()
                } else {
                    format!(
                        "ambiguous geometric support: {} same-family features match",
                        candidates.len()
                    )
                },
                automatic: false,
            });
        }
    }
    let mut proposals: Vec<_> = proposals.into_iter().collect();
    // Payload encoding is byte-sensitive: preserve the original answer insertion order.
    proposals.sort_by_key(|(_, old)| answers.keys().position(|id| id == old[0].0));
    for (new, old) in proposals {
        let merged_features = answer_feature(&new)
            .and_then(|(_, feature)| sources.get(feature))
            .is_some_and(|sources| sources.len() > 1);
        if old.len() == 1 && !merged_features {
            let (id, answer) = &old[0];
            let id = *id;
            out.answers.insert(new.clone(), answer.clone());
            if id != new {
                out.migrated.push((id.into(), new));
            }
        } else {
            for (id, answer) in old {
                out.unresolved.push(UnresolvedAnswer {
                    id: id.into(),
                    feature: answer_feature(id).map(|(_, f)| f.into()),
                    answer: answer.clone(),
                    automatic: false,
                    reason: if merged_features {
                        format!(
                            "multiple saved features map to {}; review required",
                            answer_feature(&new).map_or(new.as_str(), |(_, feature)| feature)
                        )
                    } else {
                        format!("multiple saved answers map to {new}; review required")
                    },
                });
            }
        }
    }
    // A datum's settings (`datum.B.control`) follow it: they stay open with an open datum.
    let open: Vec<String> = out
        .unresolved
        .iter()
        .filter(|u| u.id.starts_with("datum.") && u.id.matches('.').count() == 1)
        .map(|u| format!("{}.", u.id))
        .collect();
    let settings: Vec<String> = out
        .answers
        .keys()
        .filter(|id| open.iter().any(|datum| id.starts_with(datum.as_str())))
        .cloned()
        .collect();
    for id in settings {
        let answer = out.answers.remove(&id).expect("listed from answers");
        let datum = &id[..id.rfind('.').expect("a datum setting")];
        out.unresolved.push(UnresolvedAnswer {
            reason: format!("its datum {datum} is unresolved"),
            feature: None,
            id,
            answer,
            automatic: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_chunks_require_the_marked_ordered_list() {
        let doc = Document::parse(
            br#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('text chunks'),'2;1');
FILE_NAME('chunks.step','',(''),(''),'','','');
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));ENDSEC;
DATA;
#1=DESCRIPTIVE_REPRESENTATION_ITEM('haecceity text chunk/1','second');
#2=DESCRIPTIVE_REPRESENTATION_ITEM('haecceity text chunk/1','first');
#3=DESCRIPTIVE_REPRESENTATION_ITEM('unrelated','not text');
#4=COMPOUND_REPRESENTATION_ITEM('payload',LIST_REPRESENTATION_ITEM((#2,#1)));
#5=COMPOUND_REPRESENTATION_ITEM('payload',SET_REPRESENTATION_ITEM((#2,#1)));
#6=COMPOUND_REPRESENTATION_ITEM('payload',LIST_REPRESENTATION_ITEM((#2,#3)));
#7=COMPOUND_REPRESENTATION_ITEM('payload',LIST_REPRESENTATION_ITEM((#7)));
ENDSEC;END-ISO-10303-21;"#
                .to_vec(),
        )
        .unwrap();
        assert_eq!(descriptive_text(&doc, 4).as_deref(), Some("firstsecond"));
        for id in [5, 6, 7] {
            assert_eq!(descriptive_text(&doc, id), None);
        }
        assert_eq!(items_in_file_order(&doc, "payload"), [4]);
    }

    #[test]
    fn a_payload_is_compact_json_in_base64() {
        let p = json!({"schema": 1, "answers": {"part.material": "steel"}, "part": 0});
        let text = encode(&p);
        assert_eq!(
            String::from_utf8(STANDARD.decode(&text).unwrap()).unwrap(),
            r#"{"schema":1,"answers":{"part.material":"steel"},"part":0}"#
        );
        assert!(decode(&text).is_some());
    }

    #[test]
    fn schema_two_writes_zeros_unsigned_and_schema_one_keeps_their_signs() {
        let signed = json!([
            ["plane", [-50.0, -0.0, 0.0]],
            ["cylinder", [1e16, 0.5, -1.25]]
        ]);
        let unsigned = json!([
            ["plane", [-50.0, 0.0, 0.0]],
            ["cylinder", [1e16, 0.5, -1.25]]
        ]);
        let sha = |t: &str| -> String {
            Sha256::digest(t.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        };
        assert_eq!(
            fingerprint_of(&signed, 2),
            sha(r#"[["plane", [-50.0, 0.0, 0.0]], ["cylinder", [1e+16, 0.5, -1.25]]]"#)
        );
        assert_eq!(fingerprint_of(&signed, 2), fingerprint_of(&unsigned, 2));
        assert_eq!(
            fingerprint_of(&signed, 1),
            sha(r#"[["plane", [-50.0, -0.0, 0.0]], ["cylinder", [1e+16, 0.5, -1.25]]]"#)
        );
        assert_ne!(fingerprint_of(&signed, 1), fingerprint_of(&unsigned, 1));
    }

    #[test]
    fn what_is_not_a_payload_is_passed_over() {
        assert!(decode("not base64!").is_none());
        assert!(decode(&STANDARD.encode(r#"{"schema":4,"answers":{}}"#)).is_none());
        assert!(decode(&STANDARD.encode(r#"{"schema":2,"answers":{}}"#)).is_some());
        assert!(decode(&STANDARD.encode(r#"{"schema":2.0,"answers":{}}"#)).is_some());
        assert!(decode(&STANDARD.encode(r#"{"schema":"2","answers":{}}"#)).is_none());
        assert!(decode(&STANDARD.encode(r#"{"schema":1,"answers":[]}"#)).is_none());
        assert!(decode(&STANDARD.encode(r#"[1]"#)).is_none());
        assert!(decode(&STANDARD.encode(r#"{"schema":1.0,"answers":{}}"#)).is_some());
    }
}
