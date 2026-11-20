//! The Python oracle, for integration tests: captured cases, scenarios, and a comparison with
//! known differences.
//!
//! Each integration test that uses it says `mod common;`. The oracle is
//! `tests/fixtures/oracle/`, written by `tools/capture.py` from specify-core (Python); its
//! README lists every artifact. Comparing Rust output with it:
//!
//! ```ignore
//! mod common;
//! use common::{Tolerances, cases, check_known, compare};
//!
//! #[test]
//! fn features_match_the_oracle() {
//!     let tolerances = Tolerances::default().with("faces[*].area", 1e-6, 0.0);
//!     let mut differences = Vec::new();
//!     for case in cases() {
//!         let Some(input) = case.input() else { continue }; // a corpus file not here: skipped
//!         let Some(python) = case.artifact("analysis") else { continue };
//!         let rust = /* the port's analysis of `input`, as JSON */;
//!         differences.extend(compare(&case.name, &python, &rust, &tolerances));
//!     }
//!     check_known("features", &differences); // once per area, in that area's sweep
//! }
//! ```
//!
//! A difference must match an entry of `tests/fixtures/known_differences/<area>.json` (see its
//! README): `check_known` fails on any that does not, and on any entry that matched nothing.
//! A sweep that crosses areas filters through other areas' files with `explained`, which
//! asserts nothing.

// Each test crate uses a different part of this module.
#![allow(dead_code)]

pub mod accept_all;
pub(crate) mod corpus;
pub mod numbering;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// The verdicts a known difference may carry.
pub const VERDICTS: [&str; 6] = [
    "rust-correct",
    "rust-wrong",
    "equivalent",
    "undetermined",
    "not-applicable",
    "resolved",
];

/// `tests/fixtures`.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// `tests/fixtures/oracle`.
pub fn oracle_dir() -> PathBuf {
    fixtures().join("oracle")
}

/// Quiddity's STEP corpus, if this run has it: `QUIDDITY_CORPUS` (the capture tool's name for
/// it), else `SPECIFY_CORPUS`, when it names a directory. No default: without either, every
/// corpus case is skipped. `QUIDDITY_CORPUS_REQUIRED` makes a missing corpus fatal in CI.
pub fn corpus_dir() -> Option<PathBuf> {
    let dir = ["QUIDDITY_CORPUS", "SPECIFY_CORPUS"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|dir| dir.is_dir());
    assert!(
        dir.is_some() || std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
        "QUIDDITY_CORPUS_REQUIRED: set QUIDDITY_CORPUS or SPECIFY_CORPUS to the corpus directory"
    );
    dir
}

/// Optional live Python checks must opt into an explicit environment. Frozen captures
/// remain the default comparison; never discover another checkout on the host.
pub fn live_python() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("SPECIFY_TEST_PYTHON")?);
    assert!(
        path.is_file(),
        "SPECIFY_TEST_PYTHON is not a file: {}",
        path.display()
    );
    Some(path)
}

/// A gzipped JSON file, parsed. Panics if it cannot be read: a fixture is broken.
pub fn read_json_gz(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut text = String::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_string(&mut text)
        .unwrap_or_else(|e| panic!("{}: not gzipped UTF-8: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: not JSON: {e}", path.display()))
}

/// Whether `value` is a recorded Python failure (`{error, message, ..}`) rather than a result.
pub fn is_error(value: &Value) -> bool {
    value.get("error").is_some_and(Value::is_string)
        && value.get("message").is_some_and(Value::is_string)
}

/// One captured case: an input file of one part, a file of several parts (its file-level
/// artifacts: `parts`, the refused `analysis`, `write`, `readback`), or one part of such a
/// file (`name` ending `#k`).
#[derive(Debug, Clone)]
pub struct Case {
    /// e.g. `parts/plate`, `parts/assembly#1`, `nist/nist_ctc_01_asme1_ap242`,
    /// `corpus/cadgenbench_inputs/cgb202` (a `.gz` input's name drops `.gz`).
    pub name: String,
    /// Its directory under `tests/fixtures/oracle`.
    pub dir: PathBuf,
    /// Its `meta.json`: `kind` (`parts`, `nist`, `corpus`), `input` (relative to the kind's
    /// root), `input_sha256`, `decompressed_sha256` for a `.gz`, `part`, versions, `answers`.
    pub meta: Value,
}

