//! What is incomplete or does nothing in the intent so far, for a person to put right: shown
//! as the answers are given, each with the faces and question it is about (port of
//! `problems.py`).
//!
//! ```text
//! unlocated   a hole meant for a fastener or a fit with nothing to locate it
//! unused      a datum no tolerance is measured from
//! redundant   a datum that fixes nothing an earlier one does not
//! orphaned    an added requirement citing a datum that is gone
//! circular    an added orientation on the faces of the datum it cites
//! sized twice two sizes on one diameter, both of which would be written
//! ```
//!
//! It also states, as `info`, a fact about the part no answer changes: each sealed interior
//! void (`interior_void`), a cavity with no opening that no tool can reach as modelled.

// Capture indexing below follows successful matches with mandatory groups; optional groups use get().
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::choices::{GENERAL, code};
use crate::extras;
use crate::pyfmt::{g, signed_g};
use crate::rules::datums::{axis_locates, chosen_datums, classify, coaxial, parallel, turned_axis};
use crate::rules::{self, Ctx, DatumKind, Error, LOCATING, Result, object, str_of, truthy};

/// The problems with `answers` (defaults standing in for the rest) on `analysis`, each
/// `{severity, message, faces, about}`. Answered controls awaiting datums additionally name
/// their `code`, `datum_frame` and missing `awaiting` letters.
pub fn problems(analysis: &Value, answers: &Map<String, Value>) -> Result<Vec<Value>> {
    let ctx = Ctx::new(analysis)?;
    // Python asks for the settled answers' questions and the preview separately; both come
    // from one settling of the same answers.
    let (intent, merged, qs) = rules::proposed_in(&ctx, answers)?;
    let faces = &ctx.faces;
    let asked: HashSet<&str> = qs.iter().map(|q| q.id.as_str()).collect();
    let mut out = awaiting_datum_frames(&ctx, answers, &intent)?;
    if let Some(why) = analysis["resume"]["unverified"].as_str() {
        let mut diagnostic = problem(
            "warning",
            format!("Saved answers: {why}; review the open questions."),
            Value::Array(vec![]),
            "",
        );
        diagnostic["code"] = Value::from("saved_answers_unverified");
        out.push(diagnostic);
    }
    for saved in analysis["resume"]["unresolved"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let (Some(id), Some(reason)) = (saved["id"].as_str(), saved["reason"].as_str()) {
            let mut diagnostic = problem(
                "warning",
                format!("Saved answer {id}: {reason}; review the open questions."),
                Value::Array(vec![]),
                id,
            );
            diagnostic["code"] = Value::from("saved_answer_unresolved");
            out.push(diagnostic);
        }
    }

    let by_position = !matches!(
        rules::answer(&merged, &qs, "part.locating"),
        Some(Value::String(s)) if s == LOCATING[1]
    );
    let mut datum_faces: HashSet<i64> = HashSet::new();
    for d in &intent.datums {
        datum_faces.extend(face_ids(d.get("faces")));
    }
    let datums = chosen_datums(&ctx, &merged, &qs)?;
    let turned = turned_axis(&ctx, &merged)?.is_some();
    for q in &qs {
        if !q.id.starts_with("hole.function:") || !by_position {
            continue;
        }
        let value = merged
            .get(&q.id)
            .or(q.default.as_ref())
            .cloned()
            .unwrap_or(Value::Null);
        let feature = q.id.split_once(':').map_or("", |(_, f)| f);
        if matches!(&value, Value::String(s) if s == GENERAL)
            || asked.contains(format!("hole.position:{feature}").as_str())
        {
            continue;
        }
        if q.faces
            .iter()
            .any(|i| ctx.positioned.contains(i) || datum_faces.contains(i))
        {
            continue; // the file locates it, or it is a datum
        }
        if turned && axis_locates(&q.faces, &datums, faces)? {
            continue; // an axial hole is located by A, independently of a shoulder
        }
        let what = what(&q.prompt);
        let many = what.contains('×');
        out.push(problem(
            "warning",
            format!(
                "The {what} {} for {} but nothing locates {}: there is no datum A to measure a \
                 position from.",
                if many { "are" } else { "is" },
                code(&str_of(&value)),
                if many { "them" } else { "it" }
            ),
            Value::from(q.faces.clone()),
            &q.id,
        ));
    }

    let mut used: HashSet<String> = HashSet::new();
    for r in &intent.requirements {
        if let Some(Value::Array(letters)) = r.get("datums") {
            used.extend(letters.iter().map(str_of));
        }
    }
    // A distance measured from a datum's faces (one called out, say) uses it too.
    for d in &intent.datums {
        let mine: HashSet<i64> = face_ids(d.get("faces")).into_iter().collect();
        let measured = intent.requirements.iter().any(|r| {
            r.get("kind").and_then(Value::as_str) == Some("location")
                && r.get("reference").is_some_and(truthy)
                && face_ids(r.get("reference"))
                    .iter()
                    .all(|i| mine.contains(i))
        });
        if measured {
            used.insert(letter(d));
        }
    }
    for d in &intent.datums {
        if by_position && !d.get("existing").is_some_and(truthy) && !used.contains(&letter(d)) {
            out.push(problem(
                "info",
                format!(
                    "Datum {} is not used: no tolerance is measured from it.",
                    letter(d)
                ),
                d.get("faces").cloned().unwrap_or(Value::Null),
                &format!("datum.{}", letter(d)),
            ));
        }
    }

    let mut sorted: Vec<&Map<String, Value>> = intent.datums.iter().collect();
    sorted.sort_by_key(|d| letter(d));
    let mut seen: Vec<(&Map<String, Value>, DatumKind)> = Vec::new();
    for d in sorted {
        let ids = face_ids(d.get("faces"));
        let Some(kind) = classify(&ids, faces)? else {
            continue;
        };
        // classify() above refused empty face lists before returning a datum kind.
        let mine = faces.at(ids[0])?;
        for (earlier, theirs_kind) in &seen {
            // seen contains only datums that passed the same nonempty classify() check.
            let theirs = faces.at(face_ids(earlier.get("faces"))[0])?;
            let same = match (kind, theirs_kind) {
                (DatumKind::Plane, DatumKind::Plane) => {
                    parallel(&mine.direction()?, &theirs.direction()?)
                }
                (DatumKind::Axis, DatumKind::Axis) => coaxial(mine, theirs)?,
                _ => false,
            };
            if same {
                let relation = if kind == DatumKind::Plane {
                    "parallel to"
                } else {
                    "on the same axis as"
                };
                let (x, y) = (letter(d), letter(earlier));
                out.push(problem(
                    "warning",
                    format!(
                        "Datum {x} is {relation} {y}, so it fixes nothing {y} does not; choose \
                         one square to it."
                    ),
                    d.get("faces").cloned().unwrap_or(Value::Null),
                    &format!("datum.{x}"),
                ));
                break;
            }
        }
        seen.push((d, kind));
    }

    // Two sizes on one diameter (a fit asked for and one called out, say) would both be
    // written, and disagree.
    let sizes: Vec<&Map<String, Value>> = intent
        .requirements
        .iter()
        .filter(|r| r.get("kind").and_then(Value::as_str) == Some("size"))
        .collect();
    let size_faces: Vec<HashSet<i64>> = sizes
        .iter()
        .map(|r| face_ids(r.get("faces")).into_iter().collect())
        .collect();
    for (i, later) in sizes.iter().enumerate() {
        let Some(j) = (0..i).find(|&j| !size_faces[j].is_disjoint(&size_faces[i])) else {
            continue;
        };
        // j ranges over sizes of this same list.
        let earlier = sizes[j];
        let twice = format!("{} and {}", size_text(earlier)?, size_text(later)?);
        let nominal = later
            .get("nominal")
            .and_then(Value::as_f64)
            .ok_or_else(|| Error::Invalid("a size has no 'nominal'".to_string()))?;
        let question = |r: &Map<String, Value>| r.get("question").filter(|q| truthy(q)).map(str_of);
        let about = question(later)
            .or_else(|| question(earlier))
            .unwrap_or_default();
        out.push(problem(
            "warning",
            format!(
                "⌀{} is sized twice ({twice}); remove one, or both are written.",
                g(nominal)
            ),
            later.get("faces").cloned().unwrap_or(Value::Null),
            &about,
        ));
    }

    let letters: HashSet<String> = intent.datums.iter().map(letter).collect();
    for (key, extra) in extras::given(answers) {
        let kind = extra.get("kind");
        let datumed = kind
            .and_then(Value::as_str)
            .is_some_and(|k| extras::DATUMED.contains(&k));
        let cited: Vec<Value> = match extra.get("datums") {
            Some(v) if truthy(v) => rules::iter_of(v).unwrap_or_default(),
            _ => Vec::new(),
        };
        let picked = face_ids(extra.get("faces"));
        let kind_text = kind.map(str_of).unwrap_or_default();
        let own: Vec<String> = cited
            .iter()
            .filter(|x| {
                datumed
                    && intent
                        .datums
                        .iter()
                        .find(|d| d.get("letter") == Some(*x))
                        .is_some_and(|d| {
                            face_ids(d.get("faces")).iter().any(|i| picked.contains(i))
                        })
            })
            .map(str_of)
            .collect();
        if let Some(first) = own.first() {
            out.push(problem(
                "warning",
                format!(
                    "The {kind_text} you added is on datum {first}'s own faces, so it is measured \
                     from itself; choose other faces for {first} or for the {kind_text}."
                ),
                extra.get("faces").cloned().unwrap_or(Value::Null),
                key,
            ));
        }
        let missing: Vec<String> = cited
            .iter()
            .map(str_of)
            .filter(|x| !letters.contains(x))
            .collect();
        if datumed && let Some(first) = missing.first() {
            out.push(problem(
                "warning",
                format!(
                    "The {kind_text} you added is measured from datum {}, which has no faces yet; \
                     pick them under Datums, or it will not be written.",
                    missing.join(", ")
                ),
                extra
                    .get("faces")
                    .cloned()
                    .unwrap_or_else(|| Value::Array(Vec::new())),
                &format!("datum.{first}"),
            ));
        }
    }
    out.extend(interior_voids(analysis));
    Ok(out)
}

