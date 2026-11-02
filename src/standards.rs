//! Standards knowledge: ISO 286 fits, metric bolt sizes and knurls (port of `standards.py`; the
//! knurl table, `knurl_parameters`, is this port's own: questions Q4, Q15).
//!
//! Only what the rules use. Values are ISO 286-1/-2 tabulated values in micrometres for nominal
//! sizes up to 3150 mm; each size range is "over the lower bound, up to and including the upper".
//! Tables keyed by thread size are ordered arrays in Python's dict order, since the order
//! reaches people (the sizes listed in a refusal, the first match of a lookup). The ISO 2768-1
//! table is in `general.rs`.

// Capture indexing below follows successful matches with mandatory groups; optional groups use get().
use std::sync::LazyLock;

use regex::Regex;

use crate::pyfmt;

/// Upper bounds of the ISO 286 nominal size ranges, in mm: ISO 286-1:2010 Table 1's main ranges.
/// Above 500 mm Tables 2 and 3 split each range in two (500-560 and 560-630, ...), but every
/// deviation tabulated here is the same in both halves, so the main ranges suffice.
const RANGES: [f64; 21] = [
    3.0, 6.0, 10.0, 18.0, 30.0, 50.0, 80.0, 120.0, 180.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0,
    1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0,
];

/// Standard tolerance grades IT5 to IT11, micrometres, per size range: ISO 286-1:2010 Table 1
/// (up to 500 mm as specify-core has them; 500 to 3150 mm from the same table, cross-checked with
/// the h6, k6, m6, g6 and f7 limits of ISO 286-2:2010 Table 17).
const IT: [(u64, [i32; 21]); 7] = [
    (
        5,
        [
            4, 5, 6, 8, 9, 11, 13, 15, 18, 20, 23, 25, 27, // up to 500
            32, 36, 40, 47, 55, 65, 78, 96, // 500 to 3150
        ],
    ),
    (
        6,
        [
            6, 8, 9, 11, 13, 16, 19, 22, 25, 29, 32, 36, 40, // up to 500
            44, 50, 56, 66, 78, 92, 110, 135, // 500 to 3150
        ],
    ),
    (
        7,
        [
            10, 12, 15, 18, 21, 25, 30, 35, 40, 46, 52, 57, 63, // up to 500
            70, 80, 90, 105, 125, 150, 175, 210, // 500 to 3150
        ],
    ),
    (
        8,
        [
            14, 18, 22, 27, 33, 39, 46, 54, 63, 72, 81, 89, 97, // up to 500
            110, 125, 140, 165, 195, 230, 280, 330, // 500 to 3150
        ],
    ),
    (
        9,
        [
            25, 30, 36, 43, 52, 62, 74, 87, 100, 115, 130, 140, 155, // up to 500
            175, 200, 230, 260, 310, 370, 440, 540, // 500 to 3150
        ],
    ),
    (
        10,
        [
            40, 48, 58, 70, 84, 100, 120, 140, 160, 185, 210, 230, 250, // up to 500
            280, 320, 360, 420, 500, 600, 700, 860, // 500 to 3150
        ],
    ),
    (
        11,
        [
            60, 75, 90, 110, 130, 160, 190, 220, 250, 290, 320, 360, 400, // up to 500
            440, 500, 560, 660, 780, 920, 1100, 1350, // 500 to 3150
        ],
    ),
];

