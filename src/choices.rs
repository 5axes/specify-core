//! What a choice means and what it puts on the drawing, and typed specs read as choices (port of
//! `choices.py`).
//!
//! A hole or turned diameter is asked what it is *for*: a bolt passes, a screw threads in, a pin
//! fits. Each purpose holds the codes an engineer would name (M6, H7). [`describe`] gives every
//! option its purpose, a plain label, its code and its callout, so a person can choose by
//! either. [`interpret`] reads what someone typed ("h6", "M2", "clearance M6", "0.15") against
//! the feature's geometry and returns the answer it means, or refuses with the reason: a typed
//! spec may be anything, but it is only written if it fits the part. Nothing typed is dropped: a
//! tapped hole may be given its full-thread depth ("M5 tapped, 5.5 deep", kept as
//! `tapped:M5@5.5`), and any other number is refused, not ignored.
//!
//! Messages are Python's byte for byte: they are shown to people and the oracle compares them.
//! The regular expressions are Python's, with its Unicode classes spelled out for the regex
//! crate: `\d` is `\p{Nd}`, `\w` is `[\p{L}\p{N}_]` and `\s` is `\s` plus U+001C to U+001F (each
//! checked against Python's over every character in `tests/pyfmt.rs`). `fullmatch` is anchored
//! at both ends, `match` at the start, `search` not at all.

// Capture indexing below follows successful matches with mandatory groups; optional groups use get().
use std::fmt;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use crate::pyfmt::{self, fixed, fmt3, g, repr_str};
use crate::standards::{self, PITCH, TAP_DRILL};

pub const GENERAL: &str = "general";
pub const NONE: &str = "none";

/// Purposes, in the order they are offered, for holes and for turned diameters.
pub const HOLE_PURPOSES: [(&str, &str); 4] = [
    ("clearance", "A bolt passes through"),
    ("tapped", "A screw threads into it"),
    ("fit", "A pin, shaft or bearing fits in it"),
    (GENERAL, "Nothing in particular"),
];
pub const TURNED_PURPOSES: [(&str, &str); 4] = [
    ("fit", "It fits into a bore or bearing"),
    ("thread", "It is threaded"),
    ("knurl", "It is knurled for grip"),
    (GENERAL, "Nothing in particular"),
];

/// A typed spec that cannot be read, or that does not fit the feature. Its text is Python's
/// `SpecError` message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecError(pub String);

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SpecError {}

/// A refusal with Python's message.
fn refuse<T>(message: impl Into<String>) -> Result<T, SpecError> {
    Err(SpecError(message.into()))
}

/// Each option of a hole or turned-diameter question, for choosing by purpose or by code:
/// `{value, purpose, label, code, callout}`; empty for any other question. The error is what
/// Python raises for an option that is not a known answer (a `KeyError`'s text, e.g. `'M7'`,
/// or [`standards::fit_limits`]'s message).
pub fn describe<S: AsRef<str>>(
    qid: &str,
    options: &[S],
    diameter: Option<f64>,
) -> Result<Vec<Value>, String> {
    let Some(d) = diameter else {
        return Ok(Vec::new());
    };
    if !(qid.starts_with("hole.function:") || qid.starts_with("diameter.fit:")) {
        return Ok(Vec::new());
    }
    let hole = qid.starts_with("hole.function:");
    let purposes = if hole {
        &HOLE_PURPOSES
    } else {
        &TURNED_PURPOSES
    };
    options
        .iter()
        .map(|value| {
            let value = value.as_ref();
            let kind = kind(value, hole);
            let label = purposes
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|&(_, label)| label)
                .ok_or_else(|| repr_str(kind))?;
            Ok(json!({
                "value": value,
                "purpose": kind,
                "label": label,
                "code": code(value),
                "callout": callout(value, d, hole)?,
            }))
        })
        .collect()
}

/// An answer's purpose, its spec and, for a tapped hole given one, the minimum full thread:
/// `tapped:M5@5.5` is `("tapped", "M5", Some(5.5))`. A depth that is not a number is `None`
/// ([`check`] says so).
pub fn split(value: &str) -> (&str, &str, Option<f64>) {
    let (kind, spec) = value.split_once(':').unwrap_or((value, ""));
    match spec.split_once('@') {
        Some((spec, depth)) => (kind, spec, pyfmt::parse_float(depth)),
        None => (kind, spec, None),
    }
}

/// Resolve a blind tapped hole's implicit full-thread depth for display. Stored choice
/// values stay canonical (`tapped:M2`); their code and callout can show the actual computed
/// minimum, just as an explicit `tapped:M2@2.3` does. `None` means through or unknown depth.
pub fn with_default_full_thread(value: &str, drill_depth: Option<f64>) -> String {
    let (kind, size, full) = split(value);
    if kind == "tapped"
        && given(full).is_none()
        && let Some(depth) = drill_depth
        && standards::lookup(&PITCH, size).is_some()
    {
        return format!(
            "tapped:{size}@{}",
            fmt3(standards::full_thread(depth, size))
        );
    }
    value.to_string()
}