/// Each sealed interior void as an `info` fact (`code: "interior_void"`), with its feature,
/// faces, the kernel's shell volume (`estimated_volume`) and, for a cylinder closed by caps
/// square to its axis, its `diameter` and `length`. A void asks no question; this only states
/// it. Sealed means the kernel measured it as a closed shell (`volume_method: "closed_shell"`,
/// quiddity >= 0.4.7); an `interior_voids` chamber the air-sampling grid found behind an
/// opening (`six_axis_grid`, non-empty `openings`) is not sealed and is not stated.
fn interior_voids(analysis: &Value) -> Vec<Value> {
    let faces = analysis["faces"].as_array().map_or(&[][..], Vec::as_slice);
    let face = |id: i64| faces.iter().find(|f| f["id"].as_i64() == Some(id));
    let mut out = Vec::new();
    for feature in analysis["features"].as_array().into_iter().flatten() {
        if feature["family"] != "interior_voids"
            || feature["record"]["volume_method"] != "closed_shell"
        {
            continue;
        }
        let ids = face_ids(feature.get("faces"));
        let size = cylinder_size(&ids, face);
        let what = match size {
            Some((diameter, length)) => format!(" ⌀{} × {} long", g(diameter), g(length)),
            None => String::new(),
        };
        let mut fact = problem(
            "info",
            format!(
                "Closed internal cavity{what}: no opening to the outside; can't be drilled, \
                 tapped or moulded as modelled."
            ),
            Value::from(ids.clone()),
            "",
        );
        fact["code"] = Value::from("interior_void");
        fact["feature"] = feature["id"].clone();
        let volume = feature["record"]["estimated_volume"].as_f64();
        if let Some(volume) = volume {
            fact["estimated_volume"] = Value::from(volume);
        }
        match (size, volume) {
            (Some((diameter, length)), _) => {
                fact["diameter"] = Value::from(diameter);
                fact["length"] = Value::from(length);
            }
            (None, Some(volume)) => {
                fact["message"] = Value::from(format!(
                    "Closed internal cavity (about {} mm³): no opening to the outside; \
                     can't be drilled, tapped or moulded as modelled.",
                    g(round_sig(volume, 3))
                ));
            }
            (None, None) => {}
        }
        out.push(fact);
    }
    out
}

