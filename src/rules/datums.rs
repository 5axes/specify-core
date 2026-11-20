//! Datums: which are proposed and asked, what the answers make of them, how datum B and C are
//! held to the datums before them, and the frames a position may be measured from.

// Coordinate indexing uses [f64; 3] (or a bounded 0..3 loop), never variable-length JSON arrays.
use std::collections::HashSet;

use serde_json::{Map, Value};

use super::py::{self, dot};
use super::records::{Ctx, Datum, DatumKind, Datums, DefaultDatums, Face, Faces};
use super::{Characteristic, EXTRA_DATUMS, Error, NONE, Question, Result, invalid};
use crate::rule_sources::{self as sources, Rule};

/// The letters A to C, then the extra datums.
pub(crate) const LETTERS: [char; 8] = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'];

/// `datum_kind`: `plane` for coplanar faces, `axis` for the coaxial cylinders of one hole or
/// boss; anything else cannot be a datum here (Python's `ValueError`, with its message).
pub fn datum_kind(face_ids: &[i64], faces: &Faces) -> Result<DatumKind> {
    let shown: Vec<Value> = face_ids.iter().map(|&i| Value::from(i)).collect();
    kind_of(face_ids, &shown, faces)
}

/// `datum_kind` on faces already read as ids; `shown` is the list as given, for the message.
pub(crate) fn kind_of(face_ids: &[i64], shown: &[Value], faces: &Faces) -> Result<DatumKind> {
    classify(face_ids, faces)?.ok_or_else(|| {
        Error::Invalid(format!(
            "faces {} are neither coplanar faces nor one hole or boss, so they cannot form one \
             datum",
            py::repr(&Value::Array(shown.to_vec()))
        ))
    })
}

/// What `face_ids` form: a plane, an axis, or neither (None: Python's `ValueError`, which
/// callers may catch). A face that is not there, or lacks what the test needs, is an error
/// (Python's `KeyError`, which no caller catches).
pub(crate) fn classify(face_ids: &[i64], faces: &Faces) -> Result<Option<DatumKind>> {
    let chosen: Vec<&Face> = face_ids
        .iter()
        .map(|&i| faces.at(i))
        .collect::<Result<_>>()?;
    let Some(first) = chosen.first() else {
        // Python's chosen[0] raises IndexError; no caller passes an empty datum.
        return invalid("a datum needs at least one face");
    };
    if chosen.iter().all(|f| f.is_plane()) && all(&chosen, |f| coplanar(f, first))? {
        return Ok(Some(DatumKind::Plane));
    }
    if chosen.iter().all(|f| f.is_cylinder()) && all(&chosen, |f| coaxial(f, first))? {
        return Ok(Some(DatumKind::Axis));
    }
    Ok(None)
}