pub fn all_corpus() -> bool {
    std::env::var("SPECIFY_CORPUS_ALL").as_deref() == Ok("1")
}

/// Also accepts ledger globs and rule-artifact suffixes, e.g. corpus/x/y#0/questions.
pub fn case_selected(name: &str) -> bool {
    if all_corpus() || !name.starts_with("corpus/") {
        return true;
    }
    let stem = name
        .strip_prefix("corpus/")
        .unwrap()
        .split('#')
        .next()
        .unwrap();
    let stem = stem.split('/').take(2).collect::<Vec<_>>().join("/");
    corpus::SUBSET.iter().any(|file| glob_match(&stem, file))
}

/// Selected captures, sorted by name. Synthetic and committed NIST fixtures always run.
pub fn cases() -> Vec<Case> {
    all_cases()
        .into_iter()
        .filter(|c| case_selected(&c.name))
        .collect()
}

/// All captures, for explicit lookups and selection coverage checks.
pub fn all_cases() -> Vec<Case> {
    let root = oracle_dir();
    let mut out = Vec::new();
    collect_cases(&root, &root, &mut out);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The case named `name`; panics if it was not captured.
pub fn case(name: &str) -> Case {
    assert!(
        case_selected(name),
        "named regression {name} must be in tests/common/corpus.rs"
    );
    all_cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no oracle case {name}"))
}

fn collect_cases(root: &Path, dir: &Path, out: &mut Vec<Case>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta = path.join("meta.json.gz");
        if meta.is_file() {
            let name = path
                .strip_prefix(root)
                .expect("under the oracle")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push(Case {
                name,
                dir: path.clone(),
                meta: read_json_gz(&meta),
            });
        }
        collect_cases(root, &path, out);
    }
}

impl Case {
    /// `parts`, `nist` or `corpus`.
    pub fn kind(&self) -> &str {
        self.meta["kind"].as_str().expect("meta.kind")
    }

    /// Which part of its file this case is, for a part of a file of several; else None.
    pub fn part(&self) -> Option<usize> {
        self.meta["part"].as_u64().map(|k| k as usize)
    }

    /// The input file both engines read, or None when it is a corpus file this run does not
    /// have (the caller skips the case; the reason is printed). A `.gz` input is decompressed
    /// once to a temporary file, whose path is returned. Panics when a committed input is
    /// missing, or when the input's bytes are not those the oracle was captured from (the
    /// oracle is stale for it).
    pub fn input(&self) -> Option<PathBuf> {
        let rel = self.meta["input"].as_str().expect("meta.input");
        let root = match self.kind() {
            "parts" => fixtures().join("parts"),
            "nist" => fixtures().join("nist"),
            "corpus" => match corpus_dir() {
                Some(dir) => dir,
                None => {
                    println!("skipped {}: no corpus (set QUIDDITY_CORPUS)", self.name);
                    return None;
                }
            },
            other => panic!("{}: unknown kind {other}", self.name),
        };
        let path = root.join(rel);
        if !path.is_file() {
            if self.kind() == "corpus" {
                assert!(
                    std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
                    "QUIDDITY_CORPUS_REQUIRED: {}: missing corpus input {}",
                    self.name,
                    path.display()
                );
                println!(
                    "skipped {}: {} is not in the corpus",
                    self.name,
                    path.display()
                );
                return None;
            }
            panic!(
                "{}: committed input {} is missing",
                self.name,
                path.display()
            );
        }
        let stored = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let sha = self.meta["input_sha256"]
            .as_str()
            .expect("meta.input_sha256");
        assert_eq!(
            sha256(&stored),
            sha,
            "{}: {} is not the file the oracle was captured from",
            self.name,
            path.display()
        );
        if !rel.to_ascii_lowercase().ends_with(".gz") {
            return Some(path);
        }
        let inner = self.meta["decompressed_sha256"]
            .as_str()
            .expect("meta.decompressed_sha256 for a .gz input");
        Some(decompressed(&path, inner))
    }

