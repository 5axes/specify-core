//! Assembly BOM, using append-only CAx-IF UDAs. See `docs/bom.md`.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use haecceity::p21::{self, Attribute as A, Document, Edit, RawEntity};
use quiddity::correspondence::{self as qc, Fingerprints};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::part::{File, LoadedPart};
use crate::rules::Question;

pub const PROPERTY: &str = "specify bom";
pub const HISTORY: &str = "specify bom item numbers";
pub const CARRY: &str = "_specify_bom";
/// The code every refusal to write a BOM into a file with no assembly structure carries, at the
/// start of its message: a stable contract (docs/wasm.md, "Errors") a caller matches instead of
/// the prose after it.
pub const NO_ASSEMBLY: &str = "bom_no_assembly";

/// Every field carries its provenance, including missing/default values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub value: Value,
    pub source: String,
}
fn field(value: impl Into<Value>, source: &str) -> Field {
    Field {
        value: value.into(),
        source: source.into(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Current file's part index, for selecting analyse/write; never a persistent identity.
    pub part: usize,
    pub item_number: Field,
    pub part_number: Field,
    pub name: Field,
    pub description: Field,
    pub revision: Field,
    pub make_buy: Field,
    pub material: Field,
    pub quantity: Field,
    pub optional: Field,
    pub stock: Field,
}

fn attrs<'a>(doc: &'a Document, id: u64, entity: &str) -> Option<&'a [A]> {
    match doc.get(id)? {
        RawEntity::Simple {
            name, attributes, ..
        } if name == entity
            || haecceity::express::is_a(
                &name.to_ascii_lowercase(),
                &entity.to_ascii_lowercase(),
            ) =>
        {
            Some(attributes)
        }
        RawEntity::Complex { parts, .. } => parts
            .iter()
            .find(|p| p.name == entity)
            .map(|p| p.attributes.as_slice()),
        _ => None,
    }
}
fn reference(a: Option<&A>) -> Option<u64> {
    match a? {
        A::EntityRef(id) => Some(*id),
        _ => None,
    }
}
fn string(a: Option<&A>) -> Option<String> {
    match a? {
        A::String(s) => Some(s.clone()),
        A::Enum(s) => Some(s.to_lowercase().replace('_', " ")),
        _ => None,
    }
}
fn present(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}
fn text(doc: &Document, id: u64, entity: &str, index: usize) -> Option<String> {
    let a = attrs(doc, id, entity)?;
    if entity == "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE" && a.len() == 1 {
        return if index == 3 {
            present(string(a.first()))
        } else {
            present(string(
                attrs(doc, id, "PRODUCT_DEFINITION_FORMATION")?.get(index),
            ))
        };
    }
    present(string(a.get(index)))
}
fn formations(doc: &Document, pd: u64) -> (u64, u64, &'static str) {
    let formation = attrs(doc, pd, "PRODUCT_DEFINITION")
        .and_then(|a| reference(a.get(2)))
        .unwrap_or(0);
    let ty = if attrs(
        doc,
        formation,
        "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE",
    )
    .is_some()
    {
        "PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE"
    } else {
        "PRODUCT_DEFINITION_FORMATION"
    };
    let product = attrs(doc, formation, "PRODUCT_DEFINITION_FORMATION")
        .and_then(|a| reference(a.get(2)))
        .unwrap_or(0);
    (formation, product, ty)
}
fn occurrences(doc: &Document, pd: u64) -> Vec<u64> {
    // Only what references `pd` can name it; the index is ascending, as `ids()` is.
    doc.referrers(pd)
        .iter()
        .copied()
        .filter(|&id| {
            attrs(doc, id, "NEXT_ASSEMBLY_USAGE_OCCURRENCE")
                .is_some_and(|a| reference(a.get(4)) == Some(pd))
        })
        .collect()
}

/// UDA property -> representation -> text/integer/boolean values. No geometric loading.
pub(crate) fn properties(doc: &Document, owner: u64) -> Vec<(String, Map<String, Value>)> {
    let mut out = Vec::new();
    for &id in doc.referrers(owner) {
        let Some(a) = attrs(doc, id, "PROPERTY_DEFINITION") else {
            continue;
        };
        if reference(a.get(2)) != Some(owner) {
            continue;
        }
        let Some(name) = string(a.first()) else {
            continue;
        };
        let mut values = Map::new();
        for &link in doc.referrers(id) {
            let Some(link) = attrs(doc, link, "PROPERTY_DEFINITION_REPRESENTATION") else {
                continue;
            };
            if reference(link.first()) != Some(id) {
                continue;
            }
            let Some(rep) = reference(link.get(1)) else {
                continue;
            };
            let Some(A::List(items)) = attrs(doc, rep, "REPRESENTATION").and_then(|a| a.get(1))
            else {
                continue;
            };
            for item in items {
                let Some(item) = reference(Some(item)) else {
                    continue;
                };
                for ty in [
                    "DESCRIPTIVE_REPRESENTATION_ITEM",
                    "INTEGER_REPRESENTATION_ITEM",
                    "BOOLEAN_REPRESENTATION_ITEM",
                ] {
                    if let Some(a) = attrs(doc, item, ty) {
                        let Some(key) = string(a.first()) else {
                            continue;
                        };
                        let value = match a.get(1) {
                            Some(A::Integer(n)) => json!(n),
                            Some(A::Enum(s)) if s == "T" || s == "F" => json!(s == "T"),
                            other => string(other).map(Value::String).unwrap_or(Value::Null),
                        };
                        values.insert(key, value);
                    }
                }
            }
        }
        out.push((name, values));
    }
    out
}

/// The stock proposal that follows make/buy: bought parts come from stock, made parts are
/// to be made, and an unknown source proposes nothing.
fn stock_for(make_buy: &Value) -> Value {
    match make_buy.as_str() {
        Some("bought") => json!("from stock"),
        Some("made") => json!("to be made"),
        _ => Value::Null,
    }
}

/// What a part's name says about whether it is made or bought.
enum Guess {
    /// A project drawing/part number: made, including bespoke hardware.
    PartNumber,
    /// Its last noun names standard hardware (the noun).
    Standard(String),
    /// Anything else: made.
    Other,
}

impl Guess {
    fn value(&self) -> &'static str {
        match self {
            Guess::Standard(_) => "bought",
            _ => "made",
        }
    }
}

