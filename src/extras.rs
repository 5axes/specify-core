//! Requirements a person adds where no question asked: a flatness, a perpendicularity or
//! parallelism to a datum, a fit, a finish, or a dimension they call out, on faces they pick
//! (port of `extras.py`).
//!
//! Each is an answer keyed `extra.<id>`:
//!
//! ```json
//! {"faces": [12], "kind": "flatness", "value": "0.05"}
//! {"faces": [3, 4], "kind": "perpendicularity", "value": "0.1", "datums": ["A"]}
//! {"faces": [7], "kind": "fit", "value": "H7"}
//! {"faces": [7], "kind": "finish", "value": "Ra 0.8"}
//! {"faces": [7], "kind": "size", "value": "±0.05"}
//! {"faces": [9], "kind": "distance", "reference": [2], "value": "+0.1/-0"}
//! ```
//!
//! A called-out `size` is a diameter, toleranced ±, by limits or by a fit; a `distance` runs
//! from `reference` (flat, coplanar faces) to `faces`: a parallel flat level, or a hole's
//! axis. Either takes the place of what the general tolerance would have said of the same
//! faces. A malformed one is refused when given. One citing a datum that has since been
//! removed is not refused (the datum may come back) but is not written, and is reported as a
//! problem.

// Coordinate indexing uses [f64; 3] (or a bounded 0..3 loop), never variable-length JSON arrays.
// Capture indexing below follows successful matches with mandatory groups; optional groups use get().
use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::choices;
use crate::pyfmt::{self, g};
use crate::rules::{
    Datums, Error, Face, Faces, Result, float_of, int_of, invalid, iter_of, object, repr, round,
    str_of, truthy,
};
use crate::standards;

pub const PREFIX: &str = "extra.";
pub const KINDS: [&str; 10] = [
    "flatness",
    "perpendicularity",
    "parallelism",
    "fit",
    "finish",
    "profile",
    "runout",
    "total_runout",
    "size",
    "distance",
];
/// Measured from a datum, necessarily.
pub const ORIENTED: [&str; 4] = ["perpendicularity", "parallelism", "runout", "total_runout"];
/// Those measured from datums or that may be (a profile with none controls form alone, with
/// datums its place too).
pub const DATUMED: [&str; 5] = [
    "perpendicularity",
    "parallelism",
    "runout",
    "total_runout",
    "profile",
];

/// Python's `\s` for the regex crate (as `choices` spells it).
const S: &str = r"[\s\x1C-\x1F]";
const NUMBER: &str = r"\d*\.?\d+";

/// `_FIT`: a hole fit (capitals: H7, JS7) or a shaft fit (g6, js6).
static FIT: LazyLock<Regex> =
    // Pattern and interpolated character classes are source constants, never user text.
    LazyLock::new(|| Regex::new(r"^(JS|js|H|[fghkmnp])(\d{1,2})$").expect("pattern"));
/// `_PLUS_MINUS`: `±0.05`, `+-0.05`, `+/-0.05` or `0.05`.
static PLUS_MINUS: LazyLock<Regex> =
    // Pattern and interpolated character classes are source constants, never user text.
    LazyLock::new(|| {
        Regex::new(&format!(r"^(?:±|\+-|\+/-)?{S}*({NUMBER})$")).expect("pattern")
    });
/// `_LIMITS` with `/` between the deviations. Python's pattern is one, with a lookahead the
/// regex crate lacks: `(a)(?:/|(?=[+-]))(b)`; split in two it matches the same texts with
/// the same groups (a deviation holds no `/` or sign but its first, so the split is unique).
static LIMITS_SLASH: LazyLock<Regex> =
    // Pattern and interpolated character classes are source constants, never user text.
    LazyLock::new(|| {
        Regex::new(&format!(r"^([+-]?{NUMBER})/([+-]?{NUMBER})$")).expect("pattern")
    });
/// `_LIMITS` with the second deviation's sign between them.
static LIMITS_SIGN: LazyLock<Regex> =
    // Pattern and interpolated character classes are source constants, never user text.
    LazyLock::new(|| {
        Regex::new(&format!(r"^([+-]?{NUMBER})([+-]{NUMBER})$")).expect("pattern")
    });
/// `_finish`'s roughness: `Ra 0.8`, `ra0.8`, `0.8`, `0.8 µm`.
static FINISH: LazyLock<Regex> = LazyLock::new(|| {
    // Pattern and interpolated character classes are source constants, never user text.
    Regex::new(&format!(r"^(?:ra{S}*)?({NUMBER}){S}*(?:µm|um)?$")).expect("pattern")
});