    /// The artifact `name` (`analysis`, `questions`, `mesh`, ..: `<name>.json.gz`), if the
    /// case has it. A Python failure is an artifact too (`is_error`).
    pub fn artifact(&self, name: &str) -> Option<Value> {
        let path = self.dir.join(format!("{name}.json.gz"));
        path.is_file().then(|| read_json_gz(&path))
    }

    /// The STEP Python wrote (`written.step.gz`, synthetic and NIST cases), decompressed to a
    /// temporary file; None when the case has none.
    pub fn written_step(&self) -> Option<PathBuf> {
        let path = self.dir.join("written.step.gz");
        if !path.is_file() {
            return None;
        }
        let bytes = gunzip(&std::fs::read(&path).expect("written.step.gz"));
        Some(decompressed(&path, &sha256(&bytes)))
    }
}

/// An artifact of the oracle as a whole (`interpret`), if captured.
pub fn oracle_artifact(name: &str) -> Option<Value> {
    let path = oracle_dir().join(format!("{name}.json.gz"));
    path.is_file().then(|| read_json_gz(&path))
}

/// The sha256 of `bytes`, lower-case hex.
pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .expect("gzip");
    out
}

/// `gz` decompressed to `<temp>/specify-core-oracle/<sha>/<name without .gz>`, once per
/// process and reused across processes; panics unless its sha256 is `expected`.
pub fn decompressed(gz: &Path, expected: &str) -> PathBuf {
    static DONE: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();
    let done = DONE.get_or_init(Default::default);
    let mut done = done.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(path) = done.get(gz) {
        return path.clone();
    }
    let name = gz.file_name().expect("file name").to_string_lossy();
    let name = name
        .strip_suffix(".gz")
        .or_else(|| name.strip_suffix(".GZ"))
        .unwrap_or_else(|| panic!("{}: not a .gz", gz.display()));
    let dir = std::env::temp_dir()
        .join("specify-core-oracle")
        .join(expected);
    let out = dir.join(name);
    let fresh = std::fs::read(&out).is_ok_and(|bytes| sha256(&bytes) == expected);
    if !fresh {
        let bytes = gunzip(&std::fs::read(gz).unwrap_or_else(|e| panic!("{}: {e}", gz.display())));
        assert_eq!(
            sha256(&bytes),
            expected,
            "{}: decompressed bytes are not those the oracle was captured from",
            gz.display()
        );
        std::fs::create_dir_all(&dir).expect("temporary directory");
        // Written aside and renamed, so a concurrent process never reads half a file.
        let part = dir.join(format!("{name}.{}.part", std::process::id()));
        std::fs::write(&part, &bytes).expect("temporary file");
        std::fs::rename(&part, &out).expect("rename");
    }
    done.insert(gz.to_path_buf(), out.clone());
    out
}

/// A scenario: an answer set transcribed from one of specify-core's tests, on one case.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// The file stem under `tests/fixtures/oracle/scenarios/`.
    pub name: String,
    /// The case whose analysis it answers.
    pub case: String,
    pub answers: Value,
    pub why: String,
    /// The Python test (file::function) it comes from.
    pub from: String,
    /// The whole input (`accept_defaults`, `write` when present).
    pub spec: Value,
    /// Python's result: `questions`, `open`, `problems`, `preview`, `apply` (and `write`,
    /// `readback` when the scenario writes), each a value or a failure (`is_error`).
    pub result: Option<Value>,
}

