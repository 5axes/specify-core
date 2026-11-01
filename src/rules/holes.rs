//! Holes and hole patterns: what each is for, where it is, and the requirements an answer
//! makes of it.

use serde_json::{Map, Value};

use super::datums::datum_of;
use super::mates::suggestion;
use super::py::{self, fmt, object, sentence};
use super::records::{Ctx, Datums, Faces, Feature};
use super::{Characteristic, Error, GENERAL, NONE, Question, Result, invalid};
use crate::choices;
use crate::locations;
use crate::standards::{self, CLEARANCE, HOLE_FITS, PITCH, TAP_DRILL};

/// What a hole question is asked about: one hole, a pattern (its members less any chosen as a
/// datum), or several identical ones merged (`groups`, each keeping its own `parts`).
#[derive(Debug, Clone)]
pub(crate) struct Target<'a> {
    pub id: &'a str,
    pub family: &'a str,
    pub faces: Vec<i64>,
    pub members: Vec<&'a str>,
    /// How many identical holes or patterns were merged into this one; 0 when not merged.
    pub groups: usize,
    /// When merged: each one's id and bore faces.
    pub parts: Vec<(String, Vec<i64>)>,
}

impl<'a> Target<'a> {
    /// A pattern as a target: its id and family, its faces and members still to be given.
    pub fn of_id(feature: &Feature<'a>) -> Target<'a> {
        Target {
            id: feature.id,
            family: feature.family,
            faces: Vec::new(),
            members: Vec::new(),
            groups: 0,
            parts: Vec::new(),
        }
    }

    /// A feature as its own target.
    pub fn of(feature: &Feature<'a>) -> Result<Target<'a>> {
        Ok(Target {
            id: feature.id,
            family: feature.family,
            faces: feature.faces()?.to_vec(),
            members: feature.members.clone(),
            groups: 0,
            parts: Vec::new(),
        })
    }
}

/// `_bores`: the cylinder faces among `ids` at `diameter`: a hole's bore, not its drill point
/// or counterbore.
pub(crate) fn bores(ids: &[i64], diameter: f64, faces: &Faces) -> Result<Vec<i64>> {
    let mut out = Vec::new();
    for &i in ids {
        let face = faces.at(i)?;
        // face.get("radius", 0): a cylinder with no radius is no bore.
        if face.is_cylinder() && (2.0 * face.radius.unwrap_or(0.0) - diameter).abs() < 1e-6 {
            out.push(i);
        }
    }
    Ok(out)
}

/// `_bores_of`: a hole feature's bore faces.
pub(crate) fn bores_of(hole: &Feature<'_>, faces: &Faces) -> Result<Vec<i64>> {
    bores(hole.faces()?, hole.number("diameter")?, faces)
}

/// One part of `_hole_key`: a hole's record rounded, so equal holes compare equal.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HoleKey<'a> {
    /// A hole chosen as a datum: never merged.
    Datum(&'a str),
    Hole {
        diameter: Option<f64>,
        depth: Option<f64>,
        bottom: Option<&'a Value>,
        cbore: bool,
        spotface: bool,
        csink: bool,
        outer_steps: Vec<[Value; 2]>,
        axis: Vec<f64>,
        count: usize,
        family: &'a str,
        frames: Vec<String>,
    },
}

/// `_hole_key`: when two hole targets are the same question: holes alike in size, depth,
/// bottom, entry and direction, as many of them, located the same way. A hole chosen as a datum
/// is never merged: it is specified as that datum.
pub(crate) fn hole_key<'a>(
    target: &Target<'a>,
    hole: &Feature<'a>,
    frames: &[String],
    faces: &Faces,
    datums: &Datums,
) -> Result<HoleKey<'a>> {
    if datum_of(&bores_of(hole, faces)?, datums).is_some() {
        return Ok(HoleKey::Datum(target.id));
    }
    let rounded = |key: &str| -> Result<Option<f64>> {
        Ok(hole.optional_number(key)?.map(|x| py::round(x, 6)))
    };
    let flag = |key: &str| -> Result<bool> { Ok(py::truthy_opt(hole.get(key)?)) };
    let axis = match hole.get("axis")? {
        Some(v) if py::truthy(v) => hole
            .vector("axis")?
            .into_iter()
            .map(|x| py::round(x.abs(), 6))
            .collect(),
        _ => Vec::new(),
    };
    Ok(HoleKey::Hole {
        diameter: rounded("diameter")?,
        depth: rounded("depth")?,
        bottom: hole.get("bottom")?,
        cbore: flag("cbore")?,
        spotface: flag("spotface")?,
        csink: flag("csink")?,
        outer_steps: crate::features::outer_step_key(hole.get("outer_steps")?),
        axis,
        count: target.members.len().max(1),
        family: target.family,
        frames: frames.to_vec(),
    })
}

