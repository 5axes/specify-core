//! Semantic PMI already in the file, keyed by face index (port of `existing.py`).
//!
//! The interview must never ask for something the file already states, so each dimension,
//! geometric tolerance and datum is resolved to the faces it references. The PMI is
//! haecceity's ([`haecceity::pmi::read`]): it reads each part's PMI semantically, anchored to
//! the part's own face numbering, so PMI of other parts and of the assembly is never this
//! part's, and no text scan is needed for what OpenCascade loses.
//!
//! What differs from Python, by design:
//!
//! - **Values in millimetres, as stated.** Python reports a dimension as OpenCascade gives it,
//!   in the file's own unit (an inch NIST file's nominal stays in inches); here lengths are
//!   millimetres, the unit specify-core works in, and angles degrees (as Python reads them).
//!   Deviations keep their stated sign (Python's reader loses the lower deviation's sign and
//!   garbles limits below nominal).
//! - **One datum per letter and set of faces**, including datums no tolerance references
//!   (OpenCascade's reader builds datums only through the tolerances that cite them).
//! - **No OpenCascade artefacts.** Presentation read as a dimension (`DimensionPresentation`,
//!   specify-core's own thread callouts) is not reported; it states no size.
//!
//! The type names are OpenCascade's (`XCAFDimTolObjects_DimensionType` and
//! `XCAFDimTolObjects_GeomToleranceType` without their prefixes), because they are part of the
//! analysis JSON specify-core's rules read (`Size_*`, `Location*`, `Position`, ...).
//!
//! Notes: threads and knurls haecceity reads semantically (`thread`, `turned_knurl`), and the
//! notes specify-core (Python) writes by its own text route, which haecceity does not report as
//! anchored items (a `SHAPE_ASPECT` named for its kind and its `GEOMETRIC_ITEM_SPECIFIC_USAGE`s,
//! linked to nothing semantic): those are read from the Part 21 document as Python reads them
//! (upstream need U10, a documented stand-in).

use std::collections::BTreeSet;
use std::fmt;

use haecceity::p21::{Attribute, Document, RawEntity};
use haecceity::pmi::{
    self, Anchor, AngleUnit, DimTolerance, DimensionKind, Feature, FeatureId, Finding,
    GeneralTolerance, LengthUnit, LocationKind, PartPmi, Quantity, SizeKind, TargetShape,
    ToleranceKind, ToleranceTarget, Value as PmiValue,
};
use haecceity::step;
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::{Map, Value};

use crate::part::LoadedPart;
use crate::provenance;
use crate::pyfmt::round_half_even;

/// The notes specify-core writes on faces by its text route, by the name of their
/// `GEOMETRIC_ITEM_SPECIFIC_USAGE` (Python's `existing._NOTE`).
pub const NOTE_KINDS: [&str; 5] = [
    "internal thread",
    "external thread",
    "knurl",
    "surface finish",
    "surface texture",
];

/// The part's notes by the key `part_settings` reports them under: Python's four
/// `requirements.PART_NOTES`, plus ordered free-text `notes` (draftwright-rust #123).
pub const PART_NOTES: [(&str, &str); 5] = [
    ("surface_finish", "surface texture"),
    ("coating", "surface treatment"),
    ("heat_treatment", "heat treatment"),
    ("edges", "edge condition"),
    ("notes", "general note"),
];

/// One item of PMI the file states (Python's `ExistingPmi`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExistingPmi {
    /// `dimension`, `geometric_tolerance`, `datum` or `note`.
    pub kind: String,
    /// OpenCascade's type name without its prefix (`Size_Diameter`, `Position`), the datum's
    /// letter, or the note's kind.
    pub r#type: String,
    /// The faces it is on, ascending.
    pub faces: Vec<usize>,
    /// The second reference set of a location dimension.
    pub faces2: Vec<usize>,
    /// Nominal (mm or degrees) of a dimension, magnitude (mm) of a tolerance.
    pub value: Option<f64>,
    /// Signed deviations from nominal, mm (degrees for an angle).
    pub upper: Option<f64>,
    pub lower: Option<f64>,
    /// The datum letters a tolerance cites, primary first.
    pub datums: Vec<String>,
    /// STEP authorship token on this requirement; absent for unmarked input PMI.
    pub provenance: Option<String>,
    /// Stable inspection number read from this requirement's AP242 id, when present.
    pub number: Option<u64>,
    pub origin: Option<crate::pmi_origin::Origin>,
    pub source: Option<crate::pmi_origin::Source>,
}

impl ExistingPmi {
    fn new(kind: &str, r#type: &str, faces: Vec<usize>) -> ExistingPmi {
        ExistingPmi {
            kind: kind.into(),
            r#type: r#type.into(),
            faces,
            faces2: Vec::new(),
            value: None,
            upper: None,
            lower: None,
            datums: Vec::new(),
            provenance: None,
            number: None,
            origin: None,
            source: None,
        }
    }
}