/// The added requirements among `answers`, in the order given.
pub fn given(answers: &Map<String, Value>) -> Vec<(&String, &Map<String, Value>)> {
    answers
        .iter()
        .filter(|(k, _)| k.starts_with(PREFIX))
        .filter_map(|(k, v)| v.as_object().map(|m| (k, m)))
        .collect()
}

/// Whether `extra`'s kind is one of `kinds`.
fn kind_in(extra: &Map<String, Value>, kinds: &[&str]) -> bool {
    extra
        .get("kind")
        .and_then(Value::as_str)
        .is_some_and(|k| kinds.contains(&k))
}

/// `extra.get("datums") or ()`, iterated as Python iterates it; None where Python raises
/// (a number is not a list of letters).
fn datums_of(extra: &Map<String, Value>) -> Option<Vec<Value>> {
    match extra.get("datums") {
        Some(v) if truthy(v) => iter_of(v),
        _ => Some(Vec::new()),
    }
}

/// The datum letters added requirements are measured from (`str()` of each, as a set).
pub fn cited(answers: &Map<String, Value>) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for (key, extra) in given(answers) {
        if !kind_in(extra, &DATUMED) {
            continue;
        }
        let letters = datums_of(extra).ok_or_else(|| letters_refused(key))?;
        out.extend(letters.iter().map(str_of));
    }
    Ok(out)
}

fn letters_refused(key: &str) -> Error {
    Error::Invalid(format!("{key}: datums are letters A to H"))
}

/// The faces `ids` name, or None where Python's `[faces[int(i)] for i in ids]` raises (not a
/// list, not a face number, no such face).
fn pick<'f>(ids: &Value, faces: &'f Faces) -> Option<Vec<&'f Face>> {
    iter_of(ids)?
        .iter()
        .map(|i| faces.get(int_of(i).ok()?))
        .collect()
}

/// `str(extra.get("value", ""))`.
fn value_of(extra: &Map<String, Value>) -> String {
    extra.get("value").map_or_else(String::new, str_of)
}

/// Refuse an added requirement that cannot be read or does not suit its faces.
pub fn check(key: &str, extra: &Map<String, Value>, faces: &Faces) -> Result<()> {
    let chosen = match extra.get("faces") {
        None => Vec::new(),
        Some(ids) => match pick(ids, faces) {
            Some(chosen) => chosen,
            None => return invalid(format!("{key}: a face picked does not exist")),
        },
    };
    if chosen.is_empty() {
        return invalid(format!("{key}: pick the faces it applies to"));
    }
    let kind = extra.get("kind");
    let Some(kind) = kind.and_then(Value::as_str).filter(|k| KINDS.contains(k)) else {
        return invalid(format!(
            "{key}: {} is not one of {}",
            kind.map_or_else(|| "None".to_string(), repr),
            KINDS.join(", ")
        ));
    };
    let planes = chosen.iter().all(|f| f.is_plane());
    let cylinders = chosen.iter().all(|f| f.is_cylinder());
    let value = value_of(extra);
    if kind == "flatness" && !planes {
        return invalid(format!("{key}: flatness applies to flat faces"));
    }
    if ORIENTED.contains(&kind) && !(planes || cylinders) {
        return invalid(format!(
            "{key}: {kind} applies to flat faces, or to one hole or boss"
        ));
    }
    if DATUMED.contains(&kind) {
        let letters = datums_of(extra).ok_or_else(|| letters_refused(key))?;
        if ORIENTED.contains(&kind) && letters.is_empty() {
            return invalid(format!("{key}: say which datum it is measured from"));
        }
        let letter = |x: &Value| {
            x.as_str().is_some_and(|s| {
                let mut c = s.chars();
                matches!((c.next(), c.next()), (Some('A'..='H'), None))
            })
        };
        if !letters.iter().all(letter) {
            return Err(letters_refused(key));
        }
    }
    if kind == "finish" {
        finish(key, &value)?;
        return Ok(());
    }
    if kind == "fit" || kind == "size" {
        let mut radii: Vec<f64> = Vec::new();
        if cylinders {
            for f in &chosen {
                let r = round(f.radius()?, 6);
                if !radii.contains(&r) {
                    radii.push(r);
                }
            }
        }
        if !cylinders || radii.len() != 1 {
            return invalid(format!(
                "{key}: a {kind} applies to the cylinder(s) of one diameter"
            ));
        }
        // check() refused empty faces and requires exactly one cylinder radius above.
        let d = 2.0 * chosen[0].radius()?;
        if kind == "fit" {
            fit(key, &value, d)?;
        } else {
            limits(key, &value, d, true)?;
        }
    } else if kind == "distance" {
        let d = distance(key, extra, &chosen, faces)?;
        limits(key, &value, d, false)?;
    } else {
        zone(key, &value)?;
    }
    Ok(())
}