/// `_hole_questions`: what the hole (or holes) are for, which datums locate them and how
/// closely; and, located by ± dimensions (`planes`), how closely from the selected datum planes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn hole_questions(
    ctx: &Ctx<'_>,
    target: &Target<'_>,
    hole: &Feature<'_>,
    answers: &Map<String, Value>,
    frames: &[String],
    planes: Option<&[Vec<i64>]>,
) -> Result<Vec<Question>> {
    let diameter = hole.number("diameter")?;
    let bores = bores(&target.faces, diameter, &ctx.faces)?;
    let count = target.members.len().max(1);
    let mut label = if count > 1 {
        format!("{count}× Ø{} holes", fmt(diameter)?)
    } else {
        format!("Ø{} hole", fmt(diameter)?)
    };
    if target.groups > 0 {
        label += &format!(" in {} identical groups", target.groups);
    }
    let parts = target.parts.clone();
    // Clearance for any bolt the hole plausibly passes (ISO 273 fine to coarse); where none
    // does, a clearance hole for a bolt not stated.
    let bolts = standards::clearance_bolts(diameter);
    let tapped = standards::match_size(diameter, &TAP_DRILL);
    let mut options = vec![GENERAL.to_string()];
    options.extend(bolts.iter().map(|b| format!("clearance:{b}")));
    if let Some(t) = tapped {
        options.push(format!("tapped:{t}"));
    }
    if bolts.is_empty() && standards::largest_bolt_below(diameter).is_some() {
        options.push("clearance".to_string());
    }
    options.extend(HOLE_FITS.iter().map(|fit| format!("fit:{fit}")));
    let chosen = if let Some(size) = tapped {
        // Exact tap-drill evidence wins over a smaller screw which also fits
        // through the bore. Explicit clearance answers remain available.
        format!("tapped:{size}")
    } else if !bolts.is_empty() {
        // A bolt or tap was appended to the initial general option in this branch.
        options[1].clone()
    } else {
        GENERAL.to_string()
    };
    let mut basis = function_basis(&chosen, diameter, tapped)?;
    let mut default = Value::from(chosen);
    let mut mate = String::new();
    if let Some(mated) = suggestion(ctx.mates, &bores)? {
        let value = mated_value(mated)?;
        if let Value::String(v) = value
            && options.contains(v)
        {
            let reason = mated_reason(mated)?;
            default = value.clone();
            basis = sentence(reason);
            mate = reason.to_string();
        }
    }
    let qid = format!("hole.function:{}", target.id);
    let depth = if py::is_str(hole.get("bottom")?, "through") {
        None
    } else {
        hole.optional_number("depth")?
    };
    let mut described =
        choices::describe(&qid, &options, Some(diameter)).map_err(Error::Invalid)?;
    for choice in &mut described {
        // describe() constructs each choice as an object with a string value from options.
        let value = choice["value"]
            .as_str()
            .expect("a described choice has a value");
        let shown = choices::with_default_full_thread(value, depth);
        choice["code"] = Value::from(choices::code(&shown));
        choice["callout"] =
            Value::from(choices::callout(&shown, diameter, true).map_err(Error::Invalid)?);
    }
    let mut why = "A clearance, tapped or fitted hole each need different size and position \
                   tolerances."
        .to_string();
    if let (Some(drill), Some(size)) = (depth, tapped) {
        let full = standards::full_thread(drill, size);
        why += &format!(
            " For a blind tapped hole, set the minimum full-thread depth by typing e.g. \
             \"{size} tapped, {} mm full thread\". The default is drill depth minus 3 pitches, \
             rounded down to 0.5 mm (at least one pitch).",
            fmt(full)?
        );
        if default
            .as_str()
            .is_some_and(|v| choices::split(v).0 == "tapped")
        {
            basis += &format!(" Default minimum full thread: {} mm.", fmt(full)?);
        }
    }
    let mut out = vec![Question {
        default: Some(default.clone()),
        options: options.clone(),
        why,
        title: Some(label.clone()),
        feature: Some(target.id.to_string()),
        parts: parts.clone(),
        choices: described,
        diameter: Some(diameter),
        depth,
        basis,
        mate,
        ..Question::new(
            qid.clone(),
            "choice",
            format!(
                "What {} the {label} for?",
                if count > 1 { "are" } else { "is" }
            ),
            bores.clone(),
        )
    }];
    let function = answers.get(&qid).unwrap_or(&default);
    if let (Some(drill), Some(function)) = (depth, function.as_str()) {
        let (kind, size, inline) = choices::split(function);
        if kind == "tapped" && standards::lookup(&PITCH, size).is_some() {
            let id = format!("hole.full_thread:{}", target.id);
            if let Some(value) = answers.get(&id) {
                let text = value.as_str().ok_or_else(|| {
                    Error::Invalid(format!("{id}: a full-thread depth must be text in mm"))
                })?;
                choices::full_thread_depth(text, size, drill)?;
            }
            let full = inline.unwrap_or_else(|| standards::full_thread(drill, size));
            out.push(Question {
                default: Some(Value::from(full.to_string())),
                title: Some(label.clone()),
                feature: Some(target.id.to_string()),
                parts: parts.clone(),
                depth,
                why: "Set the minimum full-thread depth in mm. It must be positive and leave \
                      3 pitches for the tap lead and drill point. An edited value is kept \
                      without rounding and takes precedence over the depth in a tapped answer."
                    .into(),
                basis: if inline.is_some() {
                    "The depth explicitly given in the tapped-hole answer.".into()
                } else {
                    "Drill depth minus 3 pitches, rounded down to 0.5 mm (at least one pitch)."
                        .into()
                },
                ..Question::new(
                    id,
                    "text",
                    format!(
                        "What minimum full-thread depth (mm) does the {size} tapped {label} need?"
                    ),
                    bores.clone(),
                )
            });
        }
    }
    if !py::is_str(Some(function), GENERAL) && !frames.is_empty() {
        // Python reads the answer as text here, before any answer is checked: one that is
        // not text (or names no thread it knows) is refused now, as the check would.
        let Value::String(function) = function else {
            return Err(refusal(&qid, function, &options, Some(diameter), depth));
        };
        // The enclosing condition requires !frames.is_empty().
        let mut frame = frames[0].clone();
        if frames.len() > 1 {
            let fid = format!("hole.frame:{}", target.id);
            out.push(Question {
                // The enclosing condition requires !frames.is_empty().
                default: Some(Value::from(frames[0].clone())),
                options: frames.to_vec(),
                why: "The frame the position is measured from; the datums the mating part \
                      locates on."
                    .to_string(),
                title: Some(label.clone()),
                feature: Some(target.id.to_string()),
                basis: "The part's whole frame, A then B then C.".to_string(),
                ..Question::new(
                    fid.clone(),
                    "choice",
                    format!("Which datums locate the {label}?"),
                    bores.clone(),
                )
            });
            let given = answers
                .get(&fid)
                .cloned()
                // The enclosing condition requires !frames.is_empty().
                .unwrap_or_else(|| Value::from(frames[0].clone()));
            match &given {
                Value::String(f) if frames.contains(f) => frame = f.clone(),
                _ => {
                    return invalid(format!(
                        "{fid}: {} is not one of {}",
                        py::repr(&given),
                        py::repr_strs(frames)
                    ));
                }
            }
        }
        let Some(tol) = default_position(function, diameter)? else {
            return Err(refusal(
                &qid,
                &Value::from(function.clone()),
                &options,
                Some(diameter),
                depth,
            ));
        };
        let t = fmt(tol)?;
        let mut zones: Vec<String> = Vec::new();
        for z in [t.as_str(), "0.05", "0.1", "0.2", "0.5"] {
            if !zones.iter().any(|x| x == z) {
                zones.push(z.to_string());
            }
        }
        // sorted(key=float): the zones are distinct numbers.
        py::sort_by_float(&mut zones, |z| {
            crate::pyfmt::parse_float(z).unwrap_or(f64::NAN)
        });
        let mut options = vec![NONE.to_string()];
        options.extend(zones);
        out.push(Question {
            default: Some(Value::from(t)),
            options,
            why: "Locates the hole axis. Default from the fastener formula: floating T = H − F \
                  (H this hole, F the bolt; with no bolt stated, the largest standard bolt \
                  through it), fixed T = (H − F)/2; fitted holes get 0.05."
                .to_string(),
            title: Some(label.clone()),
            feature: Some(target.id.to_string()),
            frame: Some(frame.clone()),
            parts: parts.clone(),
            characteristic: Some(Characteristic::Position),
            basis: position_basis(function, diameter, tol)?,
            ..Question::new(
                format!("hole.position:{}", target.id),
                "choice",
                format!("Position tolerance (Ø) for the {label}, relative to {frame}?"),
                bores.clone(),
            )
        });
    }
    // Located by ± dimensions: how closely, from the selected datum planes.
    if let Some(planes) = planes
        && !locations::references(planes, &hole.vector("axis")?, &ctx.faces)?.is_empty()
    {
        out.push(Question {
            default: Some(Value::from(locations::TOLERANCES[0])),
            options: locations::TOLERANCES.map(String::from).to_vec(),
            why: "Their distances from the selected datum planes: the general tolerance, or a tighter ± \
                  written on each."
                .to_string(),
            title: Some(label.clone()),
            feature: Some(target.id.to_string()),
            parts,
            ..Question::new(
                format!("hole.location:{}", target.id),
                "choice",
                format!("How closely are the {label} located?"),
                bores,
            )
        });
    }
    Ok(out)
}