fn guessed(name: &str) -> Guess {
    let tokens: Vec<_> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    // A drawing prefix followed by a number identifies project hardware, including screws.
    if tokens.windows(2).any(|p| {
        p[0].len() >= 2
            && !["ISO", "DIN", "ANSI", "BS", "EN"].contains(&p[0])
            && p[0].chars().all(|c| c.is_ascii_uppercase())
            && p[1].chars().all(|c| c.is_ascii_digit())
    }) {
        return Guess::PartNumber;
    }
    let words: Vec<_> = tokens.iter().map(|s| s.to_lowercase()).collect();
    // Ignore size/designation suffixes; the last noun determines the kind of part.
    let noun = words.iter().rev().find(|s| {
        !s.chars().any(|c| c.is_ascii_digit()) && !["mm", "iso", "din", "x"].contains(&s.as_str())
    });
    match noun {
        Some(noun)
            if [
                "screw", "washer", "dowel", "bearing", "nut", "fastener", "bolt", "rivet",
                "circlip", "pin", "stud",
            ]
            .contains(&noun.as_str()) =>
        {
            Guess::Standard(noun.clone())
        }
        _ => Guess::Other,
    }
}

// Foreign attributes are untrusted input: normalise recognised values, ignore invalid values.
fn normalised(key: &str, value: &Value) -> Option<Value> {
    let text = || {
        value
            .as_str()
            .map(|s| s.trim().to_lowercase().replace(['_', '-'], " "))
    };
    match key {
        "make_buy" | "make_or_buy" => Some(json!(match text()?.as_str() {
            "made" | "make" => "made",
            "bought" | "buy" | "purchased" => "bought",
            "not known" | "unknown" => "not known",
            _ => return None,
        })),
        "stock" => Some(json!(match text()?.as_str() {
            "from stock" | "stock" | "in stock" => "from stock",
            "to be made" | "make" => "to be made",
            _ => return None,
        })),
        "optional" => Some(json!(match value {
            Value::Bool(v) => *v,
            _ => match text()?.as_str() {
                "yes" | "true" => true,
                "no" | "false" => false,
                _ => return None,
            },
        })),
        "item_number" | "find_number" => value
            .as_u64()
            .or_else(|| value.as_str()?.parse().ok())
            .filter(|n| *n > 0)
            .map(|n| json!(n)),
        _ => value
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(|s| json!(s)),
    }
}

/// Read the BOM without recognising geometry. A missing item number stays null until written.
pub fn read(file: &File) -> Result<Vec<Item>, String> {
    let doc = Document::parse(file.bytes().as_ref().clone()).map_err(|e| e.to_string())?;
    read_parsed(&doc, file.definitions(), 0)
}

/// Reuse analysis' existing parse and read only this part's PMI, never the whole assembly.
pub(crate) fn read_parsed(
    doc: &Document,
    definitions: &[haecceity::step::PartDefinition],
    offset: usize,
) -> Result<Vec<Item>, String> {
    let pmi = haecceity::pmi::read(doc, definitions).map_err(|e| e.to_string())?;
    read_rows(doc, definitions, offset, &pmi.parts)
}

pub(crate) fn read_rows(
    doc: &Document,
    definitions: &[haecceity::step::PartDefinition],
    offset: usize,
    pmi: &[haecceity::pmi::PartPmi],
) -> Result<Vec<Item>, String> {
    let saved = crate::resume::stored_in(doc);
    let mut rows = Vec::new();
    for (index, def) in definitions.iter().enumerate() {
        let part = index + offset;
        let pd = def.product_definition;
        let (formation, product, ty) = formations(doc, pd);
        let native = |v: Option<String>| {
            v.map_or_else(|| field(Value::Null, "default"), |v| field(v, "file"))
        };
        let mut row = Item {
            part,
            item_number: field(Value::Null, "default"),
            part_number: native(text(doc, product, "PRODUCT", 0)),
            name: native(
                present(Some(def.name.clone())).or_else(|| text(doc, product, "PRODUCT", 1)),
            ),
            description: native(
                text(doc, product, "PRODUCT", 2).or_else(|| text(doc, pd, "PRODUCT_DEFINITION", 1)),
            ),
            revision: native(text(doc, formation, ty, 0)),
            make_buy: native(text(doc, formation, ty, 3)),
            material: native(pmi[index].material.as_ref().map(|m| {
                m.name
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| m.id.clone())
            })),
            quantity: field(def.placements.len(), "file"),
            optional: field(false, "default"),
            stock: field(Value::Null, "default"),
        };
        let occurrences_here = occurrences(doc, pd);
        let native_numbers: Option<Vec<u64>> = occurrences_here
            .iter()
            .map(|id| {
                text(doc, *id, "NEXT_ASSEMBLY_USAGE_OCCURRENCE", 1)?
                    .parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
            })
            .collect();
        if let Some(numbers) = native_numbers
            && let Some(n) = numbers.first()
            && numbers.iter().all(|v| v == n)
        {
            row.item_number = field(*n, "file");
        }
        let owners = [pd, def.shape].into_iter().chain(occurrences_here);
        for owner in owners {
            for (set, values) in properties(doc, owner) {
                for (key, v) in &values {
                    let key = key.to_lowercase().replace([' ', '-'], "_");
                    let Some(v) = normalised(&key, v) else {
                        continue;
                    };
                    let target = match key.as_str() {
                        "make_buy" | "make_or_buy" => &mut row.make_buy,
                        "part_number" => &mut row.part_number,
                        "description" => &mut row.description,
                        "revision" => &mut row.revision,
                        "name" if set == PROPERTY => &mut row.name,
                        "optional" => &mut row.optional,
                        "stock" => &mut row.stock,
                        "item_number" | "find_number" => &mut row.item_number,
                        _ => continue,
                    };
                    // A Specify override is explicitly answered; exported properties fill gaps.
                    let suggested = values.get(&format!("default.{key}")) == Some(&json!(true));
                    if set == PROPERTY && !suggested || target.source == "default" {
                        *target = field(
                            v.clone(),
                            if set == PROPERTY && key != "item_number" && !suggested {
                                "answered"
                            } else if set == PROPERTY
                                && values.get("item_number_source") == Some(&json!("file"))
                            {
                                "file"
                            } else if set == PROPERTY {
                                "default"
                            } else {
                                "file"
                            },
                        );
                    }
                }
            }
        }
        if row.material.value.is_null() {
            let mut names = BTreeSet::new();
            for id in doc.ids() {
                if let Some(a) = attrs(doc, id, "MATERIAL_DESIGNATION")
                    && let Some(A::List(owners)) = a.get(1)
                    && owners
                        .iter()
                        .any(|a| reference(Some(a)).is_some_and(|o| o == pd || o == def.shape))
                    && let Some(name) = present(string(a.first()))
                {
                    names.insert(name);
                }
            }
            if names.len() == 1 {
                row.material = field(names.into_iter().next().unwrap_or_default(), "file");
            }
        }
        if row.make_buy.value.is_null() {
            row.make_buy = field(guessed(&def.name).value(), "default");
        }
        // A defaulted stock, including one accepted before stock followed make/buy, is
        // re-proposed from make/buy; an unknown make/buy leaves it as it is.
        let proposed = stock_for(&row.make_buy.value);
        if row.stock.source == "default" && (row.stock.value.is_null() || !proposed.is_null()) {
            row.stock = field(proposed, "default");
        }
        if row.description.value.is_null() {
            row.description = field(def.name.clone(), "default");
        }
        if let Some(a) = saved
            .get(&part)
            .and_then(|p| p.get("answers"))
            .and_then(Value::as_object)
            && a.get("part.material")
                .is_some_and(|m| Some(m) == Some(&row.material.value))
        {
            row.material.source = "answered".into();
        }
        rows.push(row);
    }
    Ok(rows)
}