/// Shaft fundamental deviations, micrometres. f, g, h give the upper deviation (es): ISO 286-1:2010
/// Table 2; k, m, n, p give the lower (ei): Table 3. k is the value for grades 4 to 7 (above
/// 500 mm it is 0 for every grade). Hole H has no table: its lower deviation is 0 at every size.
const SHAFT: [(&str, [i32; 21]); 7] = [
    (
        "f",
        [
            -6, -10, -13, -16, -20, -25, -30, -36, -43, -50, -56, -62, -68, // up to 500
            -76, -80, -86, -98, -110, -120, -130, -145, // 500 to 3150
        ],
    ),
    (
        "g",
        [
            -2, -4, -5, -6, -7, -9, -10, -12, -14, -15, -17, -18, -20, // up to 500
            -22, -24, -26, -28, -30, -32, -34, -38, // 500 to 3150
        ],
    ),
    ("h", [0; 21]),
    (
        "k",
        [
            0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4, 5, // up to 500
            0, 0, 0, 0, 0, 0, 0, 0, // 500 to 3150
        ],
    ),
    (
        "m",
        [
            2, 4, 6, 7, 8, 9, 11, 13, 15, 17, 20, 21, 23, // up to 500
            26, 30, 34, 40, 48, 58, 68, 76, // 500 to 3150
        ],
    ),
    (
        "n",
        [
            4, 8, 10, 12, 15, 17, 20, 23, 27, 31, 34, 37, 40, // up to 500
            44, 50, 56, 66, 78, 92, 110, 135, // 500 to 3150
        ],
    ),
    (
        "p",
        [
            6, 12, 15, 18, 22, 26, 32, 37, 43, 50, 56, 62, 68, // up to 500
            78, 88, 100, 120, 140, 170, 195, 240, // 500 to 3150
        ],
    ),
];

/// The shaft letters whose tabulated deviation is the upper one.
const UPPER_DEVIATION: [&str; 3] = ["f", "g", "h"];

/// Python's `_FIT = re.compile(r"^(H|JS|js|[fghkmnp])(\d{1,2})$")` used with `match`: `$` also
/// matches before a final newline, and `\d` is any Unicode decimal digit.
static FIT: LazyLock<Regex> =
    // This regex is a checked source literal, independent of the input fit.
    LazyLock::new(|| {
        Regex::new(r"^(H|JS|js|[fghkmnp])(\p{Nd}{1,2})\n?$").expect("valid pattern")
    });

/// Hole fits offered in questions, and shaft fits, most common first.
pub const HOLE_FITS: [&str; 4] = ["H7", "H8", "H9", "H11"];
pub const SHAFT_FITS: [&str; 9] = ["h6", "g6", "f7", "k6", "m6", "n6", "p6", "h7", "js6"];

/// ISO 965 tolerance classes for general-purpose metric threads.
pub const INTERNAL_THREAD_CLASS: &str = "6H";
pub const EXTERNAL_THREAD_CLASS: &str = "6g";

/// Knurl patterns as AP242's turned_knurl names them, and a common pitch, mm.
pub const KNURL_PATTERNS: [&str; 2] = ["straight", "diamond"];
pub const KNURL_PITCH: f64 = 1.0;

/// The knurling standard a knurl's `turned_knurl` parameters are derived from (questions Q4,
/// Q15).
///
/// DIN 82 (knurls) defines the knurl types, a 90° profile angle and the pitch series; the knurl's
/// nominal diameter d1 on the drawing is the outside diameter of the finished knurl. The text
/// checked is IS 3403:1981 *Dimensions for knurls*, which states DIN 82's types (RAA straight;
/// RBL/RBR one helix at 30°; RGE/RGV left- and right-hand helices crossed, 30° each way),
/// §3.1 profile angle 90°, §3.2 pitch P 0.5, 0.6, 0.8, 1.0, 1.2, 1.6 mm, §3.3 the nominal
/// diameter and §3.4 the initial diameter d2 = d1 − 0.5 P (RAA, RBL, RBR), d1 − 0.67 P (RGE).
/// The root radius is the knurling wheel's crest radius: IS 6776:1980 *Knurling wheels* (wheels
/// for knurls to IS 3403) §4.3, 0.1 mm for P up to and including 1.0 mm, 0.15 mm above. Neither
/// the DIN 82 text itself nor ISO 13444 (the indication of knurls) was available to check.
///
/// **Assumed, as no table gives it:** the tooth depth, taken as P/2, the depth of the sharp 90°
/// profile of pitch P (§3.1). It agrees with §3.4: a formed straight knurl grows by 0.5 P on
/// the diameter, the profile's depth P/2 pressed half in and raised half out. The rounding of
/// crest and root (§4.3) would make it shallower by 2 r (√2 − 1), 0.08 mm at 1 mm pitch; like
/// §3.4's factors, P/2 does not count it.
pub const KNURL_STANDARD: &str = "DIN 82";

