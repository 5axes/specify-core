//! Strict projection of #65/#77 metadata additions for the frozen pre-numbering cross-engine corpus.
//! Keep those files and their independent Python readbacks; do not recapture old evidence.
use haecceity::p21::{Attribute, Document, RawEntity, Record};
use specify_core::{characteristics, existing::ExistingPmi};
use std::collections::{BTreeMap, BTreeSet};

/// Erase only inspection identity and stored dimension intent for comparisons with Python's pre-#65 reader. Its note
/// rows aggregate equal kinds; the numbered API now keeps each requirement separate.
pub fn legacy_existing(items: Vec<ExistingPmi>) -> Vec<ExistingPmi> {
    let mut out: Vec<ExistingPmi> = Vec::new();
    for mut item in items {
        item.number = None;
        item.origin = None;
        item.source = None;
        if item.kind == "note"
            && let Some(old) = out.iter_mut().find(|old| {
                old.kind == "note" && old.r#type == item.r#type && old.provenance == item.provenance
            })
        {
            old.faces.extend(item.faces);
            old.faces.sort_unstable();
            old.faces.dedup();
        } else {
            out.push(item);
        }
    }
    out
}

/// Require every non-numbering entity and every reference to remain exactly the same,
/// allowing only the numeric-id/history and dimension-intent additions and the consequent new-record numbering.
/// In particular, this is stronger than comparing only the semantic PMI a reader models.
pub fn only_numbering_added(old: &[u8], new: &[u8]) -> Result<(), String> {
    let old = Document::parse(old.to_vec()).map_err(|e| e.to_string())?;
    let new = Document::parse(new.to_vec()).map_err(|e| e.to_string())?;
    let defs = haecceity::step::read_part_definitions(new.bytes()).map_err(|e| e.to_string())?;
    let read = haecceity::pmi::read(&new, &defs).map_err(|e| e.to_string())?;
    let mut removed = BTreeSet::new();
    for (p, prov) in read.parts.iter().zip(&read.provenance.parts) {
        for (i, a) in p.attributes.iter().enumerate().filter(|(_, a)| {
            a.name == characteristics::PROPERTY || a.name == specify_core::pmi_origin::PROPERTY
        }) {
            if a.name == characteristics::PROPERTY && a.on.is_some() {
                return Err("number history attached to a feature".into());
            }
            if a.name == specify_core::pmi_origin::PROPERTY
                && !matches!(a.on, Some(haecceity::pmi::NoteOwner::Dimension(_)))
            {
                return Err("dimension origin attached to another entity".into());
            }
            removed.extend(&prov.attributes[i]);
        }
    }
    let old_max = old.ids().max().unwrap_or(0);
    for (&id, e) in &new.graph().entities {
        if matches!(e, RawEntity::Simple { name, attributes, .. }
            if name == "ID_ATTRIBUTE" && id > old_max && matches!(attributes.first(),
                Some(Attribute::String(s)) if s.parse::<u64>().is_ok_and(|n| n > 0 && n.to_string() == *s)))
        {
            removed.insert(id);
        }
    }
    // Authorship annotations on the new history are themselves new metadata.
    let descriptions: Vec<_> = new.graph().entities.iter().filter_map(|(&id, e)|
        matches!(e, RawEntity::Simple { name, attributes, .. } if name == "DESCRIPTION_ATTRIBUTE"
            && matches!(attributes.get(1), Some(Attribute::EntityRef(target)) if removed.contains(target)))
        .then_some(id)).collect();
    removed.extend(descriptions);
    if removed.is_empty() {
        return Err("changed bytes without numbering additions".into());
    }
    let remaining: Vec<_> = new.ids().filter(|id| !removed.contains(id)).collect();
    let prior: Vec<_> = old.ids().collect();
    if remaining.len() != prior.len() {
        return Err(format!(
            "{} entities after removing number metadata, previously {}",
            remaining.len(),
            prior.len()
        ));
    }
    let mapping: BTreeMap<_, _> = remaining
        .iter()
        .copied()
        .zip(prior.iter().copied())
        .collect();
    fn remap(a: &mut Attribute, mapping: &BTreeMap<u64, u64>) {
        match a {
            Attribute::EntityRef(id) => *id = mapping.get(id).copied().unwrap_or(u64::MAX),
            Attribute::List(items) => {
                for a in items {
                    remap(a, mapping);
                }
            }
            Attribute::Typed { value, .. } => remap(value, mapping),
            _ => {}
        }
    }
    for (now, before) in remaining.into_iter().zip(prior) {
        let mut entity = new.get(now).unwrap().clone();
        match &mut entity {
            RawEntity::Simple { attributes, .. } => {
                for a in attributes {
                    remap(a, &mapping);
                }
            }
            RawEntity::Complex { parts, .. } => {
                for part in parts {
                    for a in &mut part.attributes {
                        remap(a, &mapping);
                    }
                }
            }
        }
        let current = Record::new(entity).encode().map_err(|e| e.to_string())?;
        let previous = Record::new(old.get(before).unwrap().clone())
            .encode()
            .map_err(|e| e.to_string())?;
        if current != previous {
            return Err(format!("#{before} changed: {previous} -> {current}"));
        }
    }
    Ok(())
}

// For #95, retain the independently read Python fixtures. Compare the common history
// fields (numbers, high-water mark, fingerprint and supports) exactly, and every byte
// outside its text payload. Only schema 1's obsolete value signature and schema 2's
// structural role differ; migration/role behavior is covered by revision_pmi.
pub fn comparable_number_history(bytes: &[u8]) -> Vec<u8> {
    use haecceity::p21::{Attribute, Document, Edit, RawEntity};
    use serde_json::{Value, json};
    let doc = Document::parse(bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(bytes).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let mut edit = Edit::new();
    for (part, prov) in read.parts.iter().zip(&read.provenance.parts) {
        for (i, a) in part.attributes.iter().enumerate() {
            if a.name != specify_core::characteristics::PROPERTY {
                continue;
            }
            for &id in &prov.attributes[i] {
                let Some(mut entity) = doc.get(id).cloned() else {
                    continue;
                };
                let RawEntity::Simple {
                    name, attributes, ..
                } = &mut entity
                else {
                    continue;
                };
                if name != "DESCRIPTIVE_REPRESENTATION_ITEM" {
                    continue;
                }
                let [Attribute::String(name), Attribute::String(text)] = attributes.as_mut_slice()
                else {
                    continue;
                };
                if name != "history" {
                    continue;
                }
                let mut history: Value = serde_json::from_str(text).unwrap();
                assert!(matches!(history["schema"].as_u64(), Some(1 | 2)));
                history["schema"] = json!("common fields");
                for entry in history["entries"].as_array_mut().unwrap() {
                    let key = entry["key"].as_object_mut().unwrap();
                    key.remove("specification");
                    key.remove("role");
                }
                *text = serde_json::to_string(&history).unwrap();
                edit.replace(id, entity);
            }
        }
    }
    doc.apply(&edit).unwrap().bytes
}