/// As `ExistingPmi.to_dict`: empty and absent fields are left out, but `faces` is always there
/// (a datum the file attaches to no face still has faces: none), appended last when empty as
/// Python's `setdefault` does.
impl Serialize for ExistingPmi {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("type", &self.r#type)?;
        if !self.faces.is_empty() {
            map.serialize_entry("faces", &self.faces)?;
        }
        if !self.faces2.is_empty() {
            map.serialize_entry("faces2", &self.faces2)?;
        }
        for (key, value) in [
            ("value", self.value),
            ("upper", self.upper),
            ("lower", self.lower),
        ] {
            if let Some(value) = value {
                map.serialize_entry(key, &value)?;
            }
        }
        if !self.datums.is_empty() {
            map.serialize_entry("datums", &self.datums)?;
        }
        if let Some(provenance) = &self.provenance {
            map.serialize_entry("provenance", provenance)?;
        }
        if let Some(number) = self.number {
            map.serialize_entry("number", &number)?;
        }
        if let Some(origin) = &self.origin {
            map.serialize_entry("origin", origin)?;
        }
        if let Some(source) = &self.source {
            map.serialize_entry("source", source)?;
        }
        if self.faces.is_empty() {
            map.serialize_entry("faces", &self.faces)?;
        }
        map.end()
    }
}

/// Why a file's PMI could not be read.
#[derive(Clone, Debug, PartialEq)]
pub enum ExistingError {
    /// The file does not parse as Part 21, or its parts cannot be listed.
    Unreadable(String),
    /// haecceity's PMI reader refused the file.
    Pmi(String),
}

impl fmt::Display for ExistingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExistingError::Unreadable(why) => write!(f, "cannot read the file's PMI: {why}"),
            ExistingError::Pmi(why) => write!(f, "cannot read the file's PMI: {why}"),
        }
    }
}

impl std::error::Error for ExistingError {}

/// The part's PMI as haecceity reads it, the findings that concern it (its own and those of
/// no part), and the parsed document: the file is parsed and read once, and
/// [`Reading::existing`], [`Reading::settings`] and [`Reading::findings`] all answer from it
/// (the free functions below are each one reading and one answer).
pub struct Reading {
    doc: Document,
    pmi: PartPmi,
    provenance: pmi::PartProvenance,
    findings: Vec<Finding>,
}

/// Reads the part's PMI (see [`Reading`]).
pub fn read(loaded: &LoadedPart) -> Result<Reading, ExistingError> {
    let bytes = loaded.bytes.as_ref();
    let doc =
        Document::parse(bytes.clone()).map_err(|e| ExistingError::Unreadable(e.to_string()))?;
    // The reader takes every part of the file, in the order `binding.part` indexes.
    let parts =
        step::read_part_definitions(bytes).map_err(|e| ExistingError::Unreadable(e.to_string()))?;
    read_parsed(loaded, doc, &parts)
}

/// [`read`] from the file already parsed: `doc`, `loaded.bytes` as a Part 21 document, and
/// `parts`, every part of the file in the order `binding.part` indexes (a [`crate::part::File`]'s
/// definitions). Neither is checked against the bytes; a caller holding both saves parsing the
/// file and listing its parts again.
pub fn read_parsed(
    loaded: &LoadedPart,
    doc: Document,
    parts: &[step::PartDefinition],
) -> Result<Reading, ExistingError> {
    let mut read = pmi::read(&doc, parts).map_err(|e| ExistingError::Pmi(e.to_string()))?;
    let index = loaded.binding.part;
    if index >= read.parts.len() {
        return Err(ExistingError::Pmi(format!(
            "the reader has {} parts; there is no part {index}",
            read.parts.len()
        )));
    }
    let findings = read
        .findings
        .into_iter()
        .filter(|f| f.part.is_none_or(|p| p.0 == index))
        .collect();
    Ok(Reading {
        pmi: read.parts.swap_remove(index),
        provenance: read.provenance.parts.swap_remove(index),
        findings,
        doc,
    })
}

/// The semantic PMI the file states on the part (Python's `read_existing`): dimensions, then
/// geometric tolerances, then datums, then notes on faces, each group in haecceity's order.
pub fn read_existing(loaded: &LoadedPart) -> Result<Vec<ExistingPmi>, ExistingError> {
    read(loaded)?.existing(loaded)
}

/// What haecceity reported while reading the file that concerns this part (its findings and
/// those of no part): Python's OpenCascade reader reports nothing of the kind, so the sweeps
/// record each finding kind they meet.
pub fn findings(loaded: &LoadedPart) -> Result<Vec<Finding>, ExistingError> {
    Ok(read(loaded)?.findings)
}