/// Python's truth of an optional depth: present and not zero (nan is true).
fn given(depth: Option<f64>) -> Option<f64> {
    depth.filter(|&d| d != 0.0)
}

/// An answer as an engineer would name it: `H7`, `M6 clearance`, `M5 tapped`.
pub fn code(value: &str) -> String {
    let (kind, spec, depth) = split(value);
    if value == GENERAL {
        return "general tolerance".to_string();
    }
    match kind {
        "fit" => spec.to_string(),
        "clearance" if spec.is_empty() => "clearance".to_string(),
        "clearance" => format!("{spec} clearance"),
        "tapped" => match given(depth) {
            Some(depth) => format!("{spec} tapped, {} mm full thread", fmt3(depth)),
            None => format!("{spec} tapped"),
        },
        // An ISO 261 fine pitch is named as written on a drawing: `M10x1.25` is M10×1.25.
        "thread" if spec.contains('x') && standards::external_thread(spec).is_some() => {
            format!("{} thread", spec.replace('x', "×"))
        }
        "thread" => format!("{spec} thread"),
        "knurl" => format!("{spec} knurl"),
        _ => value.to_string(),
    }
}

/// What the answer puts on the drawing for a feature of diameter `d`. The error is what Python
/// raises for an answer that is not a known one: a `KeyError`'s text (`'M7'`) for an untabulated
/// thread, or [`standards::fit_limits`]'s message. Whether the feature is a hole does not change
/// the callout (the value's kind says which fit); the flag is kept as in Python's signature.
pub fn callout(value: &str, d: f64, _hole: bool) -> Result<String, String> {
    let (kind, spec, depth) = split(value);
    let pitch = || standards::lookup(&PITCH, spec).ok_or_else(|| repr_str(spec));
    if value == GENERAL {
        return Ok(format!("⌀{}, general tolerance", fmt3(d)));
    }
    match kind {
        "tapped" => {
            let pitch = pitch()?;
            let full = given(depth)
                .map(|depth| format!(", {} mm min full thread", fmt3(depth)))
                .unwrap_or_default();
            let class = standards::INTERNAL_THREAD_CLASS;
            return Ok(format!(
                "{spec}×{}-{class}{full}, ⌀{} tap drill",
                g(pitch),
                fmt3(d)
            ));
        }
        "thread" => {
            // A turned thread may be any ISO 261 size or fine pitch (stream S3): `M10x1.25`
            // is called up as M10×1.25-6g. Anything else is refused as Python refuses it.
            let (spec, pitch) = match standards::external_thread(spec) {
                Some(found) => found,
                None => (spec, pitch()?),
            };
            return Ok(format!(
                "{spec}×{}-{}",
                g(pitch),
                standards::EXTERNAL_THREAD_CLASS
            ));
        }
        "knurl" => {
            return Ok(format!(
                "{} knurl, {} mm pitch",
                capitalize(spec),
                g(standards::KNURL_PITCH)
            ));
        }
        _ => {}
    }
    let fit = match kind {
        "clearance" => "H11",
        "fit" => spec,
        _ => value,
    };
    let (upper, lower) = standards::fit_limits(fit, d)?;
    let limits = format!("{}–{}", fixed(d + lower, 3), fixed(d + upper, 3));
    let tail = if kind == "clearance" && !spec.is_empty() {
        format!(" for {spec}")
    } else {
        String::new()
    };
    Ok(format!("⌀{} {fit} ({limits}){tail}", fmt3(d)))
}