/// A mate's suggested value.
pub(crate) fn mated_value(mated: &Map<String, Value>) -> Result<&Value> {
    mated
        .get("value")
        .ok_or_else(|| Error::Invalid("a mate suggestion has no 'value'".to_string()))
}

/// A mate's reason, in words.
pub(crate) fn mated_reason(mated: &Map<String, Value>) -> Result<&str> {
    mated
        .get("reason")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Invalid("a mate suggestion has no 'reason'".to_string()))
}

/// The refusal of an answer the rules cannot read: what `choices.check` says of it, which is
/// what Python reports once it gets that far (where it does not crash first).
fn refusal(
    qid: &str,
    value: &Value,
    options: &[String],
    diameter: Option<f64>,
    depth: Option<f64>,
) -> Error {
    match choices::check(qid, value, options, diameter, depth) {
        Err(e) => e.into(),
        Ok(()) => Error::Invalid(format!(
            "{qid}: {} names no tabulated thread",
            py::repr(value)
        )),
    }
}

/// `_function_basis`: why a hole's purpose defaults to what it does.
fn function_basis(default: &str, diameter: f64, tapped: Option<&str>) -> Result<String> {
    let d = fmt(diameter)?;
    if let Some(bolt) = default.strip_prefix("clearance:") {
        let also = tapped
            .map(|t| format!("; it is also the {t} tap drill"))
            .unwrap_or_default();
        return Ok(format!(
            "⌀{d} is an ISO 273 clearance hole for {bolt}{also}."
        ));
    }
    if let Some(size) = default.strip_prefix("tapped:") {
        return Ok(format!("⌀{d} is the tap-drill size for {size}."));
    }
    Ok(String::new()) // left to the general tolerance: nothing was chosen
}