/// For each part of `file` (parsed as `doc`), the earlier part it is a copy of, or `None`
/// (`same_as` in [`crate::api::parts`]). A copy has the same geometry up to a proper rigid
/// motion, proven face by face (`quiddity::correspondence::congruent`), the same stated BOM
/// facts ([`stated`]) and the same saved answers. Names, `PRODUCT.id` and defaults never decide
/// it, as in draftwright's pack (`Metadata::compatible`). Parts keep their own indexes (decision
/// D1); a copy points at the first of its kind. Only parts that share a face count with another
/// are built, and only congruent ones have their BOM facts read, so a file of distinct parts
/// costs next to nothing more.
pub fn copies(file: &File, doc: &Document) -> Vec<Option<usize>> {
    let defs = file.definitions();
    let mut out = vec![None; defs.len()];
    let shares = |i: usize| {
        defs.iter()
            .enumerate()
            .any(|(j, d)| j != i && d.faces.len() == defs[i].faces.len())
    };
    let shapes: Vec<_> = (0..defs.len())
        .map(|i| {
            shares(i)
                .then(|| file.load(Some(i)).ok())
                .flatten()
                .map(|l| {
                    let prints = qc::fingerprint(&l.part, &Default::default());
                    (l.part, prints)
                })
        })
        .collect();
    let congruent = |a: usize, b: usize| match (&shapes[a], &shapes[b]) {
        (Some((s, p)), Some((t, q))) => qc::congruent_with(s, p, t, q).is_some(),
        _ => false,
    };
    // Groups by geometry alone, then each is split by what the parts state.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in (0..defs.len()).filter(|&i| shapes[i].is_some()) {
        match groups.iter_mut().find(|g| congruent(g[0], i)) {
            Some(g) => g.push(i),
            None => groups.push(vec![i]),
        }
    }
    groups.retain(|g| g.len() > 1);
    if groups.is_empty() {
        return out;
    }
    let members: Vec<usize> = groups.iter().flatten().copied().collect();
    let chosen: Vec<_> = members.iter().map(|&i| defs[i].clone()).collect();
    let Ok(rows) = read_parsed(doc, &chosen, 0) else {
        return out;
    };
    let saved = crate::resume::stored_in(doc);
    let key = |i: usize| {
        let row = &rows[members.iter().position(|&m| m == i).unwrap_or_default()];
        let answers = saved.get(&i).and_then(|p| p.get("answers")).cloned();
        (stated(row, &defs[i].product_id), answers)
    };
    for group in groups {
        let mut firsts: Vec<(usize, _)> = Vec::new();
        for i in group {
            let k = key(i);
            match firsts.iter().find(|(_, f)| *f == k) {
                Some((f, _)) => out[i] = Some(*f),
                None => firsts.push((i, k)),
            }
        }
    }
    out
}