/// The material and general tolerance the file states, and the part's notes (see
/// [`Reading::settings`]).
pub fn part_settings(loaded: &LoadedPart) -> Result<Map<String, Value>, ExistingError> {
    Ok(read(loaded)?.settings(loaded))
}

/// Authorship of the values returned by [`part_settings`], without changing their shape.
/// Scalar settings map to the token; `notes` maps to an aligned array of tokens or nulls.
/// Unmarked settings are omitted. See `docs/pmi-provenance.md`.
pub fn part_settings_provenance(loaded: &LoadedPart) -> Result<Map<String, Value>, ExistingError> {
    Ok(read(loaded)?.settings_provenance(loaded))
}

impl Reading {
    pub(crate) fn model(&self) -> &PartPmi {
        &self.pmi
    }

    /// The semantic PMI the file states on the part, as [`read_existing`]; `loaded` is the part
    /// this reading is of. A tolerance citing a datum the reader does not hold is refused, not
    /// listed without it.
    pub fn existing(&self, loaded: &LoadedPart) -> Result<Vec<ExistingPmi>, ExistingError> {
        let Reading {
            doc,
            pmi,
            provenance: prov,
            ..
        } = self;
        let mut out = Vec::new();

        for (i, dimension) in pmi.dimensions.iter().enumerate() {
            let (r#type, first, second) = match &dimension.kind {
                DimensionKind::Size {
                    feature,
                    kind,
                    path,
                    angle,
                } => (
                    size_type(kind, path.is_some(), angle.is_some()),
                    faces_of(pmi, &[*feature]),
                    Vec::new(),
                ),
                DimensionKind::Location {
                    from,
                    to,
                    kind,
                    path,
                    angle,
                    ..
                } => (
                    location_type(kind, path.is_some(), angle.is_some()),
                    faces_of(pmi, &[*from]),
                    faces_of(pmi, &[*to]),
                ),
            };
            let owned = provenance::dimension(doc, &prov.dimensions[i]);
            // One Specify wrote with no tolerance of its own is a dimension the general
            // tolerance covers (#63): it states nothing the geometry does not, so it settles no
            // question, read back without the answers it was written from.
            if owned && matches!(dimension.tolerance, DimTolerance::None) {
                continue;
            }
            let mut item = ExistingPmi::new("dimension", r#type, first);
            item.provenance = owned.then(|| provenance::MARKER.into());
            let metadata =
                crate::pmi_origin::read(pmi, pmi::NoteOwner::Dimension(pmi::DimensionId(i)));
            item.origin = metadata.origin;
            item.source = metadata.source;
            item.number = crate::characteristics::on(
                doc,
                &prov.dimensions[i],
                if matches!(dimension.kind, DimensionKind::Size { .. }) {
                    "dimensional_size"
                } else {
                    "dimensional_location"
                },
            );
            item.faces2 = second;
            let nominal = dimension.nominal.as_ref();
            item.value = nominal.map(value);
            // Deviations as stated; limits (and a fit's stated limits) as their offsets from
            // nominal, the form the analysis carries.
            let offsets = |upper: &PmiValue, lower: &PmiValue| match nominal {
                Some(n) => (
                    Some(round_half_even(value(upper) - value(n), 9)),
                    Some(round_half_even(value(lower) - value(n), 9)),
                ),
                None => (None, None),
            };
            (item.upper, item.lower) = match &dimension.tolerance {
                DimTolerance::Deviations(b) => (Some(value(b.upper())), Some(value(b.lower()))),
                DimTolerance::Limits(b) => offsets(b.upper(), b.lower()),
                DimTolerance::Fit {
                    limits: Some(b), ..
                } => offsets(b.upper(), b.lower()),
                DimTolerance::None
                | DimTolerance::Basic
                | DimTolerance::Fit { limits: None, .. } => (None, None),
            };
            out.push(item);
        }

        for (i, tolerance) in pmi.tolerances.iter().enumerate() {
            let faces = match &tolerance.target {
                ToleranceTarget::Feature(f) => faces_of(pmi, &[*f]),
                ToleranceTarget::Dimension(d) => match pmi.dimensions.get(d.0).map(|d| &d.kind) {
                    Some(DimensionKind::Size { feature, .. }) => faces_of(pmi, &[*feature]),
                    Some(DimensionKind::Location { from, to, .. }) => faces_of(pmi, &[*from, *to]),
                    None => Vec::new(),
                },
                ToleranceTarget::Relation {
                    relating, related, ..
                } => faces_of(pmi, &[*relating, *related]),
                ToleranceTarget::WholePart => Vec::new(),
            };
            let mut item =
                ExistingPmi::new("geometric_tolerance", tolerance_type(tolerance.kind), faces);
            item.provenance = (tolerance.description.as_deref() == Some(provenance::MARKER))
                .then(|| provenance::MARKER.into());
            item.number =
                crate::characteristics::on(doc, &prov.tolerances[i], "geometric_tolerance");
            item.value = tolerance.magnitude.as_ref().map(value);
            item.datums = tolerance
                .datums
                .iter()
                .flat_map(|system| system.compartments())
                .flat_map(|c| &c.references)
                .map(|r| {
                    pmi.datums
                        .get(r.datum.0)
                        .map(|d| d.label().as_str().to_string())
                        .ok_or_else(|| {
                            ExistingError::Pmi(format!(
                                "a {} tolerance cites datum {}, which the reader does not hold",
                                tolerance_type(tolerance.kind),
                                r.datum.0
                            ))
                        })
                })
                .collect::<Result<_, _>>()?;
            out.push(item);
        }

        let mut seen = Vec::new();
        for (i, datum) in pmi.datums.iter().enumerate() {
            // The datum feature's faces, and those of the features its targets are or lie on.
            let mut features: Vec<FeatureId> = datum.feature().into_iter().collect();
            for target in datum.targets() {
                if let Some(t) = pmi.datum_targets.get(target.0) {
                    if let TargetShape::Area(f) | TargetShape::Curve(f) = t.shape() {
                        features.push(*f);
                    }
                    features.extend(t.on());
                }
            }
            let owned = provenance::marked(doc, &prov.datums[i], "datum");
            let key = (
                datum.label().as_str().to_string(),
                faces_of(pmi, &features),
                owned,
            );
            if !seen.contains(&key) {
                let mut item = ExistingPmi::new("datum", &key.0, key.1.clone());
                item.provenance = owned.then(|| provenance::MARKER.into());
                item.number = crate::characteristics::on(doc, &prov.datums[i], "datum");
                out.push(item);
                seen.push(key);
            }
        }

        out.extend(notes(loaded, doc, pmi, prov));
        Ok(out)
    }

    /// Authorship alongside [`Self::settings`]; values and selection order are unchanged.
    pub fn settings_provenance(&self, loaded: &LoadedPart) -> Map<String, Value> {
        let mut out = Map::new();
        let p = &self.pmi;
        let prov = &self.provenance;
        let owned = |ids: &[u64], kind: &str| provenance::marked(&self.doc, ids, kind);
        let mut notes = Vec::new();
        for (i, set) in p.attributes.iter().enumerate() {
            if set.on.is_some() || set.name != crate::writer::SEMANTIC_TEXT {
                continue;
            }
            let index = set.items.iter().find_map(|(name, v)| match v {
                pmi::AttributeValue::Integer(n) if name == crate::writer::NOTE_INDEX && *n > 0 => {
                    Some(*n)
                }
                _ => None,
            });
            for (name, v) in &set.items {
                if name == "general note" && matches!(v, pmi::AttributeValue::Text(_)) {
                    notes.push((index, owned(&prov.attributes[i], "attribute")));
                }
            }
        }
        notes.sort_by_key(|(index, _)| (index.is_none(), *index));
        if notes.iter().any(|(_, owned)| *owned) {
            out.insert(
                "notes".into(),
                Value::Array(
                    notes
                        .into_iter()
                        .map(|(_, yes)| {
                            if yes {
                                Value::from(provenance::MARKER)
                            } else {
                                Value::Null
                            }
                        })
                        .collect(),
                ),
            );
        }
        if loaded.part_count > 1 {
            return out;
        }
        if p.material.is_some() && owned(&prov.material, "material") {
            out.insert("material".into(), Value::from(provenance::MARKER));
        }
        if let Some(i) = p
            .general
            .iter()
            .position(|g| matches!(g, GeneralTolerance::Class { .. }))
            && owned(&prov.general[i], "general")
        {
            out.insert("general_tolerance".into(), Value::from(provenance::MARKER));
        }
        // Legacy note routes have precedence, and carry no Specify marker.
        let mut selected = BTreeSet::new();
        for (key, kind) in PART_NOTES {
            if key == "notes" {
                continue;
            }
            if p.notes.iter().any(|n| {
                (n.kind == kind || (key == "surface_finish" && n.kind == "general surface texture"))
                    && (key != "surface_finish" || default_finish(&n.text).is_some())
            }) {
                selected.insert(key);
            }
        }
        for (i, texture) in p.surface_textures.iter().enumerate() {
            if texture.on.is_none()
                && finish_words(texture).is_some()
                && selected.insert("surface_finish")
                && owned(&prov.surface_textures[i], "surface texture")
            {
                out.insert("surface_finish".into(), Value::from(provenance::MARKER));
            }
        }
        for (i, set) in p.attributes.iter().enumerate() {
            if set.on.is_some() || set.name != crate::writer::SEMANTIC_TEXT {
                continue;
            }
            for (name, v) in &set.items {
                if !matches!(v, pmi::AttributeValue::Text(_)) {
                    continue;
                }
                if let Some((key, _)) = PART_NOTES.iter().find(|(key, kind)| {
                    *key != "surface_finish" && *key != "notes" && *kind == name
                }) && selected.insert(key)
                    && owned(&prov.attributes[i], "attribute")
                {
                    out.insert((*key).into(), Value::from(provenance::MARKER));
                }
            }
        }
        out
    }

    /// The findings of this reading (see [`findings`]).
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// The material and general tolerance the file states, and the part's notes (surface finish
    /// default, coating, heat treatment, edges, ordered free text). The four legacy settings
    /// stay empty for a part of an assembly, as Python's `part_settings`: its text scan cannot
    /// tell whose they are. Free-text notes are read by their semantic owner in all files.
    pub fn settings(&self, loaded: &LoadedPart) -> Map<String, Value> {
        let mut out = Map::new();
        // Unlike Python's legacy text scan, these are resolved to their owning part, also
        // in assemblies. Explicit indices carry order; unindexed notes follow in read order.
        let notes = general_notes(&self.pmi);
        if !notes.is_empty() {
            out.insert("notes".into(), Value::from(notes));
        }
        if let Some(text) = self.pmi.notes.iter().find_map(|note| {
            (note.on.is_none()
                && note.kind == crate::writer::SEMANTIC_TEXT
                && edge_break_note(&note.text))
            .then_some(&note.text)
        }) {
            // A foreign part-level edge instruction answers the question as written.
            // Feature notes cannot establish a default for every edge of the part.
            out.insert("edges".into(), Value::String(text.clone()));
        }
        if loaded.part_count > 1 {
            return out;
        }
        let pmi = &self.pmi;
        if let Some(material) = &pmi.material {
            out.insert("material".into(), Value::String(material.id.clone()));
        }
        if let Some(text) = pmi.general.iter().find_map(|g| match g {
            GeneralTolerance::Class { text, .. } => Some(text),
            GeneralTolerance::Table { .. } => None,
        }) {
            out.insert("general_tolerance".into(), Value::String(text.clone()));
        }
        for (key, kind) in PART_NOTES {
            if key == "notes" {
                continue; // Free-text part notes have only the semantic attribute form.
            }
            // The finish as specify-core writes it now, and as it was written before it took
            // Draftwright's form.
            let kinds: &[&str] = if key == "surface_finish" {
                &[kind, "general surface texture"]
            } else {
                &[kind]
            };
            for kind in kinds {
                for note in pmi.notes.iter().filter(|n| n.kind == *kind) {
                    let words = if key == "surface_finish" {
                        // Only the part's default; a finish on faces, written before it was
                        // 'surface finish', is not.
                        match default_finish(&note.text) {
                            Some(words) => words.to_string(),
                            None => continue,
                        }
                    } else {
                        note.text.clone()
                    };
                    out.entry(key).or_insert(Value::String(words));
                }
            }
        }
        // The same notes in the standard forms the Rust writer writes (question Q3): the
        // default finish as a surface texture on the part, the rest as 'semantic text' on it,
        // its item named by the note's kind.
        for t in pmi.surface_textures.iter().filter(|t| t.on.is_none()) {
            if let Some(words) = finish_words(t) {
                out.entry("surface_finish").or_insert(Value::String(words));
            }
        }
        for a in &pmi.attributes {
            if a.on.is_some() || a.name != crate::writer::SEMANTIC_TEXT {
                continue;
            }
            for (item, v) in &a.items {
                let key = PART_NOTES
                    .iter()
                    .find(|(key, kind)| *key != "surface_finish" && *key != "notes" && kind == item)
                    .map(|(key, _)| *key);
                if let (Some(key), pmi::AttributeValue::Text(words)) = (key, v) {
                    out.entry(key).or_insert(Value::String(words.clone()));
                }
            }
        }
        out
    }
}

/// Only explicit edge-breaking prose, not a general manufacturing-note classifier.
pub(crate) fn edge_break_note(text: &str) -> bool {
    let words: Vec<_> = text
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    words.iter().enumerate().any(|(i, word)| {
        if !matches!(word.as_str(), "break" | "breaking") {
            return false;
        }
        let rest = &words[i + 1..];
        let rest = rest.strip_prefix(&["all".into()]).unwrap_or(rest);
        let rest = rest.strip_prefix(&["sharp".into()]).unwrap_or(rest);
        rest.first()
            .is_some_and(|word| matches!(word.as_str(), "edge" | "edges"))
    })
}

/// Part-owned semantic text, PMI practice §7.4. Each note has its own set, a `general note`
/// text item and a one-based integer `note index`. Stable sorting keeps duplicate indices in
/// read order; missing/nonpositive indices follow indexed notes in read order. No legacy
/// note or drawing text is migrated into this list.
pub(crate) fn general_notes(pmi: &PartPmi) -> Vec<String> {
    let mut notes = Vec::new();
    for set in &pmi.attributes {
        if set.on.is_some() || set.name != crate::writer::SEMANTIC_TEXT {
            continue;
        }
        let index = set.items.iter().find_map(|(name, value)| match value {
            pmi::AttributeValue::Integer(i) if name == crate::writer::NOTE_INDEX && *i > 0 => {
                Some(*i)
            }
            _ => None,
        });
        for (name, value) in &set.items {
            if name == "general note"
                && let pmi::AttributeValue::Text(text) = value
            {
                notes.push((index, text.clone()));
            }
        }
    }
    notes.sort_by_key(|(index, _)| (index.is_none(), *index));
    notes.into_iter().map(|(_, text)| text).collect()
}

/// The notes on the part's faces (Python's `existing.notes`), one per kind with every face it
/// is on: specify-core's text route read from the document (in file order), then the threads
/// and knurls haecceity reads semantically.
fn notes(
    loaded: &LoadedPart,
    doc: &Document,
    pmi: &PartPmi,
    prov: &pmi::PartProvenance,
) -> Vec<ExistingPmi> {
    let mut found: Vec<(String, bool, Option<u64>, BTreeSet<usize>)> = Vec::new();
    let mut add = |kind: &str, faces: &[usize], owned: bool, number: Option<u64>| {
        if faces.is_empty() {
            return;
        }
        match found
            .iter_mut()
            .find(|(k, o, n, _)| k == kind && *o == owned && *n == number)
        {
            Some((_, _, _, set)) => set.extend(faces),
            None => found.push((
                kind.to_string(),
                owned,
                number,
                faces.iter().copied().collect(),
            )),
        }
    };
    let graph = doc.graph();
    for entity in graph.entities.values() {
        let RawEntity::Simple {
            name, attributes, ..
        } = entity
        else {
            continue;
        };
        if name != "GEOMETRIC_ITEM_SPECIFIC_USAGE" {
            continue;
        }
        let (Some(Attribute::String(kind)), Some(Attribute::EntityRef(face))) =
            (attributes.first(), attributes.get(4))
        else {
            continue;
        };
        if NOTE_KINDS.contains(&kind.as_str())
            && let Some(index) = loaded.definition.face_index(*face)
        {
            add(kind, &[index], false, None);
        }
    }
    for (i, thread) in pmi.threads.iter().enumerate() {
        let kind = match thread.side {
            pmi::ThreadSide::Internal => "internal thread",
            pmi::ThreadSide::External => "external thread",
        };
        add(
            kind,
            &faces_of(pmi, &[thread.feature]),
            provenance::marked(doc, &prov.threads[i], "thread"),
            crate::characteristics::on(doc, &prov.threads[i], "product_definition_shape"),
        );
    }
    for (i, knurl) in pmi.knurls.iter().enumerate() {
        add(
            "knurl",
            &faces_of(pmi, &[knurl.feature]),
            provenance::marked(doc, &prov.knurls[i], "knurl"),
            crate::characteristics::on(doc, &prov.knurls[i], "product_definition_shape"),
        );
    }
    // A finish on faces as the Rust writer writes it: a surface texture on them (question Q3).
    for (i, t) in pmi.surface_textures.iter().enumerate() {
        if let Some(f) = t.on {
            add(
                "surface finish",
                &faces_of(pmi, &[f]),
                provenance::marked(doc, &prov.surface_textures[i], "surface texture"),
                crate::characteristics::texture_number(doc, &prov.surface_textures[i]),
            );
        }
    }
    found
        .into_iter()
        .map(|(kind, owned, number, faces)| {
            let mut item = ExistingPmi::new("note", &kind, faces.into_iter().collect());
            item.provenance = owned.then(|| provenance::MARKER.into());
            item.number = number;
            item
        })
        .collect()
}

/// A surface texture of one roughness in micrometres as specify-core states a finish
/// (`Ra 3.2`); `None` for any other.
fn finish_words(t: &pmi::SurfaceTexture) -> Option<String> {
    let [p] = t.parameters.as_slice() else {
        return None;
    };
    (p.value.unit == LengthUnit::Micrometre).then(|| {
        format!(
            "{} {}",
            p.characteristic,
            crate::pyfmt::g(p.value.value.to_f64())
        )
    })
}

/// `X` of "X µm unless otherwise specified" (or "stated"), the part's default finish (Python's
/// `_DEFAULT_FINISH.fullmatch`: the whole text must end so, so `X` is all before the ending).
fn default_finish(text: &str) -> Option<&str> {
    [
        " µm unless otherwise specified",
        " µm unless otherwise stated",
    ]
    .iter()
    .find_map(|tail| text.strip_suffix(tail))
}

/// The faces of `features`, following groups and derived features to what they are made of
/// (as Python's `datum_faces` follows shape aspect relationships), ascending.
pub(crate) fn faces_of(pmi: &PartPmi, features: &[FeatureId]) -> Vec<usize> {
    faces_in_arena(&pmi.features, features)
}

/// The same support resolution while the writer still owns the feature arena.
pub(crate) fn faces_in_arena(arena: &[Feature], features: &[FeatureId]) -> Vec<usize> {
    fn walk(
        arena: &[Feature],
        f: FeatureId,
        seen: &mut BTreeSet<usize>,
        out: &mut BTreeSet<usize>,
    ) {
        if !seen.insert(f.0) {
            return;
        }
        match arena.get(f.0) {
            Some(Feature::Items(items)) => out.extend(items.iter().filter_map(|a| match a {
                Anchor::Face(face) => Some(face.0),
                Anchor::Edge(_) | Anchor::Geometry(_) => None,
            })),
            Some(Feature::Group { members, .. }) => {
                for m in members {
                    walk(arena, *m, seen, out);
                }
            }
            Some(Feature::Derived { from, .. }) => {
                for m in from {
                    walk(arena, *m, seen, out);
                }
            }
            None => {}
        }
    }
    let (mut seen, mut out) = (BTreeSet::new(), BTreeSet::new());
    for f in features {
        walk(arena, *f, &mut seen, &mut out);
    }
    out.into_iter().collect()
}

/// A value in millimetres or degrees. A value stated in that unit is its decimal as written;
/// anything else is converted.
pub(crate) fn value(v: &PmiValue) -> f64 {
    match &v.quantity {
        Quantity::Length(l) if l.unit == LengthUnit::Millimetre => l.value.to_f64(),
        Quantity::Length(l) => l.mm(),
        Quantity::Angle(a) if a.unit == AngleUnit::Degree => a.value.to_f64(),
        Quantity::Angle(a) => a.rad().to_degrees(),
    }
}

/// OpenCascade's names for the size kinds (`STEPCAFControl_GDTProperty::GetDimType`).
const SIZE_TYPES: [(SizeKind, &str); 14] = [
    (SizeKind::CurveLength, "Size_CurveLength"),
    (SizeKind::Diameter, "Size_Diameter"),
    (SizeKind::SphericalDiameter, "Size_SphericalDiameter"),
    (SizeKind::Radius, "Size_Radius"),
    (SizeKind::SphericalRadius, "Size_SphericalRadius"),
    (
        SizeKind::ToroidalMinorDiameter,
        "Size_ToroidalMinorDiameter",
    ),
    (
        SizeKind::ToroidalMajorDiameter,
        "Size_ToroidalMajorDiameter",
    ),
    (SizeKind::ToroidalMinorRadius, "Size_ToroidalMinorRadius"),
    (SizeKind::ToroidalMajorRadius, "Size_ToroidalMajorRadius"),
    (
        SizeKind::ToroidalHighMajorDiameter,
        "Size_ToroidalHighMajorDiameter",
    ),
    (
        SizeKind::ToroidalLowMajorDiameter,
        "Size_ToroidalLowMajorDiameter",
    ),
    (
        SizeKind::ToroidalHighMajorRadius,
        "Size_ToroidalHighMajorRadius",
    ),
    (
        SizeKind::ToroidalLowMajorRadius,
        "Size_ToroidalLowMajorRadius",
    ),
    (SizeKind::Thickness, "Size_Thickness"),
];

/// OpenCascade's names for the location kinds. Its table has no 'linear distance centre',
/// 'inner' or 'outer' (PMI practice Table 2's one-sided forms): those, like any name outside
/// its table, are `Location_None`, its default type.
const LOCATION_TYPES: [(LocationKind, &str); 10] = [
    (LocationKind::CurvedDistance, "Location_CurvedDistance"),
    (LocationKind::LinearDistance, "Location_LinearDistance"),
    (
        LocationKind::LinearDistanceCentreOuter,
        "Location_LinearDistance_FromCenterToOuter",
    ),
    (
        LocationKind::LinearDistanceCentreInner,
        "Location_LinearDistance_FromCenterToInner",
    ),
    (
        LocationKind::LinearDistanceOuterCentre,
        "Location_LinearDistance_FromOuterToCenter",
    ),
    (
        LocationKind::LinearDistanceOuterOuter,
        "Location_LinearDistance_FromOuterToOuter",
    ),
    (
        LocationKind::LinearDistanceOuterInner,
        "Location_LinearDistance_FromOuterToInner",
    ),
    (
        LocationKind::LinearDistanceInnerCentre,
        "Location_LinearDistance_FromInnerToCenter",
    ),
    (
        LocationKind::LinearDistanceInnerOuter,
        "Location_LinearDistance_FromInnerToOuter",
    ),
    (
        LocationKind::LinearDistanceInnerInner,
        "Location_LinearDistance_FromInnerToInner",
    ),
];

/// The type name OpenCascade gives a dimensional (or angular) size: angular first, then the
/// name's kind, then a size with a path; otherwise its default, `Location_None`.
pub(crate) fn size_type(kind: &SizeKind, path: bool, angular: bool) -> &'static str {
    if angular {
        return "Size_Angular";
    }
    SIZE_TYPES
        .iter()
        .find(|(k, _)| k == kind)
        .map(|(_, name)| *name)
        .unwrap_or(if path {
            "Size_WithPath"
        } else {
            "Location_None"
        })
}

