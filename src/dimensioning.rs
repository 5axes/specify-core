//! Boundary to quiddity's shared requirement planner (#75). Shared rules select
//! holes, patterns and plane levels; this adapter reconciles intent, existing
//! thread carriers, physical PMI supports and retained later-slice dimensions.
//!
//! Analyses carry the typed recognition input so rules can plan without reopening geometry.
//! Older snapshots are upgraded at the writer boundary using the loaded part. Legacy rows
//! survive only for explicitly deferred owners (including round Cartesian envelopes).
use std::collections::{BTreeMap, BTreeSet};

use quiddity::dimensioning::{
    self as shared, Datum, Input, Kind, Policy, Required, Support, Tolerance,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::rules::Intent;

// quiddity's hole/enlargement depths are published to 0.01 mm. Ownership and
// reference checks must account for that quantisation, not invent a new floor.
const DEPTH_NEAR: f64 = 0.0051;

pub(crate) fn input(part: &haecceity::Part) -> Result<Input, String> {
    Input::from_recognition(part, vec![], Policy::default())
}

fn ids(r: &Map<String, Value>, key: &str) -> Vec<usize> {
    r.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()))
        .collect()
}
fn overlaps(a: &[usize], b: &[usize]) -> bool {
    a.iter().any(|i| b.contains(i))
}
fn on_holes(input: &Input, faces: &[usize]) -> bool {
    input
        .holes
        .iter()
        .any(|h| overlaps(faces, &h.support.faces))
}
fn deferred(input: &Input, r: &Map<String, Value>) -> bool {
    let faces = ids(r, "faces");
    // An unanchored legacy requirement remains a diagnostic, never an invented support.
    if faces.is_empty() {
        return true;
    }
    if on_holes(input, &faces) {
        // Slice one's location grammar requires principal drilling directions.
        // Oblique hole locations keep their existing physical reference path.
        return r.get("kind").and_then(Value::as_str) == Some("location")
            && input
                .holes
                .iter()
                .filter(|h| overlaps(&faces, &h.support.faces))
                .any(|h| h.direction.iter().filter(|v| v.abs() >= 1e-6).count() != 1);
    }
    if r.get("kind").and_then(Value::as_str) == Some("size") {
        return true;
    }
    if !input.cartesian_envelope {
        return true;
    }
    // Nonprincipal plane directions are outside the shared Cartesian ladder.
    let physical_principal = |faces: &[usize]| {
        !faces.is_empty()
            && input
                .planes
                .iter()
                .any(|p| faces.iter().all(|f| p.support.faces.contains(f)))
    };
    if r.get("kind").and_then(Value::as_str) == Some("location")
        && (!physical_principal(&faces) || !physical_principal(&ids(r, "reference")))
    {
        return true;
    }
    input.deferred.iter().any(|d| overlaps(&faces, &d.faces))
        // Bounded shoulder stations need a point/edge-specific PMI aspect. Keep
        // their old physical-plane witnesses until that lowering is supported.
        || input.planes.iter().any(|p| overlaps(&faces,&p.support.faces)
            && input.steps.shoulders.iter().any(|s| s.axis == p.axis && s.body == p.body
                && (s.support.at[p.axis]-p.support.at[p.axis]).abs() <= 0.00051))
        || input.steps.heights.iter().any(|s| overlaps(&faces,&s.faces)
            && plane_mismatch(input,s,input.steps.axis))
}
fn plane_mismatch(input: &Input, s: &Support, axis: usize) -> bool {
    if !s.plane || s.faces.is_empty() {
        return false;
    }
    let physical = input.planes.iter().find(|p| {
        p.axis == axis
            && p.support.body == s.body
            && s.faces.iter().all(|f| p.support.faces.contains(f))
    });
    physical.is_none_or(|p| (p.support.at[axis] - s.at[axis]).abs() > 1e-4)
}

fn pitch_witnesses(input: &Input, r: &Required) -> Result<Option<[Support; 2]>, String> {
    let Kind::Location { from, to, axis } = &r.kind else {
        return Ok(None);
    };
    if !matches!(r.pattern, Some(shared::PatternRole::Pitch { .. })) {
        return Ok(None);
    }
    let hole = input
        .holes
        .iter()
        .find(|h| h.support.feature == from.feature)
        .ok_or("pitch origin is not a recognised hole")?;
    let drill = hole
        .direction
        .iter()
        .position(|d| d.abs() > 0.999)
        .ok_or("pitch drilling direction is not principal")?;
    // A projected pitch may use diagonal lexical witnesses. Resolve the same
    // quantity to an aligned pair of real pattern members for semantic PMI.
    let target = input
        .holes
        .iter()
        .filter(|h| {
            r.covers.contains(&h.support.feature)
                && h.support.body == from.body
                && (h.support.at[*axis] - to.at[*axis]).abs() < 1e-4
                && (0..3)
                    .filter(|k| *k != *axis && *k != drill)
                    .all(|k| (h.support.at[k] - from.at[k]).abs() < 1e-4)
        })
        .min_by_key(|h| &h.support.feature)
        .ok_or("pitch has no aligned physical member witness")?;
    Ok((target.support.feature != to.feature).then(|| [from.clone(), target.support.clone()]))
}

