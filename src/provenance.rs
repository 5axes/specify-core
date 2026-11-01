//! Specify's STEP-only authorship contract; see docs/pmi-provenance.md.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use haecceity::pmi::{PartPmi, PartProvenance};
use std::collections::BTreeSet;

pub const MARKER: &str = "specify-core:written:v1";

fn string(a: Option<&Attribute>, expected: &str) -> bool {
    matches!(a, Some(Attribute::String(s)) if s == expected)
}

fn description_target(entity: &RawEntity) -> Option<u64> {
    match entity {
        RawEntity::Simple {
            name, attributes, ..
        } if name == "DESCRIPTION_ATTRIBUTE" && string(attributes.first(), MARKER) => {
            match attributes.get(1) {
                Some(Attribute::EntityRef(id)) => Some(*id),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A description attached to this exact representation/link, never to a shared owner.
fn described(doc: &Document, id: u64) -> bool {
    doc.referrers(id)
        .iter()
        .any(|r| doc.get(*r).and_then(description_target) == Some(id))
}

pub fn dimension(doc: &Document, ids: &[u64]) -> bool {
    ids.iter().any(|&id| {
        matches!(doc.get(id), Some(RawEntity::Simple { name, .. })
            if name == "SHAPE_DIMENSION_REPRESENTATION" && described(doc, id))
    })
}

/// Only the item's own root records count. A shared feature or referenced datum must not
/// transfer authorship to a foreign requirement.
pub fn marked(doc: &Document, ids: &[u64], kind: &str) -> bool {
    ids.iter().any(|&id| {
        let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
        else {
            return false;
        };
        match kind {
            "feature" => {
                (matches!(
                    name.as_str(),
                    "SHAPE_ASPECT" | "DATUM_FEATURE" | "COMPOSITE_SHAPE_ASPECT"
                ) && string(attributes.get(1), MARKER))
                    || (name == "COMPOSITE_GROUP_SHAPE_ASPECT"
                        && string(attributes.first(), MARKER))
            }
            "datum" => name == "DATUM" && string(attributes.get(1), MARKER),
            "thread" => name == "THREAD" && string(attributes.get(1), MARKER),
            "knurl" => name == "TURNED_KNURL" && string(attributes.first(), MARKER),
            "general" | "surface texture" | "attribute" | "material" => {
                name == "PROPERTY_DEFINITION_REPRESENTATION"
                    && described(doc, id)
                    && matches!(attributes.first(), Some(Attribute::EntityRef(root))
                    if ids.contains(root) && matches!(doc.get(*root),
                        Some(RawEntity::Simple { name, attributes, .. })
                            if name == "PROPERTY_DEFINITION" && (kind == "attribute" ||
                                string(attributes.first(), match kind {
                                    "general" => "default tolerances",
                                    "material" => "material property",
                                    _ => "surface texture",
                                }))))
            }
            _ => false,
        }
    })
}

fn property_helper(doc: &Document, id: u64, attributes: &[Attribute]) -> bool {
    // A blank property on THREAD / TURNED_KNURL is its parameter container, not a UDA.
    (string(attributes.first(), "") && matches!(attributes.get(2), Some(Attribute::EntityRef(root))
        if matches!(doc.get(*root), Some(RawEntity::Simple { name, .. }) if matches!(name.as_str(), "THREAD" | "TURNED_KNURL"))))
        // Surface texture's related parameter properties are helpers; a caller UDA may
        // legitimately have the same name, so identify the relationship, not just the name.
        || doc.referrers(id).iter().any(|&r| matches!(doc.get(r),
            Some(RawEntity::Simple { name, attributes, .. })
                if name == "PROPERTY_DEFINITION_RELATIONSHIP"
                    && string(attributes.first(), "surface texture parameter")
                    && attributes.get(3) == Some(&Attribute::EntityRef(id))))
}

/// Annotate only newly emitted semantic roots. In particular, preserve all UDA vocabulary,
/// material names, surface-condition parameters and thread parameter helper definitions.
pub fn mark_new(before: &Document, after: &Document) -> Edit {
    let mut edit = Edit::new();
    for (&id, entity) in &after.graph().entities {
        if before.get(id).is_some() {
            continue;
        }
        let RawEntity::Simple {
            name, attributes, ..
        } = entity
        else {
            continue;
        };
        let property_link = name == "PROPERTY_DEFINITION_REPRESENTATION"
            && matches!(attributes.first(), Some(Attribute::EntityRef(root))
                if before.get(*root).is_none() && matches!(after.get(*root),
                    Some(RawEntity::Simple { name, attributes, .. })
                        if name == "PROPERTY_DEFINITION"
                            && !property_helper(after, *root, attributes)));
        if name == "SHAPE_DIMENSION_REPRESENTATION" || property_link {
            edit.add(haecceity::p21::simple(
                "DESCRIPTION_ATTRIBUTE",
                vec![Attribute::String(MARKER.into()), Attribute::EntityRef(id)],
            ));
        }
        let slot = match name.as_str() {
            "THREAD" | "DATUM" => Some(1),
            "SHAPE_ASPECT" | "DATUM_FEATURE" | "COMPOSITE_SHAPE_ASPECT"
                if string(attributes.get(1), "") =>
            {
                Some(1)
            }
            "COMPOSITE_GROUP_SHAPE_ASPECT" if string(attributes.first(), "") => Some(0),
            "TURNED_KNURL" => Some(0),
            _ => None,
        };
        if let Some(slot) = slot {
            let mut record = entity.clone();
            if let RawEntity::Simple { attributes, .. } = &mut record {
                attributes[slot] = Attribute::String(MARKER.into());
            }
            edit.replace(id, record);
        }
    }
    edit
}

/// Alpha.8 does not consume DESCRIPTION_ATTRIBUTE. Remove our annotations of the selected
/// parts' consumed records before its replace planner runs. Foreign annotations stay intact
/// and retain the upstream planner's refusal behaviour.
pub fn before_replace(doc: &Document, parts: &[PartProvenance]) -> Edit {
    let targets: BTreeSet<_> = parts.iter().flat_map(PartProvenance::all).collect();
    let mut edit = Edit::new();
    for (&id, entity) in &doc.graph().entities {
        if description_target(entity).is_some_and(|target| targets.contains(&target)) {
            edit.remove(id);
        }
    }
    edit
}

/// Check the no-op shortcut only after every semantic item has gained its marker.
pub fn all(p: &PartPmi, prov: &PartProvenance, doc: &Document) -> bool {
    prov.dimensions.iter().all(|ids| dimension(doc, ids))
        && p.tolerances
            .iter()
            .all(|t| t.description.as_deref() == Some(MARKER))
        && [
            (&prov.features, "feature"),
            (&prov.datums, "datum"),
            (&prov.general, "general"),
            (&prov.threads, "thread"),
            (&prov.knurls, "knurl"),
            (&prov.surface_textures, "surface texture"),
            (&prov.attributes, "attribute"),
        ]
        .iter()
        .all(|(items, kind)| items.iter().all(|ids| marked(doc, ids, kind)))
        && (p.material.is_none() || marked(doc, &prov.material, "material"))
        && p.holes.is_empty()
        && p.notes.is_empty()
        && p.datum_targets.is_empty()
        && p.tolerance_relations.is_empty()
        && p.standards.is_empty()
        && prov.geometry_items.len()==p.geometry.len()
        && prov.geometry_items.iter().all(|id|matches!(doc.get(*id),Some(RawEntity::Simple {name,attributes,..}) if name=="CARTESIAN_POINT" && string(attributes.first(),MARKER)))
        && p.decimal_places.is_none()
}