/// The type name OpenCascade gives a dimensional (or angular) location, as [`size_type`].
pub(crate) fn location_type(kind: &LocationKind, path: bool, angular: bool) -> &'static str {
    if angular {
        return "Location_Angular";
    }
    LOCATION_TYPES
        .iter()
        .find(|(k, _)| k == kind)
        .map(|(_, name)| *name)
        .unwrap_or(if path {
            "Location_WithPath"
        } else {
            "Location_None"
        })
}

/// OpenCascade's name for a tolerance kind (`XCAFDimTolObjects_GeomToleranceType`).
pub fn tolerance_type(kind: ToleranceKind) -> &'static str {
    use ToleranceKind as K;
    match kind {
        K::Angularity => "Angularity",
        K::CircularRunout => "CircularRunout",
        K::Coaxiality => "Coaxiality",
        K::Concentricity => "Concentricity",
        K::Cylindricity => "Cylindricity",
        K::Flatness => "Flatness",
        K::LineProfile => "ProfileOfLine",
        K::Parallelism => "Parallelism",
        K::Perpendicularity => "Perpendicularity",
        K::Position => "Position",
        K::Roundness => "CircularityOrRoundness",
        K::Straightness => "Straightness",
        K::SurfaceProfile => "ProfileOfSurface",
        K::Symmetry => "Symmetry",
        K::TotalRunout => "TotalRunout",
    }
}

