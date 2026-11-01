//! Stable inspection identities in AP242. See `docs/characteristic-numbers.md`.
use std::collections::BTreeSet;

use haecceity::p21::{self, Attribute, Document, Edit, RawEntity};
use haecceity::pmi::{
    self, AttributeSet, AttributeValue, DimensionKind, PartPmi, PartProvenance, ToleranceTarget,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::existing::{faces_of, location_type, size_type, tolerance_type};
use crate::part::LoadedPart;
use crate::resume;

mod inspection;

/// Part-level CAx-IF UDA. The characteristic itself uses standard `ID_ATTRIBUTE`, not a UDA.
pub const PROPERTY: &str = "specify characteristic numbers";
/// Opaque revision transport in the returned answers. Keep it when saving/reapplying them.
pub const CARRY: &str = "_specify_characteristic_numbers";

/// A numbered requirement, including the exact STEP entity carrying its `id_attribute`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Characteristic {
    pub number: u64,
    pub entity: u64,
    pub kind: String,
    pub r#type: String,
    pub faces: Vec<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reference: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::pmi_origin::Origin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::pmi_origin::Source>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Key {
    specification: String,
    kind: String,
    r#type: String,
    faces: Vec<usize>,
    reference: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    key: Key,
    number: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct History {
    schema: u8,
    faces: String,
    high_water: u64,
    entries: Vec<Entry>,
}

struct Item {
    key: Key,
    entity: u64,
}

fn is_a(doc: &Document, id: u64, ancestor: &str) -> bool {
    match doc.get(id) {
        Some(RawEntity::Simple { name, .. }) => {
            haecceity::express::is_a(&name.to_ascii_lowercase(), ancestor)
        }
        Some(RawEntity::Complex { parts, .. }) => parts
            .iter()
            .any(|c| haecceity::express::is_a(&c.name.to_ascii_lowercase(), ancestor)),
        None => false,
    }
}

fn root(doc: Option<&Document>, ids: &[u64], ancestor: &str) -> u64 {
    doc.and_then(|doc| ids.iter().copied().find(|&id| is_a(doc, id, ancestor)))
        .unwrap_or(0)
}

pub(crate) fn on(doc: &Document, ids: &[u64], ancestor: &str) -> Option<u64> {
    number(doc, root(Some(doc), ids, ancestor))
}

fn texture_root(doc: Option<&Document>, ids: &[u64]) -> u64 {
    doc.and_then(|doc| {
        ids.iter().copied().find(|&id| {
            matches!(doc.get(id), Some(RawEntity::Simple {name, attributes, ..})
            if name == "PROPERTY_DEFINITION" && matches!(attributes.first(),
                Some(Attribute::String(s)) if s == "surface texture"))
        })
    })
    .unwrap_or(0)
}

pub(crate) fn texture_number(doc: &Document, ids: &[u64]) -> Option<u64> {
    number(doc, texture_root(Some(doc), ids))
}

fn measure(v: Option<&pmi::Value>) -> String {
    v.map(|v| format!("{:.9}", crate::existing::value(v)))
        .unwrap_or_default()
}

fn dimension_signature(d: &pmi::Dimension) -> String {
    let tolerance = match &d.tolerance {
        pmi::DimTolerance::Fit { class, .. } => format!("fit:{class:?}"),
        pmi::DimTolerance::Deviations(b) => {
            format!("{}:{}", measure(Some(b.upper())), measure(Some(b.lower())))
        }
        other => format!("{other:?}"),
    };
    format!("{}|{tolerance}", measure(d.nominal.as_ref()))
}

/// Values disambiguate coexisting requirements of the same kind on the same support.
/// A unique support match also carries a requirement whose value changed.
fn items(p: &PartPmi, prov: Option<&PartProvenance>, doc: Option<&Document>) -> Vec<Item> {
    let mut out = Vec::new();
    let mut add = |kind: &str, ty: String, faces, reference, specification, entity| {
        out.push(Item {
            key: Key {
                specification,
                kind: kind.into(),
                r#type: ty,
                faces,
                reference,
            },
            entity,
        });
    };
    for (i, d) in p.datums.iter().enumerate() {
        let features: Vec<_> = d.feature().into_iter().collect();
        // DATUM is the unique semantic requirement; its DATUM_FEATURE may be shared with
        // another datum or a size. Do not duplicate an id on two shape aspects (AP242 URs).
        let entity = root(doc, prov.map_or(&[], |v| &v.datums[i]), "datum");
        add(
            "datum",
            d.label().as_str().into(),
            faces_of(p, &features),
            vec![],
            String::new(),
            entity,
        );
    }
    for (i, d) in p.dimensions.iter().enumerate() {
        let (ty, faces, reference, ancestor) = match &d.kind {
            DimensionKind::Size {
                feature,
                kind,
                path,
                angle,
            } => (
                size_type(kind, path.is_some(), angle.is_some()),
                faces_of(p, &[*feature]),
                vec![],
                "dimensional_size",
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
                faces_of(p, &[*to]),
                faces_of(p, &[*from]),
                "dimensional_location",
            ),
        };
        add(
            "dimension",
            ty.into(),
            faces,
            reference,
            {
                let id = p
                    .attributes
                    .iter()
                    .find(|a| {
                        a.name == "specify dimension plan"
                            && a.on == Some(pmi::NoteOwner::Dimension(pmi::DimensionId(i)))
                    })
                    .and_then(|a| a.items.iter().find(|(k, _)| k == "requirement"))
                    .and_then(|(_, v)| match v {
                        pmi::AttributeValue::Text(s) => {
                            serde_json::from_str::<crate::dimensioning::Planned>(s).ok()
                        }
                        _ => None,
                    })
                    .map(|p| p.required.id);
                let signature = dimension_signature(d);
                id.map_or(signature.clone(), |id| format!("{signature}|planner:{id}"))
            },
            root(doc, prov.map_or(&[], |v| &v.dimensions[i]), ancestor),
        );
    }
    for (i, t) in p.tolerances.iter().enumerate() {
        let faces = match &t.target {
            ToleranceTarget::Feature(f) => faces_of(p, &[*f]),
            ToleranceTarget::Dimension(d) => match p.dimensions.get(d.0).map(|d| &d.kind) {
                Some(DimensionKind::Size { feature, .. }) => faces_of(p, &[*feature]),
                Some(DimensionKind::Location { from, to, .. }) => faces_of(p, &[*from, *to]),
                None => vec![],
            },
            ToleranceTarget::Relation {
                relating, related, ..
            } => faces_of(p, &[*relating, *related]),
            ToleranceTarget::WholePart => vec![],
        };
        add(
            "geometric_tolerance",
            tolerance_type(t.kind).into(),
            faces,
            vec![],
            format!(
                "{}|{:?}|{:?}",
                measure(t.magnitude.as_ref()),
                t.modifiers,
                t.datums.as_ref().map(|s| s
                    .compartments()
                    .iter()
                    .map(|c| c
                        .references
                        .iter()
                        .map(|r| p.datums[r.datum.0].label().as_str())
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>())
            ),
            root(
                doc,
                prov.map_or(&[], |v| &v.tolerances[i]),
                "geometric_tolerance",
            ),
        );
    }
    for (i, t) in p.surface_textures.iter().enumerate() {
        let mut parameters: Vec<_> = t
            .parameters
            .iter()
            .map(|p| p.characteristic.as_str())
            .collect();
        parameters.sort_unstable();
        let ids = prov.map_or(&[][..], |v| v.surface_textures[i].as_slice());
        let entity = texture_root(doc, ids);
        add(
            "surface_texture",
            parameters.join("|"),
            faces_of(p, &t.on.into_iter().collect::<Vec<_>>()),
            vec![],
            format!(
                "{:?}|{:?}",
                t.material_removal,
                t.parameters
                    .iter()
                    .map(|p| format!("{}:{:.9}", p.characteristic, p.value.mm()))
                    .collect::<Vec<_>>()
            ),
            entity,
        );
    }
    // THREAD and TURNED_KNURL are characterized_objects, outside id_attribute_select.
    // Their exclusive PRODUCT_DEFINITION_SHAPE is a property_definition and supplies a
    // standard id slot without putting inspection identity on a shared geometric feature.
    for (i, t) in p.threads.iter().enumerate() {
        add(
            "thread",
            format!("{:?}", t.side),
            faces_of(p, &[t.feature]),
            vec![],
            format!("{:?}", t.qualifier),
            root(
                doc,
                prov.map_or(&[], |v| &v.threads[i]),
                "product_definition_shape",
            ),
        );
    }
    for (i, k) in p.knurls.iter().enumerate() {
        add(
            "knurl",
            "knurl".into(),
            faces_of(p, &[k.feature]),
            vec![],
            format!("{:?}", k.pattern),
            root(
                doc,
                prov.map_or(&[], |v| &v.knurls[i]),
                "product_definition_shape",
            ),
        );
    }
    out
}

/// The pinned reader does not consume ids on thread/knurl definition shapes. Strip just
/// these inverse annotations before its replace planner checks for outside references.
pub(crate) fn before_replace(doc: &Document, parts: &[PartProvenance]) -> Edit {
    let targets: BTreeSet<_> = parts.iter().flat_map(PartProvenance::all).collect();
    let mut edit = Edit::new();
    for (&id, e) in &doc.graph().entities {
        if let RawEntity::Simple {
            name, attributes, ..
        } = e
            && name == "ID_ATTRIBUTE"
            && let [Attribute::String(s), Attribute::EntityRef(target)] = attributes.as_slice()
            && targets.contains(target)
            && is_a(doc, *target, "product_definition_shape")
            && s.parse::<u64>().is_ok_and(|n| n > 0 && n.to_string() == *s)
        {
            edit.remove(id);
        }
    }
    edit
}

/// A canonical positive decimal identifier on this exact entity. Non-numeric foreign ids
/// are not inspection numbers. Conflicting ids are never guessed.
pub fn number(doc: &Document, entity: u64) -> Option<u64> {
    let numbers = doc
        .referrers(entity)
        .iter()
        .filter_map(|&id| match doc.get(id) {
            Some(RawEntity::Simple {
                name, attributes, ..
            }) if name == "ID_ATTRIBUTE" => match attributes.as_slice() {
                [Attribute::String(s), Attribute::EntityRef(target)] if *target == entity => s
                    .parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0 && n.to_string() == *s),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    if numbers.len() == 1 {
        numbers.first().copied()
    } else {
        None
    }
}

pub(crate) fn read_parsed(
    doc: &Document,
    p: &PartPmi,
    prov: &PartProvenance,
) -> Vec<Characteristic> {
    items(p, Some(prov), Some(doc))
        .into_iter()
        .filter_map(|item| {
            let metadata = prov
                .dimensions
                .iter()
                .position(|ids| ids.contains(&item.entity))
                .map(|i| crate::pmi_origin::read(p, pmi::NoteOwner::Dimension(pmi::DimensionId(i))))
                .unwrap_or_default();
            Some(Characteristic {
                number: number(doc, item.entity)?,
                entity: item.entity,
                kind: item.key.kind,
                r#type: item.key.r#type,
                faces: item.key.faces,
                reference: item.key.reference,
                origin: metadata.origin,
                source: metadata.source,
            })
        })
        .collect()
}

/// Read numbers from the file, without recognising features or allocating anything.
pub fn read(loaded: &LoadedPart) -> Result<Vec<Characteristic>, String> {
    let doc = Document::parse(loaded.bytes.to_vec()).map_err(|e| e.to_string())?;
    let defs = haecceity::step::read_part_definitions(&loaded.bytes).map_err(|e| e.to_string())?;
    let p = pmi::read(&doc, &defs).map_err(|e| e.to_string())?;
    let i = loaded.binding.part;
    Ok(read_parsed(&doc, &p.parts[i], &p.provenance.parts[i]))
}

fn history(p: &PartPmi) -> Result<Option<History>, String> {
    let mut result = None;
    for a in &p.attributes {
        if a.name != PROPERTY || a.on.is_some() {
            continue;
        }
        let [(name, AttributeValue::Text(text))] = a.items.as_slice() else {
            return Err(format!("{PROPERTY}: expected one history text item"));
        };
        if name != "history" {
            return Err(format!("{PROPERTY}: missing history item"));
        }
        let h: History = serde_json::from_str(text).map_err(|e| format!("{PROPERTY}: {e}"))?;
        validate(&h)?;
        // Add mode retains older snapshots; counters can only increase.
        if result
            .as_ref()
            .is_some_and(|old: &History| old.high_water > h.high_water)
        {
            return Err(format!("{PROPERTY}: high-water mark decreased"));
        }
        result = Some(h);
    }
    Ok(result)
}

fn validate(h: &History) -> Result<(), String> {
    let numbers: BTreeSet<_> = h.entries.iter().map(|e| e.number).collect();
    if h.schema != 1
        || numbers.len() != h.entries.len()
        || numbers.iter().any(|n| *n == 0 || *n > h.high_water)
    {
        return Err(format!(
            "{PROPERTY}: invalid schema, duplicate number or high-water mark"
        ));
    }
    Ok(())
}

fn attribute(h: &History) -> AttributeSet {
    AttributeSet {
        name: PROPERTY.into(),
        description: None,
        general_property: None,
        on: None,
        items: vec![(
            "history".into(),
            AttributeValue::Text(serde_json::to_string(h).expect("history serializes")),
        )],
    }
}

fn same_support(old: &Key, new: &Key) -> bool {
    let grows = |old: &[usize], new: &[usize]| {
        (old.is_empty() && new.is_empty())
            || (!old.is_empty() && old.iter().all(|f| new.contains(f)))
    };
    old.kind == new.kind
        && old.r#type == new.r#type
        && ((grows(&old.faces, &new.faces) && grows(&old.reference, &new.reference))
            || (old.kind == "dimension"
                && old.r#type == "Location_LinearDistance"
                && grows(&old.faces, &new.reference)
                && grows(&old.reference, &new.faces)))
}

/// First allocate in inspection order; later additions use stable model order after matching
/// all existing requirements. Support growth is allowed only for a unique two-way match,
/// following resume's conservative same-geometry support rule.
pub(crate) fn allocate(
    loaded: &LoadedPart,
    p: &mut PartPmi,
    before: &PartPmi,
    prov: &PartProvenance,
    doc: &Document,
    answers: Option<&Map<String, Value>>,
    replace: bool,
) -> Result<(), String> {
    let fingerprint = resume::fingerprint(loaded).map_err(|e| e.to_string())?;
    let saved = history(before)?;
    let carried = answers
        .and_then(|a| a.get(CARRY))
        .map(|v| {
            let h: History =
                serde_json::from_value(v.clone()).map_err(|e| format!("{CARRY}: {e}"))?;
            validate(&h)?;
            if h.faces != fingerprint {
                return Err(format!("{CARRY}: history is for different geometry"));
            }
            Ok(h)
        })
        .transpose()?;
    // Prefer numbers on the requirements themselves over the sidecar identities. This is
    // also how an annotated older-engine file is upgraded without feature-ID assumptions.
    let has_saved = saved.is_some();
    let mut old = saved.unwrap_or_default();
    if let Some(h) = carried.filter(|_| !has_saved) {
        old = h;
    } else {
        old.entries = items(before, Some(prov), Some(doc))
            .into_iter()
            .filter_map(|item| {
                Some(Entry {
                    number: number(doc, item.entity)?,
                    key: item.key,
                })
            })
            .collect();
    }
    old.high_water = old
        .high_water
        .max(old.entries.iter().map(|e| e.number).max().unwrap_or(0))
        // AP242's id uniqueness also covers the shared shape aspects, not only the
        // characteristic roots. Do not collide with an imported feature's numeric id.
        .max(
            prov.all()
                .into_iter()
                .filter_map(|id| number(doc, id))
                .max()
                .unwrap_or(0),
        );
    let new = items(p, None, None);
    if !replace && new.is_empty() {
        return Ok(());
    }
    let before_items = items(before, Some(prov), Some(doc));
    let eligible = |item: &Item| replace || item.key.kind == "datum";
    let mut used = BTreeSet::new();
    // Resolve every exact requirement first: a newly inserted control must not steal the
    // number of an unchanged requirement appearing later in the intent.
    let mut matched = vec![None; new.len()];
    for (n, item) in new.iter().enumerate().filter(|(_, item)| eligible(item)) {
        if let Some((i, _)) = old
            .entries
            .iter()
            .enumerate()
            .find(|(i, e)| !used.contains(i) && e.key == item.key)
        {
            used.insert(i);
            matched[n] = Some(i);
        }
    }
    // Planner role ids distinguish equal quantities on one support. When upgrading older
    // PMI without that metadata, prefer an unchanged value/contract before the unique
    // support-growth fallback, so a newly added depth cannot steal a diameter's number.
    let signature = |key: &Key| {
        key.specification
            .split("|planner:")
            .next()
            .unwrap_or("")
            .to_owned()
    };
    for (n, item) in new.iter().enumerate() {
        if matched[n].is_some() || !eligible(item) {
            continue;
        }
        let candidates: Vec<_> = old
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                !used.contains(i)
                    && same_support(&e.key, &item.key)
                    && signature(&e.key) == signature(&item.key)
            })
            .collect();
        if let [(i, e)] = candidates.as_slice()
            && new
                .iter()
                .enumerate()
                .filter(|(n, item)| {
                    matched[*n].is_none()
                        && same_support(&e.key, &item.key)
                        && signature(&e.key) == signature(&item.key)
                })
                .count()
                == 1
        {
            used.insert(*i);
            matched[n] = Some(*i);
        }
    }
    for (n, item) in new.iter().enumerate() {
        if matched[n].is_some() || !eligible(item) {
            continue;
        }
        let candidates: Vec<_> = old
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| !used.contains(i) && same_support(&e.key, &item.key))
            .collect();
        if let [(i, e)] = candidates.as_slice()
            && new
                .iter()
                .enumerate()
                .filter(|(n, item)| matched[*n].is_none() && same_support(&e.key, &item.key))
                .count()
                == 1
        {
            used.insert(*i);
            matched[n] = Some(*i);
        }
    }
    let order = if old.high_water == 0 {
        inspection::order(loaded, p, &new)
    } else {
        (0..new.len()).collect()
    };
    let mut allocations = vec![None; new.len()];
    for n in order {
        let item = &new[n];
        let matched = matched[n];
        if !replace
            && item.key.kind == "datum"
            && before_items
                .iter()
                .any(|old| old.key == item.key && number(doc, old.entity).is_none())
        {
            continue;
        } // a reused foreign datum is not written now
        let number = if let Some(i) = matched {
            used.insert(i);
            old.entries[i].number
        } else {
            old.high_water = old
                .high_water
                .checked_add(1)
                .ok_or("characteristic number exhausted")?;
            old.high_water
        };
        allocations[n] = Some(number);
    }
    if replace {
        for (n, _) in new
            .iter()
            .enumerate()
            .filter(|(_, item)| item.key.kind == "dimension")
        {
            if let Some(matched) = matched[n]
                && let Some(old_item) = before_items
                    .iter()
                    .find(|i| number(doc, i.entity) == Some(old.entries[matched].number))
                && let Some(index) = prov
                    .dimensions
                    .iter()
                    .position(|ids| ids.contains(&old_item.entity))
                && let Some(source) = crate::pmi_origin::read(
                    before,
                    pmi::NoteOwner::Dimension(pmi::DimensionId(index)),
                )
                .source
            {
                crate::pmi_origin::carry_source(p, n - p.datums.len(), &source, &fingerprint);
            }
        }
    }
    // History and annotation emission keep semantic order, including on the first write:
    // only the assigned values change. This also keeps unchanged rewrites byte-identical.
    let mut entries = if replace { vec![] } else { old.entries.clone() };
    for (item, number) in new.iter().zip(allocations) {
        if let Some(number) = number
            && !entries.iter().any(|e| e.number == number)
        {
            entries.push(Entry {
                key: item.key.clone(),
                number,
            });
        }
    }
    if old.high_water > 0 {
        p.attributes.push(attribute(&History {
            schema: 1,
            faces: fingerprint,
            high_water: old.high_water,
            entries,
        }));
    }
    Ok(())
}

/// Numbers allocated by the same history/inspection walk as the STEP writer.
pub(crate) fn planned_numbers(p: &PartPmi) -> Result<Vec<Option<u64>>, String> {
    let h = history(p)?.unwrap_or_default();
    let mut used = BTreeSet::new();
    Ok(items(p, None, None)
        .iter()
        .map(|item| {
            h.entries
                .iter()
                .find(|entry| entry.key == item.key && !used.contains(&entry.number))
                .map(|entry| {
                    used.insert(entry.number);
                    entry.number
                })
        })
        .collect())
}

/// Append only the newly written roots' numeric identifiers. haecceity's semantic reader
/// already consumes standard id_attributes, so its normal replace removes them again.
pub(crate) fn annotate(
    before: &Document,
    after: &Document,
    defs: &[haecceity::step::PartDefinition],
) -> Result<Edit, String> {
    let read = pmi::read(after, defs).map_err(|e| e.to_string())?;
    let mut edit = Edit::new();
    for (p, prov) in read.parts.iter().zip(&read.provenance.parts) {
        let Some(h) = history(p)? else {
            continue;
        };
        let these = items(p, Some(prov), Some(after));
        let mut used: BTreeSet<_> = these
            .iter()
            .filter_map(|item| number(before, item.entity))
            .filter_map(|n| h.entries.iter().position(|e| e.number == n))
            .collect();
        for item in these {
            if item.entity == 0 {
                return Err(format!("no STEP root for {:?}", item.key));
            }
            if before.get(item.entity).is_some() {
                continue;
            }
            if let Some((i, e)) = h
                .entries
                .iter()
                .enumerate()
                .find(|(i, e)| !used.contains(i) && e.key == item.key)
            {
                used.insert(i);
                edit.add(p21::simple(
                    "ID_ATTRIBUTE",
                    vec![
                        Attribute::String(e.number.to_string()),
                        Attribute::EntityRef(item.entity),
                    ],
                ));
            } else {
                return Err(format!("no allocated number for {:?}", item.key));
            }
        }
    }
    Ok(edit)
}

/// Check the actual standard slots, not just the history UDA's round trip.
pub(crate) fn verify(doc: &Document, p: &PartPmi, prov: &PartProvenance) -> Result<(), String> {
    let Some(h) = history(p)? else {
        return Ok(());
    };
    let found = read_parsed(doc, p, prov);
    let numbers: BTreeSet<_> = found.iter().map(|c| c.number).collect();
    if numbers.len() != found.len() {
        return Err("duplicate characteristic numbers in part".into());
    }
    for e in h.entries {
        if !found.iter().any(|c| {
            c.number == e.number
                && c.kind == e.key.kind
                && c.r#type == e.key.r#type
                && c.faces == e.key.faces
                && c.reference == e.key.reference
        }) {
            return Err(format!(
                "characteristic {} does not read back from its STEP entity",
                e.number
            ));
        }
    }
    Ok(())
}

/// Carry the history with the existing revision correspondence and returned answers. The
/// transport is geometry-bound and is consumed by the ordinary writer (native and WASM).
pub(crate) fn carry_revision(
    old: &LoadedPart,
    new: &LoadedPart,
    corr: &quiddity::correspondence::Correspondence,
    answers: &mut Map<String, Value>,
) -> Result<(), String> {
    let doc = Document::parse(old.bytes.to_vec()).map_err(|e| e.to_string())?;
    let defs = haecceity::step::read_part_definitions(&old.bytes).map_err(|e| e.to_string())?;
    let read = pmi::read(&doc, &defs).map_err(|e| e.to_string())?;
    let i = old.binding.part;
    let Some(mut h) = history(&read.parts[i])? else {
        return Ok(());
    };
    let numbered = items(&read.parts[i], Some(&read.provenance.parts[i]), Some(&doc));
    h.entries = numbered
        .into_iter()
        .filter_map(|item| {
            let number = number(&doc, item.entity)?;
            let c = item.key;
            let map = |faces: Vec<usize>| -> Option<Vec<usize>> {
                let mut result = faces
                    .into_iter()
                    .map(|f| {
                        let e = corr.faces.iter().find(|e| e.old == f)?;
                        matches!(
                            e.class,
                            quiddity::correspondence::Class::Carried
                                | quiddity::correspondence::Class::Adapted
                        )
                        .then_some(e.new)
                        .flatten()
                    })
                    .collect::<Option<Vec<_>>>()?;
                result.sort_unstable();
                result.dedup();
                Some(result)
            };
            Some(Entry {
                key: Key {
                    specification: c.specification,
                    kind: c.kind,
                    r#type: c.r#type,
                    faces: map(c.faces)?,
                    reference: map(c.reference)?,
                },
                number,
            })
        })
        .collect();
    h.faces = resume::fingerprint(new).map_err(|e| e.to_string())?;
    answers.insert(
        CARRY.into(),
        serde_json::to_value(h).map_err(|e| e.to_string())?,
    );
    Ok(())
}

/// Only numbers actually carried by former characteristic roots count as retired; gaps below
/// the high-water mark may instead be reserved foreign feature identifiers.
pub(crate) fn retired(
    before: &PartPmi,
    prov: &PartProvenance,
    doc: &Document,
    after: &PartPmi,
) -> Result<Vec<u64>, String> {
    let live: BTreeSet<_> = history(after)?
        .unwrap_or_default()
        .entries
        .into_iter()
        .map(|e| e.number)
        .collect();
    let old: BTreeSet<_> = items(before, Some(prov), Some(doc))
        .into_iter()
        .filter_map(|i| number(doc, i.entity))
        .collect();
    Ok(old.difference(&live).copied().collect())
}