/// DIN 82's pitches, mm (IS 3403:1981 §3.2).
pub const KNURL_PITCHES: [f64; 6] = [0.5, 0.6, 0.8, 1.0, 1.2, 1.6];

/// The helix angle of each of a diamond knurl's two crossed helices, degrees (DIN 82 type RGE;
/// IS 6776:1980 type GE, 30° each way). It states no hand: it is cut both ways.
pub const KNURL_HELIX_ANGLE: f64 = 30.0;

/// A knurl's `turned_knurl` parameters beside its pattern, pitch and diameter, derived from
/// [`KNURL_STANDARD`], each with where it comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct KnurlParameters {
    pub teeth: u32,
    pub tooth_depth: f64,
    pub root_fillet: f64,
    /// A diamond knurl's only.
    pub helix_angle: Option<f64>,
    /// `(intent key, source)` for each value above, in that order.
    pub sources: Vec<(&'static str, String)>,
}

/// The parameters of a `pattern` knurl of `pitch` on a diameter `diameter` (the finished knurl's
/// outside diameter, DIN 82's d1), or why they cannot be derived: the number of teeth is
/// π·d/P rounded to a whole number, the pitch kept at its nominal (tool) value and the diameter
/// as designed; the tooth depth P/2 and the root fillet the wheel's crest radius (see
/// [`KNURL_STANDARD`]); a diamond knurl's helix angle 30° and no hand, a straight knurl's
/// neither.
pub fn knurl_parameters(
    pattern: &str,
    pitch: f64,
    diameter: f64,
) -> Result<KnurlParameters, String> {
    let diamond = match pattern {
        "straight" => false,
        "diamond" => true,
        other => {
            return Err(format!(
                "{} is not a knurl pattern {KNURL_STANDARD} gives (straight or diamond)",
                pyfmt::repr_str(other)
            ));
        }
    };
    if !KNURL_PITCHES.contains(&pitch) {
        let series: Vec<String> = KNURL_PITCHES.iter().map(|p| pyfmt::g(*p)).collect();
        return Err(format!(
            "a pitch of {} mm is not one of {KNURL_STANDARD}'s ({} mm)",
            pyfmt::g(pitch),
            series.join(", ")
        ));
    }
    let teeth = (std::f64::consts::PI * diameter / pitch).round();
    if !diameter.is_finite() || teeth < 1.0 || teeth > f64::from(u32::MAX) {
        return Err(format!(
            "a diameter of {} mm gives no whole number of teeth",
            pyfmt::g(diameter)
        ));
    }
    let teeth = teeth as u32;
    let tooth_depth = pitch / 2.0;
    let root_fillet = if pitch <= 1.0 { 0.1 } else { 0.15 };
    let (d, p) = (pyfmt::g(diameter), pyfmt::g(pitch));
    let mut sources = vec![
        (
            "teeth",
            format!(
                "π × {d} / {p} rounded: the diameter as designed and {KNURL_STANDARD}'s pitch \
                 {p} mm, kept at the tool's"
            ),
        ),
        (
            "tooth_depth",
            format!(
                "P/2, the depth of {KNURL_STANDARD}'s 90° profile at pitch {p} mm (no table gives \
                 it; crest and root rounding not counted)"
            ),
        ),
        (
            "root_fillet",
            format!(
                "the knurling wheel's crest radius for a pitch {} 1 mm (IS 6776:1980 §4.3, wheels \
                 for {KNURL_STANDARD} knurls)",
                if pitch <= 1.0 { "up to" } else { "over" }
            ),
        ),
    ];
    let helix_angle = diamond.then_some(KNURL_HELIX_ANGLE);
    if diamond {
        sources.push((
            "helix_angle",
            format!("{KNURL_STANDARD} type RGE: two helices crossed at 30° each way, so no hand"),
        ));
    }
    Ok(KnurlParameters {
        teeth,
        tooth_depth,
        root_fillet,
        helix_angle,
        sources,
    })
}