/// The requirement an added one makes (it has passed [`check`]), or None while a datum it
/// cites is missing.
pub fn requirement(
    key: &str,
    extra: &Map<String, Value>,
    faces: &Faces,
    datums: &Datums,
) -> Result<Option<Map<String, Value>>> {
    let ids = extra.get("faces").unwrap_or(&Value::Null);
    let Some(chosen) = pick(ids, faces) else {
        return invalid(format!("{key}: a face picked does not exist"));
    };
    let Some(first) = chosen.first() else {
        return invalid(format!("{key}: pick the faces it applies to"));
    };
    let face_ids: Vec<i64> = chosen.iter().map(|f| f.id).collect();
    let base = |kind: &Value| {
        object([
            ("kind", kind.clone()),
            ("feature", Value::from("added")),
            ("faces", Value::from(face_ids.clone())),
            ("question", Value::from(key)),
        ])
    };
    let kind = extra.get("kind").cloned().unwrap_or(Value::Null);
    let value = value_of(extra);
    let sized = |fit: String, upper: f64, lower: f64, d: f64| {
        let mut req = base(&Value::from("size"));
        req.insert("nominal".into(), Value::from(d));
        req.insert("fit".into(), Value::from(fit));
        req.insert("upper".into(), Value::from(upper));
        req.insert("lower".into(), Value::from(lower));
        req
    };
    match kind.as_str().unwrap_or("") {
        "finish" => {
            let mut req = base(&kind);
            req.insert("value".into(), Value::from(finish(key, &value)?));
            Ok(Some(req))
        }
        "fit" => {
            let d = 2.0 * first.radius()?;
            let fit = fit(key, &value, d)?;
            let (upper, lower) = standards::fit_limits(&fit, d).map_err(Error::Invalid)?;
            Ok(Some(sized(fit, upper, lower, d)))
        }
        "size" => {
            let d = 2.0 * first.radius()?;
            let tolerance = limits(key, &value, d, true)?;
            let mut req = sized(tolerance.fit.clone(), tolerance.upper, tolerance.lower, d);
            tolerance.keep_limits(&mut req);
            Ok(Some(req))
        }
        "distance" => {
            let distance = distance(key, extra, &chosen, faces)?;
            let tolerance = limits(key, &value, distance, false)?;
            let reference: Vec<i64> = match extra.get("reference").and_then(iter_of) {
                Some(items) => items
                    .iter()
                    .map(|i| int_of(i).map_err(|e| Error::Invalid(e.message().to_string())))
                    .collect::<Result<_>>()?,
                None => return invalid(format!("{key}: measure a distance from flat faces")),
            };
            let mut req = base(&Value::from("location"));
            req.insert("reference".into(), Value::from(reference));
            req.insert("distance".into(), Value::from(round(distance, 6)));
            req.insert("upper".into(), Value::from(tolerance.upper));
            req.insert("lower".into(), Value::from(tolerance.lower));
            tolerance.keep_limits(&mut req);
            Ok(Some(req))
        }
        name => {
            let tolerance = float_of(&Value::from(zone(key, &value)?)).map_err(Error::Invalid)?;
            let mut req = base(&kind);
            req.insert("tolerance".into(), Value::from(tolerance));
            if DATUMED.contains(&name) {
                let letters = datums_of(extra).ok_or_else(|| letters_refused(key))?;
                let cited_present = letters.iter().all(|x| {
                    x.as_str()
                        .and_then(one_char)
                        .is_some_and(|c| datums.contains(c))
                });
                if !cited_present {
                    return Ok(None);
                }
                req.insert("datums".into(), Value::Array(letters));
                // A perpendicular or parallel axis has a cylindrical zone; runout and profile
                // zones are not diametral.
                let diametral =
                    matches!(name, "perpendicularity" | "parallelism") && first.is_cylinder();
                req.insert("diametral".into(), Value::from(diametral));
            }
            Ok(Some(req))
        }
    }
}

fn one_char(s: &str) -> Option<char> {
    let mut c = s.chars();
    let first = c.next()?;
    c.next().is_none().then_some(first)
}