/// Every scenario, sorted by name.
pub fn scenarios() -> Vec<Scenario> {
    let dir = oracle_dir().join("scenarios");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_stem()
                .expect("stem")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path).expect("scenario");
            let spec: Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let text = |key: &str| spec[key].as_str().unwrap_or_default().to_owned();
            let result = dir.join(format!("{name}.result.json.gz"));
            Scenario {
                case: text("case"),
                answers: spec["answers"].clone(),
                why: text("why"),
                from: text("from"),
                result: result.is_file().then(|| read_json_gz(&result)),
                spec,
                name,
            }
        })
        .filter(|s| case_selected(&s.case))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------------------------

/// One place where the Rust value is not Python's. `None` on one side: the key or element is
/// missing there. Paths read `faces[3].area`, `features[id=holes:3].faces[0]` (an array aligned
/// by a key); the root is `""`.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    pub case: String,
    pub path: String,
    pub python: Option<Value>,
    pub rust: Option<Value>,
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let show = |v: &Option<Value>| match v {
            None => "(missing)".to_owned(),
            Some(v) => {
                let text = v.to_string();
                if text.chars().count() > 120 {
                    format!("{}...", text.chars().take(120).collect::<String>())
                } else {
                    text
                }
            }
        };
        write!(
            f,
            "{} {}: python {} rust {}",
            self.case,
            self.path,
            show(&self.python),
            show(&self.rust)
        )
    }
}

/// How numbers are compared, and which arrays are aligned by a key rather than by index.
/// Two numbers are equal when `|a - b| <= max(relative * max(|a|, |b|), absolute)`.
#[derive(Debug, Clone)]
pub struct Tolerances {
    pub relative: f64,
    pub absolute: f64,
    /// (path glob, relative, absolute): the first whose glob matches a number's path wins.
    pub overrides: Vec<(String, f64, f64)>,
    /// (path glob of an array, key): its elements are paired by `element[key]`.
    pub aligned: Vec<(String, String)>,
}

impl Default for Tolerances {
    /// Relative 1e-9, no absolute floor, no overrides, arrays by index.
    fn default() -> Self {
        Self {
            relative: 1e-9,
            absolute: 0.0,
            overrides: Vec::new(),
            aligned: Vec::new(),
        }
    }
}

impl Tolerances {
    /// Numbers at paths matching `glob` (e.g. `faces[*].area`) compare with these tolerances.
    pub fn with(mut self, glob: &str, relative: f64, absolute: f64) -> Self {
        self.overrides.push((glob.to_owned(), relative, absolute));
        self
    }

    /// The array at paths matching `glob` (e.g. `features`) is aligned by `element[key]`
    /// (e.g. `id`): elements pair by key, not position, and paths read `features[id=holes:3]`.
    /// Elements without the key, or with a key seen twice, fall back to their index.
    pub fn align(mut self, glob: &str, key: &str) -> Self {
        self.aligned.push((glob.to_owned(), key.to_owned()));
        self
    }

    fn numbers_at(&self, path: &str) -> (f64, f64) {
        self.overrides
            .iter()
            .find(|(glob, _, _)| glob_match(glob, path))
            .map_or((self.relative, self.absolute), |&(_, r, a)| (r, a))
    }

    fn key_at(&self, path: &str) -> Option<&str> {
        self.aligned
            .iter()
            .find(|(glob, _)| glob_match(glob, path))
            .map(|(_, key)| key.as_str())
    }
}

