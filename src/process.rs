//! How a part is made (`part.process`): a per-part answer that sets other questions' defaults.
//! See `docs/process.md`.
//!
//! A part with no answer and no value in its file is machined, and every question, default and
//! write is what it was before the process existed. The process changes defaults only: every
//! question that stays asked still takes any answer, and an answer always wins. A guess from
//! the material's printing class is a suggestion (attention `check`), never applied silently.
//!
//! The process is stored per product as a CAx-IF user defined attribute set
//! ([`PROPERTY`]) on the part's product definition, with one item ([`ITEM`]); an accepted
//! suggestion also carries `default.manufacturing_process` 'true', as the BOM's carry their
//! `default.<field>` markers (as text: haecceity's writer refuses boolean items, which its
//! reader cannot read back).

use haecceity::p21::{Attribute, Document, RawEntity};
use haecceity::pmi::{AttributeSet, AttributeValue, PartPmi};
use serde_json::{Map, Value, json};

use crate::bom::Field;
use crate::rules::{Ctx, Question};

/// The question.
pub const QUESTION: &str = "part.process";
/// The CAx-IF UDA property set Specify writes on a part's product definition.
pub const PROPERTY: &str = "specify process";
/// Its one item: the process, as one of [`PROCESSES`].
pub const ITEM: &str = "manufacturing_process";
/// Marks an accepted suggestion, as the BOM's `default.<field>` items do.
pub const DEFAULT_ITEM: &str = "default.manufacturing_process";
/// The intent's `part` key holding the process, and the one marking it a suggestion.
pub const INTENT_KEY: &str = "process";
pub const INTENT_DEFAULT_KEY: &str = "process_default";

pub const MACHINED: &str = "machined";
pub const FDM: &str = "3D printed: FDM";
pub const RESIN: &str = "3D printed: resin";
pub const POWDER: &str = "3D printed: powder";
pub const METAL: &str = "3D printed: metal";
pub const SHEET_METAL: &str = "sheet metal";
pub const MOULDED: &str = "moulded";
pub const CAST: &str = "cast";

/// The answers, in the order they are offered.
pub const PROCESSES: [&str; 8] = [
    MACHINED,
    FDM,
    RESIN,
    POWDER,
    METAL,
    SHEET_METAL,
    MOULDED,
    CAST,
];

/// Printed in plastic: FDM, resin or powder (metal printing keeps machined holes and finish).
pub fn printed_plastic(process: &str) -> bool {
    matches!(process, FDM | RESIN | POWDER)
}

/// Any 3D printing.
pub fn printed(process: &str) -> bool {
    printed_plastic(process) || process == METAL
}

/// A printed part's default general tolerance: vendor figures, not a standard (no standard
/// sets general tolerances for 3D printing), so it is a default to check. Written as a note,
/// since the kernel's general tolerance entity carries an ISO 2768 class.
pub fn general_tolerance(process: &str) -> Option<&'static str> {
    Some(match process {
        FDM => "±0.3 mm or ±0.3 % (FDM)",
        RESIN => "±0.15 mm or ±0.15 % (resin)",
        POWDER => "±0.3 mm or ±0.3 % (powder)",
        METAL => "±0.1 mm or ±0.2 % (metal)",
        _ => return None,
    })
}

/// Whether the part question `key` (`surface_finish`, `edges`, ...) is asked of a part made by
/// `process`. Printed plastic parts are not asked their roughness, edges or coating ("as
/// printed"); sheet metal, moulded and cast parts are not asked their roughness.
pub fn asks(process: &str, key: &str) -> bool {
    match key {
        "surface_finish" => matches!(process, MACHINED | METAL),
        "edges" | "coating" => !printed_plastic(process),
        _ => true,
    }
}

/// Whether a hole a machined part would tap is a clearance hole by default.
pub fn clearance_holes(process: &str) -> bool {
    printed_plastic(process) || process == SHEET_METAL
}

/// Whether holes default to ± locations rather than position tolerances.
pub fn locates_by_dimensions(process: &str) -> bool {
    printed(process) || process == SHEET_METAL
}

/// Whether a written surface finish is one no material is removed for (as printed, moulded or
/// cast): `MaterialRemoval::NotAllowed`.
pub fn removes_no_material(process: &str) -> bool {
    printed_plastic(process) || matches!(process, MOULDED | CAST)
}

/// One of [`PROCESSES`] for `text`, ignoring case and surrounding space; None for anything else.
pub fn normalised(text: &str) -> Option<&'static str> {
    let text = text.trim();
    PROCESSES
        .iter()
        .copied()
        .find(|p| p.eq_ignore_ascii_case(text))
}