/// A void's diameter and length when its faces are one cylinder (possibly split into
/// coaxial pieces of one radius) and planar caps square to its axis.
fn cylinder_size<'a>(ids: &[i64], face: impl Fn(i64) -> Option<&'a Value>) -> Option<(f64, f64)> {
    let vec3 =
        |v: &Value| -> Option<[f64; 3]> { Some([v[0].as_f64()?, v[1].as_f64()?, v[2].as_f64()?]) };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (mut axis, mut radius, mut caps) = (None::<[f64; 3]>, None::<f64>, Vec::new());
    for &id in ids {
        let f = face(id)?;
        let direction = vec3(&f["direction"])?;
        match f["kind"].as_str()? {
            "cylinder" => {
                let r = f["radius"].as_f64()?;
                if let (Some(a), Some(r0)) = (axis, radius)
                    && (dot(a, direction).abs() < 1.0 - 1e-9 || (r - r0).abs() > 1e-6)
                {
                    return None;
                }
                axis = Some(direction);
                radius = Some(r);
            }
            "plane" => caps.push((direction, vec3(&f["centroid"])?)),
            _ => return None,
        }
    }
    let (axis, radius) = (axis?, radius?);
    if caps.len() < 2 || caps.iter().any(|(n, _)| dot(*n, axis).abs() < 1.0 - 1e-9) {
        return None;
    }
    let along: Vec<f64> = caps.iter().map(|(_, c)| dot(*c, axis)).collect();
    let length = along.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - along.iter().copied().fold(f64::INFINITY, f64::min);
    let micron = |x: f64| (x * 1e6).round() / 1e6;
    (length > 1e-9).then_some((micron(2.0 * radius), micron(length)))
}

