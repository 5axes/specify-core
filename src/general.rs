//! ISO 2768 deviations and the legacy dimensions retained for later-slice feature owners.
//!
//! Live analyses and writes pass these provisional rows through `crate::dimensioning`:
//! quiddity selects holes, patterns and prismatic plane levels/extents; only explicitly
//! deferred owners retain the baseline path below. Old JSON snapshots use this provisional
//! format too, and the writer upgrades them against the loaded geometry (#75).
//!
//! A part's PMI otherwise states only what was specified: fits, threads, positions. What the
//! general tolerance covers (every other diameter, length and hole location) is left to
//! whoever dimensions the drawing, and an inspection list built from the PMI misses it. Here
//! those dimensions are found, toleranced by the general tolerance class (ISO 2768-1 linear),
//! for the inspection list and the view, and written into the STEP with their nominal alone, so
//! the general tolerance governs them as on a drawing (`writer.rs`, #63).
//!
//! The scheme is baseline from the datums: along each direction the part has parallel planes
//! in, every plane level is a length from one reference plane (the datum plane in that
//! direction if there is one, else the lowest). Holes not located by a position are located
//! from those same references. Diameters are each hole's and turned step's own, unless a fit,
//! thread or knurl already sizes them.

// Coordinate indexing uses [f64; 3] (or a bounded 0..3 loop), never variable-length JSON arrays.
use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{Map, Value};

use crate::locations;
use crate::pyfmt;
use crate::rules::{
    Ctx, DatumKind, Datums, Error, Face, Faces, Result, bores, bores_of, int_of, invalid, round,
};

/// ISO 2768-1 permissible deviations (±mm) for linear dimensions, by class, for nominal sizes
/// up to each bound in [`BOUNDS`] (over 0.5 mm); None where the class sets none.
pub const ISO_2768: [(char, [Option<f64>; 8]); 4] = [
    (
        'f',
        [
            Some(0.05),
            Some(0.05),
            Some(0.1),
            Some(0.15),
            Some(0.2),
            Some(0.3),
            Some(0.5),
            None,
        ],
    ),
    (
        'm',
        [
            Some(0.1),
            Some(0.1),
            Some(0.2),
            Some(0.3),
            Some(0.5),
            Some(0.8),
            Some(1.2),
            Some(2.0),
        ],
    ),
    (
        'c',
        [
            Some(0.2),
            Some(0.3),
            Some(0.5),
            Some(0.8),
            Some(1.2),
            Some(2.0),
            Some(3.0),
            Some(4.0),
        ],
    ),
    (
        'v',
        [
            None,
            Some(0.5),
            Some(1.0),
            Some(1.5),
            Some(2.5),
            Some(4.0),
            Some(6.0),
            Some(8.0),
        ],
    ),
];

/// The upper bound of each range of nominal sizes, mm.
pub const BOUNDS: [f64; 8] = [3.0, 6.0, 30.0, 120.0, 400.0, 1000.0, 2000.0, 4000.0];

/// Two plane levels closer than this are one.
const LEVEL: f64 = 1e-4;

