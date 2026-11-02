//! Requirement changes on the same semantic documents and identities as `pmiScene`.
use super::{Bind, FaceAnchor, Feature, PmiFaces, PmiItemChange, PmiScenes, pmi_on};
use crate::{api, part::LoadedPart, pmi_scene::Scene};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

struct Item {
    id: String,
    number: Option<u64>,
    kind: String,
    match_kind: String,
    dimension_role: Option<String>,
    reference_faces: Vec<usize>,
    on_faces: Vec<usize>,
    status: String,
    faces: Vec<usize>,
    text: String,
    cells: Value,
    semantic: Value,
}

// Only reference slots carry ids. Decimal strings and note text may themselves
// equal numeric STEP identities and must never be expanded as references.
fn reference_slot(key: &str) -> bool {
    matches!(
        key,
        "feature"
            | "from"
            | "to"
            | "path"
            | "members"
            | "datum"
            | "datum_target"
            | "targets"
            | "dimension"
            | "tolerance"
            | "geometry"
            | "on"
            | "area"
            | "curve"
            | "unit"
            | "length_unit"
            | "supplemental"
            | "relating"
            | "related"
            | "non_uniform"
            | "affected_plane"
            | "end"
            | "partial_area"
            | "runout"
            | "features"
    )
}

// Resolve support and unit identities, so file-local hashes/indices are not changes.
// Topology is matched separately, including one old face becoming several new faces.
fn resolve(
    value: &Value,
    rows: &BTreeMap<&str, (&str, &Value)>,
    depth: usize,
    reference: bool,
) -> Value {
    if depth > 32 {
        return json!("recursive support");
    }
    match value {
        Value::String(id) if reference && rows.contains_key(id.as_str()) => {
            let (collection, value) = rows[id.as_str()];
            if collection == "features" {
                json!("feature support")
            } else if ["dimensions", "tolerances", "threads", "knurls"].contains(&collection) {
                json!({"requirement support":collection})
            } else {
                resolve(value, rows, depth + 1, false)
            }
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !["face", "edge", "deriving_faces"].contains(&key.as_str()))
                .map(|(key, v)| {
                    (
                        key.clone(),
                        resolve(v, rows, depth + 1, reference_slot(key)),
                    )
                })
                .collect(),
        ),
        Value::Array(array) => Value::Array(
            array
                .iter()
                .map(|v| resolve(v, rows, depth + 1, reference))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn supports(
    value: &Value,
    rows: &BTreeMap<&str, (&str, &Value)>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<usize>,
    reference: bool,
) {
    match value {
        Value::String(id) if reference => {
            if let Some((collection, v)) = rows.get(id.as_str())
                // Datum references describe the frame, not the controlled feature.
                && !["datums", "datum_targets"].contains(collection)
                && seen.insert(id.clone())
            {
                supports(v, rows, seen, out, false);
            }
        }
        Value::Object(object) => {
            for (key, v) in object {
                if key == "face" {
                    out.extend(v.as_u64().map(|n| n as usize));
                } else if key == "deriving_faces" {
                    out.extend(
                        v.as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_u64)
                            .map(|n| n as usize),
                    );
                } else if key != "datums" {
                    supports(v, rows, seen, out, reference_slot(key));
                }
            }
        }
        Value::Array(array) => {
            for v in array {
                supports(v, rows, seen, out, reference);
            }
        }
        _ => {}
    }
}

// Reuse scene notation and feature labels; no new requirement classification.
pub(super) fn check_lines(
    checks: &[super::Check],
    features: &[Feature],
    scene: &Value,
) -> Vec<String> {
    if checks.is_empty() {
        return Vec::new();
    }
    let requirements = items(scene);
    checks
        .iter()
        .map(|c| {
            let feature = features
                .iter()
                .find(|f| f.id == c.feature)
                .map(|f| super::describe(f).0)
                .unwrap_or_else(|| c.feature.clone());
            let notation = requirements
                .iter()
                .filter(|r| scene["specify"][&r.id]["question"] == c.id)
                .map(|r| r.text.as_str())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("; ");
            let notation = if notation.is_empty() {
                super::fmt(&c.answer)
            } else {
                notation
            };
            let changes = [
                super::sizes(&c.changes),
                super::moved(&c.changes, c.axis)
                    .map(|m| format!("moved {m}"))
                    .unwrap_or_default(),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
            let changes = if changes.is_empty() {
                "changed"
            } else {
                &changes
            };
            format!("{feature} {changes}, so its {notation} requirement may no longer fit")
        })
        .collect()
}

fn items(scene: &Value) -> Vec<Item> {
    let mut rows = BTreeMap::new();
    for section in ["pmi", "units"] {
        for (collection, list) in scene["pmi"][section].as_object().into_iter().flatten() {
            for row in list.as_array().into_iter().flatten() {
                if let Some(id) = row["id"].as_str() {
                    rows.insert(id, (collection.as_str(), &row["value"]));
                }
            }
        }
    }
    let mut result = Vec::new();
    for (id, meta) in scene["specify"].as_object().into_iter().flatten() {
        let Some(&(collection, value)) = rows.get(id.as_str()) else {
            continue;
        };
        if collection == "attributes"
            && [
                crate::pmi_origin::PROPERTY,
                crate::characteristics::PROPERTY,
                "specify dimension plan",
            ]
            .contains(&value["name"].as_str().unwrap_or_default())
        {
            continue;
        }
        let graphics = &scene["presentation"]["items"][id];
        let notation = meta["notation"].as_str();
        let text = notation.map(str::to_owned).unwrap_or_else(|| {
            graphics
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|g| g["name"].as_str())
                .collect::<Vec<_>>()
                .join("; ")
        });
        let kind = match collection {
            "dimensions" => "dimension",
            "tolerances" => "geometric_tolerance",
            "datums" => "datum",
            "notes" | "attributes" => "note",
            "surface_textures" => "surface_texture",
            "threads" => "thread",
            "knurls" => "knurl",
            "general" => "general_tolerance",
            "datum_targets" => "datum_target",
            other => other,
        };
        let text = if text.is_empty() && notation.is_none() {
            // Some semantic requirements (e.g. manufacturing attributes) have no callout.
            value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or(kind)
                .to_owned()
        } else {
            text
        };
        let cells = if notation.is_some() && collection != "dimensions" {
            // The block's cells include other requirements. Own notation and
            // semantic fields already capture every change to this note.
            json!([])
        } else {
            json!(
                graphics
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|g| g["text"].clone())
                    .collect::<Vec<_>>()
            )
        };
        let mut faces = Vec::new();
        supports(value, &rows, &mut BTreeSet::new(), &mut faces, false);
        faces.sort_unstable();
        faces.dedup();
        result.push(Item {
            id: id.clone(),
            number: meta["number"].as_u64(),
            kind: kind.into(),
            match_kind: format!(
                "{kind}:{}",
                if collection == "dimensions" {
                    value
                        .pointer("/kind/size/kind")
                        .or_else(|| value.pointer("/kind/location/kind"))
                        .unwrap_or(&Value::Null)
                } else if collection == "tolerances" {
                    &value["kind"]
                } else {
                    &Value::Null
                }
            ),
            dimension_role: (collection == "dimensions")
                .then(|| meta["source"]["parameter"].as_str().map(str::to_owned))
                .flatten(),
            reference_faces: {
                let mut reference = Vec::new();
                if let Some(from) = value.pointer("/kind/location/from") {
                    supports(from, &rows, &mut BTreeSet::new(), &mut reference, true);
                }
                reference.sort_unstable();
                reference.dedup();
                reference
            },
            on_faces: {
                let mut primary = Vec::new();
                let target = value.pointer("/kind/location/to").unwrap_or(value);
                supports(
                    target,
                    &rows,
                    &mut BTreeSet::new(),
                    &mut primary,
                    value.pointer("/kind/location/to").is_some(),
                );
                primary.sort_unstable();
                primary.dedup();
                primary
            },
            status: meta["status"].as_str().unwrap_or("existing").into(),
            faces,
            text,
            cells,
            semantic: {
                let mut semantic = resolve(value, &rows, 0, false);
                if collection == "attributes" {
                    // AP242's UDA writer fills these boilerplate descriptors when
                    // the plan leaves them unset; they are the same requirement.
                    if semantic["description"] == "user defined attribute" {
                        semantic["description"] = Value::Null;
                    }
                    if semantic["general_property"]
                        == json!({"id":"", "name":semantic["name"], "description":null})
                    {
                        semantic["general_property"] = Value::Null;
                    }
                }
                semantic
            },
        });
    }
    result
}