fn station_path(
    edges: &[(String, usize, Support, Support)],
    body: Option<usize>,
    axis: usize,
    start: f64,
    end: f64,
    near: f64,
) -> Option<Vec<String>> {
    let mut reached = vec![(start, Vec::<String>::new())];
    let mut cursor = 0;
    while cursor < reached.len() {
        let (station, proof) = reached[cursor].clone();
        cursor += 1;
        if !proof.is_empty() && (station - end).abs() < near {
            return Some(proof);
        }
        for (id, k, a, b) in edges {
            if *k != axis || a.body != body || b.body != body {
                continue;
            }
            let next = if (a.at[axis] - station).abs() < 1e-4 {
                Some(b.at[axis])
            } else if (b.at[axis] - station).abs() < 1e-4 {
                Some(a.at[axis])
            } else {
                None
            };
            if let Some(next) = next
                && !reached.iter().any(|(at, _)| (at - next).abs() < 1e-4)
            {
                let mut by = proof.clone();
                by.push(id.clone());
                reached.push((next, by));
            }
        }
    }
    None
}

fn contract(r: &Map<String, Value>) -> Tolerance {
    if r.get("basic") == Some(&json!(true)) {
        Tolerance::Basic
    } else if r.get("general") == Some(&json!(true)) {
        Tolerance::General
    } else {
        let mut fields = vec![
            r.get("fit"),
            r.get("upper"),
            r.get("lower"),
            r.get("stored_tolerance"),
        ];
        if r.contains_key("limits") {
            fields.push(r.get("limits"));
        }
        Tolerance::Explicit(json!(fields).to_string())
    }
}
// A position frame defines its pattern geometry with Basic dimensions, even
// when an axial datum supplies no Cartesian plane-location witness.
fn location_contract(row: &Map<String, Value>) -> Map<String, Value> {
    if row.get("kind").and_then(Value::as_str) != Some("position") {
        return row.clone();
    }
    json!({"kind":"location", "feature":row.get("feature"),
        "faces":row.get("faces"), "question":row.get("question"), "basic":true})
    .as_object()
    .unwrap()
    .clone()
}

fn support(input: &Input, faces: &[usize], groups: &[Support]) -> Option<Support> {
    input
        .holes
        .iter()
        .map(|h| &h.support)
        .chain(input.planes.iter().map(|p| &p.support))
        .find(|s| !faces.is_empty() && faces.iter().all(|i| s.faces.contains(i)))
        .cloned()
        .or_else(|| {
            let members: BTreeSet<_> = input
                .holes
                .iter()
                .filter(|h| overlaps(faces, &h.support.faces))
                .map(|h| h.support.feature.as_str())
                .collect();
            let pattern = input.patterns.iter().find(|p| {
                !members.is_empty()
                    && p.members
                        .iter()
                        .map(String::as_str)
                        .collect::<BTreeSet<_>>()
                        == members
            })?;
            groups.iter().find(|s| s.feature == pattern.id()).cloned()
        })
}

/// Complete lowering information travels with a row, including support coordinates and the
/// planner's count, pattern role, reference proof and covers. Arena indices never cross JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Planned {
    pub required: Required,
    pub supports: Vec<Support>,
    pub groups: BTreeMap<String, Vec<String>>,
    pub frame: quiddity::frames::PartFrame,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witnesses: Option<[Support; 2]>,
}

impl Planned {
    /// Recognition support proves a hole's identity; a floor-bearing hole's PMI
    /// carriers use cylinders. Do this after reconciliation so carriers cannot change
    /// the planner's quantities, conflicts or existing-PMI authorities.
    pub(crate) fn bore_supports(&mut self, part: &haecceity::Part) {
        let bore = |support: &mut Support| {
            // #216 adds a planar floor to an otherwise cylindrical support.
            // Preserve the existing conical-hole grammar and carriers: only
            // floor-bearing hole supports need this compatibility lowering.
            if support.feature.starts_with("hole:")
                && support.faces.iter().any(|&i| {
                    part.faces.get(i).is_some_and(|f| {
                        matches!(f.surface, haecceity::geom::Surface::Plane { .. })
                    })
                })
            {
                support.faces.retain(|&i| {
                    part.faces.get(i).is_some_and(|f| {
                        matches!(f.surface, haecceity::geom::Surface::Cylinder { .. })
                    })
                });
            }
        };
        self.supports.iter_mut().for_each(bore);
        if let Kind::Location { from, to, .. } = &mut self.required.kind {
            bore(from);
            bore(to);
        }
        if let Some(pair) = &mut self.witnesses {
            pair.iter_mut().for_each(bore);
        }
    }
}