impl Reading {
    /// BOM metadata from this already parsed/read part; no extra STEP or PMI read.
    pub(crate) fn bom(&self, loaded: &LoadedPart) -> Result<crate::bom::Item, String> {
        crate::bom::read_rows(
            &self.doc,
            std::slice::from_ref(&loaded.definition),
            loaded.binding.part,
            std::slice::from_ref(&self.pmi),
        )?
        .into_iter()
        .next()
        .ok_or("missing BOM row".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_are_always_present_and_last_when_empty() {
        let mut d = ExistingPmi::new("dimension", "Size_Diameter", Vec::new());
        d.value = Some(8.0);
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            r#"{"kind":"dimension","type":"Size_Diameter","value":8.0,"faces":[]}"#
        );
        let datum = ExistingPmi::new("datum", "A", vec![3]);
        assert_eq!(
            serde_json::to_string(&datum).unwrap(),
            r#"{"kind":"datum","type":"A","faces":[3]}"#
        );
    }

    #[test]
    fn a_default_finish_is_read_back_to_its_value() {
        assert_eq!(
            default_finish("Ra 1.6 µm unless otherwise specified"),
            Some("Ra 1.6")
        );
        assert_eq!(
            default_finish("Ra 3.2 µm unless otherwise stated"),
            Some("Ra 3.2")
        );
        assert_eq!(default_finish("Ra 0.8 µm"), None);
    }

    #[test]
    fn edge_break_recognition_is_only_an_explicit_instruction() {
        for text in [
            "Remove all burrs and break sharp edges",
            "BREAK ALL SHARP EDGES 0.2 MAX",
            "Break edges",
            "Breaking all edges",
        ] {
            assert!(edge_break_note(text), "{text}");
        }
        for text in [
            "Mask these features before coating with primer",
            "Inspect edges",
            "Edge finish Ra 3.2",
            "Break coating before inspection",
            "Keep the sharp edges clean",
        ] {
            assert!(!edge_break_note(text), "{text}");
        }
    }

    #[test]
    fn every_tolerance_kind_has_an_opencascade_name() {
        for (_, kind) in ToleranceKind::ALL {
            assert!(!tolerance_type(kind).is_empty());
        }
        assert_eq!(
            size_type(&SizeKind::Other("x".into()), false, false),
            "Location_None"
        );
        assert_eq!(
            location_type(&LocationKind::LinearDistanceCentre, false, false),
            "Location_None"
        );
    }
}