/// `standards.bolt_diameter`, refusing as Python's `float()` does a size that is not `M` and a
/// number (an answer such as `clearance:X`, not yet checked).
fn bolt_diameter(size: &str) -> Result<f64> {
    let number = size.strip_prefix('M').unwrap_or(size);
    crate::pyfmt::parse_float(number).ok_or_else(|| {
        Error::Invalid(format!(
            "could not convert string to float: {}",
            crate::pyfmt::repr_str(number)
        ))
    })
}

/// `_position_basis`: how a hole's default position tolerance was worked out.
fn position_basis(function: &str, diameter: f64, tol: f64) -> Result<String> {
    let (kind, size, _) = choices::split(function);
    let t = fmt(tol)?;
    if kind == "fit" {
        return Ok(format!("⌀{t}: a fitted hole is located closely."));
    }
    if kind == "tapped" {
        let h = standards::lookup(&CLEARANCE, size)
            .ok_or_else(|| Error::Invalid(crate::pyfmt::repr_str(size)))?;
        return Ok(format!(
            "⌀{t} = (H − F)/2 for a fixed {size} screw: the mating part's ⌀{} clearance hole \
             less the screw, halved.",
            fmt(h)?
        ));
    }
    let bolt = if size.is_empty() {
        standards::largest_bolt_below(diameter)
    } else {
        Some(size)
    };
    Ok(match bolt {
        Some(bolt) => format!(
            "⌀{t} = H − F for a floating {bolt} bolt: this ⌀{} hole less the bolt.",
            fmt(diameter)?
        ),
        None => format!("⌀{t}: no bolt is known, so a common value."),
    })
}