/// Lower the shared slice-one requirements, accounting for existing carriers and
/// reporting quantities that cannot be reconciled. Answered location *contracts*
/// apply to the pattern's own origin and pitch; old per-member basics are generated witnesses,
/// not independently authored dimensions. Explicit face-picked dimensions remain authorities.
pub(crate) fn apply(mut input: Input, intent: &mut Intent) -> Result<(), String> {
    input.datums = intent
        .datums
        .iter()
        .filter_map(|r| {
            Some(Datum {
                label: r.get("letter")?.as_str()?.into(),
                faces: ids(r, "faces"),
            })
        })
        .collect();
    let general = intent.part.get("general_tolerance").and_then(Value::as_str);
    input.policy.general_tolerance = general.is_some();
    input.specified.clear();
    // A rounded legacy radius comparison may leave an answered bore faceless.
    // Reconcile it by stable recognition identity, preserving the answered contract.
    for row in &mut intent.requirements {
        if !ids(row, "faces").is_empty() {
            continue;
        }
        let Some(feature) = row.get("feature").and_then(Value::as_str) else {
            continue;
        };
        let recovered = input
            .holes
            .iter()
            .find(|h| h.support.feature.replacen("hole:", "holes:", 1) == feature)
            .map(|h| h.support.faces.clone())
            .or_else(|| {
                if !feature.starts_with("hole_patterns.") {
                    return None;
                }
                let named: BTreeSet<usize> = feature
                    .rsplit(':')
                    .next()?
                    .split('.')
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .ok()?;
                input.patterns.iter().find_map(|p| {
                    let faces: BTreeSet<_> = input
                        .holes
                        .iter()
                        .filter(|h| p.members.contains(&h.support.feature))
                        .flat_map(|h| h.support.faces.iter().copied())
                        .collect();
                    (faces == named && !faces.is_empty()).then(|| faces.into_iter().collect())
                })
            });
        if let Some(faces) = recovered {
            row.insert("faces".into(), json!(faces));
        }
    }
    let mut authorities = BTreeMap::new();

    // Ask the planner for hole grammar independently of plane consolidation so a Basic
    // hole origin cannot first disappear into an equal General plane interval.
    let mut holes = input.clone();
    holes.steps = Default::default();
    holes.cartesian_envelope = false;
    let (hole_requirements, _) = shared::plan(&holes);
    // Resolve an already-stated pattern origin through the shared planner, including
    // origins that its all-General pass conveyed through a coincident neighbouring hole.
    let mut group_supports = Vec::new();
    for p in &input.patterns {
        let mut pattern = holes.clone();
        pattern
            .holes
            .retain(|h| p.members.contains(&h.support.feature));
        pattern.patterns = vec![p.clone()];
        for r in shared::plan(&pattern).0 {
            if let Kind::Location { to, .. } = r.kind
                && to.feature == p.id()
                && !group_supports.contains(&to)
            {
                group_supports.push(to);
            }
        }
    }
    let mut covered_locations = BTreeSet::new();
    let generated_location = |r: &Map<String, Value>| {
        r.get("kind").and_then(Value::as_str) == Some("location")
            && r.get("question")
                .and_then(Value::as_str)
                .is_some_and(|q| q.starts_with("hole.position:") || q.starts_with("hole.location:"))
    };
    // Index support witnesses once: corpus parts can contain hundreds of locations.
    let mut witnesses_by_hole: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut planes_by_question: BTreeMap<String, BTreeSet<Vec<usize>>> = BTreeMap::new();
    for (index, r) in intent.requirements.iter().enumerate().filter(|(_, r)| {
        generated_location(r)
            || r.get("kind").and_then(Value::as_str) == Some("position")
                && r.get("question")
                    .and_then(Value::as_str)
                    .is_some_and(|q| q.starts_with("hole.position:"))
    }) {
        let faces = ids(r, "faces");
        for h in input
            .holes
            .iter()
            .filter(|h| overlaps(&faces, &h.support.faces))
        {
            witnesses_by_hole
                .entry(h.support.feature.clone())
                .or_default()
                .push(index);
        }
        if !generated_location(r) {
            continue;
        }
        planes_by_question
            .entry(
                r.get("question")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
            )
            .or_default()
            .insert(ids(r, "reference"));
    }
    // Feed each answered contract to the planner before coincident-span consolidation.
    // A General neighbour must not swallow the pattern origin needed by a position FCF.
    let mut contracts: BTreeMap<String, (Map<String, Value>, Vec<String>)> = BTreeMap::new();
    for h in &input.holes {
        if let Some(index) = witnesses_by_hole
            .get(&h.support.feature)
            .and_then(|v| v.first())
        {
            // Indices above enumerate this unchanged requirement list.
            let answer = location_contract(&intent.requirements[*index]);
            let question = answer.get("question").and_then(Value::as_str).unwrap_or("");
            let planes = planes_by_question
                .get(question)
                .cloned()
                .unwrap_or_default();
            let key = json!([contract(&answer), planes]).to_string();
            contracts
                .entry(key)
                .or_insert_with(|| (answer.clone(), vec![]))
                .1
                .push(h.support.feature.clone());
        }
    }
    for (answer, members) in contracts.into_values() {
        let mut contracted = holes.clone();
        contracted
            .holes
            .retain(|h| members.contains(&h.support.feature));
        contracted
            .patterns
            .retain(|p| p.members.iter().all(|m| members.contains(m)));
        let question = answer.get("question").and_then(Value::as_str).unwrap_or("");
        let planes = planes_by_question
            .get(question)
            .cloned()
            .unwrap_or_default();
        contracted.datums.retain(|d| planes.contains(&d.faces));
        let (mut claims, conveyed) = shared::plan(&contracted);
        // Specified-wins needs every original coincident claim before it merges
        // with a neighbour. Replay only the planner's coincident conveyances,
        // never the per-member locations conveyed by a pattern's grammar.
        let coincident: BTreeSet<_> = conveyed
            .iter()
            .filter(|c| c.why.starts_with("same physical span and tolerance class"))
            .map(|c| c.id.clone())
            .collect();
        if !coincident.is_empty() {
            let subsets: Vec<Vec<String>> = contracted
                .patterns
                .iter()
                .map(|p| p.members.clone())
                .chain(
                    contracted
                        .holes
                        .iter()
                        .map(|h| vec![h.support.feature.clone()]),
                )
                .collect();
            for members in subsets {
                let mut isolated = contracted.clone();
                isolated
                    .holes
                    .retain(|h| members.contains(&h.support.feature));
                isolated.patterns.retain(|p| p.members == members);
                for r in shared::plan(&isolated).0 {
                    if coincident.contains(&r.id) && !claims.iter().any(|c| c.id == r.id) {
                        claims.push(r);
                    }
                }
            }
        }
        for mut required in claims {
            match &required.kind {
                Kind::Location { from, .. } => {
                    if !matches!(required.pattern, Some(shared::PatternRole::Pitch { .. }))
                        && !planes
                            .iter()
                            .any(|p| !p.is_empty() && p.iter().all(|i| from.faces.contains(i)))
                    {
                        continue;
                    }
                }
                // A bolt circle's diameter is radial location geometry, not the
                // member bore's size. Position intent makes it Basic as well.
                Kind::Size { role, .. } if role == "bolt_circle.diameter" => {}
                _ => continue,
            }
            let indices: BTreeSet<_> = required
                .covers
                .iter()
                .filter_map(|id| witnesses_by_hole.get(id))
                .flatten()
                .copied()
                .collect();
            let witnesses: Vec<_> = indices
                .into_iter()
                .map(|i| &intent.requirements[i])
                .collect();
            if let Kind::Location { axis, .. } = &required.kind {
                covered_locations.extend(required.covers.iter().map(|id| (id.clone(), *axis)));
            }
            let witness = witnesses
                .iter()
                .copied()
                .find(|r| match &required.kind {
                    Kind::Location { to, .. } => ids(r, "faces") == to.faces,
                    _ => false,
                })
                .or_else(|| witnesses.first().copied())
                .unwrap_or(&answer);
            let witness = location_contract(witness);
            required.id = format!("answered:{}", required.id);
            required.tolerance = contract(&witness);
            authorities.insert(required.id.clone(), witness);
            input.specified.push(required);
        }
    }
    let generation_covered = |r: &Map<String, Value>| {
        if !generated_location(r) {
            return false;
        }
        if r.get("distance")
            .and_then(Value::as_f64)
            .is_some_and(|d| d.abs() < 1e-4)
        {
            return true;
        }
        let reference = ids(r, "reference");
        let Some(plane) = input.planes.iter().find(|p| {
            !reference.is_empty() && reference.iter().all(|i| p.support.faces.contains(i))
        }) else {
            return false;
        };
        let faces = ids(r, "faces");
        let members: Vec<_> = input
            .holes
            .iter()
            .filter(|h| overlaps(&faces, &h.support.faces))
            .collect();
        !members.is_empty()
            && members
                .iter()
                .all(|h| covered_locations.contains(&(h.support.feature.clone(), plane.axis)))
    };
    for r in &intent.requirements {
        let kind = r.get("kind").and_then(Value::as_str).unwrap_or("");
        if generation_covered(r) || !matches!(kind, "size" | "location") || deferred(&input, r) {
            continue;
        }
        let faces = ids(r, "faces");
        let mut required = if kind == "size" {
            let selected: BTreeSet<_> = input
                .holes
                .iter()
                .filter(|h| overlaps(&faces, &h.support.faces))
                .map(|h| h.support.feature.clone())
                .collect();
            let whole = hole_requirements
                .iter()
                .find(|s| {
                    matches!(&s.kind, Kind::Size {role,kind,..} if
                        if r.get("stored")==Some(&json!(true)) {
                            serde_json::to_value(kind).ok().as_ref()==r.get("size_kind") && r.get("nominal").and_then(Value::as_f64).is_some_and(|n|(n-s.nominal).abs()<1e-4)
                        } else {role=="bore.diameter"})
                        && s.covers.iter().cloned().collect::<BTreeSet<_>>() == selected
                })
                .cloned();
            whole.or_else(|| {
                let feature = if selected.len() == 1 {
                    selected.first()?.clone()
                } else {
                    format!(
                        "multiple[{}]",
                        selected.iter().cloned().collect::<Vec<_>>().join(",")
                    )
                };
                Some(Required {
                    id: format!(
                        "dim:{feature}/{}",
                        r.get("size_kind")
                            .and_then(Value::as_str)
                            .unwrap_or("bore.diameter")
                    ),
                    kind: Kind::Size {
                        feature,
                        role: if r
                            .get("size_kind")
                            .and_then(Value::as_str)
                            .is_some_and(|k| k != "diameter")
                        {
                            "stored.length".into()
                        } else {
                            "bore.diameter".into()
                        },
                        kind: r
                            .get("size_kind")
                            .cloned()
                            .and_then(|v| serde_json::from_value(v).ok())
                            .unwrap_or(shared::SizeKind::Diameter),
                    },
                    nominal: r.get("nominal")?.as_f64()?,
                    count: selected.len(),
                    pattern: None,
                    tolerance: contract(r),
                    reference: None,
                    covers: selected.into_iter().collect(),
                })
            })
        } else {
            let to = support(&input, &faces, &group_supports);
            let from = support(&input, &ids(r, "reference"), &group_supports);
            match (from, to) {
                (Some(from), Some(to)) => {
                    let axis = input
                        .planes
                        .iter()
                        .find(|p| p.support == from)
                        .map(|p| p.axis)
                        .unwrap_or_else(|| {
                            (0..3)
                                .max_by(|&a, &b| {
                                    (to.at[a] - from.at[a])
                                        .abs()
                                        .total_cmp(&(to.at[b] - from.at[b]).abs())
                                })
                                .unwrap_or(0)
                        });
                    Some(Required {
                        id: format!("dim:{}/to/{}/{}", from.feature, to.feature, axis),
                        nominal: r.get("distance").and_then(Value::as_f64).unwrap_or(0.),
                        covers: vec![to.feature.clone()],
                        kind: Kind::Location { from, to, axis },
                        count: 1,
                        pattern: None,
                        tolerance: contract(r),
                        reference: None,
                    })
                }
                _ => None,
            }
        };
        if let Some(s) = &mut required {
            // Preserve the exact stated nominal and tolerance. Full-group answers use the
            // planner's group identity, so specified-wins retires its generated equivalent.
            s.id = format!(
                "{}:{}",
                if r.get("stored") == Some(&json!(true)) {
                    "stored"
                } else {
                    "specified"
                },
                s.id
            );
            if let Some(q) = r
                .get("question")
                .and_then(Value::as_str)
                .filter(|q| q.starts_with("extra."))
            {
                s.id = format!("{}:{q}", s.id);
            }
            if r.get("stored") == Some(&json!(true)) {
                s.id = format!(
                    "{}:{}:{}",
                    s.id,
                    r.get("nominal")
                        .or_else(|| r.get("distance"))
                        .unwrap_or(&Value::Null),
                    r.get("stored_tolerance").unwrap_or(&Value::Null)
                );
            }
            s.nominal = r
                .get(if kind == "size" {
                    "nominal"
                } else {
                    "distance"
                })
                .and_then(Value::as_f64)
                .unwrap_or(s.nominal);
            s.tolerance = contract(r);
            authorities.insert(s.id.clone(), r.clone());
            input.specified.push(s.clone());
        }
    }
    let (mut required, _) = shared::plan(&input);
    // The thread callout already carries these atomic drilling requirements.
    // Keep enlargements (counterbores, spotfaces, countersinks) independent.
    required.retain(|r| {
        !matches!(&r.kind, Kind::Size { role, .. }
        if matches!(role.as_str(), "bore.diameter" | "bore.depth")
        && r.covers.iter().all(|id| input.holes.iter().find(|h| &h.support.feature == id)
            .is_some_and(|h| intent.requirements.iter().any(|a|
                a.get("kind").and_then(Value::as_str) == Some("thread")
                && overlaps(&ids(a, "faces"), &h.support.faces)
                && a.get(if role == "bore.depth" { "drill_depth" } else { "drill_diameter" })
                    .and_then(Value::as_f64).is_some_and(|n| (n-r.nominal).abs()<1e-4)))))
    });
    let mut unresolved = Vec::new();
    required.retain(|r| {
        if matches!(&r.kind, Kind::Location { from,to,.. }
            if from.feature.starts_with("shoulder:") || to.feature.starts_with("shoulder:"))
        {
            unresolved.push(json!({"kind":"location", "feature":r.covers.join(","),
                "faces":[], "nominal":r.nominal,
                "not_written_reason":format!("shared planner {}: bounded shoulder station needs point/edge-specific PMI; not written; legacy physical-plane path retained where available",r.id)})
                .as_object().unwrap().clone());
            false
        } else { true }
    });
    required.retain(|r| {
        if matches!(&r.kind,Kind::Location {from,to,axis}
            if plane_mismatch(&input,from,*axis) || plane_mismatch(&input,to,*axis)) {
            unresolved.push(json!({"kind":"location","feature":r.covers.join(","),"faces":[],
                "not_written_reason":format!("shared planner {}: support has no matching physical principal-plane station; retained legacy measured plane dimension",r.id)})
                .as_object().unwrap().clone());
            false
        } else {true}
    });
    // An answered fit/location is never replaced by a General guess when the
    // principal-plane grammar cannot represent its support or complete group.
    required.retain(|r| {
        if r.tolerance != Tolerance::General { return true; }
        let requested = match &r.kind {
            Kind::Size { role,.. } if role == "bore.diameter" => "size",
            Kind::Location { to,.. } if !to.plane => "location",
            _ => return true,
        };
        let conflict = intent.requirements.iter().any(|row| {
            let kind = row.get("kind").and_then(Value::as_str);
            let position = requested == "location" && kind == Some("position")
                && matches!(&r.kind, Kind::Location { from, to, .. }
                    if !from.faces.is_empty() && !to.faces.is_empty());
            if (kind != Some(requested) && !position)
                || row.get("general") == Some(&json!(true)) { return false; }
            if requested == "size" && row.get("size_kind").and_then(Value::as_str)
                .is_some_and(|kind| kind != "diameter") { return false; }
            if let Kind::Location { axis,.. } = &r.kind
                && let Some(reference) = support(&input,&ids(row,"reference"),&group_supports)
                && let Some(plane) = input.planes.iter().find(|p| p.support == reference)
                && plane.axis != *axis { return false; }
            let mut faces = ids(row,"faces");
            if faces.is_empty() && let Some(feature) = row.get("feature").and_then(Value::as_str)
                && (feature.starts_with("holes:") || feature.starts_with("hole_patterns.")) {
                faces = feature.rsplit(':').next().unwrap_or("").split('.')
                    .filter_map(|n| n.parse().ok()).collect();
            }
            r.covers.iter().any(|id| input.holes.iter().any(|h|
                &h.support.feature == id && overlaps(&faces,&h.support.faces)
                && match &r.kind {
                    Kind::Location { to,.. } => overlaps(&to.faces,&h.support.faces),
                    _ => true,
                }))
        });
        if conflict {
            unresolved.push(json!({"kind":requested, "feature":r.covers.join(","),
                "faces":[], "nominal":r.nominal,
                "not_written_reason":format!("shared planner {}: General contract conflicts with an answered requirement; retained answered intent instead",r.id)})
                .as_object().unwrap().clone());
        }
        !conflict
    });
    required.retain(|r| {
        if !authorities.contains_key(&r.id)
            && matches!(&r.kind, Kind::Size { role,.. } if role == "bore.depth")
            && r.covers.iter().any(|id| input.holes.iter().any(|h|
                &h.support.feature == id && h.bottom == quiddity::features::holes::Bottom::Unknown))
        {
            unresolved.push(json!({"kind":"size", "feature":r.covers.join(","),
                "faces":[], "nominal":r.nominal,
                "not_written_reason":format!("shared planner {}: bottom is unknown; bore extent is not an established blind drilling depth",r.id)})
                .as_object().unwrap().clone());
            false
        } else { true }
    });
    // Synthetic envelope supports are useful to a drawing, but cannot anchor our
    // semantic location PMI. Keep the recognition deferrals and never invent faces.
    required.retain(|r| {
        !matches!(&r.kind, Kind::Location { from, to, .. }
        if from.faces.is_empty() || to.faces.is_empty())
    });
    let same_span = |row: &Map<String, Value>| {
        let from = support(&input, &ids(row, "reference"), &group_supports);
        let to = support(&input, &ids(row, "faces"), &group_supports);
        match (from, to) {
            (Some(a), Some(b)) => required.iter().any(|r| {
                matches!(&r.kind,
                Kind::Location { from, to, axis } if
                ((a.at[*axis]-from.at[*axis]).abs()<1e-4 && (b.at[*axis]-to.at[*axis]).abs()<1e-4
                || (a.at[*axis]-to.at[*axis]).abs()<1e-4 && (b.at[*axis]-from.at[*axis]).abs()<1e-4)
                && (a.at[*axis]-b.at[*axis]).abs()>1e-4
                && a.plane && b.plane && from.plane && to.plane
                && a.body == from.body && b.body == to.body)
            }),
            _ => false,
        }
    };
    // A deferred owner does not license a second statement of a planned span.
    // Hole floors belong to the drilling callout, including floors absent from
    // the legacy recogniser's defining-face list (notably the pin's flat floor).
    let hole_floor = |row: &Map<String, Value>| {
        input.planes.iter().any(|p| {
            overlaps(&p.support.faces, &ids(row, "faces"))
                && input.holes.iter().any(|h| {
                    h.bottom != quiddity::features::holes::Bottom::Unknown
                        && h.support.body == p.support.body
                        && h.depth.is_some_and(|depth| {
                            h.direction[p.axis].abs() > 0.999
                                && (p.support.at[p.axis]
                                    - h.support.at[p.axis]
                                    - h.direction[p.axis] * depth)
                                    .abs()
                                    < DEPTH_NEAR
                        })
                })
        })
    };
    intent.general.retain(|r| !same_span(r) && !hole_floor(r));

    // Check depth references against both the planner and retained legacy spans.
    // This is boundary validation, not another selection pass: all independent
    // hole/plane requirements still come from the shared planner.
    let stored_dimensions = authorities
        .values()
        .any(|r| r.get("stored") == Some(&json!(true)));
    let mut baselines: Vec<_> = intent
        .general
        .iter()
        .filter(|row| !stored_dimensions && deferred(&input, row))
        .filter_map(|row| {
            let a = support(&input, &ids(row, "reference"), &[])?;
            let b = support(&input, &ids(row, "faces"), &[])?;
            let axis = input.planes.iter().find(|p| p.support == a)?.axis;
            (a.plane && b.plane)
                .then(|| (format!("legacy:{}/to/{}", a.feature, b.feature), axis, a, b))
        })
        .chain(required.iter().filter_map(|r| {
            let Kind::Location { from, to, axis } = &r.kind else {
                return None;
            };
            if !from.plane || r.reference.is_some() {
                return None;
            }
            // A coincident point survivor can carry a physical-plane interval.
            // Its covered plane still owns an edge, just as in the shared lint.
            let plane = if to.plane {
                Some(to)
            } else {
                input
                    .planes
                    .iter()
                    .map(|p| &p.support)
                    .chain(input.steps.heights.iter())
                    .find(|s| {
                        r.covers.contains(&s.feature)
                            && s.body == to.body
                            && (s.at[*axis] - to.at[*axis]).abs() < 1e-4
                    })
            }?;
            Some((r.id.clone(), *axis, from.clone(), plane.clone()))
        }))
        .collect();
    // Keep legacy and authored edges first, then admit each planned plane span
    // only if independent. Determined generated spans remain reference quantities.
    let all_planes = baselines.clone();
    baselines.retain(|(id, _, _, _)| id.starts_with("legacy:") || authorities.contains_key(id));
    for r in &mut required {
        if authorities.contains_key(&r.id) {
            continue;
        }
        if let Some((id, axis, from, to)) = all_planes.iter().find(|(id, _, _, _)| id == &r.id) {
            if let Some(by) = station_path(
                &baselines,
                from.body,
                *axis,
                from.at[*axis],
                to.at[*axis],
                1e-4,
            ) {
                r.reference = Some(by);
            } else {
                baselines.push((id.clone(), *axis, from.clone(), to.clone()));
            }
        }
    }
    for r in &mut required {
        let Kind::Size { role, .. } = &r.kind else {
            continue;
        };
        if !role.ends_with(".depth") || authorities.contains_key(&r.id) {
            continue;
        }
        for h in input
            .holes
            .iter()
            .filter(|h| r.covers.contains(&h.support.feature))
        {
            for axis in 0..3 {
                if h.direction[axis].abs() < 0.999 {
                    continue;
                }
                let entry = h.support.at[axis];
                let end = entry + h.direction[axis] * r.nominal;
                if let Some(by) =
                    station_path(&baselines, h.support.body, axis, entry, end, DEPTH_NEAR)
                {
                    r.reference = Some(by);
                }
            }
        }
    }

    let mut supports: Vec<_> = input
        .holes
        .iter()
        .map(|h| h.support.clone())
        .chain(input.planes.iter().map(|p| p.support.clone()))
        .chain(input.steps.heights.iter().cloned())
        .chain(group_supports.iter().cloned())
        .collect();
    let mut groups = BTreeMap::new();
    for r in &required {
        if let Kind::Location { from, to, .. } = &r.kind {
            supports.extend([from.clone(), to.clone()]);
        }
        if let Kind::Size { feature, .. } = &r.kind
            && (feature.starts_with("pattern[") || feature.starts_with("multiple["))
        {
            groups.insert(
                feature.clone(),
                r.covers
                    .iter()
                    .filter(|id| input.holes.iter().any(|h| &h.support.feature == *id))
                    .cloned()
                    .collect(),
            );
        }
    }
    for p in &input.patterns {
        groups.insert(p.id(), p.members.clone());
    }
    // The helper maps IDs into the feature arena; the consumer must populate
    // groups for all coincident claims in covers, including pattern origins.
    for r in &required {
        let Kind::Location { to, axis, .. } = &r.kind else {
            continue;
        };
        if matches!(r.pattern, Some(shared::PatternRole::Pitch { .. })) {
            continue;
        }
        let mut members = BTreeSet::from([to.feature.clone()]);
        let mut consumed = BTreeSet::new();
        for p in &input.patterns {
            if p.members.iter().all(|id| r.covers.contains(id))
                && let Some(s) = group_supports.iter().find(|s| s.feature == p.id())
                && s.body == to.body
                && (s.at[*axis] - to.at[*axis]).abs() < 1e-4
            {
                members.insert(p.id());
                consumed.extend(p.members.iter().cloned());
            }
        }
        if consumed.contains(&to.feature) {
            members.remove(&to.feature);
        }
        for s in &supports {
            if r.covers.contains(&s.feature)
                && !consumed.contains(&s.feature)
                && s.body == to.body
                && (s.at[*axis] - to.at[*axis]).abs() < 1e-4
            {
                members.insert(s.feature.clone());
            }
        }
        if !members.is_empty() && (members.len() > 1 || !members.contains(&to.feature)) {
            groups.insert(
                format!("coincident:{}", r.id),
                members.into_iter().collect(),
            );
        }
    }
    let old = intent.requirements.clone();
    let is_authority = |r: &Map<String, Value>| authorities.values().any(|a| a == r);
    intent.requirements.retain(|r| {
        let kind = r.get("kind").and_then(Value::as_str).unwrap_or("");
        !matches!(kind, "size" | "location")
            || deferred(&input, r)
            || (!generation_covered(r) && !is_authority(r))
    });
    intent.general.retain(|r| deferred(&input, r));
    for required in required {
        if general.is_none() && !authorities.contains_key(&required.id) {
            continue;
        }
        let witnesses = match pitch_witnesses(&input, &required) {
            Ok(w) => w,
            Err(why) => {
                unresolved.push(
                    json!({"kind":"location","faces":[],"feature":required.covers.join(","),
                    "not_written_reason":format!("shared planner {}: {why}",required.id)})
                    .as_object()
                    .unwrap()
                    .clone(),
                );
                continue;
            }
        };
        let mut row = authorities.get(&required.id).cloned().unwrap_or_default();
        let (kind, feature, faces, reference) = match &required.kind {
            Kind::Size { feature, .. } => {
                let faces: Vec<_> = required
                    .covers
                    .iter()
                    .flat_map(|id| input.holes.iter().filter(move |h| &h.support.feature == id))
                    .flat_map(|h| h.support.faces.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                ("size", feature.clone(), faces, vec![])
            }
            Kind::Location { from, to, .. } => (
                "location",
                to.feature.clone(),
                to.faces.clone(),
                from.faces.clone(),
            ),
        };
        let (faces, reference) = witnesses
            .as_ref()
            .map_or((faces.clone(), reference.clone()), |[from, to]| {
                (to.faces.clone(), from.faces.clone())
            });
        // Keep Specify's recognised feature spelling for origin/source and question consumers.
        let named = old
            .iter()
            .find(|r| ids(r, "faces") == faces)
            .and_then(|r| r.get("feature"))
            .cloned()
            .unwrap_or_else(|| {
                if feature.starts_with("hole:") {
                    json!(feature.replacen("hole:", "holes:", 1))
                } else {
                    row.get("feature").cloned().unwrap_or_else(|| json!(""))
                }
            });
        row.insert("kind".into(), json!(kind));
        row.insert("feature".into(), named);
        row.insert("faces".into(), json!(faces));
        row.insert(
            if kind == "size" {
                "nominal"
            } else {
                "distance"
            }
            .into(),
            json!(required.nominal),
        );
        if kind == "location" {
            row.insert("reference".into(), json!(reference));
        }
        row.remove("basic");
        row.remove("general");
        match &required.tolerance {
            Tolerance::Basic => {
                row.remove("upper");
                row.remove("lower");
                row.insert("basic".into(), json!(true));
            }
            Tolerance::General => {
                row.insert("general".into(), json!(true));
                if let Some(t) = general
                    .filter(|_| {
                        !matches!(
                            required.kind,
                            Kind::Size {
                                kind: shared::SizeKind::Angle,
                                ..
                            }
                        )
                    })
                    .and_then(|g| {
                        // Match the six-decimal nominal written to STEP; rigid-frame
                        // noise at an ISO range boundary must not widen the tolerance.
                        crate::general::deviation(g, crate::rules::round(required.nominal, 6))
                    })
                {
                    row.insert("upper".into(), json!(t));
                    row.insert("lower".into(), json!(-t));
                }
            }
            Tolerance::Untoleranced => {}
            Tolerance::Explicit(_) => {}
        }
        let is_general = matches!(
            required.tolerance,
            Tolerance::General | Tolerance::Untoleranced
        );
        let mut used = BTreeSet::new();
        match &required.kind {
            Kind::Size { feature, .. } => {
                used.insert(feature.clone());
            }
            Kind::Location { from, to, .. } => {
                used.insert(from.feature.clone());
                used.insert(to.feature.clone());
            }
        }
        if let Kind::Location { .. } = &required.kind {
            let alias = format!("coincident:{}", required.id);
            if groups.contains_key(&alias) {
                used.insert(alias);
            }
        }
        loop {
            let before = used.len();
            for (id, members) in &groups {
                if used.contains(id) {
                    used.extend(members.iter().cloned());
                }
            }
            if used.len() == before {
                break;
            }
        }
        let used_groups: BTreeMap<_, _> = groups
            .iter()
            .filter(|(id, _)| used.contains(*id))
            .map(|(id, members)| (id.clone(), members.clone()))
            .collect();
        if let Some([from, to]) = &witnesses {
            used.insert(from.feature.clone());
            used.insert(to.feature.clone());
        }
        let used_supports = supports
            .iter()
            .filter(|s| used.contains(&s.feature))
            .cloned()
            .collect();
        row.insert(
            "planner".into(),
            serde_json::to_value(Planned {
                required,
                supports: used_supports,
                groups: used_groups,
                frame: input.frame,
                witnesses,
            })
            .map_err(|e| e.to_string())?,
        );
        if is_general {
            intent.general.push(row);
        } else {
            intent.requirements.push(row);
        }
    }
    intent.requirements.extend(unresolved);
    Ok(())
}

/// Already-stated dimensions are planner authorities, not fresh writer jobs. Their exact
/// tolerance is opaque to the planner; the original PMI remains the authority in Add mode.
pub(crate) fn stated(p: &haecceity::pmi::PartPmi) -> Vec<Map<String, Value>> {
    use haecceity::pmi::{DimTolerance, DimensionKind};
    p.dimensions
        .iter()
        .filter_map(|d| {
            let mut r = Map::new();
            let value = crate::existing::value(d.nominal.as_ref()?);
            match &d.kind {
                DimensionKind::Size {
                    feature,
                    kind,
                    angle,
                    ..
                } if angle.is_some()
                    || matches!(
                        kind,
                        haecceity::pmi::SizeKind::Diameter | haecceity::pmi::SizeKind::CurveLength
                    ) =>
                {
                    r.insert(
                        "size_kind".into(),
                        json!(if angle.is_some() {
                            "angle"
                        } else if *kind == haecceity::pmi::SizeKind::Diameter {
                            "diameter"
                        } else {
                            "length"
                        }),
                    );
                    r.insert("kind".into(), json!("size"));
                    r.insert(
                        "faces".into(),
                        json!(crate::existing::faces_of(p, &[*feature])),
                    );
                    r.insert("nominal".into(), json!(value));
                }
                DimensionKind::Location {
                    from,
                    to,
                    kind: haecceity::pmi::LocationKind::LinearDistance,
                    ..
                } => {
                    r.insert("kind".into(), json!("location"));
                    r.insert("faces".into(), json!(crate::existing::faces_of(p, &[*to])));
                    r.insert(
                        "reference".into(),
                        json!(crate::existing::faces_of(p, &[*from])),
                    );
                    r.insert("distance".into(), json!(value));
                }
                _ => return None,
            }
            r.insert("stored".into(), json!(true));
            r.insert(
                "stored_tolerance".into(),
                json!(format!("{:?}", d.tolerance)),
            );
            if matches!(d.tolerance, DimTolerance::Basic) {
                r.insert("basic".into(), json!(true));
            }
            Some(r)
        })
        .collect()
}

/// Persistent metadata is text, so canonicalize insignificant floating-point frame noise
/// before it crosses native/WASM boundaries. Selection, IDs, roles and counts are unchanged.
pub(crate) fn metadata(planned: &Planned) -> Result<String, serde_json::Error> {
    fn canonical(value: &mut Value) {
        match value {
            Value::Number(n) if n.is_f64() => {
                if let Some(x) = n.as_f64() {
                    *value = json!(crate::rules::round(x, 9) + 0.0);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(canonical),
            Value::Object(m) => m.values_mut().for_each(canonical),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(planned)?;
    canonical(&mut value);
    serde_json::to_string(&value)
}