/// Every difference between `python` and `rust` (objects by key, in any order; arrays by
/// index or by key; numbers within `tolerances`; everything else exactly). Missing and extra
/// keys and elements are differences too.
pub fn compare(
    case: &str,
    python: &Value,
    rust: &Value,
    tolerances: &Tolerances,
) -> Vec<Difference> {
    let mut out = Vec::new();
    walk(case, "", python, rust, tolerances, &mut out);
    out
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

fn walk(
    case: &str,
    path: &str,
    python: &Value,
    rust: &Value,
    tol: &Tolerances,
    out: &mut Vec<Difference>,
) {
    let differ = |out: &mut Vec<Difference>| {
        out.push(Difference {
            case: case.to_owned(),
            path: path.to_owned(),
            python: Some(python.clone()),
            rust: Some(rust.clone()),
        })
    };
    match (python, rust) {
        (Value::Object(p), Value::Object(r)) => {
            for (key, pv) in p {
                let at = join(path, key);
                match r.get(key) {
                    Some(rv) => walk(case, &at, pv, rv, tol, out),
                    None => out.push(missing(case, at, Some(pv), None)),
                }
            }
            for (key, rv) in r {
                if !p.contains_key(key) {
                    out.push(missing(case, join(path, key), None, Some(rv)));
                }
            }
        }
        (Value::Array(p), Value::Array(r)) => {
            let keyed = tol
                .key_at(path)
                .and_then(|key| Some((key, by_key(p, key)?, by_key(r, key)?)));
            match keyed {
                Some((key, pk, rk)) => {
                    for (k, pv) in &pk {
                        let at = format!("{path}[{key}={k}]");
                        match rk.get(k) {
                            Some(rv) => walk(case, &at, pv, rv, tol, out),
                            None => out.push(missing(case, at, Some(pv), None)),
                        }
                    }
                    for (k, rv) in &rk {
                        if !pk.contains_key(k) {
                            out.push(missing(case, format!("{path}[{key}={k}]"), None, Some(rv)));
                        }
                    }
                }
                None => {
                    for i in 0..p.len().max(r.len()) {
                        let at = format!("{path}[{i}]");
                        match (p.get(i), r.get(i)) {
                            (Some(pv), Some(rv)) => walk(case, &at, pv, rv, tol, out),
                            (pv, rv) => out.push(missing(case, at, pv, rv)),
                        }
                    }
                }
            }
        }
        (Value::Number(p), Value::Number(r)) => {
            let equal = match (p.as_i64(), r.as_i64(), p.as_u64(), r.as_u64()) {
                (Some(a), Some(b), _, _) => a == b,
                (_, _, Some(a), Some(b)) => a == b,
                _ => {
                    let (a, b) = (
                        p.as_f64().unwrap_or(f64::NAN),
                        r.as_f64().unwrap_or(f64::NAN),
                    );
                    let (relative, absolute) = tol.numbers_at(path);
                    (a - b).abs() <= (relative * a.abs().max(b.abs())).max(absolute)
                }
            };
            if !equal {
                differ(out);
            }
        }
        _ if python == rust => {}
        _ => differ(out),
    }
}

fn missing(case: &str, path: String, python: Option<&Value>, rust: Option<&Value>) -> Difference {
    Difference {
        case: case.to_owned(),
        path,
        python: python.cloned(),
        rust: rust.cloned(),
    }
}

/// The elements of `items` by `item[key]` (a string as is, anything else as its JSON text);
/// None if any element lacks the key or two share one.
fn by_key<'a>(items: &'a [Value], key: &str) -> Option<BTreeMap<String, &'a Value>> {
    let mut out = BTreeMap::new();
    for item in items {
        let k = match item.get(key)? {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if out.insert(k, item).is_some() {
            return None;
        }
    }
    Some(out)
}

/// Whether `text` matches `glob`, where `*` matches any run of characters (including none,
/// dots and brackets) and every other character only itself.
pub fn glob_match(glob: &str, text: &str) -> bool {
    let (g, t): (Vec<char>, Vec<char>) = (glob.chars().collect(), text.chars().collect());
    let (mut gi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if gi < g.len() && g[gi] == '*' {
            star = Some((gi, ti));
            gi += 1;
        } else if gi < g.len() && g[gi] == t[ti] {
            gi += 1;
            ti += 1;
        } else if let Some((sg, st)) = star {
            gi = sg + 1;
            ti = st + 1;
            star = Some((sg, st + 1));
        } else {
            return false;
        }
    }
    g[gi..].iter().all(|&c| c == '*')
}