/// The process a material suggests, with why: a material from a printing class suggests its
/// printed process; anything else, or no material, machined.
pub fn guess(material: Option<&str>) -> (&'static str, String) {
    let class = material.and_then(|m| {
        let m = m.trim();
        crate::materials::MATERIALS
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(m))
            .map(|&(name, group, _)| (name, group))
    });
    let printed = class.and_then(|(name, group)| {
        let process = match group {
            "3D printing: filament" => FDM,
            "3D printing: resin" => RESIN,
            "3D printing: powder" => POWDER,
            "3D printing: metal" => METAL,
            _ => return None,
        };
        Some((name, group, process))
    });
    match printed {
        Some((name, group, process)) => (
            process,
            format!("{name} is in the material class \"{group}\", so {process} is suggested."),
        ),
        None => (
            MACHINED,
            "No printing material is given, so machined, as every part is unless stated."
                .to_string(),
        ),
    }
}

/// The process a file states for the part whose product definition is `product_definition`,
/// with its source as the BOM reports one: `answered` for Specify's own set without a default
/// marker, `file` for another tool's attribute, `default` for Specify's accepted suggestion.
/// UDA property sets on the product definition and its shape are read; an item's name is
/// matched ignoring case, spaces and hyphens, and a value that is not a process is ignored.
pub fn read(doc: &Document, product_definition: u64) -> Option<Field> {
    let shapes: Vec<u64> = doc
        .referrers(product_definition)
        .iter()
        .copied()
        .filter(|&id| {
            matches!(doc.get(id), Some(RawEntity::Simple { name, attributes, .. })
                if name == "PRODUCT_DEFINITION_SHAPE"
                    && matches!(attributes.get(2), Some(Attribute::EntityRef(pd)) if *pd == product_definition))
        })
        .collect();
    classify(
        std::iter::once(product_definition)
            .chain(shapes)
            .flat_map(|owner| crate::bom::properties(doc, owner)),
    )
}

/// [`read`] of a part's PMI as the kernel reads it: its part-level attribute sets.
pub fn read_pmi(pmi: &PartPmi) -> Option<Field> {
    classify(pmi.attributes.iter().filter(|s| s.on.is_none()).map(|set| {
        let items = set
            .items
            .iter()
            .filter_map(|(name, v)| match v {
                AttributeValue::Text(t) => Some((name.clone(), Value::from(t.as_str()))),
                AttributeValue::Boolean(b) => Some((name.clone(), Value::Bool(*b))),
                _ => None,
            })
            .collect();
        (set.name.clone(), items)
    }))
}

/// The process of a part's property sets `(name, items)`, with its source (see [`read`]).
fn classify(sets: impl Iterator<Item = (String, Map<String, Value>)>) -> Option<Field> {
    let (mut answered, mut file, mut suggested) = (None, None, None);
    for (set, items) in sets {
        let key = |name: &str| name.trim().to_lowercase().replace([' ', '-'], "_");
        let found = |wanted: &str| {
            items
                .iter()
                .find(|(name, _)| key(name) == wanted)
                .and_then(|(_, v)| v.as_str())
                .and_then(normalised)
        };
        // OpenCascade writes its metadata back as one item named after the set: Specify's
        // set so rewritten has lost its marker, and is another tool's statement.
        let (value, rewritten) = match (found(ITEM), found(&key(PROPERTY))) {
            (Some(v), _) => (v, false),
            (None, Some(v)) if set == PROPERTY => (v, true),
            _ => continue,
        };
        if rewritten {
            file.get_or_insert(value);
        } else if set == PROPERTY {
            // The last of Specify's sets is current, as the BOM's. Specify writes the marker
            // as text 'true' (haecceity writes no boolean items); the BOM's boolean is read too.
            let marked = matches!(items.get(DEFAULT_ITEM), Some(Value::Bool(true)))
                || items
                    .get(DEFAULT_ITEM)
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.trim().eq_ignore_ascii_case("true"));
            if marked {
                suggested = Some(value);
            } else {
                answered = Some(value);
            }
        } else if file.is_none() {
            file = Some(value);
        }
    }
    let (value, source) = match (answered, file, suggested) {
        (Some(v), _, _) => (v, "answered"),
        (None, Some(v), _) => (v, "file"),
        (None, None, Some(v)) => (v, "default"),
        _ => return None,
    };
    Some(Field {
        value: json!(value),
        source: source.into(),
    })
}

