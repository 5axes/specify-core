//! Requirement changes on the same semantic documents and identities as `pmiScene`.
use super::{Bind, Feature, PmiItemChange, pmi_on};
use crate::{api, part::LoadedPart, pmi_scene::Scene};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

struct Item {
    id: String,
    number: Option<u64>,
    kind: String,
    match_kind: String,
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
        let cells = if notation.is_some() {
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

fn mapped(item: &Item, faces: &BTreeMap<usize, Bind>) -> Option<Vec<usize>> {
    let mut result = Vec::new();
    for face in &item.faces {
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

fn compare(
    old: &[Item],
    new: &[Item],
    faces: &BTreeMap<usize, Bind>,
    old_features: &[Feature],
    new_features: &[Feature],
) -> Vec<PmiItemChange> {
    let mut pairs = BTreeMap::new();
    let mut used = BTreeSet::new();
    // Complete each stronger pass before any weaker match can consume its candidate.
    for pass in 0..4 {
        for (i, was) in old
            .iter()
            .enumerate()
            .filter(|(i, _)| !pairs.contains_key(i))
            .collect::<Vec<_>>()
        {
            let mapped = mapped(was, faces);
            let candidates: Vec<_> = new
                .iter()
                .enumerate()
                .filter(|(j, now)| {
                    !used.contains(j)
                        && match pass {
                            0 => was.number.is_some() && was.number == now.number,
                            1 => was.id == now.id,
                            2 => {
                                mapped.as_ref().is_some_and(|f| *f == now.faces)
                                    && !(was.number.is_some()
                                        && now.number.is_some()
                                        && was.number != now.number)
                            }
                            _ => {
                                was.match_kind == now.match_kind
                                    && !(was.number.is_some()
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
                    .filter(|(_, now)| now.kind == was.kind)
                    .collect::<Vec<_>>()
            } else {
                candidates
            };
            if let [(j, _)] = candidates.as_slice() {
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
) -> api::Result<Vec<PmiItemChange>> {
    let scene = |part: &LoadedPart, analysis: &Value, answers| {
        Scene::new(part)
            .and_then(|scene| scene.comparison(part, analysis, answers))
            .map_err(api::Error::Analysis)
    };
    let (old_analysis, new_analysis) = analyses;
    let before = scene(old, old_analysis, given)?;
    let before = items(&before);
    let projected = scene(new, new_analysis, (!carried.is_empty()).then_some(carried))?;
    let mut changes = if new_analysis["resume"].is_object() {
        let actual = scene(new, new_analysis, None)?;
        let mut changes = compare(&before, &items(&actual), faces, features.0, features.1);
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
        changes
    } else {
        compare(&before, &items(&projected), faces, features.0, features.1)
    };
    changes.sort_by(|a, b| {
        (a.number.is_none(), a.number, &a.id, &a.class).cmp(&(
            b.number.is_none(),
            b.number,
            &b.id,
            &b.class,
        ))
    });
    Ok(changes)
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
            status: "existing".into(),
            on_faces: faces.clone(),
            faces,
            text: text.into(),
            cells: json!([text]),
            semantic: json!({"magnitude":text}),
        }
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
        let result = compare(&old, &new, &faces, &[], &[]);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|c| c.class == "changed"));
        assert_eq!(result[0].id, "new-a");
        assert_eq!(result[0].old.as_deref(), Some("⏥ 0.2"));
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
        let result = compare(&old, &new, &BTreeMap::new(), &[], &[]);
        let numbered = result.iter().find(|c| c.number == Some(7)).unwrap();
        assert_eq!(numbered.old.as_deref(), Some("0.3"));
        assert_eq!(numbered.new.as_deref(), Some("0.1"));
        let old = [control("a", vec![0], "0.2"), control("b", vec![1], "0.3")];
        let new = [control("c", vec![2], "0.1"), control("d", vec![3], "0.4")];
        let result = compare(&old, &new, &BTreeMap::new(), &[], &[]);
        assert_eq!(result.iter().filter(|c| c.class == "removed").count(), 2);
        assert_eq!(result.iter().filter(|c| c.class == "added").count(), 2);
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