/// A table of values by metric thread size, in Python's dict order.
pub type SizeTable = [(&'static str, f64); 13];

/// ISO 273 clearance holes, mm: fine series. A hole between a bolt's fine and coarse sizes is a
/// plausible clearance hole for it.
pub const CLEARANCE_FINE: SizeTable = [
    ("M1.6", 1.7),
    ("M2", 2.2),
    ("M2.5", 2.7),
    ("M3", 3.2),
    ("M4", 4.3),
    ("M5", 5.3),
    ("M6", 6.4),
    ("M8", 8.4),
    ("M10", 10.5),
    ("M12", 13.0),
    ("M16", 17.0),
    ("M20", 21.0),
    ("M24", 25.0),
];
/// ISO 273 clearance holes, mm: coarse series.
pub const CLEARANCE_COARSE: SizeTable = [
    ("M1.6", 2.0),
    ("M2", 2.6),
    ("M2.5", 3.1),
    ("M3", 3.6),
    ("M4", 4.8),
    ("M5", 5.8),
    ("M6", 7.0),
    ("M8", 10.0),
    ("M10", 12.0),
    ("M12", 14.5),
    ("M16", 18.5),
    ("M20", 24.0),
    ("M24", 28.0),
];
/// ISO 273 medium clearance holes, mm.
pub const CLEARANCE: SizeTable = [
    ("M1.6", 1.8),
    ("M2", 2.4),
    ("M2.5", 2.9),
    ("M3", 3.4),
    ("M4", 4.5),
    ("M5", 5.5),
    ("M6", 6.6),
    ("M8", 9.0),
    ("M10", 11.0),
    ("M12", 13.5),
    ("M16", 17.5),
    ("M20", 22.0),
    ("M24", 26.0),
];
/// ISO 261 coarse tap drills, mm.
pub const TAP_DRILL: SizeTable = [
    ("M1.6", 1.25),
    ("M2", 1.6),
    ("M2.5", 2.05),
    ("M3", 2.5),
    ("M4", 3.3),
    ("M5", 4.2),
    ("M6", 5.0),
    ("M8", 6.8),
    ("M10", 8.5),
    ("M12", 10.2),
    ("M16", 14.0),
    ("M20", 17.5),
    ("M24", 21.0),
];
/// ISO 261 coarse pitches, mm.
pub const PITCH: SizeTable = [
    ("M1.6", 0.35),
    ("M2", 0.4),
    ("M2.5", 0.45),
    ("M3", 0.5),
    ("M4", 0.7),
    ("M5", 0.8),
    ("M6", 1.0),
    ("M8", 1.25),
    ("M10", 1.5),
    ("M12", 1.75),
    ("M16", 2.0),
    ("M20", 2.5),
    ("M24", 3.0),
];

pub const GENERAL_TOLERANCES: [&str; 16] = [
    "ISO 2768-f",
    "ISO 2768-m",
    "ISO 2768-c",
    "ISO 2768-v",
    "ISO 2768-fH",
    "ISO 2768-fK",
    "ISO 2768-fL",
    "ISO 2768-mH",
    "ISO 2768-mK",
    "ISO 2768-mL",
    "ISO 2768-cH",
    "ISO 2768-cK",
    "ISO 2768-cL",
    "ISO 2768-vH",
    "ISO 2768-vK",
    "ISO 2768-vL",
];

/// How near a diameter must be to a tabulated one to be taken for it, mm (Python's default
/// `tolerance`; no caller passes another).
const TOLERANCE: f64 = 0.05;

/// `table[size]`: the value for a thread size, if tabulated.
pub fn lookup(table: &SizeTable, size: &str) -> Option<f64> {
    table.iter().find(|(s, _)| *s == size).map(|&(_, v)| v)
}

/// The thread sizes of a table, in order (Python's iteration over the dict).
pub fn sizes(table: &SizeTable) -> impl Iterator<Item = &'static str> + '_ {
    table.iter().map(|&(s, _)| s)
}

/// `table[size]` for a size known to be tabulated: from a table, or checked by the caller.
/// Panics otherwise, as Python's `KeyError` would end the request.
fn at(table: &SizeTable, size: &str) -> f64 {
    // Every at() caller supplies a constant table key; the thread tables share the same keys.
    lookup(table, size).unwrap_or_else(|| panic!("{size} is not a tabulated thread"))
}

/// Upper and lower deviation in mm for `fit` (e.g. `H7`, `g6`) at `nominal`; the error is
/// Python's `ValueError` message.
pub fn fit_limits(fit: &str, nominal: f64) -> Result<(f64, f64), String> {
    let caps = FIT
        .captures(fit)
        .ok_or_else(|| format!("unsupported fit {}", pyfmt::repr_str(fit)))?;
    // FIT has two mandatory groups; captures() succeeded above.
    let letter = &caps[1];
    // int() of one or two decimal digits; "07" is grade 7.
    let grade = pyfmt::parse_digits(&caps[2])
        .ok_or_else(|| format!("unsupported fit {}", pyfmt::repr_str(fit)))?;
    let Some(&(_, it_row)) = IT.iter().find(|(g, _)| *g == grade) else {
        return Err(format!("unsupported grade IT{grade}"));
    };
    // RANGES is a nonempty constant table.
    let largest = RANGES[RANGES.len() - 1];
    if !(0.0 < nominal && nominal <= largest) {
        return Err(format!(
            "nominal size {} outside 0-3150 mm",
            pyfmt::repr(nominal)
        ));
    }
    // bisect_left: the first range whose upper bound is at least the nominal size.
    let index = RANGES.partition_point(|&upper| upper < nominal);
    // nominal is at most the last RANGES entry; each IT row has RANGES.len() columns.
    let it = f64::from(it_row[index]);
    let shaft = |letter: &str| {
        // FIT permits only H/JS/js or letters present in the constant SHAFT table; special cases return first.
        let (_, row) = SHAFT
            .iter()
            .find(|(l, _)| *l == letter)
            .expect("a shaft letter");
        // index is bounded by RANGES; every constant SHAFT row has that many columns.
        f64::from(row[index])
    };
    let (upper, lower) = if letter == "H" {
        (it, 0.0)
    } else if letter == "JS" || letter == "js" {
        (it / 2.0, -it / 2.0)
    } else if UPPER_DEVIATION.contains(&letter) {
        let upper = shaft(letter);
        (upper, upper - it)
    } else {
        if letter == "k" && grade > 7 {
            return Err("k deviations are tabulated here for grades 4 to 7 only".to_string());
        }
        let lower = shaft(letter);
        (lower + it, lower)
    };
    Ok((upper / 1000.0, lower / 1000.0))
}

/// The thread size whose table entry is within 0.05 mm of `diameter`, the first in table order.
pub fn match_size(diameter: f64, table: &SizeTable) -> Option<&'static str> {
    table
        .iter()
        .find(|&&(_, value)| (value - diameter).abs() <= TOLERANCE)
        .map(|&(size, _)| size)
}

/// Bolts `diameter` is a plausible ISO 273 clearance hole for: within the fine-to-coarse range
/// of each, nearest its medium size first (a stable sort, ties in table order).
pub fn clearance_bolts(diameter: f64) -> Vec<&'static str> {
    let mut fits: Vec<&'static str> = sizes(&CLEARANCE)
        .filter(|size| {
            at(&CLEARANCE_FINE, size) - TOLERANCE <= diameter
                && diameter <= at(&CLEARANCE_COARSE, size) + TOLERANCE
        })
        .collect();
    let distance = |size: &&str| (at(&CLEARANCE, size) - diameter).abs();
    // Python's sorted with a float key; the distances here are never nan.
    fits.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    fits
}

