//! Turned diameters: a fit, a thread or a knurl, and how true each runs about the part's axis.

use serde_json::{Map, Value};

use super::datums::classify;
use super::holes::{mated_reason, mated_value};
use super::mates::suggestion;
use super::py::{self, fmt, object, sentence};
use super::records::{Ctx, DatumKind, Datums, Feature};
use super::{Characteristic, Error, GENERAL, NONE, Question, Result, invalid};
use crate::choices;
use crate::rule_sources as sources;
use crate::standards::{self, KNURL_PATTERNS, PITCH, SHAFT_FITS};

/// Runout zones offered for a turned diameter, beside none.
pub const RUNOUTS: [&str; 4] = ["0.01", "0.02", "0.05", "0.1"];

/// A turned step's cylinder faces, or all its faces where it has none.
fn cylinders(feature: &Feature<'_>, ctx: &Ctx<'_>) -> Result<Vec<i64>> {
    let faces = feature.faces()?;
    let mut out = Vec::new();
    for &i in faces {
        if ctx.faces.at(i)?.is_cylinder() {
            out.push(i);
        }
    }
    Ok(if out.is_empty() { faces.to_vec() } else { out })
}

/// Each feature of a group with its cylinders, when the group is more than one.
fn parts(group: &[&Feature<'_>], ctx: &Ctx<'_>) -> Result<Vec<(String, Vec<i64>)>> {
    if group.len() < 2 {
        return Ok(Vec::new());
    }
    group
        .iter()
        .map(|f| Ok((f.id.to_string(), cylinders(f, ctx)?)))
        .collect()
}

/// `_turned_question`: one question for the turned diameters of one size; each keeps its own
/// fit.
pub(crate) fn turned_question(group: &[&Feature<'_>], ctx: &Ctx<'_>) -> Result<Question> {
    let feature = group
        .first()
        .ok_or_else(|| Error::Invalid("a turned group needs at least one feature".into()))?;
    let d = feature.number("diameter")?;
    let times = if group.len() > 1 {
        format!("{}× ", group.len())
    } else {
        String::new()
    };
    let mut options = vec![GENERAL.to_string()];
    options.extend(SHAFT_FITS.iter().map(|f| f.to_string()));
    // Every ISO 261 thread at this size (stream S3): the coarse one as Python offers it
    // (`thread:M10`, where Python's table has it), then the fine pitches (`thread:M10x1.25`),
    // and the coarse threads of the sizes Python's table stops short of (M14, M30).
    options.extend(
        standards::external_threads(d)
            .into_iter()
            .map(|spec| format!("thread:{spec}")),
    );
    options.extend(KNURL_PATTERNS.iter().map(|p| format!("knurl:{p}")));
    let mut on = Vec::new();
    for f in group {
        on.extend(cylinders(f, ctx)?);
    }
    let mut default = Value::from(GENERAL);
    let (mut basis, mut mate) = (String::new(), String::new());
    let mut source = &sources::TURNED_GENERAL;
    if let Some(mated) = suggestion(ctx.mates, &on)? {
        let value = mated_value(mated)?;
        if let Value::String(v) = value
            && options.contains(v)
        {
            let reason = mated_reason(mated)?;
            default = value.clone();
            basis = sentence(reason);
            source = sources::of_mate(v);
            mate = reason.to_string();
        }
    }
    let qid = format!("diameter.fit:{}", feature.id);
    let described = choices::describe(&qid, &options, Some(d)).map_err(Error::Invalid)?;
    Ok(Question {
        default: Some(default),
        options,
        why: "Functional diameters need a fit (bearing, seal, press fit); a diameter at a \
              metric size may be threaded; a grip may be knurled. The rest are covered by the \
              general tolerance."
            .to_string(),
        title: Some(format!("Turned {times}Ø{}", fmt(d)?)),
        feature: Some(feature.id.to_string()),
        parts: parts(group, ctx)?,
        choices: described,
        diameter: Some(d),
        basis,
        source: Some(source),
        mate,
        ..Question::new(
            qid,
            "choice",
            format!(
                "Is the turned {times}Ø{} diameter a fit, a thread or knurled?",
                fmt(d)?
            ),
            on,
        )
    })
}

/// `_runout_frame`: the frame a turned diameter's runout is measured from (the datums up to
/// and including the part's axis: those given, else those that would be proposed) and the
/// axis datum's faces. None for a part with no datum axis.
pub(crate) fn runout_frame(
    ctx: &Ctx<'_>,
    datums: &Datums,
    answers: &Map<String, Value>,
) -> Result<Option<(String, Vec<i64>)>> {
    if !datums.is_empty() {
        let letters = datums.letters();
        let Some(at) = datums.iter().position(|(_, d)| d.kind == DatumKind::Axis) else {
            return Ok(None);
        };
        // letters is mapped from datums; at is the position in that same ordered list.
        let frame = letters[..=at]
            .iter()
            .map(char::to_string)
            .collect::<Vec<_>>()
            .join("|");
        // at is found by enumerating datums; datums is immutable.
        return Ok(Some((frame, datums.0[at].1.faces.clone())));
    }
    let (a, b, _) = ctx.default_datums_with(answers)?;
    if !a.is_empty() && classify(&a, &ctx.faces)? == Some(DatumKind::Axis) {
        return Ok(Some(("A".to_string(), a)));
    }
    if !a.is_empty()
        && !b.is_empty()
        && classify(&a, &ctx.faces)? == Some(DatumKind::Plane)
        && classify(&b, &ctx.faces)? == Some(DatumKind::Axis)
    {
        return Ok(Some(("A|B".to_string(), b)));
    }
    Ok(None)
}

/// `_runout_question`: how true a turned diameter runs about the part's axis; none by default.
pub(crate) fn runout_question(
    group: &[&Feature<'_>],
    ctx: &Ctx<'_>,
    frame: &str,
) -> Result<Question> {
    let feature = group
        .first()
        .ok_or_else(|| Error::Invalid("a turned group needs at least one feature".into()))?;
    let d = feature.number("diameter")?;
    let times = if group.len() > 1 {
        format!("{}× ", group.len())
    } else {
        String::new()
    };
    let mut on = Vec::new();
    for f in group {
        on.extend(cylinders(f, ctx)?);
    }
    let mut options = vec![NONE.to_string()];
    options.extend(RUNOUTS.iter().map(|r| r.to_string()));
    Ok(Question {
        default: Some(Value::from(NONE)),
        options,
        why: "How far the diameter may wobble as the part turns on its axis: for bearing seats, \
              seals and anything that spins."
            .to_string(),
        title: Some(format!("Turned {times}Ø{}", fmt(d)?)),
        feature: Some(feature.id.to_string()),
        characteristic: Some(Characteristic::Runout),
        source: Some(&sources::RUNOUT_NONE),
        frame: Some(frame.to_string()),
        parts: parts(group, ctx)?,
        ..Question::new(
            format!("diameter.runout:{}", feature.id),
            "choice",
            format!(
                "Runout of the turned {times}Ø{} relative to {frame}?",
                fmt(d)?
            ),
            on,
        )
    })
}

/// A knurl's derived `turned_knurl` parameters as intent: `teeth`, `tooth_depth`,
/// `root_fillet`, a diamond's `helix_angle`, then `standard` and `derived` (each derived key
/// with its source). The writer fills an intent without them the same way.
fn knurl_fields(req: &mut Map<String, Value>, k: &standards::KnurlParameters) {
    req.insert("teeth".into(), Value::from(k.teeth));
    req.insert("tooth_depth".into(), Value::from(k.tooth_depth));
    req.insert("root_fillet".into(), Value::from(k.root_fillet));
    if let Some(a) = k.helix_angle {
        req.insert("helix_angle".into(), Value::from(a));
    }
    req.insert("standard".into(), Value::from(standards::KNURL_STANDARD));
    let derived: Map<String, Value> = k
        .sources
        .iter()
        .map(|(key, why)| ((*key).to_string(), Value::from(why.as_str())))
        .collect();
    req.insert("derived".into(), Value::Object(derived));
}

/// `_turned_requirement`: a turned diameter's answer as a fit's size, an external thread or a
/// knurl.
pub(crate) fn turned_requirement(
    value: &Value,
    d: f64,
    fid: &str,
    cylinders: &[i64],
    ctx: &Ctx<'_>,
) -> Result<Map<String, Value>> {
    let Value::String(value) = value else {
        return invalid(format!(
            "diameter.fit:{fid}: an answer is text, not {}",
            py::repr(value)
        ));
    };
    let (kind, spec) = value.split_once(':').unwrap_or((value.as_str(), ""));
    let base = |kind: &str| {
        object([
            ("kind", Value::from(kind)),
            ("feature", Value::from(fid)),
            ("faces", Value::from(cylinders.to_vec())),
        ])
    };
    if kind == "thread" {
        let feature = ctx.feature(fid)?;
        let length = match feature.get("hi")? {
            None => Value::Null,
            Some(hi) => {
                let lo = feature.get("lo")?.unwrap_or(&Value::Null);
                py::sub(hi, lo).map_err(|e| Error::Invalid(format!("feature {fid}: {e}")))?
            }
        };
        // An ISO 261 coarse or fine thread (stream S3: `M10x1.25` is designation M10, pitch
        // 1.25), else Python's coarse table, else Python's refusal.
        let (size, pitch) = match standards::external_thread(spec) {
            Some(found) => found,
            None => (
                spec,
                standards::lookup(&PITCH, spec)
                    .ok_or_else(|| Error::Invalid(crate::pyfmt::repr_str(spec)))?,
            ),
        };
        let mut req = base("thread");
        for (key, v) in [
            ("side", Value::from("external")),
            ("designation", Value::from(size)),
            ("pitch", Value::from(pitch)),
            (
                "spec",
                Value::from(format!("{size}x{}", crate::pyfmt::g(pitch))),
            ),
            ("class", Value::from(standards::EXTERNAL_THREAD_CLASS)),
            ("hand", Value::from("RH")),
            ("length", length),
            ("nominal", Value::from(d)),
        ] {
            req.insert(key.into(), v);
        }
        return Ok(req);
    }
    if kind == "knurl" {
        let mut req = base("knurl");
        req.insert("pattern".into(), Value::from(spec));
        req.insert("pitch".into(), Value::from(standards::KNURL_PITCH));
        req.insert("diameter".into(), Value::from(d));
        // AP242's turned_knurl needs more than these (WR2), so the rest is derived from the
        // knurling standard and stated with its source, visible and editable (questions Q4,
        // Q15; a difference from Python, whose knurl has these three). A pattern the standard
        // does not give states nothing more, and the writer lists it as not written.
        if let Ok(k) = standards::knurl_parameters(spec, standards::KNURL_PITCH, d) {
            knurl_fields(&mut req, &k);
        }
        return Ok(req);
    }
    let (upper, lower) = standards::fit_limits(value, d).map_err(Error::Invalid)?;
    let mut req = base("size");
    req.insert("nominal".into(), Value::from(d));
    req.insert("fit".into(), Value::from(value.as_str()));
    req.insert("upper".into(), Value::from(upper));
    req.insert("lower".into(), Value::from(lower));
    Ok(req)
}
