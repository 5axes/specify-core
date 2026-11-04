//! Revisions: a part's answers carried from one revision of its STEP file to the next, and what
//! changed.
//!
//! Answers bind exact authored STEP names where present, sharing resume's name matcher.
//! Unnamed faces use quiddity's correspondence (`quiddity::correspondence::correspond`),
//! which aligns the two parts and classifies each recognised feature and face as carried,
//! adapted, ambiguous, orphaned or new. Existing CAD PMI keeps its geometric comparison.
//!
//! An answer names its feature by family and defining faces (`hole.function:holes:12.13`) or,
//! for a datum, lists faces (`datum.A: [4]`). Each face is mapped by name or unnamed correspondence;
//! the answer is then carried to the new part's feature of that family holding the mapped faces.
//! An answer is never guessed: when a face is gone (*orphaned*), matches several faces
//! (*ambiguous*), or no single feature holds the mapped faces, it is left unresolved with its
//! value and the reason, as resuming does. An answer carried to a feature that quiddity reports
//! changed (a resized hole keeping its fit) is carried and listed for a person to check.
//!
//! The answers are those given or, when none are, the ones Specify wrote as PMI in the old
//! file. The PMI is compared too: Specify's in the new file against the answers carried, and the
//! files' own (from CAD), each item read on the new part through the face matching.
//!
//! The report also lists the questions the new part opens that the old one did not (a new hole
//! needs its function), the problems review finds that it did not before, and the features that
//! changed; [`picture`] draws the new part coloured by what changed.

use std::collections::{BTreeMap, BTreeSet};

use quiddity::correspondence::{self as qc, Class, Correspondence};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::api::{self, Result};
use crate::features::Feature;
use crate::part::LoadedPart;
use haecceity::Part;

pub mod assembly;
mod pmi;
mod pmi_picture;
mod treatments;

/// What one old face is in the new part.
#[derive(Clone, Debug, PartialEq)]
enum Bind {
    To(usize),
    Orphaned,
    Ambiguous(Vec<usize>),
    NamedRefusal { reason: String, ambiguous: bool },
}

#[derive(Clone, Debug, Serialize)]
pub struct Revised {
    /// `ok`: every answer carried and no new question or problem; `decide`: an answer is
    /// unresolved or the new part asks something new (a feature matched ambiguously decides
    /// nothing by itself: an answer on it is unresolved); `problems`: review finds problems it
    /// did not on the old part.
    pub verdict: String,
    pub summary: String,
    /// The answers on the new part: carried, renamed where the feature's ID changed.
    pub answers: Map<String, Value>,
    /// Each answer whose ID changed, `[old, new]`.
    pub migrated: Vec<(String, String)>,
    pub unresolved: Vec<Unresolved>,
    /// Answers carried to a feature quiddity reports changed: still applied, to be checked.
    pub check: Vec<Check>,
    pub features: Vec<FeatureChange>,
    /// The whole part's volume, area, box and face count, before and after.
    pub measures: Measures,
    /// Requirement changes on pmiScene identities, plus deprecated legacy comparisons.
    pub pmi: Pmi,
    /// Questions open on the new part that were not open on the old one.
    pub questions: Vec<OpenQuestion>,
    pub problems: Problems,
    /// Each new face's class (`changed` or `new`; faces carried unchanged are left out), and the
    /// old faces that are gone or moved, for [`picture`].
    #[serde(skip)]
    pub faces: BTreeMap<usize, &'static str>,
    #[serde(skip)]
    pub gone: Vec<usize>,
    #[serde(skip)]
    pub correspondence: Option<Correspondence>,
    /// The part did not move in its own frame: no move was found, or it is rounding, or the old
    /// part lies on itself after it (a screw turned about its axis).
    #[serde(skip)]
    pub still: bool,
    #[serde(skip)]
    pmi_graphics: Vec<pmi_picture::Graphic>,
    #[serde(skip)]
    check_lines: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Unresolved {
    pub id: String,
    pub answer: Value,
    /// `orphaned`, `ambiguous`, `unmatched` (the faces carried, but no single feature of the
    /// family holds them) or `stale` (the old part had no such feature either).
    pub class: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    /// The question it answers, in words, where the old part asks it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub question: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub id: String,
    pub answer: Value,
    pub feature: String,
    pub changes: BTreeMap<String, qc::Change>,
    /// The question it answers, in words.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub question: String,
    /// The part's axis its feature lies along, as for [`FeatureChange::axis`].
    #[serde(skip)]
    pub axis: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureChange {
    /// `adapted`, `ambiguous`, `orphaned`, `new`, or `replaced` (one feature in place of
    /// another, see [`FeatureChange::became`]), or `continued` (an existing chamfer
    /// carrying round new fillets; `candidates` includes its extension records).
    pub class: String,
    pub family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub changes: BTreeMap<String, qc::Change>,
    /// The feature in words, as a drawing names it (`Ø5 × 30 through hole`): the old one's, or
    /// the new one's when it is new.
    pub what: String,
    /// For a feature `replaced` by another in its place (a pattern re-laid, a hole now
    /// counterbored): the new one in words.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub became: String,
    /// Where it is (the old one, or the new one when it is new), in the part's coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 3]>,
    /// Where it is, as a person finds it on the part.
    pub place: Place,
    /// The part's axis it lies along, if any: its move along it is its length changing.
    #[serde(skip)]
    pub axis: Option<usize>,
    /// For an ambiguous one: its candidates in the same place on the new part, so that alike
    /// features read as merged (`2× R3 fillets … are now one`), split or paired.
    #[serde(skip)]
    pub there: Vec<String>,
    /// Its faces on the new part (its candidates' when ambiguous) and on the old, for
    /// [`view`].
    #[serde(skip)]
    pub new_faces: Vec<usize>,
    #[serde(skip)]
    pub old_faces: Vec<usize>,
}

/// Where a feature is, in words a person can find it by: the side of the part it is on, what
/// it sits in, and how far it is from an axis or corner they can see.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Place {
    /// `on the −Y side, in the Ø70 boss around the Ø45 hole`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub on: String,
    /// `X +27, Z −10`: along the part's axes, from `from`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub offset: String,
    /// `its axis`, `the Ø45 hole's axis` or `the −X, −Y corner`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
}

/// The PMI in the two files.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Pmi {
    /// Where the old part's answers came from: `given` (passed in), `stored` (the PMI Specify
    /// wrote in the old file) or `none`.
    pub answers_from: String,
    /// Whether the new file holds PMI Specify wrote.
    pub new_stored: bool,
    /// The new file's Specify PMI against the answers carried to it, where it has some: an
    /// answer `added`, `removed` or `changed` there.
    /// Deprecated for one release: use `items` for semantic requirements.
    pub stored: Vec<PmiChange>,
    /// The files' own PMI (dimensions, tolerances, datums and notes from CAD), matched by the
    /// faces it is on: `added`, `removed` or `changed`.
    /// Deprecated for one release: use `items` for complete notation and semantics.
    pub existing: Vec<PmiChange>,
    /// How many items of their own PMI the old and new files state.
    pub existing_count: [usize; 2],
    /// Requirement changes, using pmiScene identities, numbers and review statuses.
    pub items: Vec<PmiItemChange>,
    /// Exact comparison documents, included only with `DiffOptions::scenes`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenes: Option<PmiScenes>,
}

/// The compared documents, shaped as `pmiScene`, in each part's local frame.
#[derive(Clone, Debug, Serialize)]
pub struct PmiScenes {
    pub old: Value,
    pub new: Value,
}

/// Optional report payloads. Default JSON omits the scene documents.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiffOptions {
    pub scenes: bool,
}

/// Mesh face anchors, in the same form as scene links.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FaceAnchor {
    pub face: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PmiFaces {
    pub old: Vec<FaceAnchor>,
    pub new: Vec<FaceAnchor>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PmiItemChange {
    pub class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    pub id: String,
    /// Present only for paired items whose old identity differs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_id: Option<String>,
    pub faces: PmiFaces,
    pub kind: String,
    pub status: String,
    pub on: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PmiChange {
    pub class: String,
    /// What it is on, in words: a feature (`Ø7 × 10 through hole`), an answer's question, or
    /// faces.
    pub on: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
    /// The answer's ID, for Specify's PMI.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
}

/// An item of a file's own PMI in words: `Ø7 +0.1/0`, `position 0.1 to A|B|C`, `datum A`.
fn pmi_words(p: &Value) -> String {
    let n = |k: &str| p.get(k).and_then(Value::as_f64);
    let kind = p["kind"].as_str().unwrap_or_default();
    let ty = p["type"].as_str().unwrap_or_default();
    let words = ty.replace('_', " ").to_lowercase();
    let limits = match (n("upper"), n("lower")) {
        (Some(u), Some(l)) if (u + l).abs() < 1e-9 => format!(" ±{}", num(u)),
        (Some(u), Some(l)) => format!(
            " {}/{}",
            signed(u).replace('−', "-"),
            signed(l).replace('−', "-")
        ),
        _ => String::new(),
    };
    match kind {
        "dimension" => {
            let absolute =
                p.get("limits")
                    .and_then(Value::as_array)
                    .and_then(|bounds| match bounds.as_slice() {
                        [lower, upper] => {
                            Some(format!("{}/{}", num(lower.as_f64()?), num(upper.as_f64()?)))
                        }
                        _ => None,
                    });
            let limits = if absolute.is_some() {
                String::new()
            } else {
                limits
            };
            let value = absolute.unwrap_or_else(|| n("value").map(num).unwrap_or_default());
            let size = match ty {
                "Size_Diameter" => format!("Ø{value}"),
                "Size_Radius" => format!("R{value}"),
                "Size_SphericalRadius" => format!("SR{value}"),
                _ => format!("{words} {value}"),
            };
            // A basic dimension is drawn framed; in words it says so.
            let basic = if p["basic"] == true { " (basic)" } else { "" };
            format!("{size}{limits}{basic}")
        }
        "geometric_tolerance" => {
            let datums: Vec<&str> = p["datums"]
                .as_array()
                .map(|d| d.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let value = n("value")
                .map(|v| format!(" {}", num(v)))
                .unwrap_or_default();
            if datums.is_empty() {
                format!("{words}{value}")
            } else {
                format!("{words}{value} to {}", datums.join("|"))
            }
        }
        "datum" => format!("datum {ty}"),
        _ => {
            if words.is_empty() {
                kind.replace('_', " ")
            } else {
                format!("{} ({words})", kind.replace('_', " "))
            }
        }
    }
}

/// The faces of a PMI item.
fn pmi_faces(p: &Value, key: &str) -> Vec<usize> {
    p[key]
        .as_array()
        .map(|f| {
            f.iter()
                .filter_map(Value::as_u64)
                .map(|i| i as usize)
                .collect()
        })
        .unwrap_or_default()
}

/// What a PMI item is on, in words: the smallest feature holding its faces, else its faces.
fn pmi_on(faces: &[usize], features: &[Feature]) -> String {
    if faces.is_empty() {
        return "the part".into();
    }
    features
        .iter()
        .filter(|f| !EVIDENCE.contains(&f.family.as_str()))
        .filter(|f| !f.faces.is_empty() && faces.iter().all(|i| f.faces.contains(i)))
        .min_by_key(|f| f.faces.len())
        .map(|f| describe(f).0)
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| {
            let list: Vec<String> = faces.iter().map(usize::to_string).collect();
            format!(
                "face{} {}",
                if faces.len() > 1 { "s" } else { "" },
                list.join(", ")
            )
        })
}

/// The files' own PMI compared: each old item read on the new part through the face matching
/// (*faces*), matched to a new item of the same kind and type on the same faces.
fn compare_existing(
    old: &[Value],
    new: &[Value],
    faces: &BTreeMap<usize, Bind>,
    old_features: &[Feature],
    new_features: &[Feature],
) -> Vec<PmiChange> {
    let key = |p: &Value, faces: Vec<usize>, faces2: Vec<usize>| {
        (
            p["kind"].as_str().unwrap_or_default().to_string(),
            p["type"].as_str().unwrap_or_default().to_string(),
            faces,
            faces2,
        )
    };
    let carry = |list: Vec<usize>| -> Option<Vec<usize>> {
        let mut out: Vec<usize> = list
            .iter()
            .map(|i| match faces.get(i) {
                Some(Bind::To(n)) => Some(*n),
                _ => None,
            })
            .collect::<Option<_>>()?;
        out.sort_unstable();
        Some(out)
    };
    let mut unused: Vec<bool> = vec![true; new.len()];
    let mut out = Vec::new();
    for p in old {
        let (f, f2) = (pmi_faces(p, "faces"), pmi_faces(p, "faces2"));
        let found = match (carry(f.clone()), carry(f2)) {
            (Some(now), Some(now2)) => {
                let want = key(p, now, now2);
                (0..new.len()).find(|&j| {
                    unused[j]
                        && key(
                            &new[j],
                            pmi_faces(&new[j], "faces"),
                            pmi_faces(&new[j], "faces2"),
                        ) == want
                })
            }
            _ => None,
        };
        match found {
            Some(j) => {
                unused[j] = false;
                let (was, now) = (pmi_words(p), pmi_words(&new[j]));
                if was != now {
                    out.push(PmiChange {
                        class: "changed".into(),
                        on: pmi_on(&pmi_faces(&new[j], "faces"), new_features),
                        old: Some(was),
                        new: Some(now),
                        id: String::new(),
                    });
                }
            }
            None => out.push(PmiChange {
                class: "removed".into(),
                on: pmi_on(&f, old_features),
                old: Some(pmi_words(p)),
                new: None,
                id: String::new(),
            }),
        }
    }
    for (_, p) in new.iter().enumerate().filter(|(j, _)| unused[*j]) {
        out.push(PmiChange {
            class: "added".into(),
            on: pmi_on(&pmi_faces(p, "faces"), new_features),
            old: None,
            new: Some(pmi_words(p)),
            id: String::new(),
        });
    }
    out
}

#[derive(Clone, Debug, Serialize)]
pub struct Measures {
    pub old: Size,
    pub new: Size,
}

/// A part's size: its solids' volume (mm³) and area (mm²), `None` where a face's mass can't be
/// computed, and the box around them (mm, in the part's own coordinates).
#[derive(Clone, Debug, Serialize)]
pub struct Size {
    pub volume: Option<f64>,
    pub area: Option<f64>,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub faces: usize,
}

fn size(part: &Part) -> Size {
    let solids = 0..part.solids.len();
    let mass: Option<Vec<(f64, f64)>> = solids.clone().map(|i| part.solid_mass(i)).collect();
    let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for i in solids {
        let b = part.solid_bounds(i);
        for k in 0..3 {
            min[k] = min[k].min(b.min[k]);
            max[k] = max[k].max(b.max[k]);
        }
    }
    if min.iter().any(|x| !x.is_finite()) {
        (min, max) = ([0.0; 3], [0.0; 3]);
    }
    Size {
        volume: mass.as_ref().map(|m| m.iter().map(|(v, _)| v).sum()),
        area: mass.as_ref().map(|m| m.iter().map(|(_, a)| a).sum()),
        min,
        max,
        faces: part.faces.len(),
    }
}

/// A value in a report: a string as it is, a number as a size, a point as coordinates.
fn fmt(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), num),
        other => xyz(other).map_or_else(|| other.to_string(), |p| point(&p)),
    }
}