/// The largest tabulated bolt a hole of `diameter` could plausibly clear: one it passes, with no
/// more than twice that bolt's coarse-series play. A Ø62 bore clears an M24 by 38 mm, which is
/// not a clearance hole.
pub fn largest_bolt_below(diameter: f64) -> Option<&'static str> {
    // max() keeps the first of equal keys; bolt diameters are distinct.
    sizes(&CLEARANCE)
        .filter(|size| {
            let bolt = bolt_diameter(size);
            0.0 < diameter - bolt && diameter - bolt <= 2.0 * (at(&CLEARANCE_COARSE, size) - bolt)
        })
        .fold(None, |best: Option<&'static str>, size| match best {
            Some(b) if bolt_diameter(b) >= bolt_diameter(size) => Some(b),
            _ => Some(size),
        })
}

/// A metric thread size's nominal diameter, mm: `M2.5` is 2.5. Panics on a size that is not
/// `M` and a number, as Python's `float()` would raise; sizes come from the tables.
pub fn bolt_diameter(size: &str) -> f64 {
    let number = size.strip_prefix('M').unwrap_or(size);
    // All WASM-reachable callers supply a size from the constant metric thread tables.
    pyfmt::parse_float(number).unwrap_or_else(|| panic!("{size} is not a metric thread size"))
}

