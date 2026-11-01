//! Keep a requirement on one datum constituent distinct from the whole datum.
//!
//! haecceity 0.4.2 resolves plain children of an indivisible datum to its whole
//! feature (#168). Its writer can also use those children directly as independent
//! size supports (e.g. LM10UU's three separate Ø19 lands). Give such requirements
//! an ordinary composition of their own. The shared leaf still owns the only GISU
//! for its face (IIRU UR1); its second parent prevents whole-datum aliasing.
use std::collections::{BTreeMap, BTreeSet};

use haecceity::p21::{self, Attribute, Document, Edit, RawEntity};

fn is_a(entity: &RawEntity, ancestor: &str) -> bool {
    let is = |name: &str| haecceity::express::is_a(&name.to_ascii_lowercase(), ancestor);
    match entity {
        RawEntity::Simple { name, .. } => is(name),
        RawEntity::Complex { parts, .. } => parts.iter().any(|p| is(&p.name)),
    }
}

fn replace_reference(attribute: &mut Attribute, old: u64, new: &Attribute) {
    match attribute {
        Attribute::EntityRef(id) if *id == old => *attribute = new.clone(),
        Attribute::List(items) => {
            for item in items {
                replace_reference(item, old, new);
            }
        }
        Attribute::Typed { value, .. } => replace_reference(value, old, new),
        _ => {}
    }
}

pub(super) fn independent(before: &Document, after: &Document) -> Edit {
    let mut parents = BTreeMap::<u64, BTreeSet<u64>>::new();
    let mut members = BTreeMap::<u64, BTreeSet<u64>>::new();
    for entity in after.graph().entities.values() {
        if let RawEntity::Simple {
            name, attributes, ..
        } = entity
            && name == "SHAPE_ASPECT_RELATIONSHIP"
            && let [
                _,
                _,
                Attribute::EntityRef(parent),
                Attribute::EntityRef(child),
            ] = attributes.as_slice()
        {
            parents.entry(*child).or_default().insert(*parent);
            members.entry(*parent).or_default().insert(*child);
        }
    }
    let mut edit = Edit::new();
    let mut replacements = BTreeMap::new();
    for (child, parents) in parents {
        let Some(&parent) = parents.first().filter(|_| parents.len() == 1) else {
            continue;
        };
        let Some(datum) = after.get(parent) else {
            continue;
        };
        if !is_a(datum, "datum_feature")
            || !is_a(datum, "composite_shape_aspect")
            || members[&parent].len() < 2
        {
            continue;
        }
        let Some(RawEntity::Simple {
            name, attributes, ..
        }) = after.get(child)
        else {
            continue;
        };
        if name != "SHAPE_ASPECT" {
            continue;
        }
        let roots: Vec<_> = after
            .referrers(child)
            .iter()
            .copied()
            .filter(|id| {
                before.get(*id).is_none()
                    && after.get(*id).is_some_and(|entity| {
                        [
                            "dimensional_size",
                            "dimensional_location",
                            "geometric_tolerance",
                            "property_definition",
                        ]
                        .iter()
                        .any(|ty| is_a(entity, ty))
                    })
            })
            .collect();
        if roots.is_empty() {
            continue;
        }
        let wrapper = edit.add(p21::simple("SHAPE_ASPECT", attributes.clone()));
        edit.add(p21::simple(
            "SHAPE_ASPECT_RELATIONSHIP",
            vec![
                Attribute::String(String::new()),
                Attribute::String(String::new()),
                wrapper.to_ref(),
                Attribute::EntityRef(child),
            ],
        ));
        for id in roots {
            let entity = replacements
                .entry(id)
                .or_insert_with(|| after.get(id).unwrap().clone());
            match entity {
                RawEntity::Simple { attributes, .. } => {
                    for a in attributes {
                        replace_reference(a, child, &wrapper.to_ref());
                    }
                }
                RawEntity::Complex { parts, .. } => {
                    for p in parts {
                        for a in &mut p.attributes {
                            replace_reference(a, child, &wrapper.to_ref());
                        }
                    }
                }
            }
        }
    }
    for (id, entity) in replacements {
        edit.replace(id, entity);
    }
    edit
}