/// The answer `text` means for question `qid`; `options` are those offered, `diameter` the
/// feature's, if it has one, and `depth` a blind hole's drill depth, against which a thread depth
/// is checked.
pub fn interpret<S: AsRef<str>>(
    qid: &str,
    text: &str,
    options: &[S],
    diameter: Option<f64>,
    depth: Option<f64>,
) -> Result<String, SpecError> {
    let typed = pyfmt::squash_spaces(text);
    if typed.is_empty() {
        return refuse("Type a spec, e.g. H7 or M6.");
    }
    // On a shaft, case says which is meant: M6 is a thread, m6 a fit.
    let shaft = qid.starts_with("diameter.fit:");
    let fold = |s: &str| {
        if shaft {
            s.to_string()
        } else {
            s.to_lowercase()
        }
    };
    let folded = fold(&typed);
    for option in options {
        let option = option.as_ref();
        if folded == fold(option) || folded == fold(&code(option)) {
            return Ok(option.to_string());
        }
    }
    if let Some(d) = diameter {
        if qid.starts_with("hole.function:") {
            return hole(&typed, d, depth);
        }
        if shaft {
            return turned(&typed, d);
        }
    }
    let joined = || {
        options
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if qid.starts_with("hole.position:")
        || qid.starts_with("diameter.runout:")
        || qid.ends_with(".control")
        || qid.ends_with(".flatness")
    {
        return zone(&typed);
    }
    if qid.starts_with("hole.location:") {
        let zone = zone(&typed.replace("+-", "").replace('±', ""))?;
        return Ok(if zone == NONE {
            GENERAL.to_string()
        } else {
            format!("±{zone}")
        });
    }
    if qid.starts_with("hole.frame:") {
        let letters = typed
            .chars()
            .filter(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_uppercase().to_string())
            .collect::<Vec<_>>()
            .join("|");
        if options.iter().any(|o| o.as_ref() == letters) {
            return Ok(letters);
        }
        return refuse(format!("Measure from one of: {}.", joined()));
    }
    if qid == "part.general_tolerance" {
        static GRADE: LazyLock<Regex> =
            LazyLock::new(|| pattern(r"^(?:iso{S}*)?(?:2768{S}*-?{S}*)?([fmcv])([hkl])?$"));
        if let Some(grade) = GRADE.captures(&typed.to_lowercase()) {
            // GRADE has a mandatory capture group 1.
            let named = format!(
                "ISO 2768-{}{}",
                &grade[1],
                grade
                    .get(2)
                    .map_or(String::new(), |g| g.as_str().to_ascii_uppercase())
            );
            if options.iter().any(|o| o.as_ref() == named) {
                return Ok(named);
            }
        }
    }
    refuse(format!("Choose one of: {}.", joined()))
}

/// Refuse an answer that is neither offered nor a spec that fits the feature. `value` is the
/// answer as JSON: answers can be any JSON type, and a choice's are offered or typed text.
pub fn check<S: AsRef<str>>(
    qid: &str,
    value: &Value,
    options: &[S],
    diameter: Option<f64>,
    depth: Option<f64>,
) -> Result<(), SpecError> {
    let Value::String(value) = value else {
        return refuse(format!(
            "{qid}: an answer is text, not {} {}.",
            type_name(value),
            short_repr(value, 6)
        ));
    };
    if options.iter().any(|o| o.as_ref() == value) {
        return Ok(());
    }
    let meant = interpret(qid, value, options, diameter, depth)?;
    if meant != *value {
        return refuse(format!(
            "{} is not an answer; it reads as {}.",
            repr_str(value),
            repr_str(&meant)
        ));
    }
    Ok(())
}

/// A tolerance zone typed in mm (`0.15`, `⌀0.1`), or [`NONE`]; Python's `_zone`, which the
/// added requirements (`extras`) use too.
pub fn zone(typed: &str) -> Result<String, SpecError> {
    let lower = typed.to_lowercase();
    let low = pyfmt::strip(lower.trim_start_matches(['⌀', 'ø']));
    if NONE_WORDS.contains(&low) {
        return Ok(NONE.to_string());
    }
    let Some(value) = pyfmt::parse_float(low) else {
        return refuse("Type a tolerance in mm, e.g. 0.1, or none.");
    };
    if !(0.0 < value && value <= 10.0) {
        return refuse("A tolerance zone is more than 0 and at most 10 mm.");
    }
    Ok(fmt3(value))
}

/// A fit tabulated at diameter `d`, from its letter(s) and grade digits (`"g"`, `"6"` is `g6`;
/// `"H"`, `"07"` is `H7`); Python's `_fit`, which the added requirements use too. Python's
/// `hole` keyword is not used there, so it is not taken here.
pub fn fit(letter: &str, grade: &str, d: f64) -> Result<String, SpecError> {
    // int(grade): the grade is the one or two decimal digits a pattern matched.
    let grade = pyfmt::parse_digits(grade)
        .ok_or_else(|| SpecError(format!("invalid fit grade {}", repr_str(grade))))?;
    let fit = format!("{letter}{grade}");
    match standards::fit_limits(&fit, d) {
        Ok(_) => Ok(fit),
        Err(exc) => refuse(format!("{fit} is not tabulated here ({exc}).")),
    }
}

// --------------------------------------------------------------------------

/// Python's `\s` (and `str.isspace`), for the regex crate.
const SPACE: &str = r"[\s\x1C-\x1F]";
/// Python's `\d` on a str pattern: any Unicode decimal digit.
const DIGIT: &str = r"\p{Nd}";
/// Python's `\w` on a str pattern: `str.isalnum()` or `_`.
const WORD: &str = r"\p{L}\p{N}_";

/// A pattern written with `{S}`, `{D}` and `{W}` for Python's `\s`, `\d` and `\w` (inside a
/// class, `{W}` is the class body).
fn pattern(text: &str) -> Regex {
    let text = text
        .replace("{S}", SPACE)
        .replace("{D}", DIGIT)
        .replace("{W}", WORD);
    // Only source literals and the constant S/D/W character classes reach this compiler.
    Regex::new(&text).expect("valid pattern")
}

/// `_SIZE`, used with `re.I`: a metric size with an optional pitch (`M6`, `m2x0.4`, `M2 × 0.4`).
static SIZE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)M({D}+(?:\.{D}+)?)(?:{S}*[x×]{S}*({D}+(?:\.{D}+)?))?"));
/// Python's `re.fullmatch(r"(?:fit\s*:?\s*)?[a-z]{1,2}\d{1,2}", low)`: something like a fit.
static FIT_LIKE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(?:fit{S}*:?{S}*)?[a-z]{1,2}{D}{1,2}$"));
/// Python's `re.findall(r"[a-z]+", rest)`.
static LETTERS: LazyLock<Regex> = LazyLock::new(|| pattern(r"[a-z]+"));

