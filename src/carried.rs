//! An application's JSON payload carried in the STEP by the writer, and the original part.
//!
//! An application that keeps its own state with a part (the draftwright editor's project) has
//! the writer store it as a named JSON payload, and reads it back from the file, with the bytes
//! of the part as it was before the writer added to it. It then needs no STEP parsing of its own.
//!
//! **The payload** is a CAx-IF user defined attribute on the part, written in the same edit as
//! the PMI ([`writer::write_bytes_with`](crate::writer::write_bytes_with)): an `AttributeSet`
//! named `name` with one text item of that name, whose text is base64 of the value's compact
//! JSON (UTF-8). Short values retain
//! `DESCRIPTIVE_REPRESENTATION_ITEM('<name>','<base64>')`,
//! which is what the editor's `readProject` (draftwright-editor `src/model/step-project.ts`) and
//! draftwright-mcp's `project::read` match for `draftwright editor project`, so files written
//! either way are read either way. [`payload`] reads the last item of that name held by a user
//! defined attribute, as the writer stores it; an item of that name anywhere else in the file is
//! not the payload.
//! Long values use haecceity 0.4.3's marked, ordered text compounds; raw STEP
//! consumers must concatenate their chunks in list order. Both forms read here.
//!
//! **The original** is recorded beside it, in a second attribute named [`ORIGINAL`]:
//! `{schema: 1, sha256, header, from}`, the original's sha256, its `FILE_SCHEMA` statement
//! (which the writer changes to AP242) and the first instance number the writer could add
//! (one more than the original's largest). [`original`] takes out every instance from `from`
//! on, puts the header back, and returns the bytes only when they hash to `sha256`. Writing a
//! file that already records its original carries that record forward, so the original stays
//! the part first given, not the last written file. A part that came with PMI the writer
//! replaced has no original to give back (its bytes no longer hash to the record), and
//! [`original`] says so with `None`.
//!
//! **Size.** The core sets no limit on a payload's size: callers that take payloads from
//! elsewhere set their own (the WASM build refuses one over 8 MiB).
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use haecceity::p21::Document;
use haecceity::pmi::{self, AttributeSet, AttributeValue};
use haecceity::step::read_part_definitions;
use regex::bytes::{NoExpand, Regex};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::resume;

/// The attribute recording the original part.
pub const ORIGINAL: &str = "specify-core original";

/// Why a payload cannot be carried.
#[derive(Debug, Clone, PartialEq)]
pub struct PayloadError(pub String);
impl std::fmt::Display for PayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PayloadError {}

/// The attribute sets that carry `value` as the payload `name` in a file written from
/// `source` (the bytes being written), with the record of its original: pass them to
/// `writer::write_bytes_with` as its extras. `name` must not be empty or a name Specify uses
/// ([`resume::NAME`], [`ORIGINAL`]). Any size is carried: limits are the caller's.
pub fn attributes(
    source: &[u8],
    name: &str,
    value: &Value,
) -> Result<Vec<AttributeSet>, PayloadError> {
    let json = serde_json::to_vec(value).map_err(|e| PayloadError(e.to_string()))?;
    attributes_encoded(source, name, &STANDARD.encode(json))
}

/// [`attributes`] with the payload already encoded as base64 (the editor's `encodeProject`).
pub fn attributes_encoded(
    source: &[u8],
    name: &str,
    base64: &str,
) -> Result<Vec<AttributeSet>, PayloadError> {
    if name.is_empty() || name == resume::NAME || name == ORIGINAL {
        return Err(PayloadError(format!(
            "a payload must be named, and not '{}' or '{ORIGINAL}'",
            resume::NAME
        )));
    }
    if base64.is_empty() || !is_base64(base64) || STANDARD.decode(base64).is_err() {
        return Err(PayloadError(format!("the payload '{name}' is not base64")));
    }
    Ok(vec![
        set(name, base64),
        set(ORIGINAL, &encode(&record(source))),
    ])
}

fn set(name: &str, text: &str) -> AttributeSet {
    AttributeSet {
        name: name.into(),
        on: None,
        items: vec![(name.into(), AttributeValue::Text(text.into()))],
        description: None,
        general_property: None,
    }
}

fn is_base64(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
}

fn encode(value: &Value) -> String {
    // A Value built here (strings and integers) always serialises.
    STANDARD.encode(value.to_string())
}