/// The ± `general` (e.g. `ISO 2768-m`) allows `nominal`; None where the class sets none.
pub fn deviation(general: &str, nominal: f64) -> Option<f64> {
    let text = pyfmt::strip(general);
    let class = text.rsplit('-').next().unwrap_or(text).to_lowercase();
    let mut chars = class.chars();
    let (Some(letter), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let (_, row) = ISO_2768.iter().find(|(c, _)| *c == letter)?;
    // BOUNDS is a nonempty constant table.
    if !(0.5 <= nominal && nominal <= BOUNDS[BOUNDS.len() - 1]) {
        return None;
    }
    // bisect_left: the first range whose bound is at least the nominal size.
    // nominal is bounded by the last BOUNDS entry; every constant row has BOUNDS.len() entries.
    row[BOUNDS.partition_point(|&b| b < nominal)]
}

/// Where a distance runs, from or to: a plane level (its normal's line and its offset along
/// it), an axis (its direction's line and its point nearest the origin), rounded; else the
/// faces themselves.
#[derive(Debug, Clone, PartialEq)]
enum Place {
    Faces(Vec<i64>),
    Plane([f64; 3], f64),
    Axis([f64; 3], [f64; 3]),
}

/// `_between`: the two places a distance runs between, as a set (a frozenset of two, or one
/// when both are the same place).
#[derive(Debug, Clone)]
struct Between(Place, Place);

impl PartialEq for Between {
    /// Equal as sets: `{a, b} == {c, d}` (a set of one when both ends are the same place).
    fn eq(&self, other: &Self) -> bool {
        let (one, other_one) = (self.0 == self.1, other.0 == other.1);
        if one || other_one {
            return one == other_one && self.0 == other.0;
        }
        (self.0 == other.0 && self.1 == other.1) || (self.0 == other.1 && self.1 == other.0)
    }
}

fn between(ids: &[Value], reference: &[Value], faces: &Faces) -> Result<Between> {
    Ok(Between(place(ids, faces)?, place(reference, faces)?))
}

/// `_place`.
fn place(ids: &[Value], faces: &Faces) -> Result<Place> {
    let as_faces = || -> Result<Place> {
        let mut sorted = ids
            .iter()
            .map(|i| int_of(i).map_err(|e| Error::Invalid(e.message().to_string())))
            .collect::<Result<Vec<i64>>>()?;
        sorted.sort_unstable();
        Ok(Place::Faces(sorted))
    };
    let first = match ids.first() {
        Some(i) => faces.get(int_of(i).map_err(|e| Error::Invalid(e.message().to_string()))?),
        None => None,
    };
    let Some(first) = first else {
        return as_faces();
    };
    let n = first.direction()?;
    // A line's two directions are one: point it to a positive first non-zero component.
    let sign = n
        .iter()
        .find(|x| x.abs() > 1e-9)
        .map_or(1.0, |&x| if x > 0.0 { 1.0 } else { -1.0 });
    let line = n.map(|x| round(sign * x, 4) + 0.0);
    if first.is_plane() {
        return Ok(Place::Plane(
            line,
            round(sign * dot(&first.centroid()?, &n), 3) + 0.0,
        ));
    }
    if first.is_cylinder()
        && let Some(o) = first.origin
    {
        let t = dot(&o, &n);
        let foot = [0, 1, 2].map(|k| round(o[k] - t * n[k], 3) + 0.0);
        return Ok(Place::Axis(line, foot));
    }
    as_faces()
}

fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    0.0 + a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A requirement's list of faces (`faces`, `reference`) as given.
fn list<'r>(req: &'r Map<String, Value>, key: &str) -> Result<&'r [Value]> {
    match req.get(key) {
        Some(Value::Array(items)) => Ok(items),
        _ => invalid(format!("a requirement has no '{key}' list")),
    }
}