const NONE_WORDS: [&str; 6] = ["general", "none", "nothing", "no", "plain", "gen"];
/// Words that say a number is the thread's depth: "5.5 deep", "full thread 5.5".
const DEPTH_WORDS: [&str; 7] = ["deep", "depth", "full", "usable", "long", "length", "min"];
const UNITS: [&str; 6] = ["cm", "m", "in", "inch", "inches", "thou"];

/// Python's `text.startswith(_DEPTH_WORDS)`.
fn says_depth(text: &str) -> bool {
    DEPTH_WORDS.iter().any(|w| text.starts_with(w))
}

/// A typed hole spec: a fit, a thread or a bolt.
fn hole(typed: &str, d: f64, depth: Option<f64>) -> Result<String, SpecError> {
    static FIT: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^(?:fit{S}*:?{S}*)?(js|h)({D}{1,2})$"));
    let low = typed.to_lowercase();
    if NONE_WORDS.contains(&low.as_str()) {
        return Ok(GENERAL.to_string());
    }
    if let Some(found) = FIT.captures(&low) {
        return Ok(format!(
            "fit:{}",
            // FIT has two mandatory captures in this successful match.
            fit(&found[1].to_uppercase(), &found[2], d)?
        ));
    }
    let rest = SIZE.replacen(typed, 1, " ").to_lowercase();
    let words: Vec<&str> = LETTERS.find_iter(&rest).map(|m| m.as_str()).collect();
    let Some(size) = SIZE.captures(typed) else {
        if FIT_LIKE.is_match(&low) {
            return refuse("Holes take an H (or JS) fit, e.g. H7.");
        }
        return refuse("Type a fit (H7), a thread (M5) or a bolt (clearance M6).");
    };
    // SIZE always captures a nominal; pitch group 2 is accessed with get().
    let bolt = bolt(&size[1], size.get(2).map(|m| m.as_str()))?;
    let any_word = |prefixes: [&str; 3]| {
        words
            .iter()
            .any(|w| prefixes.iter().any(|p| w.starts_with(p)))
    };
    let clearance = any_word(["clear", "bolt", "pass"]);
    let tapped = any_word(["tap", "thread", "screw"]);
    if clearance && tapped {
        return refuse("A hole is either tapped or a clearance hole, not both.");
    }
    // bolt() returns a PITCH key; TAP_DRILL has the same constant keys.
    let drill = standards::lookup(&TAP_DRILL, bolt).expect("a tabulated bolt");
    let fits_tap = (drill - d).abs() <= 0.1;
    let nominal = standards::bolt_diameter(bolt);
    // bolt() returns a PITCH key; CLEARANCE_COARSE has the same constant keys.
    let coarse = standards::lookup(&standards::CLEARANCE_COARSE, bolt).expect("a tabulated bolt");
    let fits_bolt = 0.0 < d - nominal && d - nominal <= 2.0 * (coarse - nominal);
    if tapped || (!clearance && fits_tap) {
        if !fits_tap {
            return refuse(format!(
                "An {bolt} tap needs a ⌀{} drill; this hole is ⌀{}.",
                fmt3(drill),
                fmt3(d)
            ));
        }
        let full = thread_depth(&rest, &words, bolt, depth)?;
        return Ok(match full {
            Some(full) => format!("tapped:{bolt}@{}", fmt3(full)),
            None => format!("tapped:{bolt}"),
        });
    }
    if !lengths(&rest)?.is_empty() {
        return refuse(format!(
            "Only a tapped hole takes a depth here; {} has none.",
            code(&format!("clearance:{bolt}"))
        ));
    }
    if !fits_bolt {
        if d <= nominal {
            return refuse(format!("An {bolt} bolt will not pass a ⌀{} hole.", fmt3(d)));
        }
        // bolt() returns a PITCH key; CLEARANCE_FINE has the same constant keys.
        let fine = standards::lookup(&standards::CLEARANCE_FINE, bolt).expect("a tabulated bolt");
        return refuse(format!(
            "⌀{} is too loose to be a clearance hole for {bolt} (ISO 273: ⌀{}–{}).",
            fmt3(d),
            fmt3(fine),
            fmt3(coarse)
        ));
    }
    Ok(format!("clearance:{bolt}"))
}