/// Minimum full thread in a blind tapped hole: the drill depth less three pitches for the tap's
/// lead and the drill point, rounded down to 0.5 mm. `size` must be tabulated.
pub fn full_thread(drill_depth: f64, size: &str) -> f64 {
    let pitch = at(&PITCH, size);
    let usable = drill_depth - 3.0 * pitch;
    // Python's max(a, b) returns a unless b is greater.
    let down = (usable * 2.0).floor() / 2.0;
    if down > pitch { down } else { pitch }
}

/// The coarse metric thread whose nominal is `diameter` (within 0.05 mm): an external thread.
pub fn thread_size(diameter: f64) -> Option<&'static str> {
    sizes(&PITCH).find(|size| (bolt_diameter(size) - diameter).abs() <= TOLERANCE)
}

/// ISO 261 general-purpose metric screw threads from M1.6 to M64, for a turned diameter's
/// thread choices (stream S3, issue #3): each nominal size, its coarse pitch (none for the sizes
/// ISO 261 gives in fine pitches only, such as M15) and its fine pitches, largest first. All
/// three choice columns are kept: a third-choice size such as M15×1 or M17×1 is a bearing
/// locknut's thread on a spindle. ISO 261's bracketed pitches (M30×3, M33×3) and M14×1.25,
/// which it reserves for spark plugs, are left out. The coarse pitches agree with [`PITCH`],
/// which holes and mates keep using.
pub const ISO_261: [(&str, Option<f64>, &[f64]); 49] = [
    ("M1.6", Some(0.35), &[0.2]),
    ("M1.8", Some(0.35), &[0.2]),
    ("M2", Some(0.4), &[0.25]),
    ("M2.2", Some(0.45), &[0.25]),
    ("M2.5", Some(0.45), &[0.35]),
    ("M3", Some(0.5), &[0.35]),
    ("M3.5", Some(0.6), &[0.35]),
    ("M4", Some(0.7), &[0.5]),
    ("M4.5", Some(0.75), &[0.5]),
    ("M5", Some(0.8), &[0.5]),
    ("M5.5", None, &[0.5]),
    ("M6", Some(1.0), &[0.75]),
    ("M7", Some(1.0), &[0.75]),
    ("M8", Some(1.25), &[1.0, 0.75]),
    ("M9", Some(1.25), &[1.0, 0.75]),
    ("M10", Some(1.5), &[1.25, 1.0, 0.75]),
    ("M11", Some(1.5), &[1.0, 0.75]),
    ("M12", Some(1.75), &[1.5, 1.25, 1.0]),
    ("M14", Some(2.0), &[1.5, 1.0]),
    ("M15", None, &[1.5, 1.0]),
    ("M16", Some(2.0), &[1.5, 1.0]),
    ("M17", None, &[1.5, 1.0]),
    ("M18", Some(2.5), &[2.0, 1.5, 1.0]),
    ("M20", Some(2.5), &[2.0, 1.5, 1.0]),
    ("M22", Some(2.5), &[2.0, 1.5, 1.0]),
    ("M24", Some(3.0), &[2.0, 1.5, 1.0]),
    ("M25", None, &[2.0, 1.5, 1.0]),
    ("M26", None, &[1.5]),
    ("M27", Some(3.0), &[2.0, 1.5, 1.0]),
    ("M28", None, &[2.0, 1.5, 1.0]),
    ("M30", Some(3.5), &[2.0, 1.5, 1.0]),
    ("M32", None, &[2.0, 1.5]),
    ("M33", Some(3.5), &[2.0, 1.5]),
    ("M35", None, &[1.5]),
    ("M36", Some(4.0), &[3.0, 2.0, 1.5]),
    ("M38", None, &[1.5]),
    ("M39", Some(4.0), &[3.0, 2.0, 1.5]),
    ("M40", None, &[3.0, 2.0, 1.5]),
    ("M42", Some(4.5), &[4.0, 3.0, 2.0, 1.5]),
    ("M45", Some(4.5), &[4.0, 3.0, 2.0, 1.5]),
    ("M48", Some(5.0), &[4.0, 3.0, 2.0, 1.5]),
    ("M50", None, &[3.0, 2.0, 1.5]),
    ("M52", Some(5.0), &[4.0, 3.0, 2.0, 1.5]),
    ("M55", None, &[4.0, 3.0, 2.0, 1.5]),
    ("M56", Some(5.5), &[4.0, 3.0, 2.0, 1.5]),
    ("M58", None, &[4.0, 3.0, 2.0, 1.5]),
    ("M60", Some(5.5), &[4.0, 3.0, 2.0, 1.5]),
    ("M62", None, &[4.0, 3.0, 2.0, 1.5]),
    ("M64", Some(6.0), &[4.0, 3.0, 2.0, 1.5]),
];

