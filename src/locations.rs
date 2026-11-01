//! The per-member rows below are provisional support/contract witnesses for the shared
//! planner on live analyses. `crate::dimensioning` replaces them with the planner's pattern
//! origins and pitches before writing; old snapshots are upgraded at the writer (#75).
//!
//! Where a hole is: its distance from reference planes, as AP242 location dimensions (port of
//! the semantic part of `locations.py`).
//!
//! A hole positioned by a tolerance has **basic** dimensions (exact, boxed on a drawing) from
//! the planes of its datum frame; a hole located by ± dimensions has toleranced ones from the
//! selected datum planes. Either way a dimension runs from a plane along the hole's axis (whose
//! normal is square to it) to the axis, for each hole of a pattern. A frame of axes (a bolt
//! circle round a bore) has no plane to measure from: its holes get none here.
//!
//! Python's `mark_basic` is not ported: it edits the written file because OpenCascade cannot
//! mark a dimension basic, and haecceity writes a basic dimension as such.

// Coordinate indexing uses [f64; 3] (or a bounded 0..3 loop), never variable-length JSON arrays.
use serde_json::{Map, Value};

use crate::pyfmt;
use crate::rules::{Error, Faces, Feature, Result, object, round, str_of};

/// The ± a hole group may be located to; general leaves it to the general tolerance.
pub const TOLERANCES: [&str; 5] = ["general", "±0.2", "±0.1", "±0.05", "±0.02"];

/// Whether a plane with normal `normal` runs along `axis`: its normal square to it.
pub fn along(axis: &[f64; 3], normal: &[f64; 3]) -> bool {
    (0.0 + axis[0] * normal[0] + axis[1] * normal[1] + axis[2] * normal[2]).abs() < 1e-6
}

/// Of `planes` (each one or more coplanar faces), those a hole on `axis` can be measured
/// from.
pub fn references(planes: &[Vec<i64>], axis: &[f64; 3], faces: &Faces) -> Result<Vec<Vec<i64>>> {
    let mut out = Vec::new();
    for plane in planes {
        let Some(&first) = plane.first() else {
            continue;
        };
        let mut flat = true;
        for &i in plane {
            if !faces.at(i)?.is_plane() {
                flat = false;
                break;
            }
        }
        if flat && along(axis, &faces.at(first)?.direction()?) {
            out.push(plane.clone());
        }
    }
    Ok(out)
}

/// From the plane (`centroid`, `normal`) to the axis through `location`.
pub fn distance(location: &[f64; 3], centroid: &[f64; 3], normal: &[f64; 3]) -> f64 {
    let offset = [
        location[0] - centroid[0],
        location[1] - centroid[1],
        location[2] - centroid[2],
    ];
    (0.0 + offset[0] * normal[0] + offset[1] * normal[1] + offset[2] * normal[2]).abs()
}

/// One hole to locate: its id, its feature (whose record has `axis` and `location`) and its
/// bore faces.
pub type Member<'a> = (String, Feature<'a>, Vec<i64>);

/// A location from each of `planes` to each hole in `members`, toleranced ±`tolerance`, or
/// basic when None.
pub fn dimensions(
    members: &[Member<'_>],
    planes: &[Vec<i64>],
    faces: &Faces,
    tolerance: Option<f64>,
) -> Result<Vec<Map<String, Value>>> {
    let mut out = Vec::new();
    for (fid, hole, bores) in members {
        let axis = hole.vector("axis")?;
        for plane in references(planes, &axis, faces)? {
            // references() explicitly skips empty planes before returning them.
            let face = faces.at(plane[0])?;
            let d = distance(
                &hole.vector("location")?,
                &face.centroid()?,
                &face.direction()?,
            );
            let mut req = object([
                ("kind", Value::from("location")),
                ("feature", Value::from(fid.as_str())),
                ("faces", Value::from(bores.clone())),
                ("reference", Value::from(plane)),
                ("distance", Value::from(round(d, 6))),
            ]);
            match tolerance {
                None => {
                    req.insert("basic".into(), Value::from(true));
                }
                Some(t) => {
                    req.insert("upper".into(), Value::from(t));
                    req.insert("lower".into(), Value::from(-t));
                }
            }
            out.push(req);
        }
    }
    Ok(out)
}

/// A location answer as its ±: `±0.1` (or `0.1`, `+-0.1`) is 0.1; `general` (or nothing) is
/// None. Text that is neither is refused with Python's `float()` message.
pub fn parse(value: &Value) -> Result<Option<f64>> {
    let text = str_of(value);
    let text = pyfmt::strip(&text).to_lowercase().replace("+-", "±");
    let text = pyfmt::strip(text.trim_start_matches('±'));
    if text.is_empty() || text == "general" {
        return Ok(None);
    }
    pyfmt::parse_float(text).map(Some).ok_or_else(|| {
        Error::Invalid(format!(
            "could not convert string to float: {}",
            pyfmt::repr_str(text)
        ))
    })
}