/// The lengths in what was typed besides the thread size, refusing any other number: a thread
/// class other than the one written, a unit other than mm, or a number that cannot be read.
fn lengths(rest: &str) -> Result<Vec<f64>, SpecError> {
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| pattern(r"[{W}.@-]+"));
    static NUMBER_START: LazyLock<Regex> = LazyLock::new(|| pattern(r"^-?\.?{D}"));
    static CLASS: LazyLock<Regex> = LazyLock::new(|| pattern(r"^-?{D}[a-z]$"));
    static NUMBER: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^(-?(?:{D}+(?:\.{D}*)?|\.{D}+))([a-z]*)$"));
    // A standard named (ISO 273) is not a length; a code led by a letter (H11) is a word.
    let cleaned = without_standards(rest);
    let tokens: Vec<&str> = TOKEN.find_iter(&cleaned).map(|m| m.as_str()).collect();
    let mut out = Vec::new();
    for (i, token) in tokens.iter().map(|t| t.trim_start_matches('@')).enumerate() {
        if !NUMBER_START.is_match(token) {
            continue;
        }
        if CLASS.is_match(token) {
            // A thread class: M5-6H.
            if token.trim_start_matches('-').to_uppercase() != standards::INTERNAL_THREAD_CLASS {
                return refuse(format!(
                    "Only the {} class is written.",
                    standards::INTERNAL_THREAD_CLASS
                ));
            }
            continue;
        }
        let Some(number) = NUMBER.captures(token) else {
            return refuse(format!("{token} is not read."));
        };
        let joined = number.get(2).map_or("", |m| m.as_str());
        // A unit is joined to the number or the next token (Python's, before '@' is stripped).
        let unit = if joined.is_empty() {
            tokens.get(i + 1).copied().unwrap_or("")
        } else {
            joined
        };
        if UNITS.contains(&unit) {
            return refuse(format!(
                "Lengths are in mm; {} {unit} is not read.",
                // NUMBER has a mandatory numeric capture group 1.
                &number[1]
            ));
        }
        let tail = joined.strip_prefix("mm").unwrap_or(joined);
        if !tail.is_empty() && !says_depth(tail) {
            return refuse(format!("{token} is not read."));
        }
        // NUMBER has a mandatory numeric capture group 1.
        out.push(finite_number(&number[1])?);
    }
    Ok(out)
}

/// Python's `re.sub(r"\biso\s*\d+(?:-\d+)?", " ", rest)`. The regex crate's `\b` is its own
/// `\w`'s boundary, so Python's is checked here: a match counts only where the character before
/// it is not a word character (the match itself starts with one, `i`).
fn without_standards(rest: &str) -> String {
    static ISO: LazyLock<Regex> = LazyLock::new(|| pattern(r"iso{S}*{D}+(?:-{D}+)?"));
    static WORD_CHAR: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[{W}]$"));
    let is_word = |c: char| WORD_CHAR.is_match(c.encode_utf8(&mut [0; 4]));
    let mut out = String::with_capacity(rest.len());
    let (mut copied, mut from) = (0, 0);
    while let Some(m) = ISO.find_at(rest, from) {
        if rest[..m.start()].chars().next_back().is_some_and(is_word) {
            from = m.start() + 1; // 'i' is one byte; try the next position
            continue;
        }
        out.push_str(&rest[copied..m.start()]);
        out.push(' ');
        copied = m.end();
        from = m.end();
    }
    out.push_str(&rest[copied..]);
    out
}

/// The minimum full thread asked for, if one was, checked against the hole.
fn thread_depth(
    rest: &str,
    words: &[&str],
    bolt: &str,
    depth: Option<f64>,
) -> Result<Option<f64>, SpecError> {
    let lengths = lengths(rest)?;
    let Some(&full) = lengths.first() else {
        return Ok(None);
    };
    let example = format!("e.g. {bolt} tapped, 5 mm full thread");
    if lengths.len() > 1 {
        return refuse(format!("Give one thread depth, {example}."));
    }
    if !rest.contains('@')
        && !words
            .iter()
            .any(|w| says_depth(w.strip_prefix("mm").unwrap_or(w)))
    {
        return refuse(format!(
            "{} mm is not read: only a thread depth can be added, {example}.",
            fmt3(full)
        ));
    }
    check_thread_depth(full, bolt, depth)?;
    Ok(Some(full))
}

/// A full-thread text question's answer, in mm. Use the same limits as an inline tapped
/// answer; no rounding is applied to a designer's value.
pub fn full_thread_depth(text: &str, bolt: &str, depth: f64) -> Result<f64, SpecError> {
    let text = pyfmt::strip(text);
    let text = pyfmt::strip(text.strip_suffix("mm").unwrap_or(text));
    let full = pyfmt::parse_float(text)
        .filter(|n| n.is_finite())
        .ok_or_else(|| SpecError("Give a full-thread depth in mm, e.g. 6 or 6 mm.".into()))?;
    check_thread_depth(full, bolt, Some(depth))?;
    Ok(full)
}