/// `x` to `digits` significant figures.
fn round_sig(x: f64, digits: i32) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let scale = 10f64.powi(digits - 1 - x.abs().log10().floor() as i32);
    (x * scale).round() / scale
}

/// A datum item's letter.
fn letter(d: &Map<String, Value>) -> String {
    d.get("letter").map(str_of).unwrap_or_default()
}

/// The face numbers in a list of faces (anything else in it names no face).
fn face_ids(value: Option<&Value>) -> Vec<i64> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| {
                i.as_i64()
                    .or_else(|| i.as_f64().filter(|x| x.fract() == 0.0).map(|x| x as i64))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `_what`: what a hole question asks about (`6× Ø6.6 holes`), from its prompt.
fn what(prompt: &str) -> String {
    static WHAT: LazyLock<Regex> =
        // Pattern and interpolated character classes are source constants, never user text.
        LazyLock::new(|| Regex::new(r"^What (?:are|is) the (.+) for\?$").expect("pattern"));
    WHAT.captures(prompt)
        .map_or_else(|| "hole".to_string(), |caps| caps[1].to_string())
}

/// A size as its fit, absolute limits, or deviations.
fn size_text(req: &Map<String, Value>) -> Result<String> {
    if let Some(bounds) = req.get("limits") {
        let [lower, upper]: [f64; 2] = serde_json::from_value(bounds.clone())
            .map_err(|e| Error::Invalid(format!("a size has invalid limits: {e}")))?;
        return Ok(format!("{}/{}", g(lower), g(upper)));
    }
    if let Some(fit) = req.get("fit").filter(|f| truthy(f)) {
        return Ok(str_of(fit));
    }
    let number = |key: &str| {
        req.get(key)
            .and_then(Value::as_f64)
            .ok_or_else(|| Error::Invalid(format!("a size has no '{key}'")))
    };
    let (upper, lower) = (number("upper")?, number("lower")?);
    Ok(if upper == -lower {
        format!("±{}", g(upper))
    } else {
        format!("{}/{}", signed_g(upper), signed_g(lower))
    })
}

/// Use the proposed requirements to find each answered item's complete frame, including
/// measurement planes and the subject of a datum control. Compare with accepted/existing
/// datums, not the settled suggestions used for the other prospective diagnostics.
fn awaiting_datum_frames(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
    proposed: &rules::Intent,
) -> Result<Vec<Value>> {
    let accepted = chosen_datums(ctx, answers, &[])?;
    let mut frames: BTreeMap<&str, (Vec<String>, BTreeSet<i64>)> = BTreeMap::new();
    for req in &proposed.requirements {
        let Some(id) = req.get("question").and_then(Value::as_str) else {
            continue;
        };
        if !answers.contains_key(id)
            || !(id.starts_with("hole.position:")
                || id.starts_with("hole.location:")
                || id.starts_with("diameter.runout:")
                || id == "datum.A.flatness"
                || (id.starts_with("datum.") && id.ends_with(".control")))
        {
            continue;
        }
        let (frame, faces) = frames.entry(id).or_default();
        faces.extend(face_ids(req.get("faces")));
        if let Some(Value::Array(letters)) = req.get("datums") {
            // A position's basic locations precede its control. The control's ordered
            // references supersede those planes: D|B|C must not be reported as B|C|D.
            *frame = letters.iter().map(str_of).collect();
        }
        if let Some(reference) = req.get("reference") {
            for datum in &proposed.datums {
                if datum.get("faces") == Some(reference) {
                    let reference = letter(datum);
                    if !frame.contains(&reference) {
                        frame.push(reference);
                    }
                }
            }
        }
        if id == "datum.A.flatness" || id.ends_with(".control") {
            // C controls require B even when B is not square to C and therefore absent
            // from the perpendicularity references (datum_control checks the whole frame).
            if id == "datum.C.control" && !frame.iter().any(|x| x == "B") {
                frame.push("B".into());
            }
            if let Some(subject) = id.split('.').nth(1)
                && !frame.iter().any(|x| x == subject)
            {
                frame.push(subject.to_string());
            }
        }
    }
    let mut out = Vec::new();
    for (id, (frame, faces)) in frames {
        let awaiting: Vec<_> = frame
            .iter()
            .filter(|letter| !letter.chars().next().is_some_and(|c| accepted.contains(c)))
            .cloned()
            .collect();
        if awaiting.is_empty() {
            continue;
        }
        let mut p = problem(
            "warning",
            format!(
                "Answer {id} awaits datum frame {}: accept or answer datum {} before the complete control can be written.",
                frame.join("|"),
                awaiting.join(", ")
            ),
            Value::from(faces.into_iter().collect::<Vec<_>>()),
            id,
        );
        // problem() constructs an object; Value writes add string keys without bounds checks.
        p["code"] = Value::from("awaits_datum_frame");
        p["datum_frame"] = Value::from(frame);
        p["awaiting"] = Value::from(awaiting);
        out.push(p);
    }
    Ok(out)
}

fn problem(severity: &str, message: String, faces: Value, about: &str) -> Value {
    Value::Object(object([
        ("severity", Value::from(severity)),
        ("message", Value::from(message)),
        ("faces", faces),
        ("about", Value::from(about)),
    ]))
}