/// `_default_position`: fitted 0.05; tapped (a fixed fastener, the mating part with the
/// medium clearance hole) (H − F)/2; otherwise a floating fastener's H − F, or 0.1 with no bolt
/// known. None for a tapped size not tabulated (Python's `KeyError`).
fn default_position(function: &str, diameter: f64) -> Result<Option<f64>> {
    let (kind, size, _) = choices::split(function);
    if kind == "fit" {
        return Ok(Some(0.05));
    }
    if kind == "tapped" {
        let Some(clearance) = standards::lookup(&CLEARANCE, size) else {
            return Ok(None);
        };
        return Ok(Some(py::round((clearance - bolt_diameter(size)?) / 2.0, 2)));
    }
    let size = if size.is_empty() {
        standards::largest_bolt_below(diameter)
    } else {
        Some(size)
    };
    Ok(Some(match size {
        Some(size) => py::round(diameter - bolt_diameter(size)?, 2),
        None => 0.1,
    }))
}

/// `_hole_requirements`: a fit's size, a tapped hole's thread, or a clearance hole's H11 size.
pub(crate) fn hole_requirements(
    value: &Value,
    bores: &[i64],
    feature: &Feature<'_>,
    hole: &Feature<'_>,
) -> Result<Vec<Map<String, Value>>> {
    if py::is_str(Some(value), GENERAL) {
        return Ok(Vec::new());
    }
    let Value::String(value) = value else {
        return invalid(format!(
            "hole.function:{}: an answer is text, not {}",
            feature.id,
            py::repr(value)
        ));
    };
    let (kind, spec, full) = choices::split(value);
    let d = hole.number("diameter")?;
    let mut req = object([
        ("kind", Value::Null),
        ("feature", Value::from(feature.id)),
        ("faces", Value::from(bores.to_vec())),
    ]);
    let limits = |fit: &str| standards::fit_limits(fit, d).map_err(Error::Invalid);
    if kind == "fit" {
        let (upper, lower) = limits(spec)?;
        // req is a Map initialized with the kind key immediately above.
        req["kind"] = Value::from("size");
        req.insert("nominal".into(), Value::from(d));
        req.insert("fit".into(), Value::from(spec));
        req.insert("upper".into(), Value::from(upper));
        req.insert("lower".into(), Value::from(lower));
        return Ok(vec![req]);
    }
    if kind == "tapped" {
        let pitch = standards::lookup(&PITCH, spec)
            .ok_or_else(|| Error::Invalid(crate::pyfmt::repr_str(spec)))?;
        let depth = hole.get("depth")?;
        let through = py::is_str(hole.get("bottom")?, "through");
        let full_thread = match depth {
            _ if through => depth.cloned().unwrap_or(Value::Null),
            None => Value::Null,
            Some(depth) => match full.filter(|&f| f != 0.0) {
                Some(full) => Value::from(full),
                None => {
                    let depth = py::float_of(depth)
                        .map_err(|e| Error::Invalid(format!("feature {}: {e}", hole.id)))?;
                    Value::from(standards::full_thread(depth, spec))
                }
            },
        };
        // req is a Map initialized with the kind key immediately above.
        req["kind"] = Value::from("thread");
        for (key, v) in [
            ("side", Value::from("internal")),
            ("designation", Value::from(spec)),
            ("pitch", Value::from(pitch)),
            (
                "spec",
                Value::from(format!("{spec}x{}", crate::pyfmt::g(pitch))),
            ),
            ("class", Value::from(standards::INTERNAL_THREAD_CLASS)),
            ("hand", Value::from("RH")),
            ("through", Value::from(through)),
            (
                "drill_point",
                Value::from(py::is_str(hole.get("bottom")?, "drill_point")),
            ),
            ("drill_diameter", Value::from(d)),
            ("drill_depth", depth.cloned().unwrap_or(Value::Null)),
            ("full_thread", full_thread),
        ] {
            req.insert(key.into(), v);
        }
        return Ok(vec![req]);
    }
    // Clearance: the hole as drawn is the nominal; H11 bounds it.
    let (upper, lower) = limits("H11")?;
    // req is a Map initialized with the kind key immediately above.
    req["kind"] = Value::from("size");
    req.insert("nominal".into(), Value::from(d));
    req.insert("fit".into(), Value::from("H11"));
    req.insert("upper".into(), Value::from(upper));
    req.insert("lower".into(), Value::from(lower));
    if !spec.is_empty() {
        req.insert("clearance_for".into(), Value::from(spec));
    }
    Ok(vec![req])
}