fn check_thread_depth(full: f64, bolt: &str, depth: Option<f64>) -> Result<(), SpecError> {
    let Some(depth) = depth else {
        return refuse(
            "A thread depth is only set on a blind hole of known depth; \
             this hole is through or its depth is not known.",
        );
    };
    if full <= 0.0 {
        return refuse("A thread depth is more than 0 mm.");
    }
    let pitch = standards::lookup(&PITCH, bolt)
        .ok_or_else(|| SpecError(format!("Unknown tapped thread size {bolt}.")))?;
    let most = depth - 3.0 * pitch;
    if full > most + 1e-9 {
        return refuse(format!(
            "{} mm of full thread does not fit this {} mm deep hole: an {bolt} tap's lead and \
             the drill point take 3 pitches ({} mm), so at most {} mm.",
            fmt3(full),
            fmt3(depth),
            fmt3(3.0 * pitch),
            fmt3(most)
        ));
    }
    Ok(())
}

/// A typed turned-diameter spec: a fit, a thread or a knurl.
fn turned(typed: &str, d: f64) -> Result<String, SpecError> {
    static FIT: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^(?:fit{S}*:?{S}*)?(js|[fghkmnp])({D}{1,2})$"));
    static THREAD: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(?:thread{S}*:?{S}*)?M{D}"));
    let low = typed.to_lowercase();
    if NONE_WORDS.contains(&low.as_str()) {
        return Ok(GENERAL.to_string());
    }
    // Capital M is a thread (M6), lower case m a fit (m6).
    if let Some(found) = FIT.captures(&low)
        && !THREAD.is_match(typed)
    {
        // FIT has two mandatory captures in this successful match.
        return fit(&found[1], &found[2], d);
    }
    if low.contains("knurl") || low == "diamond" || low == "straight" {
        let Some(pattern) = standards::KNURL_PATTERNS.iter().find(|p| low.contains(*p)) else {
            return refuse("Say a straight or a diamond knurl.");
        };
        return Ok(format!("knurl:{pattern}"));
    }
    let Some(size) = SIZE.captures(typed) else {
        if FIT_LIKE.is_match(&low) {
            return refuse("Shafts take f, g, h, js, k, m, n or p fits, e.g. g6.");
        }
        return refuse("Type a fit (g6), a thread (M6) or a knurl (diamond knurl).");
    };
    let pitch = size.get(2).map(|m| m.as_str());
    // A size or pitch of ISO 261 beyond Python's coarse table is a thread too (stream S3):
    // `M10x1.25` reads as that fine thread, `M30` as the coarse M30×3.5. Only a thread of this
    // diameter's size departs from Python; any other is refused with Python's message.
    // SIZE always captures a nominal; pitch group 2 is accessed with get().
    if let Some(spec) = iso_261(&size[1], pitch, d)? {
        return Ok(format!("thread:{spec}"));
    }
    // SIZE always captures a nominal; pitch group 2 is accessed with get().
    let bolt = bolt(&size[1], pitch)?;
    let nominal = standards::bolt_diameter(bolt);
    if (nominal - d).abs() > 0.2 {
        return refuse(format!(
            "An {bolt} thread is ⌀{}; this is ⌀{}.",
            fmt3(nominal),
            fmt3(d)
        ));
    }
    Ok(format!("thread:{bolt}"))
}