/// The BOM facts a row states, which two copies must share: part number, description, revision,
/// make/buy, material, optional and stock, each only where the file or an answer states it (a
/// default is derived from the name). A part number that is only `PRODUCT.id` is an exporter's
/// identifier, not a stated part number, and is left out.
fn stated(row: &Item, product_id: &str) -> Vec<Option<Value>> {
    let raw_id = row.part_number.source == "file" && row.part_number.value == json!(product_id);
    [
        &row.part_number,
        &row.description,
        &row.revision,
        &row.make_buy,
        &row.material,
        &row.optional,
        &row.stock,
    ]
    .iter()
    .enumerate()
    .map(|(n, f)| (f.source != "default" && !(n == 0 && raw_id)).then(|| f.value.clone()))
    .collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_path(path: &Path) -> Result<Vec<Item>, String> {
    read(&File::read(path).map_err(|e| e.to_string())?)
}
pub fn read_bytes(bytes: Vec<u8>) -> Result<Vec<Item>, String> {
    read(&File::of_bytes(Path::new("bom.step"), bytes).map_err(|e| e.to_string())?)
}

/// The BOM questions: optional, never blocking a write, and without a `default`, so accepting
/// defaults never writes them; their proposals are `suggestions`, accepted only explicitly
/// ([`accept_suggestions`]). Each is `routine`, except a make/buy proposal its name and its
/// recognised pockets disagree on (`check`). Stock is not asked: it follows make/buy.
pub(crate) fn questions(analysis: &Map<String, Value>) -> Vec<Question> {
    let Some(bom) = analysis.get("bom") else {
        return vec![];
    };
    let machined = analysis
        .get("features")
        .and_then(Value::as_array)
        .is_some_and(|features| {
            features
                .iter()
                .filter_map(|f| f.get("family").and_then(Value::as_str))
                .any(|family| family.contains("pocket") || family.contains("keyway"))
        });
    let mut out = Vec::new();
    for (key, prompt, options) in [
        (
            "make_buy",
            "Made or bought?",
            vec!["made", "bought", "not known"],
        ),
        ("description", "Description", vec![]),
        ("part_number", "Part number", vec![]),
        ("revision", "Revision", vec![]),
        ("optional", "Optional item?", vec!["no", "yes"]),
    ] {
        if matches!(key, "part_number" | "revision") && bom[key]["source"] == "file" {
            continue;
        }
        let value = &bom[key]["value"];
        let mut q = Question::new(
            format!("bom.{key}"),
            if options.is_empty() { "text" } else { "choice" },
            prompt.into(),
            vec![],
        );
        q.options = options.into_iter().map(String::from).collect();
        q.why = "Shown in the drawing's parts list.".into();
        q.attention = "routine";
        let suggestion = match key {
            "make_buy" if bom[key]["source"] == "answered" => {
                q.reason = "Your answer".into();
                value.clone()
            }
            "make_buy" if bom[key]["source"] != "default" => {
                q.reason = "From the file".into();
                value.clone()
            }
            "make_buy" => {
                let guess = guessed(bom["name"]["value"].as_str().unwrap_or(""));
                q.reason = match &guess {
                    Guess::PartNumber => "Proposed: made (the name carries a part number)".into(),
                    Guess::Standard(noun) if machined => {
                        q.attention = "check";
                        format!(
                            "Its name says {noun}, but it has machined pockets: made to your \
                             drawing, or bought?"
                        )
                    }
                    Guess::Standard(noun) => format!("Proposed: bought (a standard {noun})"),
                    Guess::Other => "Proposed: made (it isn't a standard part)".into(),
                };
                // The row's proposal (`read_rows`: the name's guess, or one accepted earlier).
                if machined {
                    json!("made")
                } else {
                    value.clone()
                }
            }
            "description" => value.clone(),
            "optional" => json!(if value == true { "yes" } else { "no" }),
            _ => Value::Null,
        };
        if !suggestion.is_null() {
            q.suggestions.push(suggestion);
        }
        q.basis = q.reason.clone();
        out.push(q);
    }
    out
}

/// Explicitly accept the optional BOM proposals. Ordinary default acceptance never does.
/// `bom_suggestions` records provenance separately from explicit answers.
pub fn accept_suggestions(
    analysis: &Value,
    answers: &Map<String, Value>,
    intent: &mut crate::rules::Intent,
) -> Result<(), String> {
    let object = analysis.as_object().ok_or("analysis is not an object")?;
    if !analysis.get("bom").is_some_and(Value::is_object) {
        return Err(format!(
            "{NO_ASSEMBLY}: the file has no assembly structure, so there is no BOM to write \
             (BOM acceptance needs a part of an assembly, analysed with analyse --part)"
        ));
    }
    let mut fields = intent
        .part
        .get("bom")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut suggested = Vec::new();
    let questions = questions(object);
    // Stock is not asked but is proposed with the rest: from the file, or following make/buy
    // (an explicit answer over the proposal).
    let stock = if analysis["bom"]["stock"]["source"] == "default" {
        let make_buy = answers.get("bom.make_buy").cloned().or_else(|| {
            questions
                .iter()
                .find(|q| q.id == "bom.make_buy")
                .and_then(|q| q.suggestions.first().cloned())
        });
        make_buy.map_or(Value::Null, |m| stock_for(&m))
    } else {
        analysis["bom"]["stock"]["value"].clone()
    };
    let proposals = questions
        .iter()
        .map(|q| (q.id.as_str(), q.suggestions.first().cloned()))
        .chain([("bom.stock", Some(stock).filter(|v| !v.is_null()))]);
    for (id, value) in proposals {
        let key = id.strip_prefix("bom.").unwrap_or("");
        if !answers.contains_key(id)
            && analysis["bom"][key]["source"] != "answered"
            && let Some(value) = value
            && !value.as_str().is_some_and(|s| s.trim().is_empty())
        {
            fields.insert(key.into(), value);
            suggested.push(key.to_owned());
        }
    }
    intent.part.insert("bom".into(), json!(fields));
    intent
        .part
        .insert("bom_suggestions".into(), json!(suggested));
    Ok(())
}

pub(crate) fn answered_in(
    doc: &Document,
    definition: &haecceity::step::PartDefinition,
) -> Map<String, Value> {
    let mut values = Map::new();
    for owner in occurrences(doc, definition.product_definition) {
        for (set, fields) in properties(doc, owner) {
            if set != PROPERTY {
                continue;
            }
            for (key, value) in &fields {
                if validate(
                    key,
                    &if key == "optional" {
                        json!(if value == true { "yes" } else { "no" })
                    } else {
                        value.clone()
                    },
                )
                .is_ok()
                    && fields.get(&format!("default.{key}")) != Some(&json!(true))
                    && !value.as_str().is_some_and(|s| s.trim().is_empty())
                {
                    values.insert(
                        format!("bom.{key}"),
                        if key == "optional" {
                            json!(if value == true { "yes" } else { "no" })
                        } else {
                            value.clone()
                        },
                    );
                }
            }
        }
    }
    values
}

pub(crate) fn validate(key: &str, value: &Value) -> Result<(), String> {
    let ok = match key {
        "make_buy" => value
            .as_str()
            .is_some_and(|s| ["made", "bought", "not known"].contains(&s)),
        "optional" => value.as_str().is_some_and(|s| ["yes", "no"].contains(&s)),
        "stock" => value
            .as_str()
            .is_some_and(|s| ["from stock", "to be made"].contains(&s)),
        "description" | "part_number" | "revision" | "name" => value.is_string(),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("invalid bom.{key}: {value}"))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    number: u64,
    name: String,
    geometry: Fingerprints,
    #[serde(default)]
    fields: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct History {
    schema: u8,
    high_water: u64,
    entries: Vec<Entry>,
}
fn history(doc: &Document) -> Result<Option<History>, String> {
    let mut last: Option<History> = None;
    for id in doc.ids() {
        let Some(a) = attrs(doc, id, "PROPERTY_DEFINITION") else {
            continue;
        };
        if string(a.first()).as_deref() != Some(HISTORY) {
            continue;
        }
        let Some(owner) = reference(a.get(2)) else {
            continue;
        };
        // properties are returned in record order; process one owner's snapshots once.
        if doc.ids().any(|k| {
            k < id
                && attrs(doc, k, "PROPERTY_DEFINITION").is_some_and(|v| {
                    string(v.first()).as_deref() == Some(HISTORY)
                        && reference(v.get(2)) == Some(owner)
                })
        }) {
            continue;
        }
        for (set, values) in properties(doc, owner) {
            if set != HISTORY {
                continue;
            }
            let encoded = if let Some(text) = values.get("history").and_then(Value::as_str) {
                text.to_owned()
            } else {
                let mut chunks: Vec<_> = values
                    .iter()
                    .filter(|(k, _)| k.starts_with("history."))
                    .collect();
                chunks.sort_by_key(|(k, _)| *k);
                if chunks.is_empty() {
                    return Err("BOM history has no history text".into());
                }
                let mut encoded = String::new();
                for (i, (key, value)) in chunks.into_iter().enumerate() {
                    if key != &format!("history.{:06}", i + 1) {
                        return Err("BOM history chunks are incomplete".into());
                    }
                    encoded.push_str(value.as_str().ok_or("BOM history chunk is not text")?);
                }
                encoded
            };
            let h: History =
                serde_json::from_str(&encoded).map_err(|e| format!("BOM history: {e}"))?;
            check_history(&h)?;
            if last
                .as_ref()
                .is_some_and(|old| old.high_water > h.high_water)
            {
                return Err("BOM history counter went backwards".into());
            }
            last = Some(h);
        }
    }
    Ok(last)
}
fn check_history(h: &History) -> Result<(), String> {
    let mut used = BTreeSet::new();
    if h.schema != 1
        || h.high_water > i64::MAX as u64
        || h.entries
            .iter()
            .any(|e| e.number == 0 || e.number > h.high_water || !used.insert(e.number))
    {
        return Err("invalid BOM allocation history".into());
    }
    Ok(())
}

fn uda(edit: &mut Edit, owner: u64, name: &str, values: &Map<String, Value>) {
    let s = |v: &str| A::String(v.into());
    // Keep string tokens below OpenCascade's STEP scanner limit. Geometry history
    // can exceed 64 KiB; splitting the UDA items preserves standard STEP syntax.
    let chunked;
    let values = if name == HISTORY {
        let text = values.get("history").and_then(Value::as_str).unwrap_or("");
        let chars: Vec<_> = text.chars().collect();
        chunked = chars
            .chunks(2048)
            .enumerate()
            .map(|(i, chars)| {
                (
                    format!("history.{:06}", i + 1),
                    json!(chars.iter().collect::<String>()),
                )
            })
            .collect::<Map<_, _>>();
        &chunked
    } else {
        values
    };
    let items: Vec<_> = values
        .iter()
        .map(|(key, value)| {
            let (ty, value) = match value {
                Value::Bool(v) => (
                    "BOOLEAN_REPRESENTATION_ITEM",
                    A::Enum(if *v { "T" } else { "F" }.into()),
                ),
                Value::Number(n) if n.as_i64().is_some() => (
                    "INTEGER_REPRESENTATION_ITEM",
                    A::Integer(n.as_i64().unwrap_or(0)),
                ),
                Value::String(v) => ("DESCRIPTIVE_REPRESENTATION_ITEM", s(v)),
                _ => ("DESCRIPTIVE_REPRESENTATION_ITEM", s(&value.to_string())),
            };
            edit.add(p21::simple(ty, vec![s(key), value])).to_ref()
        })
        .collect();
    let context = edit.add(p21::simple(
        "REPRESENTATION_CONTEXT",
        vec![s(""), s("user defined attributes")],
    ));
    let rep = edit.add(p21::simple(
        "REPRESENTATION",
        vec![s(""), A::List(items), context.to_ref()],
    ));
    let property = edit.add(p21::simple(
        "PROPERTY_DEFINITION",
        vec![s(name), s("user defined attribute"), A::EntityRef(owner)],
    ));
    edit.add(p21::simple(
        "PROPERTY_DEFINITION_REPRESENTATION",
        vec![property.to_ref(), rep.to_ref()],
    ));
    let general = edit.add(p21::simple(
        "GENERAL_PROPERTY",
        vec![s(""), s(name), A::Unset],
    ));
    edit.add(p21::simple(
        "GENERAL_PROPERTY_ASSOCIATION",
        vec![s(""), A::Unset, general.to_ref(), property.to_ref()],
    ));
}

/// The assembly's `PRODUCT_DEFINITION_SHAPE` a BOM is written under, or why a BOM cannot be
/// written into `doc`: it needs a product definition that is not one of the parts
/// (`definitions`), with a shape, and an assembly occurrence of every part. Questions about the
/// BOM are asked only when this holds, so they are never asked of what cannot be written.
pub(crate) fn assembly_shape(
    doc: &Document,
    definitions: &[haecceity::step::PartDefinition],
) -> Result<u64, String> {
    let leaf: BTreeSet<_> = definitions.iter().map(|d| d.product_definition).collect();
    let owner = doc
        .ids()
        .find(|&id| attrs(doc, id, "PRODUCT_DEFINITION").is_some() && !leaf.contains(&id))
        .ok_or_else(|| {
            format!("{NO_ASSEMBLY}: BOM writing requires an assembly product definition")
        })?;
    let shape = doc
        .referrers(owner)
        .iter()
        .copied()
        .find(|&id| {
            attrs(doc, id, "PRODUCT_DEFINITION_SHAPE")
                .is_some_and(|a| reference(a.get(2)) == Some(owner))
        })
        .ok_or("BOM assembly has no definition shape for history")?;
    if let Some(i) = definitions
        .iter()
        .position(|d| occurrences(doc, d.product_definition).is_empty())
    {
        return Err(format!("BOM part {i} has no assembly occurrence"));
    }
    Ok(shape)
}

/// Pure append after the PMI candidate, before its final readback/atomic publication.
pub(crate) fn append(bytes: Vec<u8>, jobs: &[crate::writer::Job<'_>]) -> Result<Vec<u8>, String> {
    if !jobs
        .iter()
        .any(|(_, intent, _)| intent.part.contains_key("bom"))
    {
        return Ok(bytes);
    }
    let original = Document::parse(jobs[0].0.bytes.as_ref().clone()).map_err(|e| e.to_string())?;
    let bytes = shape_metadata(bytes, original.max_id(), jobs)?;
    let doc = Document::parse(bytes.clone()).map_err(|e| e.to_string())?;
    let file = File::of_bytes(Path::new("bom.step"), bytes.clone()).map_err(|e| e.to_string())?;
    let parts = file.load_all().map_err(|e| e.to_string())?;
    let rows = read(&file)?;
    let existing = history(&original)?;
    let mut old = if let Some(h) = existing {
        h
    } else {
        let transported = jobs
            .iter()
            .filter_map(|(_, _, a)| a.and_then(|a| a.get(CARRY)))
            .map(|v| {
                serde_json::from_value::<History>(v.clone())
                    .map_err(|e| format!("BOM transport: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for h in &transported {
            check_history(h)?;
        }
        if transported.windows(2).any(|w| w[0] != w[1]) {
            return Err("conflicting BOM histories in answers".into());
        }
        transported.first().cloned().unwrap_or(History {
            schema: 1,
            ..History::default()
        })
    };
    check_history(&old)?;
    let geometry: Vec<_> = parts
        .iter()
        .map(|p| qc::recognise(&p.part).fingerprints)
        .collect();
    if old.high_water == 0 {
        for (i, row) in rows.iter().enumerate() {
            if row.item_number.source == "file"
                && let Some(number) = row
                    .item_number
                    .value
                    .as_u64()
                    .or_else(|| row.item_number.value.as_str().and_then(|s| s.parse().ok()))
            {
                old.high_water = old.high_water.max(number);
                old.entries.push(Entry {
                    number,
                    name: parts[i].name.clone(),
                    geometry: geometry[i].clone(),
                    fields: Map::new(),
                    material: None,
                });
            }
        }
        check_history(&old)?;
    }
    let numbers = allocate(
        &mut old,
        &parts.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        &geometry,
        &rows
            .iter()
            .map(|r| r.item_number.value.as_u64())
            .collect::<Vec<_>>(),
    )?;
    let owner = assembly_shape(&doc, file.definitions())?;
    let mut edit = Edit::new();
    let mut expected = BTreeMap::new();
    for (i, p) in parts.iter().enumerate() {
        let occurrences = occurrences(&doc, p.definition.product_definition);
        if occurrences.is_empty() {
            return Err(format!("BOM part {i} has no assembly occurrence"));
        }
        let mut values = old.entries[i].fields.clone();
        values.insert("item_number".into(), json!(numbers[i]));
        // Preserve earlier overrides when writing only one part or a subset of fields.
        for occurrence in &occurrences {
            for (set, previous) in properties(&doc, *occurrence) {
                if set == PROPERTY {
                    values.extend(previous);
                }
            }
        }
        // Old snapshots may contain accepted empty text; never propagate it to a new set.
        values.retain(|_, v| !v.as_str().is_some_and(|s| s.trim().is_empty()));
        values.insert("item_number".into(), json!(numbers[i]));
        if rows[i].item_number.source == "file" {
            values.insert("item_number_source".into(), json!("file"));
        }
        if let Some((_, intent, _)) = jobs.iter().find(|(p, _, _)| p.binding.part == i)
            && let Some(bom) = intent.part.get("bom").and_then(Value::as_object)
        {
            for (key, value) in bom {
                validate(key, value)?;
                if value.as_str().is_some_and(|s| s.trim().is_empty()) {
                    continue;
                }
                let suggested = intent
                    .part
                    .get("bom_suggestions")
                    .and_then(Value::as_array)
                    .is_some_and(|keys| keys.contains(&json!(key)));
                values.remove(&format!("default.{key}"));
                if suggested {
                    values.insert(format!("default.{key}"), json!(true));
                }
                values.insert(
                    key.clone(),
                    if key == "optional" {
                        json!(value == "yes")
                    } else {
                        value.clone()
                    },
                );
            }
        }
        if values.get("optional") == Some(&json!(true)) && p.placements.len() != 1 {
            return Err(format!(
                "BOM part {i}: optional 0–1 requires exactly one occurrence"
            ));
        }
        let changed = rows[i].item_number.value != json!(numbers[i])
            || occurrences.iter().any(|o| {
                properties(&doc, *o)
                    .into_iter()
                    .rfind(|(n, _)| n == PROPERTY)
                    .map(|(_, v)| v)
                    != Some(values.clone())
            });
        if changed {
            for occurrence in occurrences {
                uda(&mut edit, occurrence, PROPERTY, &values);
            }
        }
        old.entries[i].fields = values
            .iter()
            .filter(|(key, _)| {
                !key.starts_with("default.")
                    && values.get(&format!("default.{key}")) != Some(&json!(true))
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        old.entries[i].material = (rows[i].material.source == "answered")
            .then(|| rows[i].material.value.as_str().map(String::from))
            .flatten();
        expected.insert(i, values);
    }
    if history(&doc)?.as_ref() != Some(&old) {
        uda(
            &mut edit,
            owner,
            HISTORY,
            &Map::from_iter([(
                "history".into(),
                json!(serde_json::to_string(&old).map_err(|e| e.to_string())?),
            )]),
        );
    }
    let written = doc.apply(&edit).map_err(|e| e.to_string())?.bytes;
    let after = Document::parse(written.clone()).map_err(|e| e.to_string())?;
    let new_violations: Vec<_> = haecceity::express::validate_document(after.graph())
        .into_iter()
        .filter(|v| v.id > doc.max_id())
        .collect();
    if !new_violations.is_empty() {
        return Err(format!("BOM schema validation: {new_violations:?}"));
    }
    // Prove the BOM append has not changed even one existing record.
    for id in doc.ids() {
        if doc.span(id).map(|r| &bytes[r]) != after.span(id).map(|r| &written[r]) {
            return Err(format!("BOM append changed #{id}"));
        }
    }
    if history(&after)? != Some(old) {
        return Err("BOM history did not read back".into());
    }
    let back = read_bytes(written.clone())?;
    for (i, values) in expected {
        let value = serde_json::to_value(&back[i]).map_err(|e| e.to_string())?;
        for (key, expected) in &values {
            if key == "item_number_source"
                || key.starts_with("default.")
                || values.get(&format!("default.{key}")) == Some(&json!(true))
                    && value[key]["source"] == "file"
            {
                continue;
            }
            if value[key]["value"] != *expected {
                return Err(format!("BOM part {i} {key} did not read back"));
            }
        }
    }
    Ok(written)
}

fn allocate(
    h: &mut History,
    names: &[String],
    geometry: &[Fingerprints],
    preferred: &[Option<u64>],
) -> Result<Vec<u64>, String> {
    let mut matches: Vec<Option<usize>> = vec![None; geometry.len()];
    let mut used = BTreeSet::new();
    let compatible = |a: &Fingerprints, b: &Fingerprints| {
        if a == b {
            return true;
        }
        qc::correspond(a, b).is_ok_and(|c| {
            let n = c
                .faces
                .iter()
                .filter(|e| {
                    matches!(e.class, qc::Class::Carried | qc::Class::Adapted) && e.new.is_some()
                })
                .count();
            // Even a one-face part can correspond. No arbitrary three-face minimum.
            n > 0 && n * 2 > a.faces.len().max(b.faces.len())
        })
    };
    // Explicit stored identity has priority, with revision correspondence tolerance.
    for (i, number) in preferred.iter().enumerate() {
        if let Some(j) = h
            .entries
            .iter()
            .position(|e| Some(e.number) == *number && compatible(&e.geometry, &geometry[i]))
            && used.insert(j)
        {
            matches[i] = Some(j);
        }
    }
    // Equal parts are indistinguishable geometrically. Preserve named identities first,
    // then pair in stored history order and current occurrence/part order.
    for named in [true, false] {
        for (i, g) in geometry.iter().enumerate() {
            if matches[i].is_some() {
                continue;
            }
            if let Some((j, _)) = h.entries.iter().enumerate().find(|(j, e)| {
                !used.contains(j) && (!named || e.name == names[i]) && e.geometry == *g
            }) {
                matches[i] = Some(j);
                used.insert(j);
            }
        }
    }
    let mut pairs = Vec::new();
    for (i, g) in geometry
        .iter()
        .enumerate()
        .filter(|(i, _)| matches[*i].is_none())
    {
        for (j, e) in h
            .entries
            .iter()
            .enumerate()
            .filter(|(j, _)| !used.contains(j))
        {
            if compatible(&e.geometry, g) {
                pairs.push((i, j));
            }
        }
    }
    // A mutually unique revised part is safe even after renaming.
    for &(i, j) in &pairs {
        if pairs.iter().filter(|(k, _)| *k == i).count() == 1
            && pairs.iter().filter(|(_, k)| *k == j).count() == 1
        {
            matches[i] = Some(j);
            used.insert(j);
        }
    }
    // Tolerant identical groups (including changed fingerprint values) retain order.
    for named in [true, false] {
        for (i, g) in geometry.iter().enumerate() {
            if matches[i].is_some() {
                continue;
            }
            let candidates: Vec<_> = pairs
                .iter()
                .filter(|(k, j)| {
                    *k == i && !used.contains(j) && (!named || h.entries[*j].name == names[i])
                })
                .map(|(_, j)| *j)
                .collect();
            if let Some(&j) = candidates.first()
                && candidates
                    .iter()
                    .all(|k| compatible(&h.entries[j].geometry, &h.entries[*k].geometry))
                && pairs
                    .iter()
                    .filter(|(_, k)| *k == j)
                    .all(|(k, _)| compatible(g, &geometry[*k]))
            {
                matches[i] = Some(j);
                used.insert(j);
            }
        }
    }
    let mut entries = Vec::new();
    let mut numbers = Vec::new();
    for (i, g) in geometry.iter().enumerate() {
        let number = if let Some(j) = matches[i] {
            h.entries[j].number
        } else {
            h.high_water = h
                .high_water
                .checked_add(1)
                .ok_or("BOM item counter exhausted")?;
            if h.high_water > i64::MAX as u64 {
                return Err("BOM item counter exhausted".into());
            }
            h.high_water
        };
        entries.push(Entry {
            number,
            name: names[i].clone(),
            geometry: g.clone(),
            fields: matches[i]
                .map(|j| h.entries[j].fields.clone())
                .unwrap_or_default(),
            material: matches[i].and_then(|j| h.entries[j].material.clone()),
        });
        numbers.push(number);
    }
    h.entries = entries;
    Ok(numbers)
}

pub(crate) fn carry(old: &LoadedPart, answers: &mut Map<String, Value>) -> Result<(), String> {
    let doc = Document::parse(old.bytes.as_ref().clone()).map_err(|e| e.to_string())?;
    if let Some(h) = history(&doc)? {
        answers.insert(
            CARRY.into(),
            serde_json::to_value(h).map_err(|e| e.to_string())?,
        );
    }
    Ok(())
}

/// The PMI reader deliberately reports assembly UDAs as out of scope. Only our own
/// independently verified general-property links are handled by the BOM reader.
pub(crate) fn handles_finding(doc: &Document, f: &haecceity::pmi::Finding) -> bool {
    if f.kind.as_str() != "assembly-pmi" || f.ids.is_empty() {
        return false;
    }
    f.ids.iter().all(|&id| {
        let associations: Vec<_> = if attrs(doc, id, "GENERAL_PROPERTY_ASSOCIATION").is_some() {
            vec![id]
        } else if attrs(doc, id, "GENERAL_PROPERTY").is_some() {
            doc.referrers(id).to_vec()
        } else {
            return false;
        };
        !associations.is_empty()
            && associations.into_iter().all(|id| {
                attrs(doc, id, "GENERAL_PROPERTY_ASSOCIATION")
                    .and_then(|a| reference(a.get(3)))
                    .and_then(|p| attrs(doc, p, "PROPERTY_DEFINITION"))
                    .and_then(|a| string(a.first()))
                    .is_some_and(|n| n == PROPERTY || n == HISTORY)
            })
    })
}

/// OCCT ReadMaterials treats every descriptive property on a PD as material,
/// regardless of its property name. Geometry-bound Specify bookkeeping uses the
/// part definition shape (the reader's equivalent whole-part owner) on BOM writes.
/// Only this write's newly emitted records are changed; input records stay intact.
fn shape_metadata(
    bytes: Vec<u8>,
    input_max: u64,
    jobs: &[crate::writer::Job<'_>],
) -> Result<Vec<u8>, String> {
    let doc = Document::parse(bytes.clone()).map_err(|e| e.to_string())?;
    let mut edit = Edit::new();
    for id in doc.ids().filter(|id| *id > input_max) {
        let Some(a) = attrs(&doc, id, "PROPERTY_DEFINITION") else {
            continue;
        };
        let Some(name) = string(a.first()) else {
            continue;
        };
        if ![crate::resume::NAME, crate::characteristics::PROPERTY].contains(&name.as_str()) {
            continue;
        }
        if let Some((part, _, _)) = jobs
            .iter()
            .find(|(p, _, _)| reference(a.get(2)) == Some(p.definition.product_definition))
        {
            let mut a = a.to_vec();
            a[2] = A::EntityRef(part.definition.shape);
            edit.replace(id, p21::simple("PROPERTY_DEFINITION", a));
        }
    }
    doc.apply(&edit).map(|w| w.bytes).map_err(|e| e.to_string())
}

/// Restore BOM answers through geometric assembly correspondence, including added/name-
/// unpaired parts. Material remains an ordinary `part.material` answer for the existing writer.
pub(crate) fn assembly_answers(
    old: &[LoadedPart],
    new: &[LoadedPart],
) -> Result<Vec<Map<String, Value>>, String> {
    let mut out = vec![Map::new(); new.len()];
    let Some(first) = old.first() else {
        return Ok(out);
    };
    let doc = Document::parse(first.bytes.as_ref().clone()).map_err(|e| e.to_string())?;
    let Some(mut h) = history(&doc)? else {
        return Ok(out);
    };
    let source = serde_json::to_value(&h).map_err(|e| e.to_string())?;
    let geometry: Vec<_> = new
        .iter()
        .map(|p| qc::recognise(&p.part).fingerprints)
        .collect();
    allocate(
        &mut h,
        &new.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
        &geometry,
        &vec![None; new.len()],
    )?;
    let new_material = if let Some(first) = new.first() {
        let file = File::of_shared(&first.path, std::sync::Arc::clone(&first.bytes))
            .map_err(|e| e.to_string())?;
        read(&file)?
            .into_iter()
            .map(|r| r.material.value)
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    for (i, e) in h.entries.iter().enumerate() {
        out[i].insert(CARRY.into(), source.clone());
        for (key, value) in &e.fields {
            if key == "item_number" || key == "item_number_source" {
                continue;
            }
            let value = if key == "optional" {
                json!(if value == true { "yes" } else { "no" })
            } else {
                value.clone()
            };
            out[i].insert(format!("bom.{key}"), value);
        }
        if new_material[i].is_null()
            && let Some(material) = &e.material
        {
            out[i].insert("part.material".into(), json!(material));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pin() -> Fingerprints {
        let f = File::read(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/pin.step"),
        )
        .unwrap();
        qc::recognise(&f.load(None).unwrap().part).fingerprints
    }
    fn initial(g: Fingerprints, names: &[&str]) -> History {
        History {
            schema: 1,
            high_water: names.len() as u64,
            entries: names
                .iter()
                .enumerate()
                .map(|(i, n)| Entry {
                    number: i as u64 + 1,
                    name: (*n).into(),
                    geometry: g.clone(),
                    fields: Map::new(),
                    material: None,
                })
                .collect(),
        }
    }
    #[test]
    fn perturbation_duplicates_and_small_parts_keep_numbers() {
        let g = pin();
        let mut changed = g.clone();
        changed.scale *= 1.0 + 1e-8;
        for f in &mut changed.faces {
            f.area = f.area.map(|a| a * (1.0 + 1e-8));
        }
        let mut h = initial(g.clone(), &["a", "b"]);
        assert_eq!(
            allocate(
                &mut h,
                &["b".into(), "a".into()],
                &[changed.clone(), changed.clone()],
                &[Some(2), Some(1)]
            )
            .unwrap(),
            [2, 1]
        );
        let mut h = initial(g.clone(), &["same", "same"]);
        assert_eq!(
            allocate(
                &mut h,
                &["same".into(), "same".into()],
                &[changed.clone(), changed],
                &[None, None]
            )
            .unwrap(),
            [1, 2]
        );
        let mut small = g;
        small.faces.truncate(1);
        small.faces[0].index = 0;
        small.faces[0].neighbours.clear();
        small.faces[0].adjacency.clear();
        small.features.clear();
        let mut changed = small.clone();
        changed.scale *= 1.0 + 1e-8;
        let mut h = initial(small, &["small"]);
        assert_eq!(
            allocate(&mut h, &["renamed".into()], &[changed], &[None]).unwrap(),
            [1]
        );
    }
    #[test]
    fn a_defaulted_stock_follows_make_buy_and_not_known_proposes_none() {
        let f = File::read(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/assembly.step"),
        )
        .unwrap();
        let doc = Document::parse(f.bytes().as_ref().clone()).unwrap();
        let mut edit = Edit::new();
        // Part 0: a "to be made" suggestion accepted before stock followed make/buy.
        let owner = occurrences(&doc, f.definitions()[0].product_definition)[0];
        uda(
            &mut edit,
            owner,
            PROPERTY,
            &Map::from_iter([
                ("make_buy".into(), json!("bought")),
                ("stock".into(), json!("to be made")),
                ("default.stock".into(), json!(true)),
            ]),
        );
        // Part 1: make/buy not known in the file.
        uda(
            &mut edit,
            f.definitions()[1].shape,
            "Exporter",
            &Map::from_iter([("Make-Buy".into(), json!("not known"))]),
        );
        let rows = read_bytes(doc.apply(&edit).unwrap().bytes).unwrap();
        assert_eq!(rows[0].make_buy, field("bought", "answered"));
        assert_eq!(rows[0].stock, field("from stock", "default"));
        assert_eq!(rows[1].make_buy, field("not known", "file"));
        assert_eq!(rows[1].stock, field(Value::Null, "default"));
    }
    #[test]
    fn a_pocketed_screw_proposed_made_is_proposed_to_be_made() {
        let analysis = json!({
            "bom": {
                "name": {"value": "Grub screw M4x5", "source": "file"},
                "make_buy": {"value": "bought", "source": "default"},
                "stock": {"value": "from stock", "source": "default"},
            },
            "features": [{"family": "pockets"}],
        });
        let qs = questions(analysis.as_object().unwrap());
        let make_buy = qs.iter().find(|q| q.id == "bom.make_buy").unwrap();
        assert_eq!(make_buy.suggestions, [json!("made")]);
        // Stock is not asked; accepting the proposals makes it follow the made proposal.
        assert!(qs.iter().all(|q| q.id != "bom.stock"));
        let mut intent = crate::rules::Intent::new(json!({}));
        accept_suggestions(&analysis, &Map::new(), &mut intent).unwrap();
        assert_eq!(intent.part["bom"]["make_buy"], "made");
        assert_eq!(intent.part["bom"]["stock"], "to be made");
    }
    #[test]
    fn a_make_buy_answered_earlier_is_the_answer_not_the_file() {
        let reason = |source: &str| {
            let analysis = json!({"bom": {
                "name": {"value": "plate", "source": "file"},
                "make_buy": {"value": "bought", "source": source},
            }});
            let qs = questions(analysis.as_object().unwrap());
            assert_eq!(qs[0].suggestions, [json!("bought")]);
            assert_eq!(qs[0].attention, "routine");
            qs[0].reason.clone()
        };
        assert_eq!(reason("answered"), "Your answer");
        assert_eq!(reason("file"), "From the file");
    }
    #[test]
    fn foreign_keys_ignore_case_and_invalid_values() {
        let f = File::read(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/assembly.step"),
        )
        .unwrap();
        let doc = Document::parse(f.bytes().as_ref().clone()).unwrap();
        let mut edit = Edit::new();
        uda(
            &mut edit,
            f.definitions()[0].shape,
            "Exporter",
            &Map::from_iter([
                ("Description".into(), json!("Exported description")),
                ("Stock".into(), json!("FROM_STOCK")),
                ("Make-Buy".into(), json!("Purchased")),
                ("Optional".into(), json!("YES")),
            ]),
        );
        uda(
            &mut edit,
            f.definitions()[1].shape,
            "Exporter",
            &Map::from_iter([
                ("Stock".into(), json!("ask Fred in stores")),
                ("Make-Buy".into(), json!("unclassified")),
            ]),
        );
        let owner = occurrences(&doc, f.definitions()[0].product_definition)[0];
        uda(
            &mut edit,
            owner,
            PROPERTY,
            &Map::from_iter([("revision".into(), json!("User revision"))]),
        );
        uda(
            &mut edit,
            owner,
            "Later export",
            &Map::from_iter([("Revision".into(), json!("Export revision"))]),
        );
        let rows = read_bytes(doc.apply(&edit).unwrap().bytes).unwrap();
        assert_eq!(rows[0].revision, field("User revision", "answered"));
        assert_eq!(rows[0].description, field("Exported description", "file"));
        assert_eq!(rows[0].stock, field("from stock", "file"));
        assert_eq!(rows[0].make_buy, field("bought", "file"));
        assert_eq!(rows[0].optional, field(true, "file"));
        // Unusable stock text is ignored; the proposal follows the bought make/buy guess.
        assert_eq!(rows[1].stock, field("from stock", "default"));
        assert_eq!(rows[1].make_buy, field("bought", "default"));
    }
    #[test]
    fn hardware_tokens_and_project_numbers() {
        for name in [
            "GRM-01 shank",
            "GRM-02 shaft",
            "GRM-03 thumbwheel drive screw",
            "GRM-04 drive plate",
            "GRM-05 depth-lock bolt",
            "GRM-06 blade retainer",
            "GRM-07 blade",
            "push rod",
            "spacer",
            "bearing housing",
            "leadscrew",
            "minute",
        ] {
            assert_eq!(guessed(name).value(), "made", "{name}");
        }
        for name in [
            "ISO 4762 screw M2x6",
            "grub screw M4x5",
            "flat washer M2",
            "pan-head screw M2x6",
            "pan-head screw M2x10",
            "bolt",
            "rivet",
            "circlip",
            "pin",
            "stud",
            "nut",
            "bearing",
            "dowel",
        ] {
            assert_eq!(guessed(name).value(), "bought", "{name}");
        }
        assert_eq!(normalised("stock", &json!("unusable text")), None);
        assert_eq!(
            normalised("stock", &json!("FROM_STOCK")),
            Some(json!("from stock"))
        );
        assert_eq!(
            normalised("make_buy", &json!("Purchased")),
            Some(json!("bought"))
        );
    }
}