fn mapped(support: &[usize], faces: &BTreeMap<usize, Bind>) -> Option<Vec<usize>> {
    let mut result = Vec::new();
    for face in support {
        match faces.get(face) {
            Some(Bind::To(face)) => result.push(*face),
            Some(Bind::Ambiguous(candidates)) if !candidates.is_empty() => {
                result.extend(candidates)
            }
            _ => return None,
        }
    }
    result.sort_unstable();
    result.dedup();
    Some(result)
}

// A growth match is weaker than exact support. Keep location endpoints separate:
// the same hole can support both a member pitch and a location to the whole pattern.
// Stated measurement roles distinguish bore/counterbore sizes and location axes;
// values, generated ids and fresh characteristic numbers are never growth evidence.
fn support_growth(was: &Item, now: &Item, faces: &BTreeMap<usize, Bind>) -> bool {
    if was.match_kind != now.match_kind
        || was.dimension_role.is_none()
        || was.dimension_role != now.dimension_role
    {
        return false;
    }
    let subset = |old: &[usize], new: &[usize]| {
        mapped(old, faces).is_some_and(|mapped| {
            if mapped.is_empty() {
                new.is_empty()
            } else {
                mapped.iter().all(|face| new.contains(face))
            }
        })
    };
    mapped(&was.faces, faces).is_some_and(|mapped| {
        !mapped.is_empty()
            && mapped.len() < now.faces.len()
            && mapped.iter().all(|face| now.faces.contains(face))
    }) && subset(&was.on_faces, &now.on_faces)
        && subset(&was.reference_faces, &now.reference_faces)
}