/// Specify's own process set in a part's PMI (the last), if any: its value.
pub(crate) fn specify_set(pmi: &PartPmi) -> Option<&'static str> {
    pmi.attributes
        .iter()
        .filter(|s| s.on.is_none() && s.name == PROPERTY)
        .filter_map(|s| {
            s.items.iter().find_map(|(name, v)| match v {
                AttributeValue::Text(t) if name == ITEM => normalised(t),
                _ => None,
            })
        })
        .next_back()
}

/// The attribute set written for a part's process; `suggested` marks an accepted suggestion.
///
/// The marker comes first: OpenCascade's XCAF keeps one value per property set (the last
/// item's), so its metadata (`TDataStd_NamedData` key `specify process`) holds the process.
pub fn attribute_set(process: &str, suggested: bool) -> AttributeSet {
    let mut items = Vec::new();
    if suggested {
        items.push((
            DEFAULT_ITEM.to_string(),
            AttributeValue::Text("true".into()),
        ));
    }
    items.push((ITEM.to_string(), AttributeValue::Text(process.to_string())));
    AttributeSet {
        name: PROPERTY.into(),
        on: None,
        items,
        description: None,
        general_property: None,
    }
}

/// Whether the part is bought, from a BOM answer or the file (not a guess): a bought part is
/// asked no process.
fn bought(ctx: &Ctx<'_>, answers: &Map<String, Value>) -> bool {
    if let Some(answer) = answers.get("bom.make_buy") {
        return answer.as_str() == Some("bought");
    }
    let row = &ctx.analysis.get("bom").and_then(|b| b.get("make_buy"));
    row.is_some_and(|r| {
        r.get("value").and_then(Value::as_str) == Some("bought")
            && matches!(
                r.get("source").and_then(Value::as_str),
                Some("file" | "answered")
            )
    })
}

