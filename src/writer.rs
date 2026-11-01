//! Intent written into the STEP as AP242 semantic PMI through haecceity, and verified by reading
//! back (port of `writer.py`, and of what `requirements.py` and `merge.py` achieve).
//!
//! Each part's intent becomes one [`PartPmi`] (`part_pmi`): datums A–H on their datum
//! features (coplanar faces, or a hole's cylinders for an axis datum), sizes (an ISO 286 fit as
//! the fit, `±` and limits as deviations), locations (basic, or deviations) from the reference
//! faces to the feature, geometric tolerances with their zone, modifier and datum system,
//! material, the general tolerance as a class, threads (with their manufacturing data as a
//! user defined attribute set, upstream need U8), knurls, surface finish and the part's notes.
//! Every part is written in one `haecceity::pmi::write` edit of the file, which validates every
//! instance it makes against the AP242 schema before anything is returned.
//!
//! **Provenance.** Every authored semantic requirement carries [`PMI_PROVENANCE`] in
//! standard AP242 fields or representation descriptions. Input PMI stays unmarked; see
//! `docs/pmi-provenance.md` for the exact STEP-only reader contract.
//!
//! **Modes.** A part with no PMI, or one specify-core wrote (answers stored that match its
//! faces, [`resume`]), is written by `Mode::Replace`: afterwards its PMI is exactly the
//! intent's. A part with PMI from elsewhere is written by `Mode::Add`: every original byte is
//! kept, the new items are written beside the file's own, a datum letter the file has is the
//! file's datum, and no answers are stored (its PMI could not be told apart from what was
//! added, as in Python). In an assembly each part written has its own mode by the same rule: a
//! part that resumes its answers is replaced while another with PMI from elsewhere is added to
//! in the same write (two haecceity edits, replace then add; one when the modes agree). An assembly
//! follows Python's rule that each part is written with its answers, but not its rule that every
//! part with answers stored is written again: one part may be written alone, and haecceity's
//! per-part replace leaves the other parts, their PMI and their stored answers byte for byte as
//! they were (decision D3, 2026-10-15; Python refused, as OpenCascade rewrote the whole file).
//! An assembly with PMI from elsewhere, which Python refuses because of OpenCascade, is written
//! by `Mode::Add` (question Q6, a deliberate difference).
//!
//! **Notes and finish** (question Q3, decided 2026-10-09). A surface finish (`Ra 3.2`, on faces
//! or the part's default) is a surface texture in ISO 10303-1110's form, 'any process allowed'
//! (ISO 1302's basic symbol, which an unqualified `Ra 3.2` means); the part's other notes
//! (coating, heat treatment, edges) are the PMI practice's 'semantic text' on the part (§7.4),
//! each named by its kind in Python's text route ('surface treatment', 'heat treatment', 'edge
//! condition'), which specify-core reads. Free-text part notes use `general note` plus a
//! one-based integer `note index` in each set (see `docs/part-notes.md`).
//!
//! **Knurls** are written as AP242's `turned_knurl`, the parameters it needs beyond the intent's
//! diameter and pitch derived from the knurling standard where the intent does not state them,
//! and the warnings say so (questions Q4, Q15; `knurl`).
//!
//! **What is not written** is listed in the report's `not_written`, each saying why, never
//! dropped silently: a finish that is not `Ra <micrometres>`, and a knurl whose pattern or pitch
//! the standard does not give. So is a requirement that names no faces (`the size on
//! holes:16`), having nothing in the file to be written on; the rest of the intent is written
//! (question Q17).
//!
//! **What is not ported**, because it only works round OpenCascade: the text fixes
//! (`_drop_runout_zones`, `_type_limits`, `_simple_tolerances`, `_part_datums`), the metre
//! scale for tolerances, fits written as bare limits, one datum label per tolerance, the text
//! route for threads, knurls, notes and the general tolerance, the leaders, and `merge.py`'s
//! transplant (haecceity's add mode keeps the original bytes itself).
//!
//! **Atomic output.** The result is written beside the destination (`.name.pid.rand.part`),
//! read back from there and verified (`verify`, the stand-in for upstream need U7), and only
//! then renamed over the destination: a symbolic link is written through to its target, an
//! existing file keeps its permissions, and any failure leaves the destination and the source
//! as they were.
//!
//! **In memory.** [`write_bytes`] (one part, of a file of one part or of an assembly) and
//! [`write_parts_bytes`] (any parts of one file, an assembly's included) are the same write for a caller with no
//! filesystem (the editor's browser build): the bytes are returned rather than staged, after the
//! same verification of what was written. The file's other parts, where the writer consults
//! them, are loaded from the parts' bytes, never from a path. An unchanged resumed write
//! retains the original bytes when its complete planned PMI and stored answers match them;
//! the replacement candidate is still written and verified before that no-op is accepted.
//!
//! **Caller attributes.** [`write_bytes_with`] and [`write_parts_bytes_with`] accept extra
//! [`AttributeSet`]s, written as given and verified in the same edit as the PMI and stored
//! answers. [`write_with`] and [`write_parts_with`] do the same for a destination, including
//! `.gz`. Nonempty extras are written on one part, the one job's (a file of one part, or one
//! part of an assembly, on that part's product definition); a write of several parts of an
//! assembly with extras is refused. The stored-answers name ([`resume::NAME`])
//! is reserved for both set and item names, since resume reads items by name. Extras follow
//! the usual write mode: on a resumed part replace removes the previous attributes along with
//! the previous PMI and answers, so supply every extra wanted in the new write; add leaves
//! existing attributes alone. There is no separate merge or update by attribute name.

use std::collections::BTreeSet;
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use haecceity::p21::{self, Attribute, Document, RawEntity};
use haecceity::pmi::write::{Mode, PresentationPolicy, differences_as_stated};
use haecceity::pmi::{
    self, Anchor, Bounds, Compartment, Datum, DatumLabel, DatumReference, DatumSystem, Decimal,
    DimTolerance, Dimension, DimensionKind, FaceIndex, Feature, FeatureId, FundamentalDeviation,
    GeneralTolerance, GeometricTolerance, Hand, Iso286Class, Length, LengthUnit, LocationKind,
    Material, NoteOwner, PartId, PartPmi, PmiRead, Ratio, SizeKind, Thread, ThreadSide,
    ToleranceGrade, ToleranceKind, ToleranceModifier, ToleranceTarget, Value, Zone, ZoneForm,
};
use haecceity::pmi::{
    AttributeSet, AttributeValue, MaterialRemoval, SurfaceTexture, SurfaceTextureParameter,
};
use haecceity::step::{PartDefinition, read_part_definitions};
use serde::Serialize;
use serde_json::{Map, Value as Json, json};

use crate::part::{self, LoadedPart};
use crate::provenance;

use crate::pyfmt;

use crate::pyjson;
use crate::resume;
use crate::rules::Intent;

mod datum_supports;

/// Stable STEP authorship token. See `docs/pmi-provenance.md` for its AP242 fields.
pub const PMI_PROVENANCE: &str = provenance::MARKER;

/// What a write did (Python's `WriteReport`; `to_dict`'s keys and order).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WriteReport {
    /// The destination as given (a symbolic link stays the link's path).
    pub output: PathBuf,
    /// One line per item written, in intent order (Python's lines).
    pub written: Vec<String>,
    /// Intent that has no place in the file yet, each saying what and why; it must reach the
    /// drawing another way.
    pub not_written: Vec<String>,
    pub warnings: Vec<String>,
}

impl WriteReport {
    /// Python's `to_dict`: `{output, written, not_written, warnings}`.
    pub fn to_dict(&self) -> Json {
        json!({
            "output": self.output.to_string_lossy(),
            "written": self.written,
            "not_written": self.not_written,
            "warnings": self.warnings,
        })
    }
}

/// The file written does not read back as what was written (Python's `VerificationError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationError(pub String);

impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VerificationError {}

/// Why nothing was written. The destination and the source are as they were in every case.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteError {
    /// What was asked cannot be written as asked (Python's `ValueError`, with its message): the
    /// command line's exit 2, serve's `invalid`.
    Invalid(String),
    /// haecceity would not write it: an AP214/AP203 file whose instances do not all validate
    /// against AP242 (named), or an item it refuses (named). Exit 1, serve's `failed`.
    Refused(String),
    /// The file written does not read back as written. Exit 1.
    Verification(VerificationError),
    /// The file could not be read, loaded or written. Exit 1.
    Failed(String),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Invalid(why) | WriteError::Failed(why) => f.write_str(why),
            WriteError::Refused(why) => write!(f, "haecceity will not write it: {why}"),
            WriteError::Verification(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<VerificationError> for WriteError {
    fn from(e: VerificationError) -> Self {
        WriteError::Verification(e)
    }
}

fn invalid<T>(why: impl Into<String>) -> Result<T, WriteError> {
    Err(WriteError::Invalid(why.into()))
}

/// A part to write: the part, its intent, and the answers it came from (Python's `Job`).
pub type Job<'a> = (&'a LoadedPart, &'a Intent, Option<&'a Map<String, Json>>);