/// `_fit`: a fit typed for a diameter `d`, tabulated there.
fn fit(key: &str, value: &str, d: f64) -> Result<String> {
    let Some(caps) = FIT.captures(pyfmt::strip(value)) else {
        return invalid(format!(
            "{key}: {} is not a fit, e.g. H7 for a hole or g6 for a shaft",
            pyfmt::repr_str(value)
        ));
    };
    // FIT has two mandatory capture groups after a successful match.
    choices::fit(&caps[1], &caps[2], d).map_err(|e| Error::Invalid(format!("{key}: {e}")))
}

struct DimensionTolerance {
    fit: String,
    upper: f64,
    lower: f64,
    limits: Option<[f64; 2]>,
}

impl DimensionTolerance {
    fn keep_limits(&self, req: &mut Map<String, Value>) {
        if let Some(bounds) = self.limits {
            req.insert("limits".into(), Value::from(bounds));
        }
    }
}

/// Fit, deviations and optional absolute limits from `±0.05` (or `0.05`),
/// `+0.1/-0`, or, where `fits`, `H7` on a diameter of `nominal`.
/// The fit is empty unless one is given.
/// Unsigned ascending `5.98/6.02` states absolute bounds. `limits:-6.02/-5.98`
/// permits signed bounds without changing the existing signed-deviation grammar.
fn limits(key: &str, value: &str, nominal: f64, fits: bool) -> Result<DimensionTolerance> {
    let text = pyfmt::strip(value).replace(' ', "").replace('−', "-");
    if fits && FIT.is_match(&text) {
        let fit = fit(key, &text, nominal)?;
        let (upper, lower) = standards::fit_limits(&fit, nominal).map_err(Error::Invalid)?;
        return Ok(DimensionTolerance {
            fit,
            upper,
            lower,
            limits: None,
        });
    }
    let number = |s: &str| pyfmt::parse_float(s).unwrap_or(f64::NAN);
    let explicit = text.strip_prefix("limits:");
    if let Some(caps) = LIMITS_SLASH
        .captures(explicit.unwrap_or(&text))
        .filter(|caps| {
            explicit.is_some()
                || [&caps[1], &caps[2]]
                    .iter()
                    .all(|part| !part.starts_with(['+', '-']))
        })
    {
        let (lower, upper) = (number(&caps[1]), number(&caps[2]));
        if !lower.is_finite() || !upper.is_finite() || lower >= upper {
            return invalid(format!(
                "{key}: absolute limits must be finite and increasing"
            ));
        }
        let (u, l) = (upper - nominal, lower - nominal);
        if !u.is_finite() || !l.is_finite() {
            return invalid(format!("{key}: limit deviations must be finite"));
        }
        return Ok(DimensionTolerance {
            fit: String::new(),
            upper: u,
            lower: l,
            limits: Some([lower, upper]),
        });
    }
    let (upper, lower) = if let Some(caps) = PLUS_MINUS.captures(&text) {
        // PLUS_MINUS has a mandatory numeric capture group 1.
        let upper = number(&caps[1]);
        (upper, -upper)
    } else if let Some(caps) = LIMITS_SLASH
        .captures(&text)
        .or_else(|| LIMITS_SIGN.captures(&text))
        .filter(|caps| {
            // Both LIMITS variants have mandatory numeric capture groups 1 and 2.
            [&caps[1], &caps[2]]
                .iter()
                .all(|part| part.starts_with(['+', '-']) || number(part) == 0.0)
        })
    {
        // Both LIMITS variants have mandatory numeric capture groups 1 and 2.
        let (a, b) = (number(&caps[1]), number(&caps[2]));
        // sorted(reverse=True): the larger first; equal values keep their order.
        if b > a { (b, a) } else { (a, b) }
    } else {
        let fit = if fits { " or a fit, e.g. H7" } else { "" };
        return invalid(format!(
            "{key}: give ±0.05, or deviations from {} such as +0.1/-0{fit}",
            g(nominal)
        ));
    };
    if upper == lower {
        return invalid(format!("{key}: the limits must differ"));
    }
    // A deviation as big as a quarter of the dimension is a limit typed as one.
    if upper.abs().max(lower.abs()) > 10.0f64.min(nominal / 4.0) {
        return invalid(format!(
            "{key}: {} is too big a deviation for {}; give deviations from it, e.g. +0.1/-0, \
             not the limits themselves",
            pyfmt::strip(value),
            g(nominal)
        ));
    }
    Ok(DimensionTolerance {
        fit: String::new(),
        upper: upper + 0.0,
        lower: lower + 0.0,
        limits: None,
    }) // no -0.0
}