/// Python's `all(test(f) for f in faces)`, stopping at the first false.
fn all(faces: &[&Face], test: impl Fn(&Face) -> Result<bool>) -> Result<bool> {
    for f in faces {
        if !test(f)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `_coplanar`: same normal and the same level along it.
pub(crate) fn coplanar(f: &Face, g: &Face) -> Result<bool> {
    let n = f.direction()?;
    let (a, b) = (f.centroid()?, g.centroid()?);
    let offset = 0.0 + (a[0] - b[0]) * n[0] + (a[1] - b[1]) * n[1] + (a[2] - b[2]) * n[2];
    Ok(same_direction(&n, &g.direction()?) && offset.abs() < 1e-4)
}

/// `_coaxial`: the same radius on one axis (faces of one diameter).
pub(crate) fn coaxial(f: &Face, g: &Face) -> Result<bool> {
    Ok((f.radius()? - g.radius()?).abs() <= 1e-6 && same_axis(f, g)?)
}

/// One axis, independent of diameter and direction sign.
pub(crate) fn same_axis(f: &Face, g: &Face) -> Result<bool> {
    if !parallel(&f.direction()?, &g.direction()?) {
        return Ok(false);
    }
    let (o, p) = (f.origin()?, g.origin()?);
    let d = [o[0] - p[0], o[1] - p[1], o[2] - p[2]];
    let u = g.direction()?;
    let cross = [
        d[1] * u[2] - d[2] * u[1],
        d[2] * u[0] - d[0] * u[2],
        d[0] * u[1] - d[1] * u[0],
    ];
    Ok(hypot(&cross) < 1e-4)
}

/// `math.hypot` of three components, scaled so it neither overflows nor underflows.
fn hypot(v: &[f64; 3]) -> f64 {
    let scale = v.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    if scale == 0.0 || !scale.is_finite() {
        return scale;
    }
    let s = v.iter().map(|x| (x / scale) * (x / scale)).sum::<f64>();
    scale * s.sqrt()
}

fn same_direction(a: &[f64; 3], b: &[f64; 3]) -> bool {
    dot(a, b) > 1.0 - 1e-6
}

/// `_parallel`: the same line, either way along it.
pub(crate) fn parallel(a: &[f64; 3], b: &[f64; 3]) -> bool {
    (dot(a, b).abs() - 1.0).abs() < 1e-6
}

/// `_perpendicular`.
pub(crate) fn perpendicular(a: &[f64; 3], b: &[f64; 3]) -> bool {
    dot(a, b).abs() < 1e-6
}

/// Stable descending area order. Integration noise must not choose between equal faces.
fn sort_by_area<T>(items: &mut [(T, f64)]) {
    // An approximate comparison is not transitive. Insertion sort preserves input order
    // for ties without asking the standard sort to assume a total order.
    for i in 1..items.len() {
        let mut j = i;
        while j > 0 {
            let (previous, next) = (items[j - 1].1, items[j].1);
            let tolerance = 1e-6_f64.max(1e-6 * previous.abs().max(next.abs()));
            if next - previous <= tolerance {
                break;
            }
            items.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// The planes by area, largest first (a stable sort, as Python's `sorted(key=-area)`).
fn planes_by_area(faces: &Faces) -> Result<Vec<&Face>> {
    let mut planes: Vec<(&Face, f64)> = faces
        .iter()
        .filter(|f| f.is_plane())
        .map(|f| Ok((f, f.area()?)))
        .collect::<Result<_>>()?;
    sort_by_area(&mut planes);
    Ok(planes.into_iter().map(|(f, _)| f).collect())
}

/// Default datums. With `turned`, A has the greatest cylindrical area (diameter breaks ties),
/// including every split face of that diameter; B is a perpendicular locating face, preferring
/// assembly contact, otherwise the largest area. C is empty: a surface of revolution has no
/// clocking datum. Without `turned`, retain the resting/largest plane and square planes.
/// Callers with recognised features use `turned_axis` to establish whether the part is turned.
pub fn default_datums(faces: &Faces, turned: bool) -> Result<DefaultDatums> {
    if turned {
        let cylinders: Vec<_> = faces.iter().filter(|f| f.is_cylinder()).collect();
        if let Some(axis) = primary_cylinder(&cylinders)? {
            return turned_datums(faces, axis, &HashSet::new());
        }
    }
    let planes = planes_by_area(faces)?;
    let Some(&largest) = planes.first() else {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    };
    let a = resting_face(&planes)?.unwrap_or(largest);
    let a_ids = coplanar_ids(&planes, a)?;
    let normal = a.direction()?;
    let mut square = Vec::new();
    for p in &planes {
        if perpendicular(&p.direction()?, &normal) {
            square.push(*p);
        }
    }
    let Some(&b) = square.first() else {
        return Ok((a_ids, Vec::new(), Vec::new()));
    };
    let b_normal = b.direction()?;
    let mut c = Vec::new();
    // square includes the nonempty primary plane group selected above.
    for p in &square[1..] {
        if perpendicular(&p.direction()?, &b_normal) {
            c.push(p.id);
            break;
        }
    }
    Ok((a_ids, vec![b.id], c))
}

/// Greatest total area of split cylindrical faces, then largest diameter, then first in
/// analysis order. Area favours a substantial locating seat over a short, broad collar.
/// Do not combine different diameters
/// into a datum: they establish the same nominal axis but are distinct datum features.
fn primary_cylinder<'f>(cylinders: &[&'f Face]) -> Result<Option<&'f Face>> {
    let mut best: Option<(&Face, f64, f64)> = None;
    for &f in cylinders {
        let radius = f.radius()?;
        let mut area = 0.0;
        for &g in cylinders {
            if coaxial(f, g)? {
                area += g.area()?;
            }
        }
        if best.is_none_or(|(_, r, a)| area > a || (area == a && radius > r)) {
            best = Some((f, radius, area));
        }
    }
    Ok(best.map(|(f, _, _)| f))
}

/// Explicit turned-part detection: recognised turned-step (or boss) cylinders share one axis and
/// dominate the lateral planar area. This admits auxiliary cross holes, flats and keyways,
/// while a boss on a mostly prismatic body or steps on different axes retain plane datums.
/// Only recognised principal diameters are candidates for A, never an auxiliary hole.
pub(crate) fn turned_axis<'c>(
    ctx: &'c Ctx<'_>,
    answers: &Map<String, Value>,
) -> Result<Option<&'c Face>> {
    let mut ids = Vec::new();
    // An interrupted, single-diameter shaft can be recognised only as a boss. Apply the
    // same dominance and common-axis safeguards, rather than requiring multiple steps.
    let family = if ctx.has_family("turned_steps") {
        "turned_steps"
    } else if ctx.has_family("turned_bodies") {
        // #123 removes a nearly full-length OD boss. The body profile still names
        // its external cylinders; preserve their datum role without making body
        // questions or treating the bore as an external locating seat.
        "turned_bodies"
    } else {
        "bosses"
    };
    for feature in ctx.features.iter().filter(|f| f.family == family) {
        let external: HashSet<i64> = if family == "turned_bodies" {
            feature
                .get("profile")?
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|s| s["side"] == "external" && s["kind"] == "cylinder")
                .filter_map(|s| s["face"].as_i64())
                .collect()
        } else {
            HashSet::new()
        };
        for &id in feature.faces()? {
            if family == "turned_bodies" && !external.contains(&id) {
                continue;
            }
            if ctx.faces.at(id)?.is_cylinder() && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    let cylinders: Vec<_> = ids
        .iter()
        .map(|&i| ctx.faces.at(i))
        .collect::<Result<_>>()?;
    let Some(axis) = primary_cylinder(&cylinders)? else {
        return Ok(None);
    };
    if !all(&cylinders, |f| same_axis(f, axis))? {
        return Ok(None);
    }
    let turned_area = cylinders
        .iter()
        .try_fold(0.0, |sum, f| -> Result<f64> { Ok(sum + f.area()?) })?;
    let mut lateral_area = 0.0;
    for face in ctx.faces.iter().filter(|f| f.is_plane()) {
        if !parallel(&face.direction()?, &axis.direction()?) {
            lateral_area += face.area()?;
        }
    }
    if turned_area <= lateral_area {
        return Ok(None);
    }
    // Keep rough surfaces in the detection evidence, but never use them as a seat.
    let excluded = rough_cylinders(ctx, answers)?;
    let mut plain = Vec::new();
    for &f in &cylinders {
        let mut rough = false;
        for &id in &excluded {
            let g = ctx.faces.at(id)?;
            if g.is_cylinder() && coaxial(f, g)? {
                rough = true;
                break;
            }
        }
        if !rough {
            plain.push(f);
        }
    }
    primary_cylinder(&plain)
}

/// Known threads/knurls, from semantic PMI, recognition, or the effective diameter answer.
/// Resolve grouped and typed answers exactly as the diameter questions do. An explicit
/// plain answer cannot erase a thread or knurl already recorded in the file.
fn rough_cylinders(ctx: &Ctx<'_>, answers: &Map<String, Value>) -> Result<HashSet<i64>> {
    let mut excluded = HashSet::new();
    if let Some(items) = ctx.analysis.get("existing").and_then(Value::as_array) {
        for e in items {
            if e["kind"] == "note"
                && matches!(
                    e["type"].as_str(),
                    Some("knurl" | "external thread" | "internal thread")
                )
                && let Some(ids) = e["faces"].as_array()
            {
                excluded.extend(ids.iter().filter_map(Value::as_i64));
            }
        }
    }
    for f in &ctx.features {
        if matches!(
            f.family,
            "knurls"
                | "turned_knurls"
                | "threads"
                | "external_threads"
                | "internal_threads"
                | "modelled_threads"
        ) {
            excluded.extend(f.faces()?);
        }
    }
    let mut consider = |q: Question| -> Result<()> {
        let value = answers.get(&q.id).or(q.default.as_ref());
        if let Some(Value::String(value)) = value {
            let canonical =
                crate::choices::interpret(&q.id, value, &q.options, q.diameter, q.depth)
                    .map_err(|e| Error::Invalid(e.to_string()))?;
            if canonical.starts_with("thread:") || canonical.starts_with("knurl:") {
                excluded.extend(q.faces);
            }
        }
        Ok(())
    };
    let mut turned = Vec::new();
    for f in ctx.features.iter().filter(|f| f.family == "turned_steps") {
        // Retained answers still describe the surface when PMI coverage hides its
        // question, or changes which feature leads a same-diameter question group.
        if answers.contains_key(&format!("diameter.fit:{}", f.id)) {
            consider(super::turned::turned_question(&[f], ctx)?)?;
        }
        if !f.faces()?.iter().any(|id| ctx.covered.contains(id)) {
            turned.push(f);
        }
    }
    let diameters = turned
        .iter()
        .map(|f| Ok(py::round(f.number("diameter")?, 6)))
        .collect::<Result<Vec<_>>>()?;
    for group in super::same(&turned, &diameters) {
        let group: Vec<_> = group.into_iter().copied().collect();
        consider(super::turned::turned_question(&group, ctx)?)?;
    }
    Ok(excluded)
}

pub(crate) fn defaults_with(ctx: &Ctx<'_>, answers: &Map<String, Value>) -> Result<DefaultDatums> {
    match turned_axis(ctx, answers)? {
        Some(axis) => turned_datums(&ctx.faces, axis, &recess_planes(ctx)?),
        None => default_datums(&ctx.faces, false),
    }
}

/// A recess's defining faces and end walls are poor axial stops. The recogniser's slot
/// faces may include its end walls; its long-axis interval also identifies omitted ends.
/// Also exclude blind-hole bottoms even when the recogniser lists only their bores.
fn recess_planes(ctx: &Ctx<'_>) -> Result<HashSet<i64>> {
    let mut ids = HashSet::new();
    for f in &ctx.features {
        if matches!(
            f.family,
            "slots" | "channels" | "pockets" | "section_recesses" | "holes"
        ) {
            for &id in f.faces()? {
                if ctx.faces.at(id)?.is_plane() {
                    ids.insert(id);
                }
            }
        }
        if f.family == "section_recesses"
            && let Some(evidence) = f.get("evidence")?
            && let Some(constituents) = evidence["constituent_faces"].as_array()
        {
            for id in constituents.iter().filter_map(Value::as_i64) {
                if ctx.faces.at(id)?.is_plane() {
                    ids.insert(id);
                }
            }
        }
        if matches!(f.family, "slots" | "channels") {
            let end_frame = if f.get("end_direction")?.is_some() {
                Some((
                    f.vector("end_direction")?,
                    [f.vector("end_lo")?, f.vector("end_hi")?],
                ))
            } else {
                crate::features::end_planes(f.family, f.record()?)
                    .map_err(|e| super::Error::Invalid(e.to_string()))?
            };
            if let Some((direction, ends)) = end_frame {
                for p in ctx.faces.iter().filter(|p| p.is_plane()) {
                    if parallel(&p.direction()?, &direction) {
                        let centroid = p.centroid()?;
                        if ends.iter().any(|end| {
                            let offset = std::array::from_fn(|i| centroid[i] - end[i]);
                            // Ordinary aligned endpoints can retain the 0.01-mm grid; converted
                            // endpoints are exact. Keep the legacy half-step allowance plus
                            // the 1e-6-mm geometry floor for the shared reader.
                            dot(&offset, &direction).abs() <= 0.005001
                        }) {
                            ids.insert(p.id);
                        }
                    }
                }
            }
        }
        if f.family == "holes"
            && f.get("bottom")?.and_then(Value::as_str) == Some("flat")
            && let Some(depth) = f.optional_number("depth")?
        {
            let (location, direction) = (f.vector("location")?, f.vector("axis")?);
            for p in ctx.faces.iter().filter(|p| p.is_plane()) {
                let c = p.centroid()?;
                let offset = [c[0] - location[0], c[1] - location[1], c[2] - location[2]];
                if parallel(&p.direction()?, &direction)
                    && (dot(&offset, &direction) - depth).abs() < 1e-4
                {
                    ids.insert(p.id);
                }
            }
        }
    }
    Ok(ids)
}

fn turned_datums(faces: &Faces, axis: &Face, recesses: &HashSet<i64>) -> Result<DefaultDatums> {
    let mut a = Vec::new();
    for face in faces.iter().filter(|f| f.is_cylinder()) {
        if coaxial(face, axis)? {
            a.push(face.id);
        }
    }
    a.sort_unstable();
    let mut shoulders = Vec::new();
    for face in planes_by_area(faces)? {
        if parallel(&face.direction()?, &axis.direction()?) {
            shoulders.push(face);
        }
    }
    let seats: Vec<_> = shoulders
        .iter()
        .copied()
        .filter(|f| !recesses.contains(&f.id))
        .collect();
    let seats = if seats.is_empty() && shoulders.len() == 1 {
        &shoulders
    } else {
        &seats
    };
    let b = match seats.first() {
        Some(&largest) => coplanar_ids(seats, resting_face(seats)?.unwrap_or(largest))?,
        None => Vec::new(),
    };
    Ok((a, b, Vec::new()))
}

fn coplanar_ids(planes: &[&Face], plane: &Face) -> Result<Vec<i64>> {
    let mut ids = Vec::new();
    for &p in planes {
        if coplanar(p, plane)? {
            ids.push(p.id);
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

/// A primary axis locates a coaxial hole without a transverse datum or a shoulder. Require
/// every bore face to lie on it, so an offset/parallel or cross hole receives no exemption.
pub(crate) fn axis_locates(bores: &[i64], datums: &Datums, faces: &Faces) -> Result<bool> {
    let Some(a) = datums.get('A').filter(|d| d.kind == DatumKind::Axis) else {
        return Ok(false);
    };
    if bores.is_empty() {
        return Ok(false);
    }
    let first = a
        .faces
        .first()
        .ok_or_else(|| Error::Invalid("datum A needs at least one face".into()))?;
    let axis = faces.at(*first)?;
    for &id in bores {
        let face = faces.at(id)?;
        if !face.is_cylinder() || !same_axis(face, axis)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `_resting_face`: in an assembly, the plane where the part meets another, the one with the
/// most contact area (the first of equals); None where no plane touches anything.
fn resting_face<'f>(planes: &[&'f Face]) -> Result<Option<&'f Face>> {
    let touching: Vec<&Face> = planes
        .iter()
        .copied()
        .filter(|p| py::truthy_opt(p.contact.as_ref()))
        .collect();
    let mut best: Option<(&Face, f64)> = None;
    for &plane in &touching {
        let mut area = 0.0;
        for p in &touching {
            if coplanar(p, plane)? {
                area += contact_area(p)?;
            }
        }
        // max() keeps the first of equal keys.
        if best.is_none_or(|(_, top)| area > top) {
            best = Some((plane, area));
        }
    }
    Ok(best.map(|(f, _)| f))
}

fn contact_area(face: &Face) -> Result<f64> {
    face.contact
        .as_ref()
        .and_then(|c| c.get("area"))
        .and_then(Value::as_f64)
        .ok_or_else(|| Error::Invalid(format!("face {}: its 'contact' has no 'area'", face.id)))
}

/// `_datum_a_doubt`: why the largest plane may not be the primary datum, when another plane
/// (coplanar faces together), facing elsewhere, is nearly as large.
pub(crate) fn datum_a_doubt(faces: &Faces) -> Result<String> {
    let mut groups: Vec<(&Face, f64)> = Vec::new();
    for f in planes_by_area(faces)? {
        let mut joined = false;
        for group in groups.iter_mut() {
            if coplanar(f, group.0)? {
                group.1 += f.area()?;
                joined = true;
                break;
            }
        }
        if !joined {
            groups.push((f, f.area()?));
        }
    }
    sort_by_area(&mut groups);
    // Both group indices are guarded by len() > 1.
    if groups.len() > 1 && groups[1].1 >= 0.8 * groups[0].1 {
        return Ok(format!(
            "face {} is nearly as large as face {}; the primary datum is whichever the part is \
             mounted on",
            // The enclosing branch checks groups.len() > 1.
            groups[1].0.id,
            groups[0].0.id
        ));
    }
    Ok(String::new())
}

/// `_datum_questions`: the letters in `propose`, A to C with their defaults (a letter beyond C
/// asked empty); any letter the person has given or removed, always. A file with datums has its
/// frame: only the letters it lacks are asked, and none is proposed.
pub(crate) fn datum_questions(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
    propose: &str,
) -> Result<Vec<Question>> {
    let by_dimensions = py::is_str(answers.get("part.locating"), super::LOCATING[1]);
    let why = if by_dimensions {
        "Datums: the part is measured from these faces or axes. Choose one face, several \
         coplanar faces, or a hole or boss (its axis); leave empty to remove a datum."
    } else {
        "Datums fix the part for measurement; tolerances refer to them. A datum is one \
         face, several coplanar faces, or a hole or boss (its axis)."
    };
    let turned = turned_axis(ctx, answers)?.is_some();
    let mut basis_a = if turned {
        "The axis of the plain turned seat with the greatest total cylindrical area; \
         diameter breaks ties. Threads and knurls are excluded from locating seats."
    } else {
        "The largest flat face: usually what the part sits on or is clamped by."
    }
    .to_string();
    let basis_b = if turned {
        "The locating face perpendicular to A: assembly contact first, otherwise the largest \
         area, excluding recess walls unless the only perpendicular face."
    } else {
        "The largest flat face square to A."
    };
    let basis_c = "The largest flat face square to A and B.";
    let (mut source_a, source_b): (&'static Rule, &'static Rule) = if turned {
        (&sources::DATUM_A_TURNED, &sources::DATUM_B_TURNED)
    } else {
        (&sources::DATUM_A_PRISMATIC, &sources::DATUM_B_PRISMATIC)
    };
    // Which letters have a default, and what it is.
    let mut defaults: Vec<(char, Vec<i64>)> = Vec::new();
    let mut resting = String::new();
    if !propose.is_empty() && ctx.in_file.is_empty() {
        let (a, b, c) = &ctx.default_datums_with(answers)?;
        for (x, d) in [('A', a), ('B', b), ('C', c)] {
            if propose.contains(x) {
                defaults.push((x, d.clone()));
            }
        }
        let mut contact = None;
        for &i in a {
            let face = ctx.faces.at(i)?;
            if py::truthy_opt(face.contact.as_ref()) {
                contact = face.contact.as_ref();
                break;
            }
        }
        if let Some(contact) = contact.filter(|_| !turned) {
            let part = contact
                .get("part")
                .ok_or_else(|| Error::Invalid("a face's 'contact' has no 'part'".to_string()))?;
            resting = format!(
                "it is where the part meets {}: what locates it there",
                py::str_of(part)
            );
            basis_a = py::sentence(&resting);
            source_a = &sources::DATUM_A_CONTACT;
        }
        for x in propose.chars() {
            if !"ABC".contains(x) && !defaults.iter().any(|(y, _)| *y == x) {
                defaults.push((x, Vec::new()));
            }
        }
    }
    let mut out = Vec::new();
    for letter in LETTERS {
        let known = defaults.iter().find(|(x, _)| *x == letter);
        let given = answers.contains_key(&format!("datum.{letter}"));
        if ctx.file_datum(letter).is_some()
            || (!given && (known.is_none() || after_none(ctx, answers, letter)))
        {
            continue;
        }
        let default = known.map(|(_, d)| d.clone()).unwrap_or_default();
        let prompt = if by_dimensions {
            format!(
                "Datums: where is the part measured from? Choose datum {letter}{} (empty for none)",
                if letter == 'A' { " (primary)" } else { "" }
            )
        } else if letter == 'A' {
            "Which face(s), hole or boss is datum A (primary)?".to_string()
        } else {
            format!("Which face(s), hole or boss is datum {letter}? (empty for none)")
        };
        let (basis, source) = match letter {
            _ if default.is_empty() => (String::new(), &sources::DATUM_NONE),
            'A' => (basis_a.clone(), source_a),
            'B' => (basis_b.to_string(), source_b),
            'C' => (basis_c.to_string(), &sources::DATUM_C_PRISMATIC),
            _ => (String::new(), &sources::DATUM_NONE),
        };
        out.push(Question {
            default: Some(Value::from(default.clone())),
            why: if "ABC".contains(letter) {
                why.to_string()
            } else {
                "An extra datum, for features located from something other than A.".to_string()
            },
            basis,
            source: Some(source),
            mate: if letter == 'A' {
                resting.clone()
            } else {
                String::new()
            },
            ..Question::new(format!("datum.{letter}"), "faces", prompt, default)
        });
    }
    Ok(out)
}

/// Whether an earlier datum the file lacks was decided none (#217): a datum without the one
/// before it makes no sense, so C is not proposed once B is none, nor B once A is. A letter
/// the person has given stays asked, so its answer is still shown and can be removed.
fn after_none(ctx: &Ctx<'_>, answers: &Map<String, Value>, letter: char) -> bool {
    LETTERS.iter().take_while(|&&l| l != letter).any(|&l| {
        ctx.file_datum(l).is_none()
            && answers
                .get(&format!("datum.{l}"))
                .is_some_and(|v| !py::truthy(v))
    })
}

/// `_datums`: each datum in letter order, the file's own first (one whose faces cannot be read
/// cannot be cited), then those the answers (or the questions' defaults) choose.
pub(crate) fn chosen_datums(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
    qs: &[Question],
) -> Result<Datums> {
    let mut out = Vec::new();
    for letter in LETTERS {
        if let Some(ids) = ctx.file_datum(letter) {
            if !ids.is_empty() {
                for &i in ids {
                    if !ctx.faces.contains(i) {
                        // Python's KeyError: the file's datum names a face the part lacks.
                        return invalid(format!(
                            "the file's datum {letter} is on face {i}, which the part does not have"
                        ));
                    }
                }
                // A datum whose faces form neither a plane nor an axis is not citable.
                if let Some(kind) = classify(ids, &ctx.faces)? {
                    out.push((
                        letter,
                        Datum {
                            faces: ids.to_vec(),
                            kind,
                            existing: true,
                        },
                    ));
                }
            }
            continue;
        }
        let qid = format!("datum.{letter}");
        let Some(chosen) = super::answer(answers, qs, &qid) else {
            continue;
        };
        if !py::truthy(chosen) {
            continue;
        }
        let Some(items) = py::iter_of(chosen) else {
            // Python's TypeError: a number is not a list of faces.
            return invalid(format!(
                "datum {letter}: {} is not a list of faces",
                py::repr(chosen)
            ));
        };
        let mut ids = Vec::with_capacity(items.len());
        for item in &items {
            let id = py::int_of(item)
                .map_err(|e| Error::Invalid(format!("datum {letter}: {}", e.message())))?;
            if !ctx.faces.contains(id) {
                return invalid(format!(
                    "datum {letter}: face {} does not exist",
                    py::str_of(item)
                ));
            }
            ids.push(id);
        }
        let kind = kind_of(&ids, &items, &ctx.faces).map_err(|e| match e {
            Error::Invalid(m) => Error::Invalid(format!("datum {letter}: {m}")),
            other => other,
        })?;
        ids.sort_unstable();
        out.push((
            letter,
            Datum {
                faces: ids,
                kind,
                existing: false,
            },
        ));
    }
    Ok(Datums(out))
}

/// The selected planar measurement origins, in datum precedence order. Removed datums
/// contribute no origin; cylindrical datums are axes, not substitute edge planes.
pub(crate) fn planes(datums: &Datums) -> Vec<Vec<i64>> {
    datums
        .iter()
        .filter(|(_, d)| d.kind == DatumKind::Plane)
        .map(|(_, d)| d.faces.clone())
        .collect()
}

/// The tolerance that relates datum B or C to the datums before it (`_datum_control`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    /// `perpendicularity` or `position`.
    pub kind: &'static str,
    pub datums: Vec<&'static str>,
    pub diametral: bool,
}

/// `_datum_control`: a tertiary hole is located from A|B; otherwise the feature is held square
/// to the earlier datums it is square to. None when there is no characteristic for the
/// geometry (a feature parallel to A would need parallelism).
pub fn datum_control(letter: char, datums: &Datums, faces: &Faces) -> Result<Option<Control>> {
    if !"BC".contains(letter) || !datums.contains(letter) || !datums.contains('A') {
        return Ok(None);
    }
    if letter == 'C' && !datums.contains('B') {
        return Ok(None);
    }
    // The entry checks datums.contains(letter), and datums is not mutated.
    let datum = datums.get(letter).expect("checked above");
    let axis = datum.kind == DatumKind::Axis;
    if letter == 'C' && axis {
        return Ok(Some(Control {
            kind: "position",
            datums: vec!["A", "B"],
            diametral: true,
        }));
    }
    let candidates: &[(char, &'static str)] = if letter == 'B' {
        &[('A', "A")]
    } else {
        &[('A', "A"), ('B', "B")]
    };
    let mut refs = Vec::new();
    for &(r, name) in candidates {
        // A is checked at entry; the C branch also checks B before selecting candidates.
        let other = datums.get(r).expect("checked above");
        if square(datum, other, faces)? {
            refs.push(name);
        }
    }
    if refs.first() != Some(&"A") {
        return Ok(None);
    }
    Ok(Some(Control {
        kind: "perpendicularity",
        datums: refs,
        diametral: axis,
    }))
}

/// `_square`: planes by their normals; an axis to a plane when it runs along the normal.
fn square(feature: &Datum, datum: &Datum, faces: &Faces) -> Result<bool> {
    let first = |d: &Datum| -> Result<[f64; 3]> {
        let Some(&i) = d.faces.first() else {
            return invalid("a datum needs at least one face");
        };
        faces.at(i)?.direction()
    };
    let (u, v) = (first(feature)?, first(datum)?);
    Ok(if feature.kind == datum.kind {
        perpendicular(&u, &v)
    } else {
        parallel(&u, &v)
    })
}

/// `_datum_control_questions`: how well each datum feature must be made (flatness of A, the
/// control of B and C); not asked of a datum the file already tolerances.
pub(crate) fn datum_control_questions(ctx: &Ctx<'_>, datums: &Datums) -> Result<Vec<Question>> {
    let zones = || {
        [NONE, "0.01", "0.02", "0.05", "0.1"]
            .map(String::from)
            .to_vec()
    };
    let mut out = Vec::new();
    if let Some(a) = datums.get('A')
        && a.kind == DatumKind::Plane
        && !a.faces.iter().any(|i| ctx.toleranced.contains(i))
    {
        out.push(Question {
            default: Some(Value::from(NONE)),
            options: zones(),
            characteristic: Some(Characteristic::Flatness),
            source: Some(&sources::DATUM_CONTROL_NONE),
            why: "Datum A is usually the face the part is clamped or mounted on.".to_string(),
            ..Question::new(
                "datum.A.flatness".to_string(),
                "choice",
                "Flatness required on datum A?".to_string(),
                a.faces.clone(),
            )
        });
    }
    for letter in ['B', 'C'] {
        let Some(control) = datum_control(letter, datums, &ctx.faces)? else {
            continue;
        };
        // datum_control returned Some only after checking this datum exists; datums is immutable.
        let datum = datums.get(letter).expect("a control has its datum");
        if datum.faces.iter().any(|i| ctx.toleranced.contains(i)) {
            continue;
        }
        let prompt = format!(
            "{} {}required on datum {letter}, relative to {}?",
            capitalize(control.kind),
            if control.diametral { "(Ø) " } else { "" },
            control.datums.join("|")
        );
        out.push(Question {
            default: Some(Value::from(NONE)),
            options: zones(),
            characteristic: Some(if control.kind == "position" {
                Characteristic::Position
            } else {
                Characteristic::Perpendicularity
            }),
            source: Some(&sources::DATUM_CONTROL_NONE),
            why: "Orients (and for a tertiary hole, locates) the datum feature, so the frame it \
                  builds is repeatable."
                .to_string(),
            ..Question::new(
                format!("datum.{letter}.control"),
                "choice",
                prompt,
                datum.faces.clone(),
            )
        });
    }
    Ok(out)
}

/// Python's `str.capitalize` of an ASCII word.
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => format!(
            "{}{}",
            first.to_ascii_uppercase(),
            chars.as_str().to_ascii_lowercase()
        ),
        None => String::new(),
    }
}

/// `_frames`: the frames to offer, the primary frame then each extra datum in A's place; none
/// without A (a position needs a primary datum).
pub(crate) fn frames(letters: &[char]) -> Vec<String> {
    if !letters.contains(&'A') {
        return Vec::new();
    }
    let primary: Vec<char> = letters
        .iter()
        .copied()
        .filter(|x| "ABC".contains(*x))
        .collect();
    let join = |xs: &[char]| xs.iter().map(char::to_string).collect::<Vec<_>>().join("|");
    let mut out = vec![join(&primary)];
    for &x in letters {
        if EXTRA_DATUMS.contains(x) {
            let mut frame = vec![x];
            frame.extend_from_slice(&primary[1..]);
            out.push(join(&frame));
        }
    }
    out
}

/// `_datum_of`: the axis datum `bores` form, if the hole was chosen as one.
pub(crate) fn datum_of(bores: &[i64], datums: &Datums) -> Option<char> {
    if bores.is_empty() {
        return None;
    }
    datums
        .iter()
        .find(|(_, d)| d.kind == DatumKind::Axis && bores.iter().all(|i| d.faces.contains(i)))
        .map(|(x, _)| *x)
}

/// `_hole_frames`: the frames a hole can be positioned from. A datum hole whose control
/// locates or orients it gets none; one without a control keeps its position, from the frames
/// with its own letter taken out.
pub(crate) fn hole_frames(
    bores: &[i64],
    datums: &Datums,
    controls: &[char],
    frames: &[String],
) -> Vec<String> {
    let Some(letter) = datum_of(bores, datums) else {
        return frames.to_vec();
    };
    if controls.contains(&letter) {
        return Vec::new();
    }
    let own = letter.to_string();
    let mut out: Vec<String> = Vec::new();
    for frame in frames {
        let kept: Vec<&str> = frame.split('|').filter(|x| *x != own).collect();
        let kept = kept.join("|");
        if !kept.is_empty() && !out.contains(&kept) {
            out.push(kept);
        }
    }
    out
}

#[cfg(test)]
mod area_tests {
    use super::sort_by_area;

    #[test]
    fn area_ties_cross_rounding_boundaries_and_scale_with_area() {
        for (a, b) in [
            (1.00000049, 1.00000051),
            (1000.0, 1000.0009),
            (0.0, 0.0000009),
        ] {
            let mut faces = [(0, a), (1, b)];
            sort_by_area(&mut faces);
            assert_eq!(faces.map(|(id, _)| id), [0, 1]);
        }
        let mut faces = [(0, 1.0), (1, 1.000002)];
        sort_by_area(&mut faces);
        assert_eq!(faces.map(|(id, _)| id), [1, 0]);
    }
}