/// The general dimensions the analysis needs beside the `specified` requirements, measured
/// from `datums`, each toleranced by `general` and marked `general`.
pub fn dimensions(
    ctx: &Ctx<'_>,
    datums: &Datums,
    specified: &[Map<String, Value>],
    general: &str,
) -> Result<Vec<Map<String, Value>>> {
    let faces = &ctx.faces;
    fn kind(r: &Map<String, Value>) -> &str {
        r.get("kind").and_then(Value::as_str).unwrap_or("")
    }
    let mut sized: HashSet<i64> = HashSet::new();
    let mut located: HashSet<&str> = HashSet::new();
    let mut called: Vec<Between> = Vec::new();
    for r in specified {
        match kind(r) {
            "size" | "thread" | "knurl" => {
                for i in list(r, "faces")? {
                    sized.insert(int_of(i).map_err(|e| Error::Invalid(e.message().to_string()))?);
                }
            }
            "location" | "position" => {}
            _ => continue,
        }
        if matches!(kind(r), "location" | "position")
            && let Some(f) = r.get("feature").and_then(Value::as_str)
        {
            located.insert(f);
        }
        // A distance called out between the same places (a plane level, a hole's axis) says
        // what the general one would, whichever of their faces were picked.
        if kind(r) == "location" {
            called.push(between(list(r, "faces")?, list(r, "reference")?, faces)?);
        }
    }
    // Each pattern member's pattern (a member of two: the last, as Python's dict).
    let mut members: HashMap<&str, &str> = HashMap::new();
    for f in ctx.features.iter().filter(|f| f.family == "hole_patterns") {
        for m in &f.members {
            members.insert(m, f.id);
        }
    }
    let mut out = Vec::new();
    let mut toleranced = |req: Map<String, Value>, value: f64| -> Result<()> {
        let Some(t) = deviation(general, value) else {
            return Ok(());
        };
        if kind(&req) == "location" {
            let here = between(list(&req, "faces")?, list(&req, "reference")?, faces)?;
            if called.contains(&here) {
                return Ok(());
            }
        }
        let mut req = req;
        req.insert("upper".into(), Value::from(t));
        req.insert("lower".into(), Value::from(-t));
        req.insert("general".into(), Value::from(true));
        out.push(req);
        Ok(())
    };
    let size = |id: &str, faces: Vec<i64>, d: f64| {
        crate::rules::object([
            ("kind", Value::from("size")),
            ("feature", Value::from(id)),
            ("faces", Value::from(faces)),
            ("nominal", Value::from(d)),
        ])
    };

    // Diameters: one requirement on a pattern covers each of its holes.
    for feature in &ctx.features {
        let holes = match feature.family {
            "holes" if !members.contains_key(feature.id) => vec![feature],
            "hole_patterns" if !feature.members.is_empty() => feature
                .members
                .iter()
                .map(|m| ctx.feature(m))
                .collect::<Result<Vec<_>>>()?,
            "turned_steps" => {
                let d = feature.number("diameter")?;
                let mut cylinders = Vec::new();
                for &i in feature.faces()? {
                    if faces.at(i)?.is_cylinder() {
                        cylinders.push(i);
                    }
                }
                if !cylinders.is_empty() && !cylinders.iter().any(|i| sized.contains(i)) {
                    toleranced(size(feature.id, cylinders, d), d)?;
                }
                continue;
            }
            _ => continue,
        };
        // The match constructs one hole or accepts only a nonempty member list.
        let d = holes[0].number("diameter")?;
        let mut on = Vec::new();
        for h in &holes {
            on.extend(bores(h.faces()?, d, faces)?);
        }
        if !on.is_empty() && !on.iter().any(|i| sized.contains(i)) {
            toleranced(size(feature.id, on, d), d)?;
        }
    }

    // Lengths: every plane level from its direction's reference. A hole's own floors and
    // counterbores are the hole's, not the part's.
    let mut in_holes: HashSet<i64> = HashSet::new();
    for f in ctx.features.iter().filter(|f| f.family == "holes") {
        in_holes.extend(f.faces()?);
    }
    let planes: Vec<&Face> = faces
        .iter()
        .filter(|f| f.is_plane() && !in_holes.contains(&f.id))
        .collect();
    let mut references: Vec<Vec<i64>> = Vec::new();
    for line in lines(&planes)? {
        let levels = levels(&line)?;
        let reference = match reference(&levels, datums) {
            Some(r) => r,
            // lines() creates nonempty groups, and levels() emits a nonempty face group for each.
            None => levels[0].1.clone(),
        };
        references.push(reference.clone());
        // reference and line come from nonempty groups constructed by levels()/lines().
        let base = dot(&faces.at(reference[0])?.centroid()?, &line[0].direction()?);
        for (offset, ids) in &levels {
            if *ids == reference || (offset - base).abs() < LEVEL {
                continue;
            }
            let length = (offset - base).abs();
            toleranced(
                crate::rules::object([
                    ("kind", Value::from("location")),
                    ("feature", Value::from("")),
                    ("faces", Value::from(ids.clone())),
                    ("reference", Value::from(reference.clone())),
                    ("distance", Value::from(round(length, 6))),
                ]),
                length,
            )?;
        }
    }

    // Hole locations, for holes no position or ± location already places.
    for feature in ctx.features.iter().filter(|f| f.family == "holes") {
        if located.contains(feature.id)
            || members.get(feature.id).is_some_and(|p| located.contains(p))
        {
            continue;
        }
        let member = (
            feature.id.to_string(),
            feature.clone(),
            bores_of(feature, faces)?,
        );
        for mut req in locations::dimensions(&[member], &references, faces, None)? {
            req.shift_remove("basic");
            let distance = req
                .get("distance")
                .and_then(Value::as_f64)
                .unwrap_or_default();
            toleranced(req, distance)?;
        }
    }
    Ok(out)
}