/// The ISO 261 answer spec a typed nominal (and pitch, if typed) names: the bare size
/// for its coarse pitch, `M10x1.25` for a fine one. `None` for a size ISO 261 does not list, or
/// not of diameter `d`, left to [`bolt`]'s refusal; a size of diameter `d` with a pitch ISO 261
/// does not give it, or with no pitch where it has no coarse one, is refused here.
fn iso_261(nominal: &str, pitch: Option<&str>, d: f64) -> Result<Option<String>, SpecError> {
    let number = finite_number(nominal)?;
    let size = format!("M{}", fmt3(number));
    let Some(&(size, coarse, fine)) = standards::ISO_261.iter().find(|(s, _, _)| *s == size) else {
        return Ok(None);
    };
    if (standards::bolt_diameter(size) - d).abs() > 0.2 {
        return Ok(None);
    }
    let pitches = || {
        coarse
            .iter()
            .chain(fine)
            .map(|p| g(*p))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match pitch.map(finite_number).transpose()? {
        None => match coarse {
            Some(_) => Ok(Some(size.to_string())),
            None => refuse(format!(
                "{size} has no coarse pitch: type its pitch ({} mm), e.g. {size}×{}.",
                pitches(),
                // Every ISO_261 row without a coarse pitch has at least one constant fine pitch.
                g(fine[0])
            )),
        },
        Some(p) if coarse == Some(p) => Ok(Some(size.to_string())),
        Some(p) if fine.contains(&p) => Ok(Some(format!("{size}x{}", g(p)))),
        Some(p) => refuse(format!(
            "{size}×{} is not an ISO 261 thread: {size} takes a pitch of {} mm.",
            g(p),
            pitches()
        )),
    }
}

/// Regex matches are not a guarantee of finite or representable numeric input.
fn finite_number(text: &str) -> Result<f64, SpecError> {
    pyfmt::parse_float(text)
        .filter(|x| x.is_finite())
        .ok_or_else(|| SpecError(format!("{} is not a finite number", repr_str(text))))
}

/// The tabulated thread size a typed nominal (and pitch, if typed) names.
fn bolt(nominal: &str, pitch: Option<&str>) -> Result<&'static str, SpecError> {
    let number = finite_number(nominal)?;
    let size = format!("M{}", fmt3(number));
    let Some(&(size, coarse)) = PITCH.iter().find(|(s, _)| *s == size) else {
        let known = standards::sizes(&PITCH).collect::<Vec<_>>().join(", ");
        return refuse(format!(
            "{size} is not a tabulated metric thread ({known})."
        ));
    };
    if let Some(pitch) = pitch {
        let pitch = finite_number(pitch)?;
        if (pitch - coarse).abs() > 1e-9 {
            return refuse(format!(
                "Only coarse pitch is tabulated: {size}×{}.",
                g(coarse)
            ));
        }
    }
    Ok(size)
}

/// An option's purpose: for a hole, the part before `:`; for a turned diameter, thread, knurl
/// or (anything else) fit.
fn kind(value: &str, hole: bool) -> &str {
    if value == GENERAL {
        return GENERAL;
    }
    let kind = value.split_once(':').map_or(value, |(kind, _)| kind);
    if hole || kind == "thread" || kind == "knurl" {
        kind
    } else {
        "fit"
    }
}

/// Python's `str.capitalize`: the first character title-cased, the rest lower case.
///
/// The tail is lower-cased in the context of the whole text, as Python does, so a final sigma
/// after the first letter is `ς` (`"aΣ"` gives `"Aς"`). Only Σ lower-cases by context, and
/// never at the start, so the first character's lower case is a prefix that can be cut off.
fn capitalize(text: &str) -> String {
    match text.chars().next() {
        Some(first) => {
            let lower = text.to_lowercase();
            let cut = first.to_lowercase().map(char::len_utf8).sum::<usize>();
            // The first scalar lowercases identically at the start of the full string, so cut is a UTF-8 boundary.
            titlecase(first) + &lower[cut..]
        }
        None => String::new(),
    }
}

/// A character's title case (Unicode 16.0, as Python 3.14's `str.title`): its upper case except
/// for the digraphs, Georgian, the Greek letters with ypogegrammeni and the ligatures listed
/// here, which are every character whose title case is not its upper case.
fn titlecase(c: char) -> String {
    let code = c as u32;
    // Every branch supplies a Unicode scalar literal or ORs 0x8 into U+1F80..U+1FAF.
    let one = |code: u32| char::from_u32(code).expect("a character").to_string();
    match code {
        0x01c4..=0x01c6 => one(0x01c5),
        0x01c7..=0x01c9 => one(0x01c8),
        0x01ca..=0x01cc => one(0x01cb),
        0x01f1..=0x01f3 => one(0x01f2),
        // Georgian Mkhedruli is its own title case (its upper case is Mtavruli).
        0x10d0..=0x10fa | 0x10fd..=0x10ff => c.to_string(),
        0x1f80..=0x1faf => one(code | 0x8),
        0x1fb3 | 0x1fbc => one(0x1fbc),
        0x1fc3 | 0x1fcc => one(0x1fcc),
        0x1ff3 | 0x1ffc => one(0x1ffc),
        _ => match c {
            '\u{df}' => "Ss",
            '\u{587}' => "\u{535}\u{582}",
            '\u{1fb2}' => "\u{1fba}\u{345}",
            '\u{1fb4}' => "\u{386}\u{345}",
            '\u{1fb7}' => "\u{391}\u{342}\u{345}",
            '\u{1fc2}' => "\u{1fca}\u{345}",
            '\u{1fc4}' => "\u{389}\u{345}",
            '\u{1fc7}' => "\u{397}\u{342}\u{345}",
            '\u{1ff2}' => "\u{1ffa}\u{345}",
            '\u{1ff4}' => "\u{38f}\u{345}",
            '\u{1ff7}' => "\u{3a9}\u{342}\u{345}",
            '\u{fb00}' => "Ff",
            '\u{fb01}' => "Fi",
            '\u{fb02}' => "Fl",
            '\u{fb03}' => "Ffi",
            '\u{fb04}' => "Ffl",
            '\u{fb05}' | '\u{fb06}' => "St",
            '\u{fb13}' => "\u{544}\u{576}",
            '\u{fb14}' => "\u{544}\u{565}",
            '\u{fb15}' => "\u{544}\u{56b}",
            '\u{fb16}' => "\u{54e}\u{576}",
            '\u{fb17}' => "\u{544}\u{56d}",
            _ => return c.to_uppercase().collect(),
        }
        .to_string(),
    }
}