fn compare(
    old: &[Item],
    new: &[Item],
    faces: &BTreeMap<usize, Bind>,
    old_features: &[Feature],
    new_features: &[Feature],
    carried_numbers: bool,
) -> Vec<PmiItemChange> {
    let mut pairs = BTreeMap::new();
    let mut used = BTreeSet::new();
    // Complete each stronger pass before any weaker match can consume its candidate.
    for pass in usize::from(!carried_numbers)..5 {
        for (i, was) in old
            .iter()
            .enumerate()
            .filter(|(i, _)| !pairs.contains_key(i))
            .collect::<Vec<_>>()
        {
            let mapped = mapped(&was.faces, faces);
            let candidates: Vec<_> = new
                .iter()
                .enumerate()
                .filter(|(j, now)| {
                    !used.contains(j)
                        && match pass {
                            0 => was.number.is_some() && was.number == now.number,
                            1 => {
                                was.id == now.id
                                    && ((carried_numbers
                                        && was.number.is_some()
                                        && now.number.is_some())
                                        || mapped.as_ref().is_none_or(|f| *f == now.faces))
                            }
                            2 => {
                                mapped.as_ref().is_some_and(|f| *f == now.faces)
                                    && !(carried_numbers
                                        && was.number.is_some()
                                        && now.number.is_some()
                                        && was.number != now.number)
                            }
                            3 => {
                                support_growth(was, now, faces)
                                    && !(carried_numbers
                                        && was.number.is_some()
                                        && now.number.is_some()
                                        && was.number != now.number)
                            }
                            _ => {
                                was.match_kind == now.match_kind
                                    && !(carried_numbers
                                        && was.number.is_some()
                                        && now.number.is_some()
                                        && was.number != now.number)
                                    && old
                                        .iter()
                                        .enumerate()
                                        .filter(|(k, other)| {
                                            !pairs.contains_key(k)
                                                && other.match_kind == was.match_kind
                                        })
                                        .count()
                                        == 1
                            }
                        }
                })
                .collect();
            let candidates = if candidates.len() > 1 || pass == 2 {
                candidates
                    .into_iter()
                    .filter(|(_, now)| {
                        if pass == 2 {
                            now.match_kind == was.match_kind
                        } else {
                            now.kind == was.kind
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                candidates
            };
            if let [(j, now)] = candidates.as_slice() {
                // Require uniqueness in both directions, before consuming any
                // growth candidate. Two old requirements may not claim one new row.
                if pass == 3
                    && old
                        .iter()
                        .enumerate()
                        .filter(|(k, other)| {
                            !pairs.contains_key(k)
                                && support_growth(other, now, faces)
                                && !(carried_numbers
                                    && other.number.is_some()
                                    && now.number.is_some()
                                    && other.number != now.number)
                        })
                        .count()
                        != 1
                {
                    continue;
                }
                pairs.insert(i, *j);
                used.insert(*j);
            }
        }
    }
    let change = |class: &str, was: Option<&Item>, now: Option<&Item>, reason: Option<String>| {
        let Some(item) = now.or(was) else {
            unreachable!("a change has an item")
        };
        PmiItemChange {
            class: class.into(),
            number: item.number,
            id: item.id.clone(),
            old_id: was
                .zip(now)
                .filter(|(a, b)| a.id != b.id)
                .map(|(a, _)| a.id.clone()),
            faces: PmiFaces {
                old: was
                    .into_iter()
                    .flat_map(|p| &p.faces)
                    .map(|&face| FaceAnchor { face })
                    .collect(),
                new: now
                    .into_iter()
                    .flat_map(|p| &p.faces)
                    .map(|&face| FaceAnchor { face })
                    .collect(),
            },
            kind: item.kind.clone(),
            status: item.status.clone(),
            on: pmi_on(
                &item.on_faces,
                if now.is_some() {
                    new_features
                } else {
                    old_features
                },
            ),
            old: was.map(|p| p.text.clone()),
            new: now.map(|p| p.text.clone()),
            reason,
        }
    };
    let mut result = Vec::new();
    for (i, was) in old.iter().enumerate() {
        if let Some(&j) = pairs.get(&i) {
            let now = &new[j];
            let support_changed = was
                .faces
                .iter()
                .map(|face| match faces.get(face) {
                    Some(Bind::To(face)) => Some(*face),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .is_some_and(|mut mapped| {
                    mapped.sort_unstable();
                    mapped.dedup();
                    mapped != now.faces
                });
            if was.text != now.text
                || was.cells != now.cells
                || was.semantic != now.semantic
                || support_changed
            {
                let reason = (was.text == now.text).then(|| {
                    if support_changed {
                        "support changed".into()
                    } else {
                        "semantic fields changed".into()
                    }
                });
                result.push(change("changed", Some(was), Some(now), reason));
            }
            if was.number != now.number {
                result.push(change(
                    "renumbered",
                    Some(was),
                    Some(now),
                    Some(format!(
                        "characteristic number {} → {}",
                        was.number.map_or("none".into(), |n| format!("#{n}")),
                        now.number.map_or("none".into(), |n| format!("#{n}"))
                    )),
                ));
            }
        } else {
            result.push(change(
                "removed",
                Some(was),
                None,
                was.number.map(|_| "retired".into()),
            ));
        }
    }
    for (_, now) in new.iter().enumerate().filter(|(j, _)| !used.contains(j)) {
        result.push(change("added", None, Some(now), None));
    }
    result.sort_by(|a, b| {
        (a.number.is_none(), a.number, &a.id, &a.class).cmp(&(
            b.number.is_none(),
            b.number,
            &b.id,
            &b.class,
        ))
    });
    result
}

pub(super) fn diff(
    old: &LoadedPart,
    new: &LoadedPart,
    analyses: (&Value, &Value),
    given: Option<&Map<String, Value>>,
    carried: &Map<String, Value>,
    faces: &BTreeMap<usize, Bind>,
    features: (&[Feature], &[Feature]),
) -> api::Result<(Vec<PmiItemChange>, PmiScenes)> {
    let destination = Scene::new(new).map_err(api::Error::Analysis)?;
    let carried_numbers = destination
        .carries_numbers(carried)
        .map_err(api::Error::Analysis)?;
    let scene = |part: &LoadedPart, analysis: &Value, answers| {
        if std::ptr::eq(part, new) {
            destination.comparison(part, analysis, answers)
        } else {
            Scene::new(part).and_then(|scene| scene.comparison(part, analysis, answers))
        }
        .map_err(api::Error::Analysis)
    };
    let (old_analysis, new_analysis) = analyses;
    let before = scene(old, old_analysis, given)?;
    let before_items = items(&before);
    let projected = scene(new, new_analysis, (!carried.is_empty()).then_some(carried))?;
    let (mut changes, after) = if new_analysis["resume"].is_object() {
        let actual = scene(new, new_analysis, None)?;
        let mut changes = compare(
            &before_items,
            &items(&actual),
            faces,
            features.0,
            features.1,
            carried_numbers,
        );
        // Compare the two plans too: generated requirements can follow geometry
        // without any answer changing. File edits own the final requirement when
        // they differ; otherwise retain the geometry-derived change from the plan.
        let source = given.or_else(|| old_analysis["resume"]["answers"].as_object());
        if let Some(source) = source {
            let old_plan = scene(old, old_analysis, Some(source))?;
            for mut change in compare(
                &items(&old_plan),
                &items(&projected),
                faces,
                features.0,
                features.1,
                carried_numbers,
            ) {
                if !changes.iter().any(|actual| {
                    actual.id == change.id
                        || (actual.number.is_some() && actual.number == change.number)
                }) {
                    change.reason = Some("carried requirement differs from the new file".into());
                    changes.push(change);
                }
            }
        }
        (changes, actual)
    } else {
        (
            compare(
                &before_items,
                &items(&projected),
                faces,
                features.0,
                features.1,
                carried_numbers,
            ),
            projected,
        )
    };
    changes.sort_by(|a, b| {
        (a.number.is_none(), a.number, &a.id, &a.class).cmp(&(
            b.number.is_none(),
            b.number,
            &b.id,
            &b.class,
        ))
    });
    Ok((
        changes,
        PmiScenes {
            old: before,
            new: after,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(id: &str, faces: Vec<usize>, text: &str) -> Item {
        Item {
            id: id.into(),
            number: None,
            kind: "geometric_tolerance".into(),
            match_kind: "flatness".into(),
            dimension_role: None,
            reference_faces: Vec::new(),
            status: "existing".into(),
            on_faces: faces.clone(),
            faces,
            text: text.into(),
            cells: json!([text]),
            semantic: json!({"magnitude":text}),
        }
    }

    fn dimension(
        id: &str,
        target: Vec<usize>,
        reference: Vec<usize>,
        role: &str,
        text: &str,
    ) -> Item {
        let mut faces = target.clone();
        faces.extend(&reference);
        faces.sort_unstable();
        faces.dedup();
        let mut item = control(id, faces, text);
        item.kind = "dimension".into();
        item.match_kind = if role.starts_with("location.") {
            "dimension:location"
        } else {
            "dimension:diameter"
        }
        .into();
        item.dimension_role = Some(role.into());
        item.on_faces = target;
        item.reference_faces = reference;
        item
    }

    #[test]
    fn exact_support_reserves_its_candidate_before_support_growth() {
        let mut old = [
            dimension("old-grow", vec![0], vec![], "bore.diameter", "6"),
            dimension("old-exact", vec![0, 1], vec![], "bore.diameter", "8"),
        ];
        let mut new = [
            dimension("new-grow", vec![10, 12], vec![], "bore.diameter", "6"),
            dimension("new-exact", vec![10, 11], vec![], "bore.diameter", "8"),
        ];
        old[0].number = Some(1);
        old[1].number = Some(2);
        new[0].number = Some(20);
        new[1].number = Some(2);
        let faces = BTreeMap::from([(0, Bind::To(10)), (1, Bind::To(11))]);
        let changes = compare(&old, &new, &faces, &[], &[], false);
        assert_eq!(changes.len(), 2);
        assert!(
            changes
                .iter()
                .all(|c| c.old_id.as_deref() == Some("old-grow") && c.id == "new-grow")
        );
        assert!(
            changes
                .iter()
                .any(|c| c.class == "changed" && c.reason.as_deref() == Some("support changed"))
        );
        assert!(changes.iter().any(|c| c.class == "renumbered"));
        // #96 still keeps differently numbered items distinct on weak matches.
        let carried = compare(&old, &new, &faces, &[], &[], true);
        assert_eq!(carried.len(), 2);
        assert!(
            carried
                .iter()
                .any(|c| c.class == "removed" && c.id == "old-grow")
        );
        assert!(
            carried
                .iter()
                .any(|c| c.class == "added" && c.id == "new-grow")
        );
    }

    #[test]
    fn growth_requires_a_unique_candidate_in_both_directions() {
        let old = [
            dimension("old-a", vec![0], vec![], "bore.diameter", "6"),
            dimension("old-b", vec![1], vec![], "bore.diameter", "8"),
        ];
        let new = [
            dimension("new-a", vec![10, 11], vec![], "bore.diameter", "6"),
            dimension("new-b", vec![10, 12], vec![], "bore.diameter", "8"),
        ];
        let faces = BTreeMap::from([(0, Bind::To(10)), (1, Bind::To(11))]);
        for changes in [
            compare(&old, &new[..1], &faces, &[], &[], false),
            compare(&old[..1], &new, &faces, &[], &[], false),
        ] {
            assert_eq!(changes.len(), 3);
            assert!(
                changes
                    .iter()
                    .all(|c| c.class == "added" || c.class == "removed")
            );
        }
    }

    #[test]
    fn growth_checks_measurement_roles_and_both_location_endpoints() {
        let faces = BTreeMap::from([(0, Bind::To(10)), (1, Bind::To(11))]);
        let bore = dimension("old", vec![0], vec![], "bore.diameter", "6");
        let mut new = dimension("new", vec![10, 12], vec![], "counterbore.diameter", "6");
        assert!(!support_growth(&bore, &new, &faces));
        new.dimension_role = None;
        assert!(!support_growth(&bore, &new, &faces));
        let pitch = dimension("pitch", vec![1], vec![0], "location.length.x", "50");
        let location = dimension(
            "location",
            vec![10, 11, 12],
            vec![20],
            "location.length.x",
            "50",
        );
        assert!(!support_growth(&pitch, &location, &faces));
        let grown_pitch = dimension(
            "grown-pitch",
            vec![11, 13],
            vec![10, 12],
            "location.length.x",
            "50",
        );
        assert!(support_growth(&pitch, &grown_pitch, &faces));
    }

    #[test]
    fn multiple_split_supports_match_without_falling_back_to_unique_kind() {
        let old = [
            control("old-a", vec![0], "⏥ 0.2"),
            control("old-b", vec![1], "⏥ 0.4"),
        ];
        let new = [
            control("new-b", vec![20, 21], "⏥ 0.3"),
            control("new-a", vec![10, 11], "⏥ 0.1"),
        ];
        let faces = BTreeMap::from([
            (0, Bind::Ambiguous(vec![10, 11])),
            (1, Bind::Ambiguous(vec![20, 21])),
        ]);
        let result = compare(&old, &new, &faces, &[], &[], true);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|c| c.class == "changed"));
        assert_eq!(result[0].id, "new-a");
        assert_eq!(result[0].old.as_deref(), Some("⏥ 0.2"));
    }

    #[test]
    fn fresh_numbers_cannot_swap_two_same_kind_dimensions() {
        let mut old = [
            control("length", vec![0], "37.6"),
            control("shoulder", vec![1], "36.1"),
        ];
        old[0].number = Some(5);
        old[1].number = Some(6);
        let mut new = [
            control("length", vec![10], "37.6"),
            control("shoulder", vec![11], "36.1"),
        ];
        new[0].number = Some(6);
        new[1].number = Some(5);
        let faces = BTreeMap::from([(0, Bind::To(10)), (1, Bind::To(11))]);
        for renamed in [false, true] {
            if renamed {
                new[0].id = "new-length".into();
                new[1].id = "new-shoulder".into();
            }
            let changes = compare(&old, &new, &faces, &[], &[], false);
            assert_eq!(changes.len(), 2);
            assert!(
                changes
                    .iter()
                    .all(|c| c.class == "renumbered" && c.old == c.new)
            );
            assert!(
                changes
                    .iter()
                    .any(|c| c.reason.as_deref() == Some("characteristic number #5 → #6"))
            );
            assert!(
                changes
                    .iter()
                    .any(|c| c.reason.as_deref() == Some("characteristic number #6 → #5"))
            );
        }
    }

    #[test]
    fn a_diameter_cannot_consume_a_location_on_the_same_faces() {
        let mut old = control("pitch", vec![0], "5");
        old.kind = "dimension".into();
        old.match_kind = "dimension:linear_distance".into();
        let mut new = control("bore", vec![10], "⌀2.5");
        new.kind = "dimension".into();
        new.match_kind = "dimension:diameter".into();
        let faces = BTreeMap::from([(0, Bind::To(10))]);
        let changes = compare(&[old], &[new], &faces, &[], &[], false);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().any(|c| c.class == "added"));
        assert!(changes.iter().any(|c| c.class == "removed"));
    }

    #[test]
    fn numbers_reserve_matches_before_ids_and_ambiguous_kinds_stay_unmatched() {
        let mut old = [
            control("same-id", vec![0], "0.2"),
            control("old-b", vec![1], "0.3"),
        ];
        old[1].number = Some(7);
        let mut new = [
            control("same-id", vec![2], "0.1"),
            control("new-b", vec![3], "0.4"),
        ];
        new[0].number = Some(7);
        let result = compare(&old, &new, &BTreeMap::new(), &[], &[], true);
        let numbered = result.iter().find(|c| c.number == Some(7)).unwrap();
        assert_eq!(numbered.old.as_deref(), Some("0.3"));
        assert_eq!(numbered.new.as_deref(), Some("0.1"));
        let old = [control("a", vec![0], "0.2"), control("b", vec![1], "0.3")];
        let new = [control("c", vec![2], "0.1"), control("d", vec![3], "0.4")];
        let result = compare(&old, &new, &BTreeMap::new(), &[], &[], true);
        assert_eq!(result.iter().filter(|c| c.class == "removed").count(), 2);
        assert_eq!(result.iter().filter(|c| c.class == "added").count(), 2);
    }

    #[test]
    fn old_id_is_only_emitted_when_the_paired_identity_differs() {
        let old = [control("old", vec![0], "0.2")];
        let mut new = [control("new", vec![0], "0.1")];
        new[0].number = Some(7);
        let faces = BTreeMap::from([(0, Bind::To(0))]);
        let changes = compare(&old, &new, &faces, &[], &[], true);
        assert_eq!(changes.len(), 2);
        for class in ["changed", "renumbered"] {
            let item = changes.iter().find(|c| c.class == class).unwrap();
            assert_eq!(item.old_id.as_deref(), Some("old"));
            assert_eq!(item.id, "new");
        }
        new[0].id = "old".into();
        for item in compare(&old, &new, &faces, &[], &[], true) {
            assert!(serde_json::to_value(item).unwrap().get("old_id").is_none());
        }
    }

    #[test]
    fn numeric_decimal_and_note_strings_are_not_step_references() {
        let feature = json!({"items":[{"face":4}]});
        let rows = BTreeMap::from([("7", ("features", &feature))]);
        let requirement = json!({"nominal":{"value":"7"}, "text":"7", "on":"7"});
        let resolved = resolve(&requirement, &rows, 0, false);
        assert_eq!(resolved["nominal"]["value"], "7");
        assert_eq!(resolved["text"], "7");
        let mut faces = Vec::new();
        supports(
            &json!({"text":"7"}),
            &rows,
            &mut BTreeSet::new(),
            &mut faces,
            false,
        );
        assert!(faces.is_empty());
        supports(&requirement, &rows, &mut BTreeSet::new(), &mut faces, false);
        assert_eq!(faces, [4]);
    }
}