/// Writes `loaded` with `intent` added as AP242 to `output`, and verifies it (Python's
/// `write`). Given `answers`, a part written from a bare file or one specify-core wrote stores
/// them, to be resumed; one with PMI from elsewhere does not.
#[cfg(not(target_arch = "wasm32"))]
pub fn write(
    loaded: &LoadedPart,
    intent: &Intent,
    output: &Path,
    answers: Option<&Map<String, Json>>,
) -> Result<WriteReport, WriteError> {
    write_with(loaded, intent, output, answers, &[])
}

/// [`write`] with caller attributes, following [`write_bytes_with`]'s rules. The attributes
/// are verified before the destination is replaced; a `.gz` destination is compressed as usual.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_with(
    loaded: &LoadedPart,
    intent: &Intent,
    output: &Path,
    answers: Option<&Map<String, Json>>,
    extra: &[AttributeSet],
) -> Result<WriteReport, WriteError> {
    write_parts_with(&[(loaded, intent, answers)], output, extra)
}

/// Writes each part of one file with its intent, all in one edit (Python's `write_parts`):
/// `jobs` are (part, its intent, the answers it came from). Over references, so a caller holds
/// its parts (from `part::load_all`) and composes the jobs from them.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_parts(jobs: &[Job<'_>], output: &Path) -> Result<WriteReport, WriteError> {
    write_parts_with(jobs, output, &[])
}

/// [`write_parts`] with caller attributes, following [`write_bytes_with`]'s rules. Nonempty
/// extras are written on the one part written: several parts written together with extras are
/// refused.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_parts_with(
    jobs: &[Job<'_>],
    output: &Path,
    extra: &[AttributeSet],
) -> Result<WriteReport, WriteError> {
    check_jobs(jobs)?;
    check_extra(jobs, extra)?;

    // Written and verified beside the destination, which is replaced only once all is well.
    let target = match std::fs::symlink_metadata(output) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(output)
            .map_err(|e| WriteError::Failed(format!("{}: {e}", output.display())))?,
        _ => output.to_path_buf(),
    };
    let Some(name) = target.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err(WriteError::Failed(format!(
            "{}: not a file name",
            output.display()
        )));
    };
    let staged = target.with_file_name(format!(".{name}.{}.{}.part", std::process::id(), tag()));
    let result = write_verified(jobs, extra).and_then(|(bytes, report)| {
        // Gzipped only when the destination is named `.gz` (a write in place to a `.gz` input
        // stays gzipped); the STEP text is verified before it is compressed.
        let bytes = if output
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("gz"))
        {
            gzip(&bytes).map_err(|e| WriteError::Failed(format!("{}: {e}", output.display())))?
        } else {
            bytes
        };
        std::fs::write(&staged, &bytes)
            .map_err(|e| WriteError::Failed(format!("{}: {e}", staged.display())))?;
        let back = std::fs::read(&staged)
            .map_err(|e| WriteError::Failed(format!("{}: {e}", staged.display())))?;
        if back != bytes {
            return Err(
                VerificationError("the file on disk is not what was written".into()).into(),
            );
        }
        if let Ok(m) = std::fs::metadata(&target) {
            std::fs::set_permissions(&staged, m.permissions())
                .map_err(|e| WriteError::Failed(format!("{}: {e}", output.display())))?;
        }
        std::fs::rename(&staged, &target)
            .map_err(|e| WriteError::Failed(format!("{}: {e}", output.display())))?;
        Ok(report)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    let mut report = result?;
    report.output = output.to_path_buf();
    Ok(report)
}

/// `bytes` gzipped, deterministically (no name, no time stamp).
#[cfg(not(target_arch = "wasm32"))]
fn gzip(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, bytes)?;
    encoder.finish()
}

/// [`write`] in memory: `loaded` (from [`part::load_bytes`] or [`part::load`]) with `intent`
/// added as AP242, verified by reading back as [`write`] verifies its file, and returned as the
/// file's bytes with the report. The bytes are those [`write`] would put in its destination;
/// the report's `output` is `loaded.path`, there being no destination. `loaded` may be a part
/// of an assembly (from [`part::File::load`]): it is written as [`write_parts_bytes`] writes it
/// alone, the file's other parts, their placements, PMI and stored answers kept as they were.
pub fn write_bytes(
    loaded: &LoadedPart,
    intent: &Intent,
    answers: Option<&Map<String, Json>>,
) -> Result<(Vec<u8>, WriteReport), WriteError> {
    write_bytes_with(loaded, intent, answers, &[])
}

/// [`write_bytes`] with extra attribute sets, written as given and verified in the same edit
/// as the PMI and stored answers. Use `AttributeSet::on = None` for attributes on the part.
/// The set name and its item names must not equal [`resume::NAME`] (`pmi-assist answers`).
///
/// Extras follow the usual replace/add mode: replacing a resumed part removes its old extras,
/// as it removes its old answers; pass every extra wanted in this write. Adding to foreign
/// PMI leaves its existing attributes alone. No attribute is merged or updated by name.
/// An empty slice is byte-identical to [`write_bytes`]. For a part of an assembly the extras
/// are written on that part's product definition, as its PMI is.
pub fn write_bytes_with(
    loaded: &LoadedPart,
    intent: &Intent,
    answers: Option<&Map<String, Json>>,
    extra: &[AttributeSet],
) -> Result<(Vec<u8>, WriteReport), WriteError> {
    write_parts_bytes_with(&[(loaded, intent, answers)], extra)
}