/// The record of `source`'s original: the one `source` carries when it still gives back its
/// original, else `source` itself.
fn record(source: &[u8]) -> Value {
    let doc = Document::parse(source.to_vec()).ok();
    if let Some(doc) = &doc
        && let Some(record) = last_text(doc, ORIGINAL).and_then(|text| decode(&text))
        && restored(source, doc, &record).is_some()
    {
        return record;
    }
    let from = doc.and_then(|doc| doc.ids().max()).unwrap_or(0) + 1;
    let header = schema_statement()
        .find(source)
        .map(|m| String::from_utf8_lossy(m.as_bytes()).into_owned())
        .unwrap_or_default();
    json!({"schema": 1, "sha256": sha256(source), "header": header, "from": from})
}

/// The payload named `name` that the STEP `bytes` carries, if any: the last item of that name
/// held by a user defined attribute (as the writer stores it) whose text is base64 of JSON.
pub fn payload(bytes: &[u8], name: &str) -> Option<Value> {
    let doc = Document::parse(bytes.to_vec()).ok()?;
    carried_items(&doc, name)
        .into_iter()
        .rev()
        .find_map(|id| decode(&text_of(&doc, id)?))
}

/// [`payload`] of one part of the STEP `bytes` (an assembly's): the last item named `name` of
/// a set of that name on part `part` itself (as the writer stores it on the part it writes),
/// whose text is base64 of JSON. Another part's payload of the same name is not this part's.
pub fn part_payload(bytes: &[u8], part: usize, name: &str) -> Option<Value> {
    let defs = read_part_definitions(bytes).ok()?;
    let doc = Document::parse(bytes.to_vec()).ok()?;
    let read = pmi::read(&doc, &defs).ok()?;
    read.parts
        .get(part)?
        .attributes
        .iter()
        .rev()
        .filter(|set| set.name == name && set.on.is_none())
        .find_map(|set| {
            set.items
                .iter()
                .rev()
                .find_map(|(item, value)| match value {
                    AttributeValue::Text(text) if item == name => decode(text),
                    _ => None,
                })
        })
}

/// The items named `name` held by user defined attributes, in file order.
fn carried_items(doc: &Document, name: &str) -> Vec<u64> {
    resume::items_in_file_order(doc, name)
        .into_iter()
        .filter(|&id| resume::is_attribute_item(doc, id))
        .collect()
}

/// The bytes of the part the STEP `bytes` was first written from, without what the writer
/// added, if it records them and they hash to the sha256 recorded ([`ORIGINAL`]).
pub fn original(bytes: &[u8]) -> Option<Vec<u8>> {
    let doc = Document::parse(bytes.to_vec()).ok()?;
    let record = decode(&last_text(&doc, ORIGINAL)?)?;
    restored(bytes, &doc, &record)
}

fn restored(bytes: &[u8], doc: &Document, record: &Value) -> Option<Vec<u8>> {
    let from = record["from"].as_u64()?;
    let header = record["header"].as_str()?;
    let sha = record["sha256"].as_str()?;
    // What the writer added is every instance from `from` on, after all of the original's.
    let spans = |added: bool| {
        doc.ids()
            .filter(move |&id| (id >= from) == added)
            .filter_map(|id| doc.span(id))
    };
    let start = spans(true).map(|s| s.start).min()?;
    if spans(false).any(|s| s.end > start) {
        return None;
    }
    let end = Regex::new(r"ENDSEC\s*;")
        .ok()?
        .find_iter(bytes)
        .last()?
        .start();
    if end < start {
        return None;
    }
    let joined = [&bytes[..start], &bytes[end..]].concat();
    let statement = schema_statement();
    statement.find(&joined)?;
    let out = statement
        .replace(&joined, NoExpand(header.as_bytes()))
        .into_owned();
    (sha256(&out) == sha).then_some(out)
}

fn last_text(doc: &Document, name: &str) -> Option<String> {
    let id = *carried_items(doc, name).last()?;
    text_of(doc, id)
}

fn text_of(doc: &Document, id: u64) -> Option<String> {
    resume::descriptive_text(doc, id)
}

fn decode(text: &str) -> Option<Value> {
    if !is_base64(text) {
        return None;
    }
    serde_json::from_slice(&STANDARD.decode(text).ok()?).ok()
}

/// The header's schema statement, which the writer changes to AP242 (the editor's pattern).
fn schema_statement() -> Regex {
    // A constant pattern.
    Regex::new(r"FILE_SCHEMA\s*\([\s\S]*?\)\s*;").expect("a valid pattern")
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