/// A change in words, its move apart: `depth 10 → 12`.
fn sizes(c: &BTreeMap<String, qc::Change>) -> String {
    // A surface's control points are not a size a person reads.
    if c.keys().any(|k| k.contains("support.")) {
        return "reshaped".to_string();
    }
    c.iter()
        .filter(|(k, _)| k.as_str() != "position" && !CONTEXT.contains(&k.as_str()))
        .map(|(k, c)| {
            let k = k.strip_prefix("sizes.").unwrap_or(k).replace('_', " ");
            format!("{k} {} → {}", fmt(&c.old), fmt(&c.new))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// A move along the part's axes: `X −15`. Not along its own axis when its length changed
/// there: its middle moves as it grows.
fn moved(changes: &BTreeMap<String, qc::Change>, axis: Option<usize>) -> Option<String> {
    let lengthened = ["sizes.height", "sizes.depth"]
        .iter()
        .any(|k| changes.contains_key(*k));
    let c = changes.get("position")?;
    let (a, b) = (xyz(&c.old)?, xyz(&c.new)?);
    // The move a person sees: components a tenth of the largest or less are the feature
    // riding along a drafted or curved face.
    let largest = (0..3).map(|i| (b[i] - a[i]).abs()).fold(0.0, f64::max);
    let shift: Vec<String> = (0..3)
        .filter(|&i| (b[i] - a[i]).abs() >= 0.05 && (b[i] - a[i]).abs() > largest / 10.0)
        .filter(|&i| !(lengthened && axis == Some(i)))
        .map(|i| format!("{} {}", AXES[i], signed(b[i] - a[i])))
        .collect();
    (!shift.is_empty()).then(|| shift.join(", "))
}

/// Where several are, or nothing when there are too many to read.
fn places(at: &[Option<[f64; 3]>]) -> String {
    let points: Option<Vec<String>> = at.iter().map(|p| p.as_ref().map(point)).collect();
    match points {
        Some(p) if !p.is_empty() && p.len() <= 4 => {
            let (last, rest) = p.split_last().expect("not empty");
            if rest.is_empty() {
                format!(" at {last}")
            } else {
                format!(" at {} and {last}", rest.join(", "))
            }
        }
        _ => String::new(),
    }
}

/// A quantity with its thousands grouped, whole from 100 up: `438,357`, `5.01`.
fn amount(x: f64) -> String {
    if x.abs() < 100.0 {
        return num(x);
    }
    let digits = format!("{:.0}", x.abs());
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if x < 0.0 { format!("-{out}") } else { out }
}

#[derive(Clone, Debug, Serialize)]
pub struct OpenQuestion {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Problems {
    pub added: Vec<Value>,
    pub resolved: Vec<Value>,
}

/// Families that are facts about faces, not features a person answers for: they follow every
/// change to the faces around them.
const EVIDENCE: [&str; 2] = ["risers", "step_levels"];

/// Families that border the features a person names: they change as those do.
const ROUNDS: [&str; 5] = [
    "fillets",
    "blends",
    "chamfers",
    "freeform_surfaces",
    "cylindrical_profiles",
];

/// Whole-body contexts may co-own every face with an individual feature, or with
/// another classification. They must each keep their own fingerprint identity.
const BODY_CLASSIFICATIONS: [&str; 5] = [
    "plates",
    "polygonal_stock",
    "thin_wall_bodies",
    "sheet_metal_bodies",
    "turned_bodies",
];

/// The quiddity record of each feature: the one whose defining faces it shares most.
fn records(features: &[Feature], prints: &qc::Fingerprints) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for record in &prints.features {
        let best = features
            .iter()
            // #123: co-owning bodies keep their own identities. Preserve the shared
            // representative for a physical feature's dimensional and section views,
            // so fold() can keep one description with plain measurements.
            .filter(|f| {
                f.family == record.family
                    || (!BODY_CLASSIFICATIONS.contains(&f.family.as_str())
                        && !BODY_CLASSIFICATIONS.contains(&record.family.as_str()))
            })
            .filter(|f| !f.faces.is_empty())
            .map(|f| {
                (
                    f,
                    f.faces.iter().filter(|i| record.faces.contains(i)).count(),
                )
            })
            .filter(|(f, n)| *n > 0 && *n == f.faces.len().min(record.faces.len()))
            // The most faces shared, then the smallest feature: a hole, not its pattern.
            .max_by_key(|(f, n)| (*n, std::cmp::Reverse(f.faces.len())));
        if let Some((f, _)) = best {
            out.entry(record.id.clone()).or_insert_with(|| f.id.clone());
        }
    }
    out
}

/// A length as a drawing writes it: to two decimals, without trailing zeros.
fn num(x: f64) -> String {
    let s = format!("{:.2}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// A point to a tenth of a millimetre: `(57.7, -85, 10)`.
fn point(p: &[f64; 3]) -> String {
    let c = |x: f64| {
        let s = format!("{:.1}", x);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s == "-0" {
            "0".to_string()
        } else {
            s.to_string()
        }
    };
    format!("({}, {}, {})", c(p[0]), c(p[1]), c(p[2]))
}

fn xyz(v: &Value) -> Option<[f64; 3]> {
    match v.as_array()?.as_slice() {
        [x, y, z] => Some([x.as_f64()?, y.as_f64()?, z.as_f64()?]),
        _ => None,
    }
}

/// A count of a thing: `1 answer`, `3 answers`.
fn plural_of(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {}", several(one))
    }
}

/// One of a family, in words: `holes` → `hole`, `hole_patterns` → `hole pattern`.
fn singular(family: &str) -> String {
    let one = if family == "turned_bodies" {
        "turned_body"
    } else if family.ends_with("sses") {
        &family[..family.len() - 2]
    } else {
        family.strip_suffix('s').unwrap_or(family)
    };
    one.replace('_', " ")
}

/// Several of a thing in words: `hole` → `holes`, `boss` → `bosses`.
fn several(one: &str) -> String {
    if let Some(rest) = one.strip_prefix("face groove ") {
        return format!("face grooves {rest}");
    }
    if let Some(rest) = one.strip_prefix("pattern of ") {
        return format!("patterns of {rest}");
    }
    if let Some((noun, finish)) = one.split_once(", ") {
        return format!("{}, {finish}", several(noun));
    }
    if let Some(stem) = one.strip_suffix("body") {
        return format!("{stem}bodies");
    }
    if one.ends_with('s') {
        format!("{one}es")
    } else {
        format!("{one}s")
    }
}

const AXES: [&str; 3] = ["X", "Y", "Z"];

/// A signed length to a tenth: `+27`, `−10`.
fn signed(x: f64) -> String {
    let s = num((x * 10.0).round() / 10.0);
    match s.strip_prefix('-') {
        Some(rest) => format!("−{rest}"),
        None => format!("+{s}"),
    }
}

/// Where a feature's record puts it: its location, point, centre or its first hole's.
fn position(r: &Map<String, Value>) -> Option<[f64; 3]> {
    r.get("location")
        .or_else(|| r.get("at"))
        .or_else(|| r.get("center"))
        .or_else(|| r.get("axis_point"))
        .or_else(|| r.get("path").and_then(|p| p.get("center")))
        .and_then(xyz)
        .or_else(|| {
            r.get("holes")
                .and_then(|h| h.get(0))
                .and_then(|h| h.get("location"))
                .and_then(xyz)
        })
}

/// A feature's axis, when it lies along one of the part's (`1` for Y), with its diameter and
/// its length along it.
fn along(r: &Map<String, Value>) -> Option<(usize, f64, f64)> {
    let first = r
        .get("holes")
        .and_then(|h| h.get(0))
        .and_then(Value::as_object);
    let rec = |k: &str| r.get(k).or_else(|| first.and_then(|h| h.get(k)));
    let axis = xyz(rec("axis")?)?;
    let k = (0..3).find(|&k| axis[k].abs() > 0.999)?;
    let n = |key: &str| rec(key).and_then(Value::as_f64);
    let d = first
        .and_then(|h| h.get("diameter"))
        .and_then(Value::as_f64)
        .or_else(|| n("diameter"))
        .unwrap_or(0.0);
    Some((k, d, n("depth").or_else(|| n("height")).unwrap_or(0.0)))
}

/// Where *f* is on its part (*all* its features, *box* its size), as a person finds it.
fn locate(f: &Feature, all: &[Feature], size: &Size) -> Place {
    // A round along one of the part's axes: the boss whose edge it rounds.
    if matches!(f.family.as_str(), "fillets" | "blends")
        && let (Some(p), Some(k), Some(radius)) = (
            position(&f.record),
            f.record
                .get("axis")
                .and_then(Value::as_str)
                .and_then(|a| AXES.iter().position(|x| x.eq_ignore_ascii_case(a))),
            f.record.get("radius").and_then(Value::as_f64),
        )
    {
        let round = all
            .iter()
            .filter(|g| g.family == "bosses")
            .filter_map(|g| {
                let (q, (j, db, _)) = (position(&g.record)?, along(&g.record)?);
                let r = (0..3)
                    .filter(|&i| i != k)
                    .map(|i| (q[i] - p[i]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                (j == k && (r - db / 2.0).abs() <= radius + 0.5).then_some(db)
            })
            .min_by(f64::total_cmp);
        return match round {
            Some(db) => Place {
                on: format!("round the Ø{} boss", num(db)),
                ..Place::default()
            },
            None => Place::default(),
        };
    }
    let (Some(p), Some((k, d, length))) = (position(&f.record), along(&f.record)) else {
        return Place::default();
    };
    // Its side of the part: an end of it on a face of the part's box, or both.
    let ends = [p[k], p[k] + length, p[k] - length];
    let touches = |edge: f64| ends.iter().any(|x| (x - edge).abs() < 1.0);
    let points = f.record.get("axis").and_then(xyz).map(|a| a[k]);
    let side = match (touches(size.min[k]), touches(size.max[k])) {
        _ if f.family == "bosses" => {
            points.map(|a| format!("on the {}{} side", if a > 0.0 { "+" } else { "−" }, AXES[k]))
        }
        (true, true) => Some(format!("through the part along {}", AXES[k])),
        (true, false) => Some(format!("on the −{} side", AXES[k])),
        (false, true) => Some(format!("on the +{} side", AXES[k])),
        _ => None,
    };
    let mut on = side.unwrap_or_default();
    // Parallel features: their axis's distance from this one's.
    let parallel = |family: &'static str| {
        all.iter()
            .filter(move |g| g.family == family && g.id != f.id)
            .filter_map(move |g| {
                let (q, (j, dg, _)) = (position(&g.record)?, along(&g.record)?);
                let r = (0..3)
                    .filter(|&i| i != k)
                    .map(|i| (q[i] - p[i]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                (j == k).then_some((q, dg, r))
            })
    };
    let offset = |from: [f64; 3], sign: bool| {
        (0..3)
            .filter(|&i| i != k && (p[i] - from[i]).abs() >= 0.05)
            .map(|i| {
                let d = p[i] - from[i];
                let d = if sign {
                    signed(d)
                } else {
                    num((d * 10.0).round() / 10.0)
                };
                format!("{} {d}", AXES[i])
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let join = |on: &mut String, text: String| {
        if !on.is_empty() {
            on.push_str(", ");
        }
        on.push_str(&text);
    };
    // The smallest boss it sits in, and the bore through that boss.
    let boss = parallel("bosses")
        .filter(|(_, db, r)| *db > d && r + d / 2.0 <= db / 2.0 + 0.01)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((q, db, r)) = boss {
        let bore = parallel("holes")
            .filter(|(h, dh, _)| {
                *dh < db
                    && (0..3)
                        .filter(|&i| i != k)
                        .all(|i| (h[i] - q[i]).abs() < 0.01)
            })
            .map(|(_, dh, _)| dh)
            .filter(|dh| *dh > d)
            .max_by(f64::total_cmp);
        join(
            &mut on,
            match bore {
                Some(dh) => format!("in the Ø{} boss around the Ø{} hole", num(db), num(dh)),
                None => format!("in the Ø{} boss", num(db)),
            },
        );
        if r < 0.05 {
            return Place {
                on,
                ..Place::default()
            };
        }
        return Place {
            on,
            offset: offset(q, true),
            from: "its axis".into(),
        };
    }
    // Else a larger hole it sits near.
    if let Some((q, dh, _)) = parallel("holes")
        .filter(|(_, dh, r)| *dh >= 2.0 * d && *r < *dh)
        .min_by(|a, b| a.2.total_cmp(&b.2))
    {
        return Place {
            on,
            offset: offset(q, true),
            from: format!("the Ø{} hole's axis", num(dh)),
        };
    }
    // Else the corner of the part's box.
    let corner = (0..3)
        .filter(|&i| i != k)
        .map(|i| format!("−{}", AXES[i]))
        .collect::<Vec<_>>()
        .join(", ");
    Place {
        on,
        offset: offset(size.min, false),
        from: format!("the {corner} corner"),
    }
}

/// A feature in words, as a drawing names it (`Ø5 × 30 through hole`), and where it is.
/// Keys of a feature's changes that are its surroundings, not the feature: what it borders.
const CONTEXT: [&str; 2] = ["adjacency", "neighbourhood"];

/// The changes read as a person would say them, not as features were paired:
///
/// - one line for a feature recognised twice (the same faces, as two records: a pocket by its
///   depth and by its section), the record whose sizes read as words kept;
/// - a pattern laid out again (another count, circle or spacing) as one pattern become
///   another, its holes' own pairings folded into it;
/// - a hole that became another kind of hole in its place (counterbored, blind) as that.
fn fold(changes: &mut Vec<FeatureChange>, old: &[Feature], new: &[Feature]) {
    // A tenth of the smaller feature's radius, with a 0.1 mm rounding floor.
    // The part's box must not make two small, distant holes count as the same place.
    let radius = |f: &Feature| {
        if f.family == "hole_patterns" {
            if f.id.contains("bolt_circle") {
                return f
                    .record
                    .get("diameter")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    / 2.0;
            }
            if let (Some(center), Some(holes)) = (
                position(&f.record),
                f.record.get("holes").and_then(Value::as_array),
            ) {
                return holes
                    .iter()
                    .filter_map(Value::as_object)
                    .filter_map(position)
                    .map(|p| {
                        (0..3)
                            .map(|i| (p[i] - center[i]).powi(2))
                            .sum::<f64>()
                            .sqrt()
                    })
                    .fold(0.0, f64::max);
            }
        }
        f.record
            .get("diameter")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            / 2.0
    };
    let near = |a: &Feature, b: &Feature, across: Option<usize>| match (
        position(&a.record),
        position(&b.record),
    ) {
        (Some(p), Some(q)) => {
            let tolerance = (radius(a).min(radius(b)) * 0.1).max(0.1);
            (0..3)
                .filter(|&i| Some(i) != across)
                .map(|i| (p[i] - q[i]).powi(2))
                .sum::<f64>()
                .sqrt()
                <= tolerance
        }
        _ => false,
    };
    let find = |fs: &'_ [Feature], id: &str| fs.iter().find(|f| f.id == id).cloned();

    // The same faces recognised twice: the record with plain sizes, not a section's sides.
    let plain = |c: &FeatureChange| {
        c.changes
            .keys()
            .filter(|k| k.matches('.').count() > 1)
            .count()
    };
    let mut kept: Vec<FeatureChange> = Vec::new();
    for c in changes.drain(..) {
        match kept.iter_mut().find(|k| {
            k.class == c.class
                && k.family == c.family
                && k.old == c.old
                && k.new == c.new
                && k.old.is_some()
        }) {
            Some(k) if plain(&c) < plain(k) => *k = c,
            Some(_) => {}
            None => kept.push(c),
        }
    }
    *changes = kept;

    // A pattern laid out again: the old one gone or unpaired, a new one in its place.
    let mut folded_old: BTreeSet<String> = BTreeSet::new();
    let mut folded_new: BTreeSet<String> = BTreeSet::new();
    for i in 0..changes.len() {
        let c = &changes[i];
        if c.family != "hole_patterns" || !matches!(c.class.as_str(), "orphaned" | "ambiguous") {
            continue;
        }
        let Some(was) = c.old.as_deref().and_then(|id| find(old, id)) else {
            continue;
        };
        let candidate = c
            .candidates
            .iter()
            .filter_map(|id| find(new, id))
            .find(|f| f.family == "hole_patterns")
            .or_else(|| {
                changes
                    .iter()
                    // Where a pattern is along its holes' axis is where they start, not it.
                    .filter(|n| {
                        n.class == "new"
                            && n.family == "hole_patterns"
                            && n.new
                                .as_deref()
                                .and_then(|id| find(new, id))
                                .is_some_and(|now| near(&was, &now, c.axis.or(n.axis)))
                    })
                    .find_map(|n| n.new.as_deref().and_then(|id| find(new, id)))
            });
        let Some(now) = candidate else { continue };
        folded_old.extend(was.members.iter().cloned());
        folded_new.extend(now.members.iter().cloned());
        folded_new.insert(now.id.clone());
        let (became, _) = describe(&now);
        let c = &mut changes[i];
        c.class = "replaced".into();
        c.new = Some(now.id.clone());
        c.candidates.clear();
        c.became = became;
    }
    changes.retain(|c| {
        let member_old =
            c.family == "holes" && c.old.as_ref().is_some_and(|id| folded_old.contains(id));
        let member_new =
            c.class != "replaced" && c.new.as_ref().is_some_and(|id| folded_new.contains(id));
        let ambiguous_member = c.family == "holes"
            && c.class == "ambiguous"
            && c.candidates.iter().all(|id| folded_new.contains(id));
        !(member_old || member_new || ambiguous_member)
    });

    // A hole that became another kind of hole in its place.
    let mut taken: BTreeSet<usize> = BTreeSet::new();
    for i in 0..changes.len() {
        if changes[i].family != "holes" || changes[i].class != "orphaned" {
            continue;
        }
        let axis = changes[i].axis;
        let Some(was) = changes[i].old.as_deref().and_then(|id| find(old, id)) else {
            continue;
        };
        let Some(j) = (0..changes.len()).find(|&j| {
            !taken.contains(&j)
                && changes[j].class == "new"
                && changes[j].family == "holes"
                && changes[j].axis == axis
                && changes[j]
                    .new
                    .as_deref()
                    .and_then(|id| find(new, id))
                    .is_some_and(|now| near(&was, &now, axis))
        }) else {
            continue;
        };
        taken.insert(j);
        let (became, id) = (changes[j].what.clone(), changes[j].new.clone());
        let c = &mut changes[i];
        c.class = "replaced".into();
        c.new = id;
        c.became = became;
    }
    let mut index = 0;
    changes.retain(|_| {
        let keep = !taken.contains(&index);
        index += 1;
        keep
    });

    // A pattern whose holes changed kind "moves" only as its holes' faces do: that is theirs.
    let replaced: BTreeSet<&str> = changes
        .iter()
        .filter(|c| c.class == "replaced" && c.family == "holes")
        .filter_map(|c| c.old.as_deref())
        .collect();
    let knock_on: Vec<bool> = changes
        .iter()
        .map(|c| {
            c.family == "hole_patterns"
                && c.class == "adapted"
                && sizes(&c.changes).is_empty()
                && c.old
                    .as_deref()
                    .and_then(|id| find(old, id))
                    .is_some_and(|p| p.members.iter().any(|m| replaced.contains(m.as_str())))
        })
        .collect();
    let mut index = 0;
    changes.retain(|_| {
        let keep = !knock_on[index];
        index += 1;
        keep
    });
}

/// A tangent chain of unchanged bevels can grow round new fillets. Count its old
/// chamfers, not the extra faces. Require a carried face as the anchor, equal bevel
/// sizes, and each extra face to touch a new fillet or bridge two such faces.
fn continue_chamfers(
    changes: &mut Vec<FeatureChange>,
    old: &[Feature],
    new: &[Feature],
    corr: &Correspondence,
    prints: &qc::Fingerprints,
) {
    let added = |f: &Feature| {
        changes
            .iter()
            .any(|c| c.class == "new" && c.new.as_ref() == Some(&f.id))
    };
    let fillets: BTreeSet<usize> = new
        .iter()
        .filter(|f| f.family == "fillets" && added(f))
        .flat_map(|f| f.faces.iter().copied())
        .collect();
    if fillets.is_empty() {
        return;
    }
    let neighbours: BTreeMap<_, _> = prints
        .faces
        .iter()
        .map(|f| (f.index, &f.neighbours))
        .collect();
    let touches_fillet = |i: &usize| {
        neighbours
            .get(i)
            .is_some_and(|ns| ns.iter().any(|(j, _)| fillets.contains(j)))
    };
    let mut continued = Vec::new();
    let mut absorbed = BTreeSet::new();
    for was in old.iter().filter(|f| f.family == "chamfers") {
        // Never turn a resized, removed or ambiguously matched bevel into a continuation.
        if changes.iter().any(|c| c.old.as_ref() == Some(&was.id)) {
            continue;
        }
        let mapped: Option<Vec<usize>> = was
            .faces
            .iter()
            .map(|i| {
                corr.faces
                    .iter()
                    .find(|e| e.old == *i && matches!(e.class, Class::Carried | Class::Adapted))
                    .and_then(|e| e.new)
            })
            .collect();
        let Some(mapped) = mapped.filter(|fs| !fs.is_empty()) else {
            continue;
        };
        let same_sizes = |f: &&Feature| {
            ["leg1", "leg2", "angle"].iter().all(|key| {
                match (
                    was.record.get(*key).and_then(Value::as_f64),
                    f.record.get(*key).and_then(Value::as_f64),
                ) {
                    (Some(a), Some(b)) => (a - b).abs() <= 0.01,
                    _ => false,
                }
            })
        };
        let bevels: Vec<_> = new
            .iter()
            .filter(|f| f.family == "chamfers")
            .filter(same_sizes)
            .collect();
        let Some(anchor) = bevels
            .iter()
            .find(|f| mapped.iter().all(|i| f.faces.contains(i)))
        else {
            continue;
        };
        let allowed: BTreeSet<_> = bevels
            .iter()
            .flat_map(|f| f.faces.iter().copied())
            .collect();
        let mut reached: BTreeSet<_> = mapped.into_iter().collect();
        let mut pending: Vec<_> = reached.iter().copied().collect();
        while let Some(i) = pending.pop() {
            if let Some(ns) = neighbours.get(&i) {
                for (j, arc) in *ns {
                    if arc == "smooth" && allowed.contains(j) && reached.insert(*j) {
                        pending.push(*j);
                    }
                }
            }
        }
        let extensions: Vec<_> = bevels
            .iter()
            .filter(|f| {
                added(f)
                    && !f.faces.is_empty()
                    && f.faces.iter().all(|i| {
                        reached.contains(i)
                            && (touches_fillet(i)
                                || neighbours.get(i).is_some_and(|ns| {
                                    ns.iter()
                                        .filter(|(j, arc)| {
                                            arc == "smooth"
                                                && reached.contains(j)
                                                && touches_fillet(j)
                                        })
                                        .count()
                                        >= 2
                                }))
                    })
            })
            .collect();
        if extensions.is_empty() {
            continue;
        }
        let candidates: Vec<_> = std::iter::once(anchor.id.clone())
            .chain(extensions.iter().map(|f| f.id.clone()))
            .collect();
        absorbed.extend(extensions.iter().map(|f| f.id.clone()));
        continued.push(FeatureChange {
            class: "continued".into(),
            family: "chamfers".into(),
            old: Some(was.id.clone()),
            new: Some(anchor.id.clone()),
            candidates,
            changes: BTreeMap::new(),
            what: "chamfer".into(),
            became: String::new(),
            at: None,
            place: Place::default(),
            axis: None,
            there: Vec::new(),
            new_faces: Vec::new(),
            old_faces: Vec::new(),
        });
    }
    changes
        .retain(|c| !(c.class == "new" && c.new.as_ref().is_some_and(|id| absorbed.contains(id))));
    changes.extend(continued);
}

fn describe(f: &Feature) -> (String, Option<[f64; 3]>) {
    let r = &f.record;
    let n = |k: &str| r.get(k).and_then(Value::as_f64);
    let at = position(r);
    let hole = |h: &Map<String, Value>| {
        let n = |k: &str| h.get(k).and_then(Value::as_f64);
        let mut finishes = Vec::new();
        let mut step = |v: &Value, word: &str| {
            if let (Some(d), Some(depth)) = (v["diameter"].as_f64(), v["depth"].as_f64()) {
                finishes.push(format!("{word} Ø{} × {} deep", num(d), num(depth)));
            }
        };
        for (key, word) in [("cbore", "counterbored"), ("spotface", "spotfaced")] {
            if let Some(v) = h.get(key) {
                step(v, word);
            }
        }
        if let Some(steps) = h.get("outer_steps").and_then(Value::as_array) {
            for v in steps {
                step(v, "counterbored");
            }
        }
        if let Some(v) = h.get("csink").filter(|v| !v.is_null()) {
            let mut finish = "countersunk".to_string();
            if let Some(d) = v["major_diameter"].as_f64() {
                finish += &format!(" Ø{}", num(d));
            }
            if let Some(angle) = v["included_angle"].as_f64() {
                finish += &format!(" × {}°", num(angle));
            }
            if let Some(depth) = v["depth"].as_f64() {
                finish += &format!(", {} deep", num(depth));
            }
            finishes.push(finish);
        }
        let d = n("diameter").map(|d| format!("Ø{}", num(d)));
        // A finished through hole's bore depth excludes the mouth enlargement: call it
        // through and give each enlargement's own depth, rather than imply a shorter hole.
        let depth = n("depth").filter(|_| finishes.is_empty() || h["bottom"] != "through");
        let size = match (d, depth) {
            (Some(d), Some(depth)) => format!("{d} × {} ", num(depth)),
            (Some(d), None) => format!("{d} "),
            _ => String::new(),
        };
        let kind = match h.get("bottom").and_then(Value::as_str) {
            Some("through") => "through hole",
            Some(_) => "blind hole",
            None => "hole",
        };
        let finish = if finishes.is_empty() {
            String::new()
        } else {
            format!(", {}", finishes.join(", "))
        };
        format!("{size}{kind}{finish}")
    };
    let what = if let Some(description) = f.description() {
        description
    } else {
        match f.family.as_str() {
            "holes" => hole(r),
            "hole_patterns" => {
                let holes = r.get("holes").and_then(Value::as_array);
                let count = holes.map_or(0, Vec::len);
                let first = holes.and_then(|h| h.first()).and_then(Value::as_object);
                let each = first.map_or_else(|| "holes".to_string(), |h| several(&hole(h)));
                match n("diameter").filter(|_| f.id.contains("bolt_circle")) {
                    Some(d) => format!("pattern of {count} × {each} on a Ø{} circle", num(d)),
                    None => format!("pattern of {count} × {each}"),
                }
            }
            "bosses" => match (n("diameter"), n("height")) {
                (Some(d), Some(h)) => format!("Ø{} × {} boss", num(d), num(h)),
                _ => "boss".into(),
            },
            "fillets" | "blends" => match n("radius") {
                Some(radius) => format!("R{} {}", num(radius), singular(&f.family)),
                None => singular(&f.family),
            },
            "chamfers" => match (n("leg1"), n("leg2"), n("angle")) {
                (Some(a), Some(b), Some(angle)) if (a - b).abs() < 0.01 => {
                    format!("{} × {}° chamfer", num(a), num(angle))
                }
                (Some(a), Some(b), Some(angle)) => {
                    format!("{} × {} × {}° chamfer", num(a), num(b), num(angle))
                }
                _ => "chamfer".into(),
            },
            "slots" => match (n("length"), n("width"), n("d_lo"), n("d_hi")) {
                (Some(l), Some(w), Some(lo), Some(hi)) => {
                    format!("{} × {} × {} slot", num(l), num(w), num(hi - lo))
                }
                (Some(l), Some(w), ..) => format!("{} × {} slot", num(l), num(w)),
                _ => "slot".into(),
            },
            "plates" => match (n("lo"), n("hi")) {
                (Some(lo), Some(hi)) => format!("{} thick plate", num(hi - lo)),
                _ => "plate".into(),
            },
            other => singular(other),
        }
    };
    (what, at)
}

/// The question and feature of an answer on one feature (`hole.function:holes:12.13`), as
/// resuming reads them.
fn feature_answer(id: &str) -> Option<(&str, &str, Vec<usize>)> {
    let (question, feature) = id.split_once(':')?;
    if !matches!(
        question,
        "hole.function"
            | "hole.full_thread"
            | "hole.frame"
            | "hole.position"
            | "hole.location"
            | "diameter.fit"
            | "diameter.runout"
    ) {
        return None;
    }
    let (family, support) = feature.split_once(':')?;
    let faces = support
        .split('.')
        .map(|s| s.parse().ok())
        .collect::<Option<Vec<usize>>>()?;
    Some((question, family, faces))
}

struct Matching<'a> {
    named: bool,
    faces: BTreeMap<usize, Bind>,
    old_features: &'a [Feature],
    new_features: &'a [Feature],
}

impl Matching<'_> {
    /// The old faces in the new part, or the first that does not carry.
    fn faces(&self, old: &[usize]) -> std::result::Result<Vec<usize>, (usize, Bind)> {
        old.iter()
            .map(|i| match self.faces.get(i) {
                Some(Bind::To(n)) => Ok(*n),
                Some(other) => Err((*i, other.clone())),
                None => Err((*i, Bind::Orphaned)),
            })
            .collect()
    }

    /// The new feature of *family* holding *faces*, or every candidate.
    fn feature(&self, family: &str, faces: &[usize]) -> std::result::Result<&Feature, Vec<String>> {
        let found: Vec<&Feature> = self
            .new_features
            .iter()
            .filter(|f| {
                f.id.split_once(':').map(|(fam, _)| fam) == Some(family)
                    && faces.iter().all(|i| f.faces.contains(i))
            })
            .collect();
        match found.as_slice() {
            [one] => Ok(one),
            many => Err(many.iter().map(|f| f.id.clone()).collect()),
        }
    }

    /// *id*'s answer on the new part: its new ID, or why it does not carry.
    fn answer(
        &self,
        id: &str,
        answer: &Value,
    ) -> std::result::Result<(String, Value), Box<Unresolved>> {
        let unresolved = |class: &str, reason: String, candidates: Vec<String>| {
            Box::new(Unresolved {
                id: id.into(),
                answer: answer.clone(),
                class: class.into(),
                reason,
                candidates,
                question: String::new(),
            })
        };
        let gone = |(face, bind): (usize, Bind)| match bind {
            Bind::NamedRefusal { reason, ambiguous } => unresolved(
                if ambiguous { "ambiguous" } else { "unresolved" },
                reason,
                Vec::new(),
            ),
            Bind::Ambiguous(c) => unresolved(
                "ambiguous",
                format!("face {face} matches more than one face of the new part"),
                c.iter().map(|n| format!("face {n}")).collect(),
            ),
            _ => unresolved(
                "orphaned",
                format!("face {face} has no match in the new part"),
                Vec::new(),
            ),
        };
        if let Some((question, family, faces)) = feature_answer(id) {
            let feature = &id[question.len() + 1..];
            if !self.old_features.iter().any(|f| f.id == feature) {
                return Err(unresolved(
                    "stale",
                    format!("the old part has no {feature} either"),
                    Vec::new(),
                ));
            }
            let now = self.faces(&faces).map_err(gone)?;
            return match self.feature(family, &now) {
                Ok(f) => Ok((format!("{question}:{}", f.id), answer.clone())),
                Err(candidates) if candidates.is_empty() => Err(unresolved(
                    "unmatched",
                    format!("no {family} feature of the new part holds its faces"),
                    candidates,
                )),
                Err(candidates) => Err(unresolved(
                    "ambiguous",
                    format!("several {family} features of the new part hold its faces"),
                    candidates,
                )),
            };
        }
        if id.starts_with("datum.")
            && let Some(list) = answer.as_array()
            && list.iter().all(Value::is_u64)
        {
            let old: Vec<usize> = list
                .iter()
                .filter_map(Value::as_u64)
                .map(|i| i as usize)
                .collect();
            let now = self.faces(&old).map_err(gone)?;
            return Ok((
                id.into(),
                Value::Array(now.into_iter().map(Value::from).collect()),
            ));
        }
        let value = if self.named {
            let mut failure = None;
            crate::resume::names::map_value(id, answer, |i| {
                self.faces(&[i]).map(|v| v[0]).map_err(|error| {
                    let refusal = gone(error);
                    let reason = refusal.reason.clone();
                    failure = Some(refusal);
                    reason
                })
            })
            .map_err(|reason| {
                failure.unwrap_or_else(|| unresolved("unresolved", reason, Vec::new()))
            })?
        } else {
            answer.clone()
        };
        Ok((id.into(), value))
    }
}

/// Carry *answers* (and *automatic*, the IDs the default path answered) from *old* to *new*.
pub fn diff(old: &LoadedPart, new: &LoadedPart, given: &Map<String, Value>) -> Result<Revised> {
    diff_with_options(old, new, given, DiffOptions::default())
}

/// Compare once, optionally retaining the exact scenes in the report JSON.
pub fn diff_with_options(
    old: &LoadedPart,
    new: &LoadedPart,
    given: &Map<String, Value>,
    options: DiffOptions,
) -> Result<Revised> {
    crate::answer_file::check_for_diff(old, given)
        .map_err(|why| api::Error::Rules(crate::rules::Error::Invalid(why)))?;
    // One recognition of each part, for both its analysis and its fingerprints.
    let old_q = qc::recognise(&old.part);
    let new_q = qc::recognise(&new.part);
    let (old_analysis, new_analysis) = (
        api::analyse_recognised(old, &old_q)?,
        api::analyse_recognised(new, &new_q)?,
    );
    // The PMI Specify wrote in a file: its answers.
    let stored = |a: &Value| a["resume"]["answers"].as_object().cloned();
    let (old_stored, new_stored) = (stored(&old_analysis), stored(&new_analysis));
    // Answers given win; else the old file's own.
    let (answers, answers_from) = match (&old_stored, given.is_empty()) {
        (_, false) => (given.clone(), "given"),
        (Some(a), true) => (a.clone(), "stored"),
        (None, true) => (Map::new(), "none"),
    };
    let answers = &answers;
    let features = |a: &Value| -> Result<Vec<Feature>> {
        serde_json::from_value(a["features"].clone())
            .map_err(|e| api::Error::Analysis(format!("the analysis's features: {e}")))
    };
    let (old_features, new_features) = (features(&old_analysis)?, features(&new_analysis)?);
    let corr =
        qc::correspond(&old_q.fingerprints, &new_q.fingerprints).map_err(api::Error::Analysis)?;
    let geometric_faces: BTreeMap<usize, Bind> = corr
        .faces
        .iter()
        .map(|e| {
            let bind = match e.class {
                Class::Carried | Class::Adapted => e.new.map_or(Bind::Orphaned, Bind::To),
                Class::Ambiguous => Bind::Ambiguous(e.candidates.clone()),
                _ => Bind::Orphaned,
            };
            (e.old, bind)
        })
        .collect();
    // Stored answers bind authored names. Existing CAD PMI keeps its independent
    // geometric comparison: generic duplicate CAD labels are not saved answer anchors.
    let mut faces = geometric_faces.clone();
    let identities = crate::resume::names::identities(&old.face_names);
    for (i, binding) in crate::resume::names::match_names(&identities, &new.face_names) {
        use crate::resume::names::NameMatch;
        let bind = match binding {
            NameMatch::To(i) => Bind::To(i),
            NameMatch::Missing(reason) => Bind::NamedRefusal {
                reason,
                ambiguous: false,
            },
            NameMatch::Ambiguous(reason) => Bind::NamedRefusal {
                reason,
                ambiguous: true,
            },
        };
        faces.insert(i, bind);
    }
    let matching = Matching {
        named: identities.iter().any(Option::is_some),
        faces,
        old_features: &old_features,
        new_features: &new_features,
    };

    let (old_size, new_size) = (size(&old.part), size(&new.part));

    // Features, by specify-core's IDs.
    let (old_records, new_records) = (
        records(&old_features, &old_q.fingerprints),
        records(&new_features, &new_q.fingerprints),
    );
    let family_of = |fs: &[Feature], id: &str| {
        fs.iter()
            .find(|f| f.id == id)
            .map(|f| f.family.clone())
            .unwrap_or_default()
    };
    let mut feature_changes = Vec::new();
    let mut changed: BTreeMap<String, BTreeMap<String, qc::Change>> = BTreeMap::new();
    let mut reached = BTreeSet::new();
    for e in &corr.features {
        let Some(old_id) = old_records.get(&e.old) else {
            continue;
        };
        if EVIDENCE.contains(&family_of(&old_features, old_id).as_str()) {
            continue;
        }
        let new_id = e.new.as_ref().and_then(|n| new_records.get(n)).cloned();
        if let Some(n) = &new_id {
            reached.insert(n.clone());
        }
        let class = match e.class {
            Class::Carried => continue,
            // What a feature borders is context, not a change to it: a chamfer whose corners
            // are rounded is the same chamfer.
            Class::Adapted if e.changes.keys().all(|k| CONTEXT.contains(&k.as_str())) => continue,
            Class::Adapted => "adapted",
            Class::Ambiguous => "ambiguous",
            _ => "orphaned",
        };
        if let Some(n) = &new_id {
            changed.insert(n.clone(), e.changes.clone());
        }
        let candidates: Vec<String> = e
            .candidates
            .iter()
            .filter_map(|c| new_records.get(c))
            .cloned()
            .collect();
        reached.extend(candidates.iter().cloned());
        let old_feature = old_features.iter().find(|f| &f.id == old_id);
        let (what, at) = old_feature.map_or_else(|| (String::new(), None), describe);
        let place = old_feature
            .map(|f| locate(f, &old_features, &old_size))
            .unwrap_or_default();
        feature_changes.push(FeatureChange {
            class: class.into(),
            family: family_of(&old_features, old_id),
            old: Some(old_id.clone()),
            new: new_id,
            candidates,
            changes: e.changes.clone(),
            what,
            became: String::new(),
            at,
            place,
            axis: old_feature
                .and_then(|f| along(&f.record))
                .map(|(k, _, _)| k),
            there: Vec::new(),
            new_faces: Vec::new(),
            old_faces: Vec::new(),
        });
    }
    for f in &new_features {
        if !f.faces.is_empty()
            && !EVIDENCE.contains(&f.family.as_str())
            && new_records.values().any(|v| v == &f.id)
            && !reached.contains(&f.id)
        {
            let (what, at) = describe(f);
            let place = locate(f, &new_features, &new_size);
            feature_changes.push(FeatureChange {
                class: "new".into(),
                family: f.family.clone(),
                old: None,
                new: Some(f.id.clone()),
                candidates: Vec::new(),
                changes: BTreeMap::new(),
                what,
                became: String::new(),
                at,
                place,
                axis: along(&f.record).map(|(k, _, _)| k),
                there: Vec::new(),
                new_faces: Vec::new(),
                old_faces: Vec::new(),
            });
        }
    }

    fold(&mut feature_changes, &old_features, &new_features);

    treatments::fold_end_chamfers(
        &mut feature_changes,
        &old_features,
        &new_features,
        &old.part,
        &new.part,
        &corr,
    );

    continue_chamfers(
        &mut feature_changes,
        &old_features,
        &new_features,
        &corr,
        &new_q.fingerprints,
    );

    // An ambiguous feature's candidates in its own place on the new part.
    for f in feature_changes
        .iter_mut()
        .filter(|f| f.class == "ambiguous")
    {
        f.there = f
            .candidates
            .iter()
            .filter(|c| {
                f.place.on.is_empty()
                    || new_features
                        .iter()
                        .find(|g| &&g.id == c)
                        .is_some_and(|g| locate(g, &new_features, &new_size).on == f.place.on)
            })
            .cloned()
            .collect();
    }

    // The answers.
    let mut carried = Map::new();
    let (mut migrated, mut unresolved, mut check) = (Vec::new(), Vec::new(), Vec::new());
    // Each question the old part asks, in words.
    let prompts: BTreeMap<String, String> = api::questions(&old_analysis, answers)?
        .into_iter()
        .map(|q| (q.id, q.prompt))
        .collect();
    let asked = |id: &str| prompts.get(id).cloned().unwrap_or_default();
    let results: Vec<_> = answers
        .iter()
        .filter(|(id, _)| id.as_str() != crate::answer_file::META)
        .map(|(id, answer)| (id, answer, matching.answer(id, answer)))
        .collect();
    let mut reached: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, r) in &results {
        if let Ok((now, _)) = r {
            *reached.entry(now.as_str()).or_default() += 1;
        }
    }
    for (id, answer, result) in &results {
        match result {
            // Two answers would carry to one: neither is chosen, as resuming does.
            Ok((now, _)) if reached[now.as_str()] > 1 => unresolved.push(Unresolved {
                id: (*id).clone(),
                answer: (*answer).clone(),
                class: "ambiguous".into(),
                reason: format!("several requirements map to {now}; review required"),
                candidates: vec![now.clone()],
                question: String::new(),
            }),
            Ok((now, value)) => {
                let (id, now, value) = (*id, now.clone(), value.clone());
                if now != *id {
                    migrated.push((id.clone(), now.clone()));
                }
                // A pattern changes with its members: an answer on two holes is checked when
                // they are resized.
                let changes_of = |feature: &str| {
                    changed.get(feature).cloned().or_else(|| {
                        new_features
                            .iter()
                            .find(|f| f.id == feature)
                            .and_then(|f| f.members.iter().find_map(|m| changed.get(m).cloned()))
                    })
                };
                if let Some(feature) = now.split_once(':').map(|(_, f)| f.to_string())
                    && let Some(changes) = changes_of(&feature)
                {
                    check.push(Check {
                        id: now.clone(),
                        answer: value.clone(),
                        axis: new_features
                            .iter()
                            .find(|f| f.id == feature)
                            .and_then(|f| along(&f.record))
                            .map(|(k, _, _)| k),
                        feature,
                        changes,
                        question: asked(id),
                    });
                }
                carried.insert(now, value);
            }
            Err(u) => unresolved.push((**u).clone()),
        }
    }
    // A datum's settings (`datum.A.flatness`) follow it: they stay open with an open datum.
    let open_datums: Vec<(String, String)> = unresolved
        .iter()
        .filter(|u| u.id.starts_with("datum.") && u.id.matches('.').count() == 1)
        .map(|u| (format!("{}.", u.id), u.class.clone()))
        .collect();
    let settings: Vec<(String, String)> = carried
        .keys()
        .filter_map(|id| {
            open_datums
                .iter()
                .find(|(datum, _)| id.starts_with(datum.as_str()))
                .map(|(_, class)| (id.clone(), class.clone()))
        })
        .collect();
    for (id, class) in settings {
        let answer = carried
            .remove(&id)
            .expect("listed from the carried answers");
        let datum = &id[..id.rfind('.').expect("a datum setting")];
        unresolved.push(Unresolved {
            reason: format!("its datum {datum} is unresolved"),
            id,
            answer,
            class,
            candidates: Vec::new(),
            question: String::new(),
        });
    }
    for u in &mut unresolved {
        u.question = asked(&u.id);
        // An answer on a removed feature says so, not which face went.
        if u.class == "orphaned"
            && let Some((question, _, _)) = feature_answer(&u.id)
            && let Some(gone) = feature_changes.iter().find(|f| {
                f.class == "orphaned" && f.old.as_deref() == Some(&u.id[question.len() + 1..])
            })
            && !gone.what.is_empty()
        {
            u.reason = format!("the {} was removed", gone.what);
        }
    }

    // What the new part asks and finds that the old one did not.
    // An old ID in the new part; one that does not carry names nothing new, so it can't match a
    // new question or problem even where its old number is reused.
    let rename = |id: &str| {
        matching
            .answer(id, &Value::Null)
            .map(|(n, _)| n)
            .unwrap_or_else(|_| format!("\u{0}gone {id}"))
    };
    let open = |analysis: &Value, answers: &Map<String, Value>| -> Result<Vec<(String, String)>> {
        let qs = api::questions(analysis, answers)?;
        Ok(crate::rules::open_questions(&qs, answers)
            .iter()
            .map(|q| {
                (
                    q.id.clone(),
                    q.title
                        .as_ref()
                        .filter(|t| !q.prompt.contains(t.as_str()))
                        .map_or_else(|| q.prompt.clone(), |t| format!("{t}: {}", q.prompt)),
                )
            })
            .collect())
    };
    let before: BTreeSet<String> = open(&old_analysis, answers)?
        .into_iter()
        .map(|(id, _)| rename(&id))
        .collect();
    let questions: Vec<OpenQuestion> = open(&new_analysis, &carried)?
        .into_iter()
        .filter(|(id, _)| !before.contains(id))
        .map(|(id, title)| OpenQuestion { id, title })
        .collect();
    let warnings = |analysis: &Value, answers: &Map<String, Value>| -> Result<Vec<Value>> {
        let review = api::review(analysis, answers)?;
        Ok(review["problems"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p["severity"] != "info")
            .collect())
    };
    // A problem is its code, its subject and its message; an old problem's subject (an answer
    // ID, which its message may quote) is read in the new part first.
    let key = |p: &Value, old: bool| {
        let text = |k: &str| p[k].as_str().unwrap_or_default().to_string();
        let (about, message) = (text("about"), text("message"));
        if old && !about.is_empty() {
            let now = rename(&about);
            let message = message.replace(&about, &now);
            (text("code"), now, message)
        } else {
            (text("code"), about, message)
        }
    };
    let (was, now) = (
        warnings(&old_analysis, answers)?,
        warnings(&new_analysis, &carried)?,
    );
    let (was_keys, now_keys): (BTreeSet<_>, BTreeSet<_>) = (
        was.iter().map(|p| key(p, true)).collect(),
        now.iter().map(|p| key(p, false)).collect(),
    );
    let problems = Problems {
        added: now
            .iter()
            .filter(|p| !was_keys.contains(&key(p, false)))
            .cloned()
            .collect(),
        resolved: was
            .iter()
            .filter(|p| !now_keys.contains(&key(p, true)))
            .cloned()
            .collect(),
    };

    // The picture's faces.
    let mut face_class: BTreeMap<usize, &'static str> = BTreeMap::new();
    for n in &corr.new_faces {
        face_class.insert(*n, "new");
    }
    let mut gone: Vec<usize> = corr
        .faces
        .iter()
        .filter(|e| e.class == Class::Orphaned)
        .map(|e| e.old)
        .collect();
    for c in &mut feature_changes {
        let faces_of = |fs: &[Feature], id: &Option<String>| -> Vec<usize> {
            id.as_ref()
                .and_then(|id| fs.iter().find(|f| &f.id == id))
                .map(|f| f.faces.clone())
                .unwrap_or_default()
        };
        c.old_faces = faces_of(&old_features, &c.old);
        c.new_faces = if matches!(c.class.as_str(), "ambiguous" | "continued") {
            let mut faces: Vec<usize> = c
                .candidates
                .iter()
                .flat_map(|id| faces_of(&new_features, &Some(id.clone())))
                .collect();
            faces.sort_unstable();
            faces.dedup();
            faces
        } else {
            faces_of(&new_features, &c.new)
        };
        match c.class.as_str() {
            "adapted" | "continued" => {
                for &i in &c.new_faces {
                    face_class.insert(i, "changed");
                }
                if c.changes.contains_key("position") {
                    gone.extend(faces_of(&old_features, &c.old));
                }
            }
            "ambiguous" => {
                for id in &c.candidates {
                    for i in faces_of(&new_features, &Some(id.clone())) {
                        face_class.insert(i, "ambiguous");
                    }
                }
            }
            "new" => {
                for i in faces_of(&new_features, &c.new) {
                    face_class.insert(i, "new");
                }
            }
            "replaced" => {
                for i in faces_of(&new_features, &c.new) {
                    face_class.insert(i, "changed");
                }
                gone.extend(faces_of(&old_features, &c.old));
            }
            _ => gone.extend(faces_of(&old_features, &c.old)),
        }
    }
    gone.sort_unstable();
    gone.dedup();

    // The PMI: Specify's in the new file against the answers carried, and the files' own.
    let new_prompts: BTreeMap<String, String> = api::questions(&new_analysis, &carried)?
        .into_iter()
        .map(|q| (q.id, q.prompt))
        .collect();
    let mut stored_changes = Vec::new();
    if let Some(now) = &new_stored {
        let ids: BTreeSet<&String> = carried.keys().chain(now.keys()).collect();
        let word = |v: &Value| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        for id in ids {
            let (was, is) = (carried.get(id), now.get(id));
            let class = match (was, is) {
                (Some(a), Some(b)) if a == b => continue,
                (Some(_), Some(_)) => "changed",
                (Some(_), None) => "removed",
                _ => "added",
            };
            stored_changes.push(PmiChange {
                class: class.into(),
                on: new_prompts.get(id).cloned().unwrap_or_default(),
                old: was.map(word),
                new: is.map(word),
                id: id.clone(),
            });
        }
    }
    let list = |a: &Value| a["existing"].as_array().cloned().unwrap_or_default();
    let (old_existing, new_existing) = (list(&old_analysis), list(&new_analysis));
    // Plan scenes with the same geometry-bound history as the eventual write.
    // Keep opaque transport out of legacy answer counts and stored-answer diffs.
    let mut pmi_answers = carried.clone();
    crate::characteristics::carry_revision(old, new, &corr, &mut pmi_answers)
        .map_err(api::Error::Analysis)?;
    let (items, scenes) = pmi::diff(
        old,
        new,
        (&old_analysis, &new_analysis),
        (!given.is_empty()).then_some(given),
        &pmi_answers,
        (
            &geometric_faces,
            corr.alignment.found.then_some(&corr.alignment.rotation),
        ),
        (&old_features, &new_features),
    )?;

    let check_lines = pmi::check_lines(&check, &new_features, &scenes.new);
    let pmi_graphics = pmi_picture::graphics(&items, &scenes);
    let pmi = Pmi {
        answers_from: answers_from.into(),
        new_stored: new_stored.is_some(),
        stored: stored_changes,
        existing: compare_existing(
            &old_existing,
            &new_existing,
            &geometric_faces,
            &old_features,
            &new_features,
        ),
        existing_count: [old_existing.len(), new_existing.len()],
        items,
        scenes: options.scenes.then_some(scenes),
    };
    let open_count = unresolved.len() + questions.len();
    let verdict = if !problems.added.is_empty() {
        "problems"
    } else if open_count > 0 {
        "decide"
    } else {
        "ok"
    };
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    // What changed in the part, counted by family: `2 holes removed, 1 boss changed`.
    let mut counts: Vec<((&str, &str), usize)> = Vec::new();
    for f in &feature_changes {
        let done = match f.class.as_str() {
            "orphaned" => "removed",
            "new" => "added",
            "replaced" | "continued" => "changed",
            "adapted"
                if f.changes
                    .keys()
                    .all(|k| k == "position" || k == "adjacency") =>
            {
                "moved"
            }
            "adapted" => "changed",
            _ => continue,
        };
        match counts
            .iter_mut()
            .find(|(k, _)| *k == (f.family.as_str(), done))
        {
            Some((_, n)) => *n += 1,
            None => counts.push(((f.family.as_str(), done), 1)),
        }
    }
    // Rounds and surfaces follow the features they border: counted together, after them.
    let follows = |family: &str| ROUNDS.contains(&family);
    let mut part: Vec<String> = counts
        .iter()
        .filter(|((family, _), _)| !follows(family))
        .map(|((family, done), n)| {
            let one = singular(family);
            format!("{} {done}", plural(*n, &one, &several(&one)))
        })
        .collect();
    let rounds: usize = counts
        .iter()
        .filter(|((family, _), _)| follows(family))
        .map(|(_, n)| n)
        .sum();
    if rounds > 0 {
        part.push(if part.is_empty() {
            format!(
                "{} changed",
                plural(rounds, "round or surface", "rounds and surfaces")
            )
        } else {
            format!(
                "{} with them",
                plural(rounds, "round or surface", "rounds and surfaces")
            )
        });
    }
    let pmi_changes = pmi.items.len();
    if pmi_changes > 0 {
        part.push(format!(
            "{} to the PMI",
            plural(pmi_changes, "change", "changes")
        ));
    }
    let mut part = if part.is_empty() {
        "No feature changed".to_string()
    } else {
        part.join(", ")
    };
    part[..1].make_ascii_uppercase();
    let mut summary = match verdict {
        "ok" if carried.is_empty() => {
            format!(
                "{part}. There are no saved requirements to apply, and no new requirements to specify."
            )
        }
        "ok" => format!(
            "{part}. {} to the changed part.",
            if carried.len() == 1 {
                "The requirement still applies".to_string()
            } else {
                format!("All {} requirements still apply", carried.len())
            }
        ),
        "decide" => {
            let mut parts = Vec::new();
            if !unresolved.is_empty() {
                parts.push(format!(
                    "{} could not be applied",
                    plural(unresolved.len(), "requirement", "requirements")
                ));
            }
            if !questions.is_empty() {
                parts.push(format!(
                    "{} to specify",
                    plural(questions.len(), "new requirement", "new requirements")
                ));
            }
            format!("{part}. A decision is needed: {}.", parts.join(", "))
        }
        _ => format!(
            "{part}. Review finds {} on the new part it did not before.",
            plural(problems.added.len(), "problem", "problems")
        ),
    };
    if !check_lines.is_empty() {
        summary.push_str(&format!(
            " {} need{} checking: {}",
            plural_of(check.len(), "requirement"),
            if check.len() == 1 { "s" } else { "" },
            check_lines.join("; ")
        ));
        summary.push('.');
    }
    carried = pmi_answers;
    crate::bom::carry(old, &mut carried).map_err(api::Error::Analysis)?;
    crate::answer_file::attach(
        new,
        &mut carried,
        answers,
        unresolved.iter().map(|u| (u.id.clone(), u.reason.clone())),
    )
    .map_err(api::Error::Analysis)?;
    let alignment = corr.alignment.clone();
    let mut revised = Revised {
        pmi_graphics,
        check_lines,
        pmi,
        verdict: verdict.into(),
        summary,
        answers: carried,
        migrated,
        unresolved,
        check,
        features: feature_changes,
        measures: Measures {
            old: old_size,
            new: new_size,
        },
        questions,
        problems,
        faces: face_class,
        gone,
        correspondence: Some(corr),
        still: true,
    };
    revised.still = revised.motion().is_none() || onto_itself(&old_q.fingerprints, &alignment);
    Ok(revised)
}

/// Whether *a* takes a part onto itself: each face, moved, has a face of the same surface and
/// area where it now is.
fn onto_itself(prints: &qc::Fingerprints, a: &qc::Alignment) -> bool {
    let tol = prints.scale * 1e-3;
    let moved = |p: [f64; 3]| {
        [0, 1, 2].map(|i| {
            a.rotation[i][0] * p[0]
                + a.rotation[i][1] * p[1]
                + a.rotation[i][2] * p[2]
                + a.translation[i]
        })
    };
    let same_area = |x: Option<f64>, y: Option<f64>| match (x, y) {
        (Some(x), Some(y)) => (x - y).abs() <= 1e-3 * x.abs().max(y.abs()),
        (x, y) => x == y,
    };
    prints.faces.iter().all(|f| {
        let c = moved(f.centroid);
        prints.faces.iter().any(|g| {
            g.surface == f.surface
                && same_area(f.area, g.area)
                && (0..3)
                    .map(|k| (c[k] - g.centroid[k]).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    <= tol
        })
    })
}

impl Revised {
    /// The report's "What changed" lines, alike changes to alike features as one, each with the
    /// indices in [`Revised::features`] of the features it is about.
    pub fn change_lines(&self) -> Vec<(String, Vec<usize>)> {
        let mut out = Vec::new();
        // Alike changes to alike features read as one line: `2× Ø5 × 30 through holes removed`.
        type Group<'a> = (
            (&'a str, &'a str, String, &'a str, &'a str, &'a str),
            Vec<&'a FeatureChange>,
            Vec<usize>,
        );
        let mut groups: Vec<Group> = Vec::new();
        for (i, f) in self.features.iter().enumerate() {
            let key = (
                f.class.as_str(),
                f.what.as_str(),
                sizes(&f.changes),
                f.place.on.as_str(),
                f.place.from.as_str(),
                f.became.as_str(),
            );
            match groups.iter_mut().find(|(k, _, _)| *k == key) {
                Some((_, fs, is)) => {
                    fs.push(f);
                    is.push(i);
                }
                None => groups.push((key, vec![f], vec![i])),
            }
        }
        for ((class, what, sized, on, from, became), fs, is) in &groups {
            let what = if what.is_empty() { "feature" } else { what };
            let label = if fs.len() > 1 {
                format!("{}× {}", fs.len(), several(what))
            } else {
                what.to_string()
            };
            // Where they are: in words where the part gives them, else as coordinates.
            let offsets: Vec<&str> = fs
                .iter()
                .map(|f| f.place.offset.as_str())
                .filter(|o| !o.is_empty())
                .collect();
            let place = if on.is_empty() && from.is_empty() {
                places(&fs.iter().map(|f| f.at).collect::<Vec<_>>())
                    .trim_start()
                    .to_string()
            } else {
                let mut place = on.to_string();
                if !offsets.is_empty() && offsets.len() <= 4 {
                    if !place.is_empty() {
                        place += ", ";
                    }
                    place += &format!("at {} from {from}", offsets.join("; "));
                }
                place
            };
            let place = |sep: &str| {
                if place.is_empty() {
                    String::new()
                } else {
                    format!("{sep}{place}")
                }
            };
            let mut moves: Vec<String> = fs
                .iter()
                .filter_map(|f| moved(&f.changes, f.axis))
                .collect();
            moves.dedup();
            let line = match *class {
                "orphaned" => format!("{label} removed{}", place(": ")),
                // One in place of another: `4× Ø6 × 10 through holes → Ø6 × 7 counterbored …`.
                "replaced" => {
                    let became = if fs.len() > 1 {
                        several(became)
                    } else {
                        became.to_string()
                    };
                    format!("{label} → {became}{}", place(": "))
                }
                "new" => format!("{label} added{}", place(": ")),
                "continued" => format!(
                    "{label} now run{} round the new fillets",
                    if fs.len() == 1 { "s" } else { "" }
                ),
                "ambiguous" => {
                    // What became of alike features that don't pair one to one.
                    let mut there: Vec<&String> = fs.iter().flat_map(|f| &f.there).collect();
                    there.sort();
                    there.dedup();
                    let (was, now) = (fs.len(), there.len());
                    let are = if was > 1 { "are" } else { "is" };
                    match now {
                        0 => format!(
                            "{label}{}: no longer one to one with the new part",
                            place(", ")
                        ),
                        1 if was > 1 => format!("{label}{}, {are} now one", place(", ")),
                        _ if now < was => format!("{label}{}, {are} now {now}", place(", ")),
                        _ if now > was && was == 1 => {
                            format!("{label}{}, is now split in {now}", place(", "))
                        }
                        _ if now > was => format!("{label}{}, are now {now}", place(", ")),
                        _ => format!(
                            "{label}{}: still there, but alike, so the old and new can't be \
                             paired one to one",
                            place(", ")
                        ),
                    }
                }
                _ if sized.is_empty() && !moves.is_empty() && moves.len() <= 4 => {
                    format!("{label}{}: moved {}", place(", "), moves.join("; "))
                }
                _ if sized.is_empty() => format!("{label}{}: moved", place(", ")),
                _ if !moves.is_empty() && moves.len() <= 4 => {
                    format!(
                        "{label}{}: {sized}; moved {}",
                        place(", "),
                        moves.join("; ")
                    )
                }
                _ => format!("{label}{}: {sized}", place(", ")),
            };
            out.push((line, is.clone()));
        }
        out
    }

    /// The whole part's move between the revisions, in words, when one was found and it is
    /// more than rounding: `turned 90° about Z, moved X +15, Y −8`.
    pub fn motion(&self) -> Option<String> {
        let a = &self.correspondence.as_ref()?.alignment;
        if !a.found {
            return None;
        }
        let r = &a.rotation;
        let turn = (((r[0][0] + r[1][1] + r[2][2] - 1.0) / 2.0).clamp(-1.0, 1.0))
            .acos()
            .to_degrees();
        let mut words = Vec::new();
        if turn >= 0.5 {
            let mut axis = [r[2][1] - r[1][2], r[0][2] - r[2][0], r[1][0] - r[0][1]];
            let mut length = axis.iter().map(|v| v * v).sum::<f64>().sqrt();
            if length < 1e-6 && turn > 90.0 {
                // At π the skew part vanishes; R + I = 2 n nᵀ. Its largest
                // column gives the axis without dividing by a tiny sine of the turn.
                axis = (0..3)
                    .map(|j| std::array::from_fn(|i| r[i][j] + if i == j { 1.0 } else { 0.0 }))
                    .max_by(|a: &[f64; 3], b| {
                        let norm = |v: &[f64; 3]| v.iter().map(|x| x * x).sum::<f64>();
                        norm(a).total_cmp(&norm(b))
                    })
                    .expect("three columns");
                length = axis.iter().map(|v| v * v).sum::<f64>().sqrt();
            }
            let about = (0..3)
                .find(|&i| length > 1e-9 && (axis[i] / length).abs() > 0.99)
                .map_or_else(|| "a skew axis".to_string(), |i| AXES[i].to_string());
            words.push(format!("turned {}° about {about}", num(turn)));
        }
        let shift: Vec<String> = (0..3)
            .filter(|&i| a.translation[i].abs() >= 0.05)
            .map(|i| format!("{} {}", AXES[i], signed(a.translation[i])))
            .collect();
        if !shift.is_empty() {
            words.push(format!("moved {}", shift.join(", ")));
        }
        (!words.is_empty()).then(|| words.join(", "))
    }

    /// The report as Markdown, for a pull request comment; *image* is the picture's link.
    pub fn markdown(&self, title: &str, image: Option<&str>) -> String {
        self.markdown_headed(&format!("### {title}: Specify"), image, None)
    }

    /// The verdict's mark: ✅ ok, ⚠️ decide, ❌ problems.
    pub fn mark(&self) -> &'static str {
        mark(&self.verdict)
    }

    /// [`Revised::markdown`] under *heading* (an assembly's part's), with *link*, a line
    /// linking to the page that shows the part in 3D, after the summary.
    pub fn markdown_headed(
        &self,
        heading: &str,
        image: Option<&str>,
        link: Option<&str>,
    ) -> String {
        use std::fmt::Write;
        let mut s = format!("{heading}\n\n{} {}\n\n", self.mark(), self.summary);
        if let Some(image) = image {
            let _ = writeln!(
                s,
                "![The new part: changed in orange, new in green, can't tell in purple, gone or moved from in red]({image})\n"
            );
        }
        if let Some(link) = link {
            let _ = writeln!(s, "{link}\n");
        }
        {
            let (o, n) = (&self.measures.old, &self.measures.new);
            let change = |old: f64, new: f64, unit: &str| {
                let d = new - old;
                if d.abs() < 0.005 {
                    "unchanged".to_string()
                } else if old.abs() > 0.0 {
                    let sign = if d > 0.0 { "+" } else { "" };
                    format!(
                        "{sign}{} {unit} ({sign}{}%)",
                        amount(d),
                        num(d / old * 100.0)
                    )
                } else {
                    format!("{} {unit}", amount(d))
                }
            };
            let row =
                |s: &mut String, what: &str, old: Option<f64>, new: Option<f64>, unit: &str| {
                    let shown = |v: Option<f64>| {
                        v.map_or_else(
                            || "unknown".to_string(),
                            |v| format!("{} {unit}", amount(v)),
                        )
                    };
                    let delta = match (old, new) {
                        (Some(a), Some(b)) => change(a, b, unit),
                        _ => String::new(),
                    };
                    let _ = writeln!(s, "| {what} | {} | {} | {delta} |", shown(old), shown(new));
                };
            let extent = |z: &Size| [0, 1, 2].map(|k| z.max[k] - z.min[k]);
            let boxed = |z: &Size| extent(z).map(num).join(" × ") + " mm";
            let (eo, en) = (extent(o), extent(n));
            let axes = ["X", "Y", "Z"];
            let grown: Vec<String> = (0..3)
                .filter(|&k| (en[k] - eo[k]).abs() >= 0.005)
                .map(|k| {
                    let d = en[k] - eo[k];
                    format!("{} {}{}", axes[k], if d > 0.0 { "+" } else { "" }, num(d))
                })
                .collect();
            let shifted = (0..3).any(|k| (n.min[k] - o.min[k]).abs() >= 0.005);
            let box_change = match (grown.is_empty(), shifted) {
                (true, false) => "unchanged".to_string(),
                (true, true) => "same size, moved".to_string(),
                _ => grown.join(", "),
            };
            s += "| | Before | After | Change |\n|---|---|---|---|\n";
            row(&mut s, "Volume", o.volume, n.volume, "mm³");
            row(&mut s, "Surface area", o.area, n.area, "mm²");
            let _ = writeln!(
                s,
                "| Bounding box | {} | {} | {box_change} |",
                boxed(o),
                boxed(n)
            );
            let faces = n.faces as f64 - o.faces as f64;
            let _ = writeln!(
                s,
                "| Faces | {} | {} | {} |\n",
                o.faces,
                n.faces,
                if faces == 0.0 {
                    "unchanged".to_string()
                } else {
                    format!("{}{faces}", if faces > 0.0 { "+" } else { "" })
                }
            );
        }
        if let Some(moved) = self.motion() {
            let _ = writeln!(
                s,
                "The whole part moved between the revisions ({moved}); each feature is compared \
                 where it is now.\n"
            );
        }
        if !self.features.is_empty() {
            s += "**What changed**\n\n";
            for (line, _) in self.change_lines() {
                let _ = writeln!(s, "- {line}");
            }
            s += "\n";
        }
        {
            let p = &self.pmi;
            let mut lines = Vec::new();
            if p.answers_from == "stored" {
                lines.push(
                    "The comparison uses the requirements stored in the old file's Specify PMI."
                        .into(),
                );
            }
            // Only where the old file held them: answers kept beside the files stay there.
            if p.answers_from == "stored" && !p.new_stored && !self.answers.is_empty() {
                lines.push(
                    "The new file has no Specify PMI yet: write the applicable requirements \
                     (`answers.json`) to it with `specify-core write`."
                        .into(),
                );
            }
            for c in &p.items {
                let prefix = c.number.map_or(String::new(), |n| format!("#{n} "));
                let was = c.old.as_deref().unwrap_or_default();
                let now = c.new.as_deref().unwrap_or_default();
                let text = match c.class.as_str() {
                    "changed" => format!("{was} → {now}"),
                    "removed" if c.number.is_some() => format!("retired: {was}"),
                    "removed" => format!("removed: {was}"),
                    "renumbered" => {
                        format!("{now}: {}", c.reason.as_deref().unwrap_or("renumbered"))
                    }
                    _ => format!("added: {now}"),
                };
                let reason = c
                    .reason
                    .as_deref()
                    .filter(|r| !["retired"].contains(r) && c.class != "renumbered")
                    .map_or(String::new(), |r| format!("; {r}"));
                lines.push(format!("{prefix}{text} ({}{reason})", c.on));
            }
            if !lines.is_empty() {
                s += "**PMI**\n\n";
                for l in lines {
                    let _ = writeln!(s, "- {l}");
                }
                s += "\n";
            }
        }
        if !self.unresolved.is_empty() {
            s += "**Requirements not applied**\n\n";
            for u in &self.unresolved {
                let _ = writeln!(
                    s,
                    "- **{}** (`{}`): {}{}",
                    fmt(&u.answer),
                    u.id,
                    u.reason,
                    if u.candidates.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", u.candidates.join(", "))
                    }
                );
            }
            s += "\n";
        }
        if !self.check.is_empty() {
            s += "**Requirements to check**\n\n";
            for line in &self.check_lines {
                let _ = writeln!(s, "- {line}.");
            }
            s += "\n";
        }
        if !self.questions.is_empty() {
            s += "**Requirements to specify**\n\n";
            for q in &self.questions {
                let _ = writeln!(s, "- {} (`{}`)", q.title, q.id);
            }
            s += "\n";
        }
        if !self.problems.added.is_empty() {
            s += "**New problems**\n\n";
            for p in &self.problems.added {
                let _ = writeln!(s, "- {}", p["message"].as_str().unwrap_or_default());
            }
            s += "\n";
        }
        if !self.migrated.is_empty() {
            let _ = writeln!(
                s,
                "{} carried to renumbered faces.\n",
                if self.migrated.len() == 1 {
                    "1 requirement was".to_string()
                } else {
                    format!("{} requirements were", self.migrated.len())
                }
            );
        }
        s
    }
}

/// What a viewer needs to draw the new part as [`picture`] draws it: both parts' meshes (as
/// `specify-core mesh` writes them), each new face's class (`changed`, `new` or `ambiguous`;
/// unchanged faces left out), the old faces that are gone or moved, and `motion`, the rotation
/// rows and translation taking the old part's frame to the new one's (`null` when none was
/// found, the frames then taken as one). `changes` is the report's "What changed" lines, in its
/// order, each with its features (indices in the report's `features`) and their faces on the
/// new part and the old, for a viewer that links the lines and the faces.
pub fn view(old: &LoadedPart, new: &LoadedPart, revised: &Revised) -> Value {
    let motion = revised
        .correspondence
        .as_ref()
        .map(|c| &c.alignment)
        .filter(|a| a.found)
        .map(|a| serde_json::json!({"rotation": a.rotation, "translation": a.translation}));
    serde_json::json!({
        "new": crate::mesh::mesh(new),
        "old": crate::mesh::mesh(old),
        "faces": revised.faces.iter().map(|(f, c)| (f.to_string(), Value::from(*c))).collect::<Map<_, _>>(),
        "gone": revised.gone,
        "motion": motion,
        "changes": revised.change_lines().into_iter().map(|(line, features)| {
            let faces = |of: fn(&FeatureChange) -> &Vec<usize>| -> Vec<usize> {
                let mut faces: Vec<usize> =
                    features.iter().flat_map(|&i| of(&revised.features[i]).iter().copied()).collect();
                faces.sort_unstable();
                faces.dedup();
                faces
            };
            serde_json::json!({
                "line": line,
                "features": features,
                "new": faces(|f| &f.new_faces),
                "old": faces(|f| &f.old_faces),
            })
        }).collect::<Vec<_>>(),
    })
}

/// The new part's picture: an isometric view, its faces coloured by what changed (changed
/// orange, new green, ambiguous purple, the rest grey) with the old part's gone or moved faces
/// over it in translucent red, as a PNG *width* pixels wide.
pub fn picture(old: &LoadedPart, new: &LoadedPart, revised: &Revised, width: u32) -> Vec<u8> {
    let colour = |class: Option<&&str>| -> [f64; 3] {
        match class.copied() {
            Some("changed") => [0.88, 0.48, 0.0],
            Some("new") => [0.04, 0.56, 0.24],
            Some("ambiguous") => [0.48, 0.25, 0.75],
            _ => [0.80, 0.81, 0.83],
        }
    };
    let mesh_new = crate::mesh::mesh(new);
    let mesh_old = crate::mesh::mesh(old);
    let solid = triangles(&mesh_new, &|f| Some(colour(revised.faces.get(&f))), &|p| p);
    let gone: BTreeSet<usize> = revised.gone.iter().copied().collect();
    let motion = revised
        .correspondence
        .as_ref()
        .map(|c| &c.alignment)
        .filter(|a| a.found);
    let ghost = triangles(
        &mesh_old,
        &|f| gone.contains(&f).then_some([0.84, 0.16, 0.16]),
        &|p| match motion {
            Some(a) => [0, 1, 2].map(|i| {
                a.rotation[i][0] * p[0]
                    + a.rotation[i][1] * p[1]
                    + a.rotation[i][2] * p[2]
                    + a.translation[i]
            }),
            None => p,
        },
    );
    let edges = edges(&mesh_new, &|p| p);
    let graphics: Vec<_> = revised
        .pmi_graphics
        .iter()
        .map(|g| {
            if g.old {
                g.placed(&|p| match motion {
                    Some(a) => [0, 1, 2].map(|i| {
                        a.rotation[i][0] * p[0]
                            + a.rotation[i][1] * p[1]
                            + a.rotation[i][2] * p[2]
                            + a.translation[i]
                    }),
                    None => p,
                })
            } else {
                g.clone()
            }
        })
        .collect();
    render_with_pmi(&solid, &ghost, &edges, &graphics, false, width)
}

/// A triangle and its colour, for [`render`].
type Triangle = ([[f64; 3]; 3], [f64; 3]);

/// A mesh's triangles (as [`crate::mesh::mesh`] writes it), each vertex put where *place*
/// says, of the faces *keep* gives a colour.
fn triangles(
    mesh: &Value,
    keep: &dyn Fn(usize) -> Option<[f64; 3]>,
    place: &dyn Fn([f64; 3]) -> [f64; 3],
) -> Vec<Triangle> {
    let pos: Vec<f64> = mesh["positions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect();
    let idx: Vec<usize> = mesh["indices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|i| i as usize)
        .collect();
    let vertex = |v: usize| place([pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]]);
    let mut out = Vec::new();
    for (face, range) in mesh["faces"].as_array().into_iter().flatten().enumerate() {
        let (Some(start), Some(count)) = (range[0].as_u64(), range[1].as_u64()) else {
            continue;
        };
        let Some(c) = keep(face) else { continue };
        for t in start as usize..(start + count) as usize {
            out.push((
                [
                    vertex(idx[t * 3]),
                    vertex(idx[t * 3 + 1]),
                    vertex(idx[t * 3 + 2]),
                ],
                c,
            ));
        }
    }
    out
}

/// A mesh's edges as polylines, each point put where *place* says.
fn edges(mesh: &Value, place: &dyn Fn([f64; 3]) -> [f64; 3]) -> Vec<Vec<[f64; 3]>> {
    mesh["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            let p: Vec<f64> = e
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_f64)
                .collect();
            p.chunks(3)
                .filter(|c| c.len() == 3)
                .map(|c| place([c[0], c[1], c[2]]))
                .collect()
        })
        .collect()
}

/// A z-buffered isometric rendering (looking along −(1, −1, 1), z up), 2× supersampled. The
/// ghost is drawn over everything, or, *in_front*, only where it is in front of the solid (not
/// where a solid face lies on it).
fn render_with_pmi(
    solid: &[Triangle],
    ghost: &[Triangle],
    edges: &[Vec<[f64; 3]>],
    graphics: &[pmi_picture::Graphic],
    in_front: bool,
    width: u32,
) -> Vec<u8> {
    let norm = |v: [f64; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|x| x / n)
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let toward = norm([1.0, -1.0, 1.0]);
    let right = norm(cross([0.0, 0.0, 1.0], toward));
    let up = cross(toward, right);
    let light = norm([0.4, -0.8, 1.0]);
    let all = solid
        .iter()
        .chain(ghost)
        .flat_map(|(t, _)| t.iter().copied())
        .chain(graphics.iter().flat_map(|g| g.points().copied()));
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in all {
        let q = [dot(p, right), dot(p, up)];
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let w = width as usize * 2;
    // As wide as asked, and no taller than twice that: a long thin part standing up is drawn
    // smaller rather than in a towering buffer.
    let scale = ((w as f64 * 0.9) / (hi[0] - lo[0]).max(1e-9))
        .min((w as f64 * 2.0 * 0.9) / (hi[1] - lo[1]).max(1e-9));
    let h = (((hi[1] - lo[1]) * scale) / 0.9).ceil().max(1.0) as usize + 2;
    let (ox, oy) = (
        (w as f64 - (hi[0] - lo[0]) * scale) / 2.0 - lo[0] * scale,
        h as f64 * 0.05 + hi[1] * scale,
    );
    let screen = |p: [f64; 3]| -> [f64; 3] {
        [
            ox + dot(p, right) * scale,
            oy - dot(p, up) * scale,
            dot(p, toward),
        ]
    };
    let mut rgb = vec![[1.0f64; 3]; w * h];
    let mut depth = vec![f64::NEG_INFINITY; w * h];
    let fill = |tri: &[[f64; 3]; 3],
                c: [f64; 3],
                alpha: f64,
                test: bool,
                behind: Option<f64>,
                shaded: bool,
                rgb: &mut Vec<[f64; 3]>,
                depth: &mut Vec<f64>| {
        let n = norm(cross(
            [
                tri[1][0] - tri[0][0],
                tri[1][1] - tri[0][1],
                tri[1][2] - tri[0][2],
            ],
            [
                tri[2][0] - tri[0][0],
                tri[2][1] - tri[0][1],
                tri[2][2] - tri[0][2],
            ],
        ));
        let n = if dot(n, toward) < 0.0 {
            n.map(|x| -x)
        } else {
            n
        };
        let shade = 0.45 + 0.55 * dot(n, light).max(0.0);
        let c = if shaded {
            c.map(|x| (x * shade).min(1.0))
        } else {
            c
        };
        let s = tri.map(screen);
        let (x0, x1) = (
            s.iter()
                .map(|p| p[0])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.0) as usize,
            (s.iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil() as usize)
                .min(w - 1),
        );
        let (y0, y1) = (
            s.iter()
                .map(|p| p[1])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.0) as usize,
            (s.iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil() as usize)
                .min(h - 1),
        );
        let area =
            (s[1][0] - s[0][0]) * (s[2][1] - s[0][1]) - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
        if area.abs() < 1e-12 {
            return;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let b0 = ((s[1][0] - px) * (s[2][1] - py) - (s[2][0] - px) * (s[1][1] - py)) / area;
                let b1 = ((s[2][0] - px) * (s[0][1] - py) - (s[0][0] - px) * (s[2][1] - py)) / area;
                let b2 = 1.0 - b0 - b1;
                if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                    continue;
                }
                let z = b0 * s[0][2] + b1 * s[1][2] + b2 * s[2][2];
                let i = y * w + x;
                if behind.is_some_and(|allowance| z <= depth[i] + allowance) {
                    continue;
                }
                if test {
                    if z <= depth[i] {
                        continue;
                    }
                    depth[i] = z;
                }
                rgb[i] = [0, 1, 2].map(|k| rgb[i][k] * (1.0 - alpha) + c[k] * alpha);
            }
        }
    };
    for (t, c) in solid {
        fill(t, *c, 1.0, true, None, true, &mut rgb, &mut depth);
    }
    // The edges, where they are not behind a face (a little depth allowance for the face they
    // bound).
    let allowance = (hi[0] - lo[0]).max(hi[1] - lo[1]) * 0.004;
    for e in edges {
        for seg in e.windows(2) {
            let (a, b) = (screen(seg[0]), screen(seg[1]));
            let steps = ((b[0] - a[0]).hypot(b[1] - a[1]).ceil() as usize).max(1);
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                let p = [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (x, y) = (p[0] as usize + dx, p[1] as usize + dy);
                    if x < w && y < h && p[2] + allowance >= depth[y * w + x] {
                        rgb[y * w + x] = [0.2, 0.2, 0.2];
                    }
                }
            }
        }
    }
    for (t, c) in ghost {
        fill(
            t,
            *c,
            0.45,
            false,
            in_front.then_some(allowance),
            true,
            &mut rgb,
            &mut depth,
        );
    }
    // PMI is an overlay, including leaders hidden by the solid. No layout or glyph
    // synthesis: these are exactly the compared presentation's strokes and fills.
    for graphic in graphics {
        for triangle in &graphic.triangles {
            fill(
                triangle,
                graphic.colour,
                graphic.alpha,
                false,
                None,
                false,
                &mut rgb,
                &mut depth,
            );
        }
        // Blend each covered pixel once, including shared polyline endpoints.
        let mut covered = BTreeSet::new();
        for line in &graphic.polylines {
            for segment in line.windows(2) {
                let (a, b) = (screen(segment[0]), screen(segment[1]));
                let steps = ((b[0] - a[0]).hypot(b[1] - a[1]).ceil() as usize).max(1);
                for k in 0..=steps {
                    let t = k as f64 / steps as f64;
                    let x = (a[0] + (b[0] - a[0]) * t).floor() as isize;
                    let y = (a[1] + (b[1] - a[1]) * t).floor() as isize;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (x, y) = (x + dx, y + dy);
                        if x >= 0 && y >= 0 && x < w as isize && y < h as isize {
                            covered.insert(y as usize * w + x as usize);
                        }
                    }
                }
            }
        }
        for i in covered {
            rgb[i] = [0, 1, 2]
                .map(|k| rgb[i][k] * (1.0 - graphic.alpha) + graphic.colour[k] * graphic.alpha);
        }
    }
    // Down-sample 2× and encode.
    let (ow, oh) = (w / 2, h / 2);
    let mut bytes = Vec::with_capacity(ow * oh * 3);
    for y in 0..oh {
        for x in 0..ow {
            let quad =
                [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| rgb[(2 * y + dy) * w + 2 * x + dx]);
            for k in 0..3 {
                let mean = quad.iter().map(|p| p[k]).sum::<f64>() / 4.0;
                bytes.push((mean * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, ow as u32, oh as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        // Invariant: writing to a Vec cannot fail, and the buffer is exactly ow × oh × 3.
        let mut writer = encoder.write_header().expect("PNG header");
        writer.write_image_data(&bytes).expect("PNG data");
    }
    out
}

/// A verdict's mark: ✅ ok, ⚠️ decide, ❌ problems.
fn mark(verdict: &str) -> &'static str {
    match verdict {
        "ok" => "✅",
        "decide" => "⚠️",
        _ => "❌",
    }
}

#[cfg(test)]
mod tests;