/// `_lines`: the planes grouped by the line their normals lie on, by face id.
fn lines<'f>(planes: &[&'f Face]) -> Result<Vec<Vec<&'f Face>>> {
    let mut sorted = planes.to_vec();
    sorted.sort_by_key(|p| p.id);
    let mut out: Vec<Vec<&Face>> = Vec::new();
    for plane in sorted {
        let n = plane.direction()?;
        let mut placed = false;
        for line in out.iter_mut() {
            // lines are created with one plane and are only appended to.
            if (dot(&line[0].direction()?, &n).abs() - 1.0).abs() < 1e-6 {
                line.push(plane);
                placed = true;
                break;
            }
        }
        if !placed {
            out.push(vec![plane]);
        }
    }
    Ok(out)
}

/// `_levels`: the planes on one line as (offset, faces) per level, lowest first.
fn levels(line: &[&Face]) -> Result<Vec<(f64, Vec<i64>)>> {
    // levels() is private and receives only nonempty groups from lines().
    let n = line[0].direction()?;
    let mut planes: Vec<(&Face, f64)> = line
        .iter()
        .map(|p| Ok((*p, dot(&p.centroid()?, &n))))
        .collect::<Result<_>>()?;
    planes.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(f64, Vec<i64>)> = Vec::new();
    for (plane, offset) in planes {
        match out.last_mut() {
            Some(last) if (last.0 - offset).abs() < LEVEL => last.1.push(plane.id),
            _ => out.push((offset, vec![plane.id])),
        }
    }
    for (_, ids) in out.iter_mut() {
        ids.sort_unstable();
    }
    Ok(out)
}

/// `_reference`: the faces of the first datum plane (by letter) among `levels`.
fn reference(levels: &[(f64, Vec<i64>)], datums: &Datums) -> Option<Vec<i64>> {
    let ids: BTreeSet<i64> = levels.iter().flat_map(|(_, l)| l.iter().copied()).collect();
    let mut letters = datums.letters();
    letters.sort_unstable();
    letters.into_iter().find_map(|x| {
        let d = datums.get(x)?;
        (d.kind == DatumKind::Plane && d.faces.iter().all(|i| ids.contains(i))).then(|| {
            let mut faces = d.faces.clone();
            faces.sort_unstable();
            faces
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // test_call_out.py::test_a_distance_leaves_the_general_list_by_place_not_by_the_faces_picked
    #[test]
    fn a_distance_leaves_the_general_list_by_place_not_by_the_faces_picked() {
        let z = json!([0.0, 0.0, 1.0]);
        let faces = Faces::from_values(&[
            json!({"id": 1, "kind": "plane", "centroid": [0, 0, 0], "direction": z}),
            json!({"id": 2, "kind": "plane", "centroid": [50, 0, 0], "direction": [0, 0, -1.0]}),
            json!({"id": 3, "kind": "plane", "centroid": [0, 0, 10], "direction": z}),
            json!({"id": 4, "kind": "plane", "centroid": [40, 5, 10], "direction": z}),
            json!({"id": 5, "kind": "plane", "centroid": [0, 0, 12], "direction": z}),
        ])
        .unwrap();
        let ids = |v: &[i64]| v.iter().map(|&i| Value::from(i)).collect::<Vec<_>>();
        let b = |a: &[i64], r: &[i64]| between(&ids(a), &ids(r), &faces).unwrap();
        // Two faces at one level, picked one at a time, from either face of the base.
        assert_eq!(b(&[3, 4], &[1, 2]), b(&[4], &[2]));
        assert_eq!(b(&[3], &[1]), b(&[1], &[3]));
        assert_ne!(b(&[5], &[1]), b(&[3], &[1]));
    }
}