/// The process the file states (`analysis.part.process`), which is shown and not asked.
fn stated(ctx: &Ctx<'_>) -> Option<&'static str> {
    ctx.stated
        .and_then(|p| p.get("process"))
        .and_then(Value::as_str)
        .and_then(normalised)
}

/// The material to guess from: the answer, else what the file states.
fn material<'a>(ctx: &'a Ctx<'_>, answers: &'a Map<String, Value>) -> Option<&'a str> {
    answers
        .get("part.material")
        .and_then(Value::as_str)
        .or_else(|| {
            ctx.stated
                .and_then(|p| p.get("material"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            ctx.analysis
                .get("bom")
                .and_then(|b| b.get("material"))
                .and_then(|m| m.get("value"))
                .and_then(Value::as_str)
        })
}

/// The process the other questions follow: none for a bought part (as machined); else the
/// answer, the file's, or machined. A suggestion is not applied until it is accepted.
pub(crate) fn current(ctx: &Ctx<'_>, answers: &Map<String, Value>) -> &'static str {
    if bought(ctx, answers) {
        return MACHINED;
    }
    answers
        .get(QUESTION)
        .and_then(Value::as_str)
        .and_then(|a| PROCESSES.iter().copied().find(|p| *p == a))
        .or_else(|| stated(ctx))
        .unwrap_or(MACHINED)
}

/// Whether an answer to `part.process` is read: not for a bought part.
pub(crate) fn applies(ctx: &Ctx<'_>, answers: &Map<String, Value>) -> bool {
    !bought(ctx, answers)
}

/// The process question, unless the part is bought or its file states the process. Its
/// default is the guess from the current material (worded as the accepted suggestion stored in
/// the file when that agrees); any but machined is marked `check`.
pub(crate) fn question(ctx: &Ctx<'_>, answers: &Map<String, Value>) -> Option<Question> {
    if bought(ctx, answers) || stated(ctx).is_some() {
        return None;
    }
    let stored = ctx
        .analysis
        .get("process")
        .filter(|f| f.get("source").and_then(Value::as_str) == Some("default"))
        .and_then(|f| f.get("value"))
        .and_then(Value::as_str)
        .and_then(normalised);
    // The stored suggestion stands only while the current material still suggests it: a
    // changed material makes its own suggestion.
    let (guessed, why) = guess(material(ctx, answers));
    let (default, basis) = match stored {
        Some(p) if p == guessed => (
            p,
            format!("{p} was suggested and accepted when the file was last written."),
        ),
        _ => (guessed, why),
    };
    // A printed (or stored) suggestion needs a person; machined is what no answer means.
    let (attention, reason) = if default == MACHINED {
        ("routine", String::new())
    } else {
        ("check", basis.clone())
    };
    Some(Question {
        default: Some(Value::from(default)),
        options: PROCESSES.map(String::from).to_vec(),
        why: "How the part is made sets the other questions' defaults: printed parts get a ± \
              general tolerance, clearance holes and ± locations, and are not asked their \
              roughness, edges or coating. Every default can still be changed."
            .into(),
        attention,
        reason,
        basis,
        ..Question::new(
            QUESTION.into(),
            "choice",
            "How is the part made?".into(),
            Vec::new(),
        )
    })
}

/// The process the file holds for a part, as analysis reports it (`analysis.process`), and
/// whether the file states it (`analysis.part.process`): Specify's answer or another tool's
/// value is stated unless `answered` (resumed answers) already hold the process.
pub(crate) fn analysis_entries(
    doc: &Document,
    product_definition: u64,
    answered: Option<&Map<String, Value>>,
    analysis: &mut Map<String, Value>,
) {
    stated_entries(read(doc, product_definition), answered, analysis);
}

/// [`analysis_entries`] of the process already read.
pub(crate) fn stated_entries(
    field: Option<Field>,
    answered: Option<&Map<String, Value>>,
    analysis: &mut Map<String, Value>,
) {
    let Some(field) = field else {
        return;
    };
    let resumed = answered.is_some_and(|a| a.contains_key(QUESTION));
    if field.source != "default" && !resumed {
        if let Some(Value::Object(part)) = analysis.get_mut("part") {
            part.insert("process".into(), field.value.clone());
        } else {
            analysis.insert("part".into(), json!({"process": field.value}));
        }
    }
    analysis.insert(
        "process".into(),
        json!({"value": field.value, "source": field.source}),
    );
}

/// The part's process with its source, as the BOM reports a field: `answered` (an answer, or
/// Specify's answer stored in the file), `file` (another tool's value), `default` (an accepted
/// suggestion stored in the file, else the guess). None for a bought part, which has no process.
pub fn of(
    analysis: &Value,
    answers: &Map<String, Value>,
) -> Result<Option<Field>, crate::rules::Error> {
    let ctx = Ctx::new(analysis)?;
    if !applies(&ctx, answers) {
        return Ok(None);
    }
    let field = |value: &str, source: &str| Field {
        value: json!(value),
        source: source.into(),
    };
    if let Some(answer) = answers
        .get(QUESTION)
        .and_then(Value::as_str)
        .and_then(|a| PROCESSES.iter().copied().find(|p| *p == a))
    {
        return Ok(Some(field(answer, "answered")));
    }
    if let Some(value) = stated(&ctx) {
        let source = analysis
            .get("process")
            .and_then(|f| f.get("source"))
            .and_then(Value::as_str)
            .filter(|s| *s == "answered")
            .unwrap_or("file");
        return Ok(Some(field(value, source)));
    }
    let default = question(&ctx, answers)
        .and_then(|q| q.default)
        .and_then(|d| d.as_str().map(String::from))
        .unwrap_or_else(|| MACHINED.into());
    Ok(Some(field(&default, "default")))
}

/// Answers only another process offers, which lapse under the current one: a heat-set insert
/// or printed thread on a part not printed in plastic, a ± general tolerance on a part whose
/// process offers none. Each with why, as an `unasked` row says it; the answer is kept.
pub(crate) fn lapsed(ctx: &Ctx<'_>, answers: &Map<String, Value>) -> Vec<(String, String)> {
    let process = current(ctx, answers);
    let mut out = Vec::new();
    for (id, value) in answers {
        let Some(text) = value.as_str() else {
            continue;
        };
        let lapses = if id.starts_with("hole.function:") {
            crate::choices::PRINTED_HOLES.contains(&text) && !printed_plastic(process)
        } else if id == "part.general_tolerance" {
            crate::general::plus_minus(text).is_some() && general_tolerance(process).is_none()
        } else {
            false
        };
        if lapses {
            out.push((
                id.clone(),
                format!(
                    "{id} '{text}' is not offered when part.process is '{process}', so its \
                     answer was not written"
                ),
            ));
        }
    }
    out
}

/// `answers` without the `lapsed` ones, which the rules do not read.
pub(crate) fn without_lapsed<'a>(
    ctx: &Ctx<'_>,
    answers: &'a Map<String, Value>,
) -> std::borrow::Cow<'a, Map<String, Value>> {
    let lapsed = lapsed(ctx, answers);
    if lapsed.is_empty() {
        return std::borrow::Cow::Borrowed(answers);
    }
    let mut kept = answers.clone();
    kept.retain(|id, _| !lapsed.iter().any(|(l, _)| l == id));
    std::borrow::Cow::Owned(kept)
}