/// The ISO 261 threads a turned diameter of `diameter` may carry (its nominal within 0.05 mm,
/// as [`thread_size`]), as answer specs: the coarse thread as its bare size (`M10`, the spec
/// Python's answers use) first, then each fine pitch as `M10x1.25`. Empty where no ISO 261
/// size matches.
pub fn external_threads(diameter: f64) -> Vec<String> {
    let Some(&(size, coarse, fine)) = ISO_261.iter().find(|(size, _, _)| {
        let nominal = bolt_diameter(size);
        // Compare endpoints so a decimal boundary such as 10.05 is included despite
        // cancellation in (diameter - nominal). No rounding or snapping outside it.
        nominal - TOLERANCE <= diameter && diameter <= nominal + TOLERANCE
    }) else {
        return Vec::new();
    };
    let coarse = coarse.map(|_| size.to_string());
    coarse
        .into_iter()
        .chain(fine.iter().map(|p| format!("{size}x{}", pyfmt::g(*p))))
        .collect()
}

/// An external thread spec's ISO 261 size and pitch: `M10` is the coarse M10×1.5 (for a size
/// with no coarse pitch, none), `M10x1.25` a fine pitch ISO 261 gives for M10. `None` for
/// anything else.
pub fn external_thread(spec: &str) -> Option<(&'static str, f64)> {
    let (size, pitch) = match spec.split_once('x') {
        Some((size, pitch)) => (size, Some(pyfmt::parse_float(pitch)?)),
        None => (spec, None),
    };
    let &(size, coarse, fine) = ISO_261.iter().find(|(s, _, _)| *s == size)?;
    match pitch {
        None => coarse.map(|c| (size, c)),
        Some(p) if coarse == Some(p) || fine.contains(&p) => Some((size, p)),
        Some(_) => None,
    }
}
