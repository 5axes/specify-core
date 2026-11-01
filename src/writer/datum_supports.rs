//! Temporary workaround: keep a datum constituent distinct from the whole datum.
//!
//! haecceity 0.4.2 resolves plain children of an indivisible datum to its whole
//! feature (#168). Its writer can also use those children directly as independent
//! size supports (e.g. LM10UU's three separate Ø19 lands). Give such requirements
//! an ordinary composition of their own. The shared leaf still owns the only GISU
//! for its face (IIRU UR1); its second parent prevents whole-datum aliasing.
//!
//! Remove this adjustment when haecceity resolves multi-child datum constituents
//! itself (planned for 0.4.3): a plain child keeps its own identity unless its face
//! set is the whole datum. This compensates specifically for the broader #168
//! child-to-datum aliasing in 0.4.2.
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

#[cfg(test)]
mod tests {
    use super::*;
    use haecceity::pmi::write::{Mode, PresentationPolicy};
    use haecceity::pmi::{self, DimensionKind, Feature, PartId, SizeKind};

    #[test]
    fn remove_workaround_when_upstream_preserves_independent_datum_children() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/freecad/bearing-lm10uu.step.gz");
        let loaded = crate::part::load(&path, None).unwrap();
        let analysis = crate::api::analyse(&loaded).unwrap();
        let answers = serde_json::json!({"part.material":"6061-T6"});
        let intent =
            crate::rules::apply_with_defaults(&analysis, answers.as_object().unwrap(), true)
                .unwrap();
        let source = super::super::Source::of(&loaded.bytes).unwrap();
        let (model, _) =
            super::super::part_pmi(&loaded, &intent, &source.read.parts[0], Mode::Replace).unwrap();
        let (edit, _) = pmi::write(
            &source.doc,
            &source.defs,
            &[(PartId(0), model)],
            Mode::Replace,
            PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        let raw = Document::parse(source.doc.apply(&edit).unwrap().bytes).unwrap();
        let read = pmi::read(&raw, &source.defs).unwrap();
        let lands: Vec<_> = read.parts[0]
            .dimensions
            .iter()
            .filter_map(|d| {
                let DimensionKind::Size {
                    feature,
                    kind: SizeKind::Diameter,
                    ..
                } = d.kind
                else {
                    return None;
                };
                let nominal = d.nominal.as_ref()?;
                if !matches!(&nominal.quantity, pmi::Quantity::Length(l) if l.mm() == 19.0) {
                    return None;
                }
                let Feature::Items(items) = &read.parts[0].features[feature.0] else {
                    panic!("expected face-supported lands");
                };
                Some(items.len())
            })
            .collect();
        assert_eq!(
            lands,
            [3, 3, 3],
            "haecceity no longer aliases the three Ø19 lands to the whole datum: \
             remove writer::datum_supports and its call (planned for 0.4.3)"
        );
    }
}