/// Python's type name of a JSON value as `json.loads` gives it. A number serde_json reads as an
/// integer is Python's `int`; one with a point or exponent is `float`. (An integer too large for
/// 64 bits is read as a float here, where Python keeps an `int`.)
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python's `reprlib.repr` (its defaults: 6 levels, 6 list items, 4 dict items, 30 characters
/// of a str, 40 of an int) of a JSON value; `level` is the levels left.
pub fn short_repr(value: &Value, level: usize) -> String {
    const MAX_LIST: usize = 6;
    const MAX_DICT: usize = 4;
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(x) if n.is_f64() => shorten(&pyfmt::repr(x), 30),
            _ => shorten(&n.to_string(), 40),
        },
        Value::String(s) => short_str(s),
        Value::Array(items) => {
            if items.is_empty() {
                "[]".to_string()
            } else if level == 0 {
                "[...]".to_string()
            } else {
                let mut pieces: Vec<String> = items
                    .iter()
                    .take(MAX_LIST)
                    .map(|item| short_repr(item, level - 1))
                    .collect();
                if items.len() > MAX_LIST {
                    pieces.push("...".to_string());
                }
                format!("[{}]", pieces.join(", "))
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                "{}".to_string()
            } else if level == 0 {
                "{...}".to_string()
            } else {
                // Python sorts the keys (str order is code point order, as Rust's).
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut pieces: Vec<String> = keys
                    .iter()
                    .take(MAX_DICT)
                    .map(|key| format!("{}: {}", short_str(key), short_repr(&map[*key], level - 1)))
                    .collect();
                if map.len() > MAX_DICT {
                    pieces.push("...".to_string());
                }
                format!("{{{}}}", pieces.join(", "))
            }
        }
    }
}

/// `reprlib`'s cut of a long repr to `max` characters: its head and tail around `...`.
fn shorten(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let i = (max - 3) / 2;
    let j = max - 3 - i;
    let head: String = chars[..i].iter().collect();
    let tail: String = chars[chars.len() - j..].iter().collect();
    format!("{head}...{tail}")
}

/// `reprlib`'s repr of a str: the repr of its first 30 characters, or, if that is longer than
/// 30, the repr of its head and tail cut to 30 around `...`.
fn short_str(text: &str) -> String {
    const MAX: usize = 30;
    let chars: Vec<char> = text.chars().collect();
    let s = repr_str(&chars.iter().take(MAX).collect::<String>());
    let s_chars: Vec<char> = s.chars().collect();
    if s_chars.len() <= MAX {
        return s;
    }
    let i = (MAX - 3) / 2;
    let j = MAX - 3 - i;
    // Python's x[:i] + x[len(x)-j:], where a negative start counts from the end.
    let head: String = chars.iter().take(i).collect();
    let tail: String = chars[from_end(chars.len(), j)..].iter().collect();
    let s: Vec<char> = repr_str(&(head + &tail)).chars().collect();
    let head: String = s.iter().take(i).collect();
    let tail: String = s[from_end(s.len(), j)..].iter().collect();
    format!("{head}...{tail}")
}

/// Python's start index `len - j` of a slice, a negative one counted from the end and clamped
/// at 0.
fn from_end(len: usize, j: usize) -> usize {
    if j <= len {
        len - j
    } else {
        // len - j < 0: Python adds len once more, then clamps at 0.
        (2 * len).saturating_sub(j)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pattern the readers compile for Python's `\s` matches exactly the characters
    /// Python's `re` `\s` does (`tests/fixtures/pyfmt.json`, from `tests/fixtures/pyfmt.py`).
    #[test]
    fn space_pattern_is_pythons() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/pyfmt.json"
        ))
        .expect("tests/fixtures/pyfmt.json");
        let data: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let mut inside = vec![false; 0x110000];
        for range in data["re_space"].as_array().unwrap() {
            let (a, b) = (range[0].as_u64().unwrap(), range[1].as_u64().unwrap());
            inside[a as usize..=b as usize].fill(true);
        }
        let space = pattern("^{S}$");
        let wrong: Vec<String> = (0..0x110000u32)
            .filter_map(|c| char::from_u32(c).map(|ch| (c, ch)))
            .filter(|&(c, ch)| space.is_match(ch.encode_utf8(&mut [0; 4])) != inside[c as usize])
            .map(|(c, _)| format!("U+{c:04X}"))
            .collect();
        assert!(
            wrong.is_empty(),
            "SPACE differs from Python's \\s at {wrong:?}"
        );
    }
}