// ---------------------------------------------------------------------------------------------
// Known differences
// ---------------------------------------------------------------------------------------------

/// An entry of `tests/fixtures/known_differences/<area>.json`: the differences whose case and
/// path match its globs, what they are (`pattern`), whether Rust or Python is right
/// (`verdict`), and what was checked to say so (`evidence`).
#[derive(Debug, Clone)]
pub struct KnownDifference {
    pub case: String,
    pub path: String,
    pub pattern: String,
    pub verdict: String,
    pub evidence: String,
}

impl KnownDifference {
    pub fn matches(&self, difference: &Difference) -> bool {
        glob_match(&self.case, &difference.case) && glob_match(&self.path, &difference.path)
    }
}

/// `tests/fixtures/known_differences/<area>.json`.
pub fn known_differences_file(area: &str) -> PathBuf {
    fixtures()
        .join("known_differences")
        .join(format!("{area}.json"))
}

/// The entries of `path` (none if it does not exist yet), each checked: every field a
/// non-empty string, the verdict one of `VERDICTS`. Panics naming every bad entry.
pub fn load_known(path: &Path) -> Vec<KnownDifference> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let value: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: not JSON: {e}", path.display()));
    let Value::Array(items) = value else {
        panic!("{}: not a JSON array of entries", path.display());
    };
    let mut bad = Vec::new();
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match entry(item) {
            Ok(known) => out.push(known),
            Err(why) => bad.push(format!("  entry {i}: {why}")),
        }
    }
    assert!(
        bad.is_empty(),
        "{}: bad entries:\n{}",
        path.display(),
        bad.join("\n")
    );
    out
}

fn entry(item: &Value) -> Result<KnownDifference, String> {
    let empty = Map::new();
    let fields = item.as_object().unwrap_or(&empty);
    let field = |key: &str| match fields.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        _ => Err(format!("'{key}' must be a non-empty string")),
    };
    let known = KnownDifference {
        case: field("case")?,
        path: field("path")?,
        pattern: field("pattern")?,
        verdict: field("verdict")?,
        evidence: field("evidence")?,
    };
    if !VERDICTS.contains(&known.verdict.as_str()) {
        return Err(format!(
            "verdict '{}' is not one of {}",
            known.verdict,
            VERDICTS.join(", ")
        ));
    }
    if let Some(extra) = fields
        .keys()
        .find(|k| !["case", "path", "pattern", "verdict", "evidence"].contains(&k.as_str()))
    {
        return Err(format!("unknown field '{extra}'"));
    }
    Ok(known)
}

/// The asserting check, called once by `area`'s own full oracle sweep: every difference must
/// match an entry of `known_differences/<area>.json`, and every entry must match some
/// difference (else it is stale), for selected corpus cases. Unselected history is retained
/// for SPECIFY_CORPUS_ALL=1. Unknown and returning resolved differences always fail.
/// Panics listing every unmatched difference and every stale
/// entry; otherwise prints a tally of the differences by verdict.
pub fn check_known(area: &str, differences: &[Difference]) {
    let path = known_differences_file(area);
    let entries = load_known(&path);
    check_known_with(&path.display().to_string(), &entries, differences);
}