/// [`write_parts`] in memory: each part of one file with its intent, all in one edit, verified by
/// reading back as [`write_parts`] verifies its file, and returned as the file's bytes with the
/// report (its `output` the first part's `path`, there being no destination). Unlike
/// [`write_bytes`], a part of an assembly is written: the file's other parts are loaded from the
/// parts' bytes where the writer needs them. The jobs are refused as [`write_parts`] refuses
/// them.
pub fn write_parts_bytes(jobs: &[Job<'_>]) -> Result<(Vec<u8>, WriteReport), WriteError> {
    write_parts_bytes_with(jobs, &[])
}

/// [`write_parts_bytes`] with caller attributes, following [`write_bytes_with`]'s rules.
/// Nonempty extras are written on the one part written (of a file of one part or of an
/// assembly); several parts written together with extras are refused. With an empty slice,
/// assemblies work as in [`write_parts_bytes`].
pub fn write_parts_bytes_with(
    jobs: &[Job<'_>],
    extra: &[AttributeSet],
) -> Result<(Vec<u8>, WriteReport), WriteError> {
    check_jobs(jobs)?;
    check_extra(jobs, extra)?;
    write_verified(jobs, extra)
}

/// Reserve both names: resume reads descriptive items by name, independently of their set.
/// Extras are the one written part's: with several parts written they would have no one owner.
fn check_extra(jobs: &[Job<'_>], extra: &[AttributeSet]) -> Result<(), WriteError> {
    if extra
        .iter()
        .any(|set| set.name == crate::pmi_origin::PROPERTY)
    {
        return invalid("extra attribute set name 'specify pmi origin' is reserved");
    }
    if extra
        .iter()
        .any(|set| set.name == crate::characteristics::PROPERTY)
    {
        return invalid(format!(
            "extra attribute set name '{}' is reserved for characteristic numbers",
            crate::characteristics::PROPERTY
        ));
    }
    if extra.iter().any(|set| {
        set.name == resume::NAME || set.items.iter().any(|(name, _)| name == resume::NAME)
    }) {
        return invalid(format!(
            "extra attribute set and item names must not be '{}', reserved for stored answers",
            resume::NAME
        ));
    }
    if !extra.is_empty() && jobs.len() != 1 {
        return invalid(
            "extra attributes are written on one part: write one part of an assembly at a time",
        );
    }
    Ok(())
}

/// What [`write_parts`] and [`write_parts_bytes`] refuse before writing anything: no jobs, parts
/// of different files, a part twice, an intent not made for its part.
fn check_jobs(jobs: &[Job<'_>]) -> Result<(), WriteError> {
    let Some(&(first, _, _)) = jobs.first() else {
        return invalid("nothing to write");
    };
    if jobs.iter().any(|(l, _, _)| l.path != first.path) {
        return invalid("the parts written together must be of one file");
    }
    let parts: BTreeSet<usize> = jobs.iter().map(|(l, _, _)| l.binding.part).collect();
    if parts.len() != jobs.len() {
        return invalid("a part is written once");
    }
    for (loaded, intent, _) in jobs {
        check_binding(loaded, intent)?;
    }
    Ok(())
}

/// Eight hex digits that differ between writes, for the staged file's name (Python's
/// `uuid4().hex[:8]`); nothing written depends on them.
#[cfg(not(target_arch = "wasm32"))]
fn tag() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!(
        "{:08x}",
        nanos ^ COUNT.fetch_add(1, Ordering::Relaxed).rotate_left(16)
    )
}

/// Python's `_check_binding`: the intent must be for these bytes, this part, its face count
/// and this loader, with Python's messages. Python compares with `==`, so `2` and `2.0` agree.
fn check_binding(loaded: &LoadedPart, intent: &Intent) -> Result<(), WriteError> {
    let binding = &intent.binding;
    let get = |key: &str| binding.get(key);
    if get("source_sha256") != Some(&json!(loaded.binding.source_sha256)) {
        return invalid("intent was made for a different file");
    }
    // An analysis made before parts were told apart is of a file of one part.
    let part = get("part").cloned().unwrap_or(json!(0));
    if !py_eq(&part, &json!(loaded.binding.part)) {
        return invalid(format!(
            "intent was made for part {}, not part {}",
            py_str(&part),
            loaded.binding.part
        ));
    }
    if !get("face_count").is_some_and(|v| py_eq(v, &json!(loaded.binding.face_count))) {
        return invalid("intent face count does not match the file");
    }
    // A loader change may number the faces differently for the same bytes.
    let version = get("loader_version").cloned().unwrap_or(Json::Null);
    if !py_eq(&version, &json!(loaded.binding.loader_version)) {
        return invalid(format!(
            "intent was made by loader version {}, not {}: analyse the file again",
            py_repr(&version),
            loaded.binding.loader_version
        ));
    }
    Ok(())
}

/// Python's `==` of two JSON values as far as the binding needs it: numbers by value (a bool is
/// 0 or 1), anything else structurally.
fn py_eq(a: &Json, b: &Json) -> bool {
    let num = |v: &Json| match v {
        Json::Number(n) => n.as_f64(),
        Json::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    };
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Python's `repr()` of a JSON value, for messages (`None`, `True`, `1`, `'2'`).
fn py_repr(v: &Json) -> String {
    match v {
        Json::Null => "None".into(),
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Number(n) if n.is_f64() => pyfmt::repr(n.as_f64().unwrap_or(f64::NAN)),
        Json::Number(n) => n.to_string(),
        Json::String(s) => pyfmt::repr_str(s),
        Json::Array(items) => {
            let items: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", items.join(", "))
        }
        Json::Object(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", pyfmt::repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// Python's `str()` of a JSON value: a string as it is, anything else its `repr`.
fn py_str(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// The file as read once: its document, its parts and their PMI.
struct Source {
    doc: Document,
    defs: Vec<PartDefinition>,
    read: PmiRead,
}

impl Source {
    fn of(bytes: &[u8]) -> Result<Source, WriteError> {
        let failed = |e: String| WriteError::Failed(format!("cannot read the file: {e}"));
        let defs = read_part_definitions(bytes).map_err(|e| failed(e.to_string()))?;
        let doc = Document::parse(bytes.to_vec()).map_err(|e| failed(e.to_string()))?;
        let read = pmi::read(&doc, &defs).map_err(|e| failed(e.to_string()))?;
        if read.parts.len() != defs.len() {
            return Err(failed(format!(
                "{} parts read with PMI, {} in the file",
                read.parts.len(),
                defs.len()
            )));
        }
        Ok(Source { doc, defs, read })
    }

    /// Whether the part has PMI other than stored answers.
    fn has_pmi(&self, part: usize) -> bool {
        // Every job's part was checked against these bytes' parts; Source::of verifies the reader's part count.
        let mut p = self.read.parts[part].clone();
        p.attributes.retain(|a| !resume::is_answers(a));
        p != PartPmi::default()
    }
}

/// One part to write, as planned.
struct Planned<'a> {
    loaded: &'a LoadedPart,
    pmi: PartPmi,
    /// The payload stored with it, if answers are stored.
    payload: Option<Json>,
    lines: Lines,
}

/// The report's lines for one part, before any part-name prefix.
#[derive(Default)]
struct Lines {
    written: Vec<String>,
    not_written: Vec<String>,
    warnings: Vec<String>,
}

/// Writes `jobs` and verifies the result by reading it back: the file's bytes and the report
/// (its `output` the first part's path, for the caller to replace).
fn write_verified(
    jobs: &[Job<'_>],
    extra: &[AttributeSet],
) -> Result<(Vec<u8>, WriteReport), WriteError> {
    // Every write_verified caller first rejects empty jobs; a session supplies exactly one job.
    let first = jobs[0].0;
    let bytes: &[u8] = &first.bytes;
    let source = Source::of(bytes)?;
    let failed = |e: crate::features::FeatureError| WriteError::Failed(e.to_string());

    if first.part_count > 1 {
        // Stored answers are how a later write knows the PMI is specify-core's.
        if jobs.iter().any(|(_, _, answers)| answers.is_none()) {
            return invalid("a part of an assembly is written with the answers it came from");
        }
        let stored = resume::stored(bytes);
        // A part not written now whose answers are stored is loaded from these same bytes
        // (never from the path, which an in-memory caller does not have), once for all of them.
        let mut file = None;
        let mut stale = Vec::new();
        for &k in stored.keys() {
            let current = if k >= first.part_count {
                None
            } else if let Some((l, _, _)) = jobs.iter().find(|(l, _, _)| l.binding.part == k) {
                resume::answers_for(l).map_err(failed)?
            } else {
                let load_failed = |e: part::LoadError| WriteError::Failed(e.to_string());
                if file.is_none() {
                    let read = part::File::of_shared(&first.path, Arc::clone(&first.bytes))
                        .map_err(load_failed)?;
                    file = Some(read);
                }
                // file was assigned Some on the immediately preceding branch when it was None.
                let l = file
                    .as_ref()
                    .expect("read above")
                    .load(Some(k))
                    .map_err(load_failed)?;
                resume::answers_for(&l).map_err(failed)?
            };
            if current.is_none() {
                stale.push(k.to_string());
            }
        }
        if !stale.is_empty() {
            return invalid(format!(
                "the answers stored for part(s) {} are not for this assembly's parts: it has \
                 changed since specify-core wrote it",
                stale.join(", ")
            ));
        }
        // Decision D3: a part with answers stored that is not written now is left as it is
        // (haecceity's replace touches only the parts written), so it need not be written
        // again with these; Python's refusal ("write them again with these") is not kept.
    }
    // Python writes a bare or resumed part from its geometry alone, and adds to any other
    // (its `merge`). Replace is the former: the part's PMI becomes exactly the intent's. A bare
    // part is Replace rather than design.md's Add so that Python-form answers that no longer
    // resume (not PMI to haecceity) go, as from Python's rewrite; question Q18. Decided for each
    // part written: Replace when it is bare or resumes, Add when it has PMI from elsewhere, so
    // such PMI is never replaced and a resumed part's own PMI is never written twice; parts not
    // written are left alone either way (D3).
    let mut modes = Vec::new();
    for (loaded, _, _) in jobs {
        let foreign = source.has_pmi(loaded.binding.part)
            && resume::answers_for(loaded).map_err(failed)?.is_none();
        modes.push(if foreign { Mode::Add } else { Mode::Replace });
    }

    let mut planned = Vec::new();
    for (&(loaded, intent, answers), &mode) in jobs.iter().zip(&modes) {
        // The binding was checked against these same bytes' parts; Source::of verifies reader counts.
        let before = &source.read.parts[loaded.binding.part];
        let (mut pmi, lines) = part_pmi(loaded, intent, before, mode)?;
        let payload = match answers {
            Some(answers) if mode == Mode::Replace => {
                let mut saved = answers.clone();
                saved.remove(crate::characteristics::CARRY);
                let payload = resume::payload(loaded, &saved).map_err(failed)?;
                pmi.attributes.push(resume::attribute_set(&payload));
                Some(payload)
            }
            _ => None,
        };
        pmi.attributes.extend_from_slice(extra);
        crate::characteristics::allocate(
            loaded,
            &mut pmi,
            before,
            &source.read.provenance.parts[loaded.binding.part],
            &source.doc,
            answers,
            mode == Mode::Replace,
        )
        .map_err(WriteError::Invalid)?;
        planned.push(Planned {
            loaded,
            pmi,
            payload,
            lines,
        });
    }

    let items: Vec<(PartId, PartPmi)> = planned
        .iter()
        .map(|p| (PartId(p.loaded.binding.part), p.pmi.clone()))
        .collect();
    let back = write_by_modes(&source, &items, &modes)?;
    verify(&source, &items, &modes, &back)?;
    let stored = resume::stored(&back);
    for p in &planned {
        let k = p.loaded.binding.part;
        if let Some(payload) = &p.payload
            && stored.get(&k).map(|m| Json::Object(m.clone())).as_ref() != Some(payload)
        {
            return Err(VerificationError(format!(
                "the answers stored for part {k} do not read back"
            ))
            .into());
        }
    }

    // A no-op on a resumed file should keep its bytes, including entity numbers and datum
    // presentation. haecceity replaces equivalent PMI with freshly numbered instances. The
    // candidate above was still written and fully verified; keep the original only when
    // every planned part (including caller attributes and the exact answers payload) already
    // states precisely that candidate. Changed/removed notes or answers take the edit path.
    let unchanged = modes.iter().all(|m| *m == Mode::Replace)
        && planned.iter().all(|p| p.payload.is_some())
        && items.iter().all(|(part, pmi)| {
            // items is mapped from the same validated session bindings used to plan the writes above.
            differences_as_stated(&source.read.parts[part.0], pmi).is_empty()
                && provenance::all(
                    &source.read.parts[part.0],
                    &source.read.provenance.parts[part.0],
                    &source.doc,
                )
        });
    let back = if unchanged { bytes.to_vec() } else { back };

    // In an assembly written together, each line says which part it is of.
    let mut report = WriteReport {
        output: first.path.clone(),
        written: Vec::new(),
        not_written: Vec::new(),
        warnings: Vec::new(),
    };
    for p in planned {
        let prefix = if jobs.len() > 1 {
            if p.loaded.name.is_empty() {
                format!("part {}: ", p.loaded.binding.part)
            } else {
                format!("{}: ", p.loaded.name)
            }
        } else {
            String::new()
        };
        let with = |lines: Vec<String>| lines.into_iter().map(|l| format!("{prefix}{l}"));
        report.written.extend(with(p.lines.written));
        report.not_written.extend(with(p.lines.not_written));
        report.warnings.extend(with(p.lines.warnings));
    }
    Ok((back, report))
}

/// The file `source` with `items` written, each part by its mode in `modes` (in the same order):
/// one haecceity edit when every part has the same mode, else the parts written by replace
/// first and then, on that file, the parts written by add (haecceity's write takes one mode per
/// edit). A part not written is untouched by either.
fn write_by_modes(
    source: &Source,
    items: &[(PartId, PartPmi)],
    modes: &[Mode],
) -> Result<Vec<u8>, WriteError> {
    let policy = PresentationPolicy::RemovePresentation;
    let refused = |e: pmi::write::WriteError| match e {
        pmi::write::WriteError::Edition { .. } | pmi::write::WriteError::Refused(_) => {
            WriteError::Refused(e.to_string())
        }
        pmi::write::WriteError::Invalid { .. } => WriteError::Invalid(e.to_string()),
        other => WriteError::Failed(format!("haecceity could not write the PMI: {other}")),
    };
    let defs = source.defs.as_slice();
    // The file written so far, when an edit has been applied.
    let mut written: Option<Vec<u8>> = None;
    for mode in [Mode::Replace, Mode::Add] {
        let these: Vec<(PartId, PartPmi)> = items
            .iter()
            .zip(modes)
            .filter(|&(_, &m)| m == mode)
            .map(|(item, _)| item.clone())
            .collect();
        if these.is_empty() {
            continue;
        }
        // The replace edit leaves every part's geometry, and so `defs`, as it was.
        let parsed = match written.take() {
            Some(bytes) => Some(Document::parse(bytes).map_err(|e| {
                WriteError::Failed(format!("cannot read the file written by replace: {e}"))
            })?),
            None => None,
        };
        let current = parsed.as_ref().unwrap_or(&source.doc);
        let stripped = if mode == Mode::Replace {
            let parts: Vec<_> = these
                .iter()
                .map(|(id, _)| source.read.provenance.parts[id.0].clone())
                .collect();
            let edit = provenance::before_replace(current, &parts);
            let unnumbered = current
                .apply(&crate::characteristics::before_replace(current, &parts))
                .map_err(|e| WriteError::Failed(e.to_string()))?;
            let current =
                Document::parse(unnumbered.bytes).map_err(|e| WriteError::Failed(e.to_string()))?;
            Some(
                Document::parse(
                    current
                        .apply(&edit)
                        .map_err(|e| WriteError::Failed(e.to_string()))?
                        .bytes,
                )
                .map_err(|e| WriteError::Failed(e.to_string()))?,
            )
        } else {
            None
        };
        let current = stripped.as_ref().unwrap_or(current);
        // Edition 1 excludes sizes/tolerances from characterized_definition. Upgrade
        // only the header, using the lossless editor; original entity records stay intact.
        let schemas = current
            .file_schema()
            .map_err(|e| WriteError::Failed(e.to_string()))?;
        let upgrade = these.iter().any(|(_, p)| {
            p.attributes
                .iter()
                .any(|a| a.name == crate::pmi_origin::PROPERTY)
        }) && schemas
            .first()
            .and_then(|s| haecceity::express::edition(s))
            .is_some_and(|e| e.type_decl("characterized_item").is_none());
        let upgraded = if upgrade {
            let mut edit = haecceity::p21::Edit::new();
            edit.set_file_schema(vec![haecceity::express::TARGET_SCHEMA.into()]);
            Some(
                Document::parse(
                    current
                        .apply(&edit)
                        .map_err(|e| WriteError::Failed(e.to_string()))?
                        .bytes,
                )
                .map_err(|e| WriteError::Failed(e.to_string()))?,
            )
        } else {
            None
        };
        let current = upgraded.as_ref().unwrap_or(current);
        let (mut edit, _) = pmi::write(current, defs, &these, mode, policy).map_err(refused)?;
        if mode == Mode::Replace {
            // Python's own form of stored answers is not PMI haecceity reads (it reports it as
            // not modelled and keeps it), so a replace takes it out here: the answers stored
            // now are the only ones.
            for (part, _) in &these {
                // these retains the same validated part ids; replace edits do not change geometry or definitions.
                for id in python_answers(current, &defs[part.0]) {
                    edit.remove(id);
                }
            }
        }
        let applied = current
            .apply(&edit)
            .map_err(|e| WriteError::Failed(format!("haecceity could not apply the edit: {e}")))?;
        let after =
            Document::parse(applied.bytes).map_err(|e| WriteError::Failed(e.to_string()))?;
        let independent = after
            .apply(&datum_supports::independent(current, &after))
            .map_err(|e| WriteError::Failed(e.to_string()))?;
        let after =
            Document::parse(independent.bytes).map_err(|e| WriteError::Failed(e.to_string()))?;
        let marker_edit = provenance::mark_new(current, &after);
        let marked = after
            .apply(&marker_edit)
            .map_err(|e| WriteError::Failed(format!("cannot mark written PMI: {e}")))?;
        let marked =
            Document::parse(marked.bytes).map_err(|e| WriteError::Failed(e.to_string()))?;
        let numbers =
            crate::characteristics::annotate(current, &marked, defs).map_err(WriteError::Failed)?;
        written = Some(
            marked
                .apply(&numbers)
                .map_err(|e| WriteError::Failed(e.to_string()))?
                .bytes,
        );
    }
    // jobs is checked nonempty and every edit group assigns written before reaching this return.
    Ok(written.expect("at least one part is written"))
}

/// The instances of Python's form of stored answers on `part` (`PROPERTY_DEFINITION('pmi-assist
/// answers', ..., #pd)`, its `PROPERTY_DEFINITION_REPRESENTATION`, the representation and its
/// item), where nothing else refers to them.
fn python_answers(doc: &Document, part: &PartDefinition) -> Vec<u64> {
    let simple = |id: u64| match doc.get(id) {
        Some(RawEntity::Simple {
            name, attributes, ..
        }) => Some((name.as_str(), attributes.as_slice())),
        _ => None,
    };
    let mut out = Vec::new();
    for &pdef in doc.referrers(part.product_definition) {
        let Some(("PROPERTY_DEFINITION", [Attribute::String(n), _, Attribute::EntityRef(pd)])) =
            simple(pdef)
        else {
            continue;
        };
        if n != resume::NAME || *pd != part.product_definition {
            continue;
        }
        let pdrs = doc.referrers(pdef);
        let [pdr] = pdrs else { continue };
        let Some(("PROPERTY_DEFINITION_REPRESENTATION", [_, Attribute::EntityRef(rep)])) =
            simple(*pdr)
        else {
            continue;
        };
        if doc.referrers(*rep) != [*pdr] {
            continue;
        }
        let Some(("REPRESENTATION", [_, Attribute::List(items), _])) = simple(*rep) else {
            continue;
        };
        let items: Vec<u64> = items
            .iter()
            .filter_map(|a| match a {
                Attribute::EntityRef(i) => Some(*i),
                _ => None,
            })
            .collect();
        if items.iter().any(|&i| doc.referrers(i) != [*rep]) {
            continue;
        }
        out.extend([pdef, *pdr, *rep]);
        out.extend(items);
    }
    out
}

/// Checks the file `back` (read from disk) against what was written (the stand-in for upstream
/// need U7, `quiddity pmi write`'s check restated): the parts and their faces are unchanged; a
/// part written by replace reads back as exactly what was written, and nothing the reader
/// consumed for it survives; a part written by add reads back as its PMI before plus exactly
/// the items written (a feature or datum equal to one it had being that one); every other part
/// reads back as before; and no finding is new.
fn verify(
    source: &Source,
    written: &[(PartId, PartPmi)],
    modes: &[Mode],
    back: &[u8],
) -> Result<(), VerificationError> {
    let fail =
        |why: String| VerificationError(format!("the file written does not read back: {why}"));
    let after_defs = read_part_definitions(back).map_err(|e| fail(e.to_string()))?;
    let after_doc = Document::parse(back.to_vec()).map_err(|e| fail(e.to_string()))?;
    let after = pmi::read(&after_doc, &after_defs).map_err(|e| fail(e.to_string()))?;
    let (defs, before) = (&source.defs, &source.read);
    let mut problems: Vec<String> = Vec::new();
    if after_defs.len() != defs.len() || after.parts.len() != defs.len() {
        return Err(fail(format!(
            "{} parts read back, not {}",
            after_defs.len(),
            defs.len()
        )));
    }
    for (i, (d0, d1)) in defs.iter().zip(&after_defs).enumerate() {
        if d0.name != d1.name
            || d0.faces.len() != d1.faces.len()
            || d0.edges.len() != d1.edges.len()
        {
            problems.push(format!("part {i}'s name, faces or edges changed"));
        }
    }
    for (i, read) in after.parts.iter().enumerate() {
        crate::characteristics::verify(&after_doc, read, &after.provenance.parts[i])
            .map_err(fail)?;
        // Source::of checked before.parts.len() == defs.len(); after.parts is checked against defs above.
        let old = &before.parts[i];
        let found = written.iter().zip(modes).find(|((p, _), _)| p.0 == i);
        match found.map(|((_, new), &mode)| (new, mode)) {
            None => problems.extend(
                differences_as_stated(old, read)
                    .into_iter()
                    .map(|d| format!("part {i} (not written): {d}")),
            ),
            Some((new, Mode::Add)) => {
                let keys = |p: &PartPmi| differences_as_stated(&PartPmi::default(), p);
                let shared: BTreeSet<String> = keys(old)
                    .into_iter()
                    .filter(|k| {
                        k.starts_with("feature only in the second: ")
                            || k.starts_with("datum only in the second: ")
                    })
                    .collect();
                let mut expected: Vec<String> = keys(new)
                    .into_iter()
                    .filter(|k| !shared.contains(k))
                    .collect();
                for d in differences_as_stated(old, read) {
                    if !d.contains(" only in the second: ") {
                        problems.push(format!("part {i}: lost: {d}"));
                    } else if let Some(at) = expected.iter().position(|k| *k == d) {
                        expected.remove(at);
                    } else {
                        problems.push(format!("part {i}: not written: {d}"));
                    }
                }
                problems.extend(
                    expected
                        .into_iter()
                        .map(|k| format!("part {i}: written but not read back: {k}")),
                );
            }
            Some((new, _)) => {
                problems.extend(
                    differences_as_stated(new, read)
                        .into_iter()
                        .map(|d| format!("part {i}: {d}")),
                );
                let prov = before.provenance.parts.get(i).ok_or_else(|| {
                    fail(format!("part {i}: the PMI reader returned no provenance"))
                })?;
                let geometry: BTreeSet<u64> = prov.geometry.iter().flatten().copied().collect();
                // An instance survives when the file written still has it, unchanged.
                fn text(doc: &Document, id: u64) -> Option<&[u8]> {
                    // Document::span returns parser-owned byte ranges within this same immutable document.
                    doc.span(id).map(|s| &doc.bytes()[s])
                }
                for id in prov.all() {
                    let original = text(&source.doc, id);
                    if !geometry.contains(&id)
                        && original.is_some()
                        && text(&after_doc, id) == original
                    {
                        problems.push(format!(
                            "part {i}: #{id}, which the reader consumed, survives"
                        ));
                    }
                }
            }
        }
    }
    for f in &after.findings {
        if !before.findings.contains(f) {
            problems.push(format!(
                "a finding the input did not have: {} {:?} {}: {}",
                f.kind.as_str(),
                f.ids,
                f.entity,
                f.detail
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(fail(problems.join("; ")))
    }
}

// ---------------------------------------------------------------------------------------------
// Intent → PartPmi
// ---------------------------------------------------------------------------------------------

/// A number of the intent as a decimal in millimetres, written as Part 21 writes a REAL (the
/// shortest text that reads back as the number).
fn mm(x: f64) -> Result<Value, WriteError> {
    Ok(Value::length(decimal(x)?, LengthUnit::Millimetre))
}

fn length(x: f64) -> Result<Length, WriteError> {
    Ok(Length {
        value: decimal(x)?,
        unit: LengthUnit::Millimetre,
    })
}

fn decimal(x: f64) -> Result<Decimal, WriteError> {
    let text = p21::encode_real(x).map_err(|e| WriteError::Invalid(e.to_string()))?;
    Decimal::parse(&text).map_err(|e| WriteError::Invalid(e.0))
}

/// A requirement's number under `key`, refused when it is missing or not a number.
fn number(req: &Map<String, Json>, key: &str) -> Result<f64, WriteError> {
    match req.get(key) {
        Some(Json::Number(n)) => n
            .as_f64()
            .ok_or_else(|| WriteError::Invalid(format!("{key} is not a number"))),
        _ => invalid(format!(
            "a {} requirement has no number '{key}'",
            py_str(req.get("kind").unwrap_or(&Json::Null))
        )),
    }
}

fn text<'a>(req: &'a Map<String, Json>, key: &str) -> Result<&'a str, WriteError> {
    req.get(key).and_then(Json::as_str).ok_or_else(|| {
        WriteError::Invalid(format!(
            "a {} requirement has no '{key}'",
            py_str(req.get("kind").unwrap_or(&Json::Null))
        ))
    })
}

/// Python's truth of an optional value.
fn truthy(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Json::String(s)) => !s.is_empty(),
        Some(Json::Array(a)) => !a.is_empty(),
        Some(Json::Object(o)) => !o.is_empty(),
    }
}

/// A list of face ids as Python prints a list (`[2, 5]`).
fn face_list(v: Option<&Json>) -> String {
    v.map_or_else(|| "[]".into(), pyjson::dumps)
}

/// The user defined attribute that carries a note in words (PMI practice §7.4).
pub const SEMANTIC_TEXT: &str = "semantic text";

/// One-based order of a free-text part note within its `semantic text` attribute set.
pub const NOTE_INDEX: &str = "note index";

/// A finish as specify-core states it (`Ra 3.2`, micrometres: `extras` and the rules' choices)
/// as a surface texture parameter; `None` for anything else.
fn texture(value: &str) -> Option<SurfaceTextureParameter> {
    let number = value.strip_prefix("Ra ")?;
    let ra = pyfmt::parse_float(number).filter(|ra| ra.is_finite() && *ra > 0.0)?;
    Some(SurfaceTextureParameter {
        characteristic: "Ra".into(),
        value: Length {
            value: decimal(ra).ok()?,
            unit: LengthUnit::Micrometre,
        },
    })
}

/// The features of the part being built: one per distinct face set (haecceity writes features
/// of equal items as one instance anyway).
struct Features<'p>(&'p mut Vec<Feature>);

/// The face ids a face list names, `None` when it names none (missing, empty or not a list of
/// face ids).
fn face_ids(faces: Option<&Json>) -> Option<Vec<usize>> {
    faces
        .and_then(Json::as_array)
        .and_then(|a| {
            a.iter()
                .map(|x| x.as_u64().map(|i| i as usize))
                .collect::<Option<Vec<_>>>()
        })
        .filter(|ids| !ids.is_empty())
}

/// What of a requirement names no faces (`the size on holes:16`), if anything does: such a
/// requirement has nothing in the file to be written on (question Q17).
fn faceless(kind: &str, req: &Map<String, Json>) -> Option<String> {
    let name = py_str(req.get("feature").unwrap_or(&Json::Null));
    let none = |key: &str| face_ids(req.get(key)).is_none();
    match kind {
        "size" if none("faces") => Some(format!("the size on {name}")),
        "location" if none("faces") => Some(format!("the location of {name}")),
        "location" if none("reference") => Some(format!("the reference of the location of {name}")),
        "thread" if none("faces") => Some(format!("the thread on {name}")),
        "knurl" if none("faces") => Some(format!("the knurl on {name}")),
        other if tolerance_kind(other).is_some() && none("faces") => {
            Some(format!("the {other} tolerance on {name}"))
        }
        _ => None,
    }
}

impl Features<'_> {
    fn of(&mut self, faces: Option<&Json>, what: &str) -> Result<FeatureId, WriteError> {
        let Some(ids) = face_ids(faces) else {
            return invalid(format!("{what} names no faces"));
        };
        let f = Feature::Items(
            ids.into_iter()
                .map(|i| Anchor::Face(FaceIndex(i)))
                .collect(),
        );
        if let Some(i) = self.0.iter().position(|x| *x == f) {
            return Ok(FeatureId(i));
        }
        self.0.push(f);
        Ok(FeatureId(self.0.len() - 1))
    }
}

/// The geometric tolerance kinds of the intent (Python's `_GEOM_TYPES`).
fn tolerance_kind(kind: &str) -> Option<ToleranceKind> {
    Some(match kind {
        "position" => ToleranceKind::Position,
        "flatness" => ToleranceKind::Flatness,
        "perpendicularity" => ToleranceKind::Perpendicularity,
        "parallelism" => ToleranceKind::Parallelism,
        "profile" => ToleranceKind::SurfaceProfile,
        "runout" => ToleranceKind::CircularRunout,
        "total_runout" => ToleranceKind::TotalRunout,
        _ => return None,
    })
}

/// An ISO 286 class from its name (`H7`, `g6`, `js11`), refused when it is not one.
fn fit_class(name: &str) -> Result<Iso286Class, WriteError> {
    let refused = || WriteError::Invalid(format!("{name:?} is not an ISO 286 tolerance class"));
    let split = name
        .find(|c: char| c.is_ascii_digit())
        .ok_or_else(refused)?;
    Ok(Iso286Class {
        // find() returned the UTF-8 byte boundary of an ASCII digit, so both string slices are valid.
        deviation: FundamentalDeviation::parse(&name[..split]).map_err(|_| refused())?,
        grade: ToleranceGrade::parse(&name[split..]).map_err(|_| refused())?,
    })
}

/// Signed deviations `upper`/`lower` from nominal, refused unless `upper > lower`.
fn deviations(upper: f64, lower: f64, what: &str) -> Result<DimTolerance, WriteError> {
    let bounds = Bounds::new(mm(upper)?, mm(lower)?)
        .map_err(|e| WriteError::Invalid(format!("{what}: {}", e.0)))?;
    Ok(DimTolerance::Deviations(bounds))
}

/// The intent of one part as haecceity's model, and the report's lines for it. `before` is the
/// part's PMI as read: in add mode a datum it already has is its own (no "datum" line).
fn part_pmi(
    loaded: &LoadedPart,
    intent: &Intent,
    before: &PartPmi,
    mode: Mode,
) -> Result<(PartPmi, Lines), WriteError> {
    part_pmi_observed(loaded, intent, before, mode, &mut |_, _, _| {})
}

pub(crate) fn scene_model(
    loaded: &LoadedPart,
    intent: &Intent,
    before: &PartPmi,
    mode: Mode,
    observe: &mut impl FnMut(&str, usize, &Map<String, Json>),
) -> Result<PartPmi, WriteError> {
    part_pmi_observed(loaded, intent, before, mode, observe).map(|(p, _)| p)
}

fn part_pmi_observed(
    loaded: &LoadedPart,
    intent: &Intent,
    before: &PartPmi,
    mode: Mode,
    observe: &mut impl FnMut(&str, usize, &Map<String, Json>),
) -> Result<(PartPmi, Lines), WriteError> {
    let mut p = PartPmi::default();
    let origins = crate::pmi_origin::Planner::new(loaded).map_err(WriteError::Failed)?;
    let mut lines = Lines::default();
    let mut features = Vec::new();
    let mut f = Features(&mut features);

    // Datums, each established by its feature; a tolerance names them by letter.
    let had: BTreeSet<&str> = if mode == Mode::Add {
        before.datums.iter().map(|d| d.label().as_str()).collect()
    } else {
        BTreeSet::new()
    };
    let mut letters: Vec<String> = Vec::new();
    for d in &intent.datums {
        let letter = d
            .get("letter")
            .and_then(Json::as_str)
            .ok_or_else(|| WriteError::Invalid("a datum of the intent has no letter".into()))?;
        let feature = f.of(d.get("faces"), &format!("datum {letter}"))?;
        let label = DatumLabel::new(letter)
            .map_err(|e| WriteError::Invalid(format!("datum {letter}: {}", e.0)))?;
        let datum =
            Datum::new(label, Some(feature), Vec::new()).map_err(|e| WriteError::Invalid(e.0))?;
        observe("datums", p.datums.len(), d);
        p.datums.push(datum);
        letters.push(letter.to_string());
        if !had.contains(letter) {
            lines.written.push(format!("datum {letter}"));
        }
    }
    // The datums a tolerance written cites (one on no faces is not written).
    let referenced: BTreeSet<String> = intent
        .requirements
        .iter()
        .filter(|r| faceless(r.get("kind").and_then(Json::as_str).unwrap_or(""), r).is_none())
        .filter_map(|r| r.get("datums").and_then(Json::as_array))
        .flatten()
        .filter_map(|l| l.as_str().map(str::to_string))
        .collect();
    let system = |req: &Map<String, Json>| -> Result<Option<DatumSystem>, WriteError> {
        let Some(list) = req.get("datums").and_then(Json::as_array) else {
            return Ok(None);
        };
        if list.is_empty() {
            return Ok(None);
        }
        let mut compartments = Vec::new();
        for l in list {
            let at = l
                .as_str()
                .and_then(|l| letters.iter().position(|x| x == l))
                .ok_or_else(|| {
                    WriteError::Invalid(format!(
                        "a tolerance cites datum {}, which the intent does not define",
                        py_str(l)
                    ))
                })?;
            compartments.push(Compartment {
                references: vec![DatumReference {
                    datum: pmi::DatumId(at),
                    modifiers: Vec::new(),
                }],
                modifiers: Vec::new(),
            });
        }
        DatumSystem::new(compartments)
            .map(Some)
            .map_err(|e| WriteError::Invalid(e.0))
    };

    // The dimensions the general tolerance covers (`general.rs`) follow the requirements, each
    // written with its nominal alone: the general tolerance written for the part governs it, as
    // on a drawing, so the file states every size and hole location, not only those specified
    // (#63). They are counted in one line of the report, not listed. A part with PMI from
    // elsewhere (`Mode::Add`) gets none: its own dimensions are the ones its designer chose, and
    // these would repeat them, on faces its PMI already uses.
    let mut general = 0usize;
    let covered: &[Map<String, Json>] = if mode == Mode::Replace {
        &intent.general
    } else {
        &[]
    };
    for req in intent.requirements.iter().chain(covered) {
        let kind = req.get("kind").and_then(Json::as_str).unwrap_or("");
        let under_general = truthy(req.get("general"));
        // Not on a datum's own faces: a size there makes the datum a datum feature of size
        // (dimensional_size_with_datum_feature), a complex instance the authorship contract
        // does not mark (docs/pmi-provenance.md), so a resumed write would not keep its bytes.
        // The general tolerance still covers it.
        if under_general
            && kind == "size"
            && intent.datums.iter().any(|d| {
                face_ids(d.get("faces")).is_some()
                    && face_ids(d.get("faces")) == face_ids(req.get("faces"))
            })
        {
            continue;
        }
        let feature_name = || py_str(req.get("feature").unwrap_or(&Json::Null));
        // A requirement on no faces (a hole whose bores the rules keep no cylinder for) is
        // listed and the rest written (question Q17).
        if let Some(what) = faceless(kind, req) {
            // One the general tolerance covers is not a requirement: nothing to report.
            if under_general {
                continue;
            }
            lines.not_written.push(format!(
                "{what} names no faces, so nothing in the file can carry it (question Q17)"
            ));
            continue;
        }
        match kind {
            "size" => {
                let feature = f.of(req.get("faces"), &format!("the size on {}", feature_name()))?;
                let nominal = number(req, "nominal")?;
                let fit = req.get("fit");
                let tolerance = match fit {
                    _ if under_general => DimTolerance::None,
                    Some(Json::String(name)) if !name.is_empty() => {
                        // The class alone is written, so it must mean what the intent's
                        // deviations say: an intent whose limits are not its class's is refused
                        // rather than silently changed.
                        let what = format!("the {name} size on {}", feature_name());
                        let (upper, lower) = (number(req, "upper")?, number(req, "lower")?);
                        let (u, l) = crate::standards::fit_limits(name, nominal)
                            .map_err(|e| WriteError::Invalid(format!("{what}: {e}")))?;
                        if (u - upper).abs() > 1e-9 || (l - lower).abs() > 1e-9 {
                            return Err(WriteError::Invalid(format!(
                                "{what}: its limits {} / {} are not the class's {} / {}",
                                pyfmt::repr(upper),
                                pyfmt::repr(lower),
                                pyfmt::repr(u),
                                pyfmt::repr(l)
                            )));
                        }
                        DimTolerance::Fit {
                            class: fit_class(name)?,
                            limits: None,
                        }
                    }
                    _ => deviations(
                        number(req, "upper")?,
                        number(req, "lower")?,
                        &format!("size on {}", feature_name()),
                    )?,
                };
                observe("dimensions", p.dimensions.len(), req);
                p.dimensions.push(Dimension {
                    kind: DimensionKind::Size {
                        feature,
                        kind: SizeKind::Diameter,
                        path: None,
                        angle: None,
                    },
                    nominal: Some(mm(nominal)?),
                    tolerance,
                    qualifier: None,
                    modifiers: Vec::new(),
                    principle: None,
                });
                origins.dimension(&mut p, req, under_general);
                if under_general {
                    general += 1;
                } else {
                    lines.written.push(format!(
                        "Ø{} {} on {}",
                        pyfmt::g(nominal),
                        fit.map_or(String::new(), py_str),
                        feature_name()
                    ));
                }
            }
            "location" => {
                let to = f.of(
                    req.get("faces"),
                    &format!("the location of {}", feature_name()),
                )?;
                let from = f.of(
                    req.get("reference"),
                    &format!("the reference of the location of {}", feature_name()),
                )?;
                let distance = number(req, "distance")?;
                let basic = truthy(req.get("basic"));
                let (tolerance, how) = if basic {
                    (DimTolerance::Basic, "basic".to_string())
                } else if under_general {
                    (DimTolerance::None, "general".to_string())
                } else {
                    let (upper, lower) = (number(req, "upper")?, number(req, "lower")?);
                    let how = if upper == -lower {
                        format!("±{}", pyfmt::g(upper))
                    } else {
                        format!("+{}/{}", pyfmt::g(upper), pyfmt::signed_g(lower))
                    };
                    let what = format!("location to {}", feature_name());
                    (deviations(upper, lower, &what)?, how)
                };
                observe("dimensions", p.dimensions.len(), req);
                p.dimensions.push(Dimension {
                    kind: DimensionKind::Location {
                        from,
                        to,
                        kind: LocationKind::LinearDistance,
                        path: None,
                        directed: false,
                        angle: None,
                    },
                    nominal: Some(mm(distance)?),
                    tolerance,
                    qualifier: None,
                    modifiers: Vec::new(),
                    principle: None,
                });
                origins.dimension(&mut p, req, under_general);
                if under_general {
                    general += 1;
                } else {
                    lines.written.push(format!(
                        "location {} {how} to {} from faces {}",
                        pyfmt::g(distance),
                        feature_name(),
                        face_list(req.get("reference"))
                    ));
                }
            }
            "thread" => {
                let feature = f.of(
                    req.get("faces"),
                    &format!("the thread on {}", feature_name()),
                )?;
                let (pmi_thread, attributes) = thread(req, feature)?;
                observe("threads", p.threads.len(), req);
                p.threads.push(pmi_thread);
                p.attributes.push(attributes);
                lines.written.push(format!(
                    "{} thread {}-{} on {}",
                    text(req, "side")?,
                    text(req, "spec")?,
                    text(req, "class")?,
                    feature_name()
                ));
            }
            "knurl" => {
                let pattern = text(req, "pattern")?;
                match knurl(req, pattern, &feature_name())? {
                    Err(why) => lines.not_written.push(format!(
                        "{pattern} knurl on {}: {why}, so AP242's turned_knurl cannot be \
                         completed (it needs at least three of the number of teeth, tooth \
                         depth, root fillet, helix angle and hand, WR2); state them in the \
                         intent (questions Q4, Q15)",
                        feature_name()
                    )),
                    Ok((mut k, filled)) => {
                        k.feature = f.of(
                            req.get("faces"),
                            &format!("the knurl on {}", feature_name()),
                        )?;
                        observe("knurls", p.knurls.len(), req);
                        p.knurls.push(k);
                        lines.warnings.extend(filled);
                        lines
                            .written
                            .push(format!("{pattern} knurl on {}", feature_name()));
                    }
                }
            }
            "finish" => {
                let value = py_str(req.get("value").unwrap_or(&Json::Null));
                let faces = face_list(req.get("faces"));
                match texture(&value) {
                    Some(parameter) if face_ids(req.get("faces")).is_some() => {
                        let on = f.of(req.get("faces"), &format!("the surface finish {value}"))?;
                        observe("surface_textures", p.surface_textures.len(), req);
                        p.surface_textures.push(SurfaceTexture {
                            on: Some(on),
                            material_removal: MaterialRemoval::AnyProcessAllowed,
                            parameters: vec![parameter],
                        });
                        lines
                            .written
                            .push(format!("surface finish {value} on faces {faces}"));
                    }
                    Some(_) => lines.not_written.push(format!(
                        "surface finish {value} on faces {faces} names no faces, so nothing in \
                         the file can carry it (question Q17)"
                    )),
                    None => lines.not_written.push(format!(
                        "surface finish {value} on faces {faces}: not a roughness as \
                         'Ra <micrometres>', the only surface texture parameter written"
                    )),
                }
            }
            other => {
                let Some(tolerance_kind) = tolerance_kind(other) else {
                    return invalid(format!(
                        "unknown requirement kind {}",
                        pyfmt::repr_str(other)
                    ));
                };
                let target = f.of(
                    req.get("faces"),
                    &format!("the {other} tolerance on {}", feature_name()),
                )?;
                let value = number(req, "tolerance")?;
                let mmc = truthy(req.get("mmc"));
                observe("tolerances", p.tolerances.len(), req);
                p.tolerances.push(GeometricTolerance {
                    kind: tolerance_kind,
                    target: ToleranceTarget::Feature(target),
                    magnitude: Some(mm(value)?),
                    zone: truthy(req.get("diametral")).then_some(Zone {
                        form: ZoneForm::CylindricalOrCircular,
                        projected: None,
                        non_uniform: None,
                        runout_angle: None,
                        affected_plane: None,
                        affected_plane_kind: None,
                    }),
                    modifiers: if mmc {
                        vec![ToleranceModifier::MaximumMaterialRequirement]
                    } else {
                        Vec::new()
                    },
                    unit_basis: None,
                    maximum: None,
                    unequal: None,
                    datums: system(req)?,
                    auxiliary: Vec::new(),
                    description: Some(PMI_PROVENANCE.into()),
                });
                let on = if truthy(req.get("feature")) {
                    feature_name()
                } else {
                    face_list(req.get("faces"))
                };
                lines.written.push(format!(
                    "{other} {}{} on {on}",
                    pyfmt::g(value),
                    if mmc { " MMC" } else { "" }
                ));
            }
        }
    }
    if general > 0 {
        lines.written.push(format!(
            "{general} dimension{} under the general tolerance",
            if general == 1 { "" } else { "s" }
        ));
    }
    for d in &intent.datums {
        let letter = d.get("letter").and_then(Json::as_str).unwrap_or("");
        if !had.contains(letter) && !referenced.contains(letter) {
            lines.warnings.push(format!(
                "datum {letter} is in the file but no tolerance references it; OCCT-based \
                 readers (including Draftwright) will not see it"
            ));
        }
    }
    // The material and the general tolerance are one each per part. In add mode a part that
    // already states one keeps it: the same again is not written twice (a warning says so),
    // another is refused, as add cannot replace it (Python adds a second; question Q18). (The
    // rules do not ask for what a part states, but a part of an assembly reports no settings,
    // as in Python.)
    let stated = |what: &str, old: Option<&str>, new: &str, lines: &mut Lines| match old {
        Some(old) if old == new => {
            lines.warnings.push(format!(
                "{what} {new} is already stated in the file; not written again"
            ));
            Ok(false)
        }
        Some(old) => invalid(format!(
            "the part already states {what} {old}; adding {new} would give it two (it was \
             not written by specify-core, so it is added to, not replaced)"
        )),
        None => Ok(true),
    };
    let add = mode == Mode::Add;
    if let Some(material) = intent.part.get("material").filter(|v| truthy(Some(v))) {
        let name = py_str(material);
        let old = before
            .material
            .as_ref()
            .map(|m| m.id.as_str())
            .filter(|_| add);
        if stated("material", old, &name, &mut lines)? {
            p.material = Some(Material {
                id: name.clone(),
                name: None,
                density: None,
            });
            lines.written.push(format!("material {name}"));
        }
    }
    if let Some(general) = intent
        .part
        .get("general_tolerance")
        .filter(|v| truthy(Some(v)))
    {
        let text = py_str(general);
        let old = before
            .general
            .iter()
            .find_map(|g| match g {
                GeneralTolerance::Class { text, .. } => Some(text.as_str()),
                GeneralTolerance::Table { .. } => None,
            })
            .filter(|_| add);
        if stated("general tolerance", old, &text, &mut lines)? {
            p.general.push(GeneralTolerance::Class {
                standard: pmi::standards::Iso2768::recognise(&text),
                text: text.clone(),
            });
            lines.written.push(format!("general tolerance {text}"));
        }
    }
    // The part's notes in standard forms (question Q3, decided 2026-10-09): its default finish
    // as a surface texture on the part (ISO 10303-1110), the rest as the PMI practice's
    // 'semantic text' on the part (§7.4), one attribute set per note. Its text item is named by
    // the kind Python's text route gives the note ('surface treatment', 'heat treatment',
    // 'edge condition'), so specify-core's `part_settings`, which scans for an item of that
    // name, reads it too.
    for key in crate::rules::NOTES {
        if key == "notes" {
            if let Some(value) = intent.part.get(key) {
                let notes = crate::rules::note_strings(value)
                    .map_err(|e| WriteError::Invalid(e.to_string()))?;
                if add && !notes.is_empty() {
                    let old = crate::existing::general_notes(before);
                    if !old.is_empty() {
                        if notes.iter().copied().eq(old.iter().map(String::as_str)) {
                            lines.warnings.push(
                                "general notes are already stated in the file; not written again"
                                    .into(),
                            );
                            continue;
                        }
                        return invalid(
                            "the part already states general notes; adding a different list \
                             would give it conflicting note indices (it was not written by \
                             specify-core, so it is added to, not replaced)",
                        );
                    }
                }
                for (i, words) in notes.into_iter().enumerate() {
                    p.attributes.push(AttributeSet {
                        description: None,
                        general_property: None,
                        name: SEMANTIC_TEXT.into(),
                        on: None,
                        items: vec![
                            ("general note".into(), AttributeValue::Text(words.into())),
                            (NOTE_INDEX.into(), AttributeValue::Integer((i + 1) as i64)),
                        ],
                    });
                    lines
                        .written
                        .push(format!("general note {} {words}", i + 1));
                }
            }
            continue;
        }
        let Some(value) = intent.part.get(key).filter(|v| truthy(Some(v))) else {
            continue;
        };
        let (name, words) = (key.replace('_', " "), py_str(value));
        if key == "surface_finish" {
            let Some(parameter) = texture(&words) else {
                lines.not_written.push(format!(
                    "{name} {words}: not a roughness as 'Ra <micrometres>', the only surface \
                     texture parameter written"
                ));
                continue;
            };
            p.surface_textures.push(SurfaceTexture {
                on: None,
                material_removal: MaterialRemoval::AnyProcessAllowed,
                parameters: vec![parameter],
            });
        } else {
            let kind = crate::existing::PART_NOTES
                .iter()
                .find(|(k, _)| *k == key)
                .map_or(name.as_str(), |(_, kind)| kind);
            p.attributes.push(AttributeSet {
                description: None,
                general_property: None,
                name: SEMANTIC_TEXT.into(),
                on: None,
                items: vec![(kind.to_string(), AttributeValue::Text(words.clone()))],
            });
        }
        lines.written.push(format!("{name} {words}"));
    }
    p.features = features;
    Ok((p, lines))
}

/// A thread of the intent as haecceity's `Thread`, with what is not thread semantics (pitch,
/// tapping drill, depths, through) as a user defined attribute set on its feature. haecceity's
/// own mapping of specify-core's threads (its `pmi_write.rs` test): one thread ('number of
/// threads' 1), form "M", the designation as the qualifier, the major diameter the nominal
/// (upstream need U8: what 'number of threads' counts and where the pitch goes are open). The
/// attribute names are Python's UDA names; 'through' is the string "true" as Python writes it
/// (haecceity refuses booleans in attribute sets: its reader cannot read them back).
fn thread(
    req: &Map<String, Json>,
    feature: FeatureId,
) -> Result<(Thread, AttributeSet), WriteError> {
    let side = text(req, "side")?;
    let internal = match side {
        "internal" => true,
        "external" => false,
        other => {
            return invalid(format!(
                "thread side {} is not internal or external",
                pyfmt::repr_str(other)
            ));
        }
    };
    let major = match req.get("nominal") {
        Some(Json::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        _ => {
            let designation = text(req, "designation")?;
            designation
                .strip_prefix('M')
                .and_then(pyfmt::parse_float)
                .ok_or_else(|| {
                    WriteError::Invalid(format!(
                        "thread designation {} is not a metric size",
                        pyfmt::repr_str(designation)
                    ))
                })?
        }
    };
    let major = length(major)?;
    let thread = Thread {
        feature,
        partial_area: Some(feature),
        side: if internal {
            ThreadSide::Internal
        } else {
            ThreadSide::External
        },
        major_diameter: major.clone(),
        minor_diameter: None,
        pitch_diameter: None,
        // The literal 1. is a valid Decimal, independent of the STEP input.
        number_of_threads: Ratio(Decimal::parse("1.").expect("a decimal")),
        form: "M".into(),
        fit_class: text(req, "class")?.into(),
        fit_class_2: None,
        hand: if req.get("hand").and_then(Json::as_str) == Some("LH") {
            Hand::Left
        } else {
            Hand::Right
        },
        crest: None,
        qualifier: Some(text(req, "spec")?.into()),
        nominal_size: Some(major),
        runout: None,
    };
    let mut items = vec![(
        "pitch".to_string(),
        AttributeValue::Measure(mm(number(req, "pitch")?)?),
    )];
    let mut measure = |key: &str, name: &str| -> Result<(), WriteError> {
        items.push((name.into(), AttributeValue::Measure(mm(number(req, key)?)?)));
        Ok(())
    };
    if internal {
        measure("drill_diameter", "tapping drill diameter")?;
        if truthy(req.get("through")) {
            items.push(("through".into(), AttributeValue::Text("true".into())));
        } else {
            measure("drill_depth", "tapping drill depth")?;
            measure("full_thread", "minimum full thread")?;
        }
    } else if req.get("length").is_some_and(|v| !v.is_null()) {
        measure("length", "thread length")?;
    }
    Ok((
        thread,
        AttributeSet {
            description: None,
            general_property: None,
            name: "thread manufacturing".into(),
            on: Some(NoteOwner::Feature(feature)),
            items,
        },
    ))
}

/// A knurl of the intent as haecceity's `Knurl` (its feature left for the caller to set), with a
/// warning for each value filled in, or why it cannot be written. AP242's `turned_knurl`
/// states six to nine parameters (WR2): the major and nominal diameters, the diametral pitch,
/// and at least three of the number of teeth, tooth depth, root fillet, helix angle and hand;
/// a diamond knurl needs the helix angle (WR9), a diagonal one the hand too (WR10). The
/// intent's diameter is the finished knurl's (DIN 82's nominal diameter d1), so it is both the
/// major and the nominal diameter; its pitch is the diametral pitch. The rest is the intent's
/// where it states them (editable), else derived from the knurling standard as the rules derive
/// them (`standards::knurl_parameters`), for an intent made without them (Python's, or one
/// stored before; questions Q4, Q15), and the report's warnings say so. A diamond knurl states
/// no hand: it is two crossed helices.
fn knurl(
    req: &Map<String, Json>,
    pattern: &str,
    name: &str,
) -> Result<Result<(pmi::Knurl, Vec<String>), String>, WriteError> {
    let diamond = match pattern {
        "straight" => false,
        "diamond" => true,
        other => {
            return Ok(Err(format!(
                "the pattern {} is not straight or diamond",
                pyfmt::repr_str(other)
            )));
        }
    };
    let (pitch, diameter) = (number(req, "pitch")?, number(req, "diameter")?);
    let stated = |key: &str| req.get(key).filter(|v| !v.is_null());
    let mut teeth = match stated("teeth") {
        None => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|t| u32::try_from(t).ok())
                .filter(|t| *t > 0)
                .ok_or_else(|| {
                    WriteError::Invalid(format!(
                        "the knurl on {name}: teeth {} is not a whole number of teeth",
                        py_str(v)
                    ))
                })?,
        ),
    };
    let optional = |key: &str| match stated(key) {
        None => Ok(None),
        Some(_) => number(req, key).map(Some),
    };
    let mut tooth_depth = optional("tooth_depth")?;
    let mut root_fillet = optional("root_fillet")?;
    let mut helix_angle = optional("helix_angle")?;
    let mut missing = Vec::new();
    for (absent, what) in [
        (teeth.is_none(), "number of teeth"),
        (tooth_depth.is_none(), "tooth depth"),
        (root_fillet.is_none(), "root fillet"),
        (diamond && helix_angle.is_none(), "helix angle"),
    ] {
        if absent {
            missing.push(what);
        }
    }
    let mut warnings = Vec::new();
    if !missing.is_empty() {
        let k = match crate::standards::knurl_parameters(pattern, pitch, diameter) {
            Ok(k) => k,
            Err(why) => return Ok(Err(why)),
        };
        let mut filled = Vec::new();
        if teeth.is_none() {
            teeth = Some(k.teeth);
            filled.push(format!("{} teeth", k.teeth));
        }
        if tooth_depth.is_none() {
            tooth_depth = Some(k.tooth_depth);
            filled.push(format!("tooth depth {}", pyfmt::g(k.tooth_depth)));
        }
        if root_fillet.is_none() {
            root_fillet = Some(k.root_fillet);
            filled.push(format!("root fillet {}", pyfmt::g(k.root_fillet)));
        }
        if diamond && helix_angle.is_none() {
            helix_angle = k.helix_angle;
            filled.push(format!(
                "helix angle {}° and no hand",
                pyfmt::g(crate::standards::KNURL_HELIX_ANGLE)
            ));
        }
        warnings.push(format!(
            "{pattern} knurl on {name}: the intent states no {}; written with {} from {} \
             (questions Q4, Q15)",
            missing.join(", "),
            filled.join(", "),
            crate::standards::KNURL_STANDARD
        ));
    }
    let optional_length = |x: Option<f64>| x.map(length).transpose();
    let k = pmi::Knurl {
        feature: FeatureId(0),
        pattern: if diamond {
            pmi::KnurlPattern::Diamond
        } else {
            pmi::KnurlPattern::Straight
        },
        major_diameter: length(diameter)?,
        nominal_diameter: length(diameter)?,
        diametral_pitch: length(pitch)?,
        number_of_teeth: teeth.map(pmi::Count),
        tooth_depth: optional_length(tooth_depth)?,
        root_fillet: optional_length(root_fillet)?,
        helix_angle: helix_angle
            .map(|a| {
                Ok::<_, WriteError>(pmi::Angle {
                    value: decimal(a)?,
                    unit: pmi::AngleUnit::Degree,
                })
            })
            .transpose()?,
        helix_hand: None,
    };
    Ok(Ok((k, warnings)))
}