/// `_distance`: from `extra`'s reference (coplanar flat faces) to its faces: a parallel level,
/// or one hole's axis.
fn distance(key: &str, extra: &Map<String, Value>, chosen: &[&Face], faces: &Faces) -> Result<f64> {
    let Some(given @ Value::Array(_)) = extra.get("reference") else {
        return invalid(format!("{key}: measure a distance from flat faces"));
    };
    let Some(reference) = pick(given, faces) else {
        return invalid(format!("{key}: a reference face does not exist"));
    };
    if reference.is_empty() || !reference.iter().all(|f| f.is_plane()) {
        return invalid(format!("{key}: measure a distance from flat faces"));
    }
    if reference
        .iter()
        .any(|r| chosen.iter().any(|c| c.id == r.id))
    {
        return invalid(format!("{key}: a face cannot be measured from itself"));
    }
    // The empty-reference check returned above.
    let n = reference[0].direction()?;
    for f in &reference {
        if !parallel(&f.direction()?, &n) {
            return invalid(format!("{key}: the faces measured from must be parallel"));
        }
    }
    // The empty-reference check returned above.
    let base = dot(&reference[0].centroid()?, &n);
    for f in &reference {
        if (dot(&f.centroid()?, &n) - base).abs() > 1e-4 {
            return invalid(format!(
                "{key}: the faces measured from must be at one level"
            ));
        }
    }
    let distance = if chosen.iter().all(|f| f.is_plane()) {
        for f in chosen {
            if !parallel(&f.direction()?, &n) {
                return invalid(format!(
                    "{key}: a distance runs to faces parallel to its reference"
                ));
            }
        }
        let level = dot(&chosen[0].centroid()?, &n);
        for f in chosen {
            if (dot(&f.centroid()?, &n) - level).abs() > 1e-4 {
                return invalid(format!("{key}: the faces measured to must be at one level"));
            }
        }
        (level - base).abs()
    } else if chosen.iter().all(|f| f.is_cylinder()) {
        let mut feet: Vec<[f64; 3]> = Vec::new();
        for f in chosen {
            let foot = foot(f)?.map(|x| round(x, 3));
            if !feet.contains(&foot) {
                feet.push(foot);
            }
        }
        if feet.len() != 1 {
            return invalid(format!("{key}: a distance runs to one hole's axis"));
        }
        if dot(&chosen[0].direction()?, &n).abs() > 1e-6 {
            return invalid(format!(
                "{key}: a hole's axis must run along the faces measured from"
            ));
        }
        (dot(&chosen[0].origin()?, &n) - base).abs()
    } else {
        return invalid(format!(
            "{key}: a distance runs to flat faces or to one hole"
        ));
    };
    if distance < 1e-4 {
        return invalid(format!("{key}: those faces are at the same level"));
    }
    Ok(distance)
}

/// `_foot`: the point of a cylinder's axis nearest the origin: faces of one hole share it.
fn foot(cylinder: &Face) -> Result<[f64; 3]> {
    let (o, a) = (cylinder.origin()?, cylinder.direction()?);
    let t = dot(&o, &a);
    Ok([o[0] - t * a[0], o[1] - t * a[1], o[2] - t * a[2]])
}

fn parallel(a: &[f64; 3], b: &[f64; 3]) -> bool {
    (dot(a, b).abs() - 1.0).abs() < 1e-6
}

fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    0.0 + a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `_finish`: `Ra 0.8`, from `Ra 0.8`, `ra0.8` or `0.8`: roughness in micrometres.
fn finish(key: &str, value: &str) -> Result<String> {
    let lower = pyfmt::strip(value).to_lowercase();
    let ra = FINISH
        .captures(&lower)
        .map(|caps| pyfmt::parse_float(&caps[1]).unwrap_or(f64::NAN))
        .filter(|&ra| 0.0 < ra && ra <= 50.0);
    match ra {
        Some(ra) => Ok(format!("Ra {}", g(ra))),
        None => invalid(format!(
            "{key}: give a roughness as Ra in micrometres, e.g. Ra 0.8"
        )),
    }
}

/// `_zone`: a tolerance zone in mm, as `choices` reads it; none is refused here.
fn zone(key: &str, value: &str) -> Result<String> {
    let zone = choices::zone(value).map_err(|e| Error::Invalid(format!("{key}: {e}")))?;
    let finite = pyfmt::parse_float(&zone).is_some_and(f64::is_finite);
    if zone == choices::NONE || !finite {
        return invalid(format!("{key}: give a tolerance in mm, e.g. 0.05"));
    }
    Ok(zone)
}