/// `check_known` against `entries` (named `source` in messages), for testing the harness.
pub fn check_known_with(source: &str, entries: &[KnownDifference], differences: &[Difference]) {
    let mut used = vec![0usize; entries.len()];
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut unmatched = Vec::new();
    for difference in differences {
        match entries
            .iter()
            .position(|e| e.verdict != "resolved" && e.matches(difference))
        {
            Some(i) => {
                used[i] += 1;
                *tally.entry(entries[i].verdict.as_str()).or_default() += 1;
            }
            None => unmatched.push(format!("  {difference}")),
        }
    }
    let stale: Vec<String> = entries
        .iter()
        .zip(&used)
        .filter(|(e, n)| e.verdict != "resolved" && **n == 0 && case_selected(&e.case))
        .map(|(e, _)| format!("  case {} path {} ({})", e.case, e.path, e.pattern))
        .collect();
    let mut problems = Vec::new();
    // Preserve corrected entries as negative regression assertions, rather than deleting
    // stale history. They never explain a departure and fail even if an active entry also
    // covers the returning difference.
    for entry in entries.iter().filter(|e| e.verdict == "resolved") {
        if let Some(difference) = differences.iter().find(|d| entry.matches(d)) {
            problems.push(format!(
                "resolved difference returned in {source}: {difference}; {}",
                entry.evidence
            ));
        }
    }
    if !unmatched.is_empty() {
        problems.push(format!(
            "{} difference(s) with no entry in {source}:\n{}",
            unmatched.len(),
            unmatched.join("\n")
        ));
    }
    if !stale.is_empty() {
        problems.push(format!(
            "{} stale entr(ies) in {source}, matching no difference:\n{}",
            stale.len(),
            stale.join("\n")
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    let counts: Vec<String> = tally.iter().map(|(v, n)| format!("{v} {n}")).collect();
    println!(
        "{source}: {} difference(s) explained ({})",
        differences.len(),
        if counts.is_empty() {
            "none".to_owned()
        } else {
            counts.join(", ")
        }
    );
}

/// The non-asserting filter, for sweeps that cross areas: the differences no entry of the
/// named areas' files matches. No stale check, no tally.
pub fn explained(areas: &[&str], differences: &[Difference]) -> Vec<Difference> {
    let entries: Vec<KnownDifference> = areas
        .iter()
        .flat_map(|area| load_known(&known_differences_file(area)))
        .collect();
    explained_with(&entries, differences)
}

/// `explained` against `entries`, for testing the harness.
pub fn explained_with(entries: &[KnownDifference], differences: &[Difference]) -> Vec<Difference> {
    differences
        .iter()
        .filter(|d| {
            !entries
                .iter()
                .any(|e| e.verdict != "resolved" && e.matches(d))
        })
        .cloned()
        .collect()
}

/// Whether `option` is a turned-diameter thread Python cannot offer (stream S3, issue #3): an
/// ISO 261 fine pitch (`thread:M10x1.25`), or a coarse size beyond Python's coarse table
/// (`thread:M30`).
pub fn is_thread_beyond_python(option: &str) -> bool {
    option.strip_prefix("thread:").is_some_and(|spec| {
        spec.contains('x')
            || specify_core::standards::lookup(&specify_core::standards::PITCH, spec).is_none()
    })
}

/// `rust` without the threads that Rust's `diameter.fit` questions offer beyond Python's (see
/// [`is_thread_beyond_python`]), from their `options` and `choices`, wherever such a question
/// sits in the value. Applied to Rust's side before comparing with the oracle, so whatever
/// else a diameter question offers must still equal Python's exactly.
pub fn without_threads_beyond_python(rust: &Value) -> Value {
    match rust {
        Value::Array(items) => {
            Value::Array(items.iter().map(without_threads_beyond_python).collect())
        }
        Value::Object(map) => {
            let diameter = map
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.starts_with("diameter.fit:"));
            let mut out = Map::new();
            for (key, v) in map {
                let kept = match (diameter, key.as_str(), v) {
                    (true, "options", Value::Array(options)) => Value::Array(
                        options
                            .iter()
                            .filter(|o| !o.as_str().is_some_and(is_thread_beyond_python))
                            .cloned()
                            .collect(),
                    ),
                    (true, "choices", Value::Array(choices)) => Value::Array(
                        choices
                            .iter()
                            .filter(|c| !c["value"].as_str().is_some_and(is_thread_beyond_python))
                            .cloned()
                            .collect(),
                    ),
                    _ => without_threads_beyond_python(v),
                };
                out.insert(key.clone(), kept);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

pub mod upgrade;
