//! A Specify session (docs/session.md): one opened STEP file (or, for a diff, the pair), its
//! answers by part, a revision number `rev` that goes up by one on every change, the recent
//! changes, and the pages listening for them. The MCP server (`specify-core mcp`) holds one per
//! opened file and the page server (src/open.rs) serves one, so the agent and the page edit one
//! set of answers; `specify-core open` holds one with no agent.
//!
//! Save writes the session's answers with the same writer as `specify-core write` (the answers
//! stored in the file), beside the file under the first free `<stem>.specified.step`, or over it
//! when opened in place (the original kept once as `.bak`). An incomplete save is written: what
//! is answered, and the answer says what is still open.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::api::{self, Error};
use crate::part::{self, LoadError};
use crate::rules::{self, Intent};
use crate::writer::WriteError;

/// The changes kept for `changes since`; an older `since` gets the whole answers instead.
pub const KEPT_CHANGES: usize = 256;

/// A session shared between the MCP server and the page server it started.
pub type Shared = Arc<Mutex<Session>>;

/// Locks a session, whatever a panicking holder left (its state is always whole).
pub fn lock(session: &Shared) -> std::sync::MutexGuard<'_, Session> {
    session.lock().unwrap_or_else(|p| p.into_inner())
}

/// A file as opened: its path (absolute, as given or as the caller chose), its name and its
/// bytes as they were on disk (gzipped if the file is).
#[derive(Clone)]
pub struct Opened {
    pub path: PathBuf,
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
}

impl Opened {
    /// `path` made absolute (not resolved: a link stays the link) and read.
    pub fn read(path: &Path) -> Result<Opened, Error> {
        let unreadable = |e: io::Error| {
            Error::Load(LoadError::Unreadable {
                path: path.to_path_buf(),
                reason: e.to_string(),
            })
        };
        let path = std::path::absolute(path).map_err(unreadable)?;
        let bytes = fs::read(&path).map_err(unreadable)?;
        Ok(Opened::of(path, bytes))
    }

    /// A file already read from `path`.
    pub fn of(path: PathBuf, bytes: Vec<u8>) -> Opened {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Opened {
            path,
            name,
            bytes: Arc::new(bytes),
        }
    }
}

/// One change to the answers: `set` the answers that are new or different, `cleared` the ids
/// no longer answered.
#[derive(Clone, Debug)]
pub struct Change {
    pub rev: u64,
    pub part: usize,
    pub source: &'static str,
    pub set: Map<String, Value>,
    pub cleared: Vec<String>,
}

impl Change {
    pub fn to_json(&self) -> Value {
        json!({"rev": self.rev, "part": self.part, "source": self.source,
               "set": self.set, "cleared": self.cleared})
    }
}

/// A save written: where, how many bytes, at which `rev`, `write`'s report, and whether any
/// required question is still open.
#[derive(Clone, Debug)]
pub struct Saved {
    pub path: String,
    pub bytes: usize,
    pub rev: u64,
    pub report: Value,
    pub complete: bool,
    pub missing: Vec<String>,
}

impl Saved {
    pub fn to_json(&self) -> Value {
        json!({"path": self.path, "bytes": self.bytes, "rev": self.rev, "report": self.report,
               "complete": self.complete, "missing": self.missing})
    }
}

/// Why nothing was saved.
#[derive(Debug)]
pub enum SaveError {
    /// An in-place save over a file changed on disk since it was opened (by anyone but this
    /// session).
    Changed(String),
    /// An answer the writer refuses (the message names the question).
    Refused(String),
    /// The file could not be written.
    Failed(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Changed(m) | SaveError::Refused(m) | SaveError::Failed(m) => f.write_str(m),
        }
    }
}

/// One opened file's answers and what listens to them.
pub struct Session {
    file: Opened,
    /// SHA-256 of the file's bytes as opened.
    sha: String,
    /// `diff` mode: the old revision.
    old: Option<Opened>,
    /// `diff` mode: the revision report (`diff`'s report.json).
    report: Option<Value>,
    in_place: bool,
    gz: bool,
    /// The file, parsed from its bytes as opened (every save writes from these).
    step: part::File,
    /// What `file` serves: the input, or what the last in-place save left.
    current: Arc<Vec<u8>>,
    /// In place: where the first save kept the original.
    backup: Option<PathBuf>,
    /// In place: SHA-256 of what the last save wrote, so the file is not taken as changed.
    written: Option<String>,
    analyses: BTreeMap<usize, Arc<Value>>,
    answers: BTreeMap<usize, Map<String, Value>>,
    rev: u64,
    changes: VecDeque<Change>,
    /// The newest `rev` whose change is no longer kept (0: none dropped).
    dropped: u64,
    subscribers: Vec<mpsc::Sender<String>>,
    /// The page server serving this session, if one was started.
    pub page: Option<crate::open::Page>,
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Session {
    /// A session of `file` with no answers yet.
    pub fn new(file: Opened, in_place: bool) -> Result<Session, Error> {
        let step = part::File::of_shared(&file.path, Arc::clone(&file.bytes))?;
        Ok(Session::of(file, step, in_place))
    }

    /// [`Session::new`] of a file the caller has parsed from `file`'s bytes.
    pub fn of(file: Opened, step: part::File, in_place: bool) -> Session {
        let gz = file.name.to_ascii_lowercase().ends_with(".gz");
        Session {
            sha: sha256(&file.bytes),
            current: Arc::clone(&file.bytes),
            file,
            old: None,
            report: None,
            in_place,
            gz,
            step,
            backup: None,
            written: None,
            analyses: BTreeMap::new(),
            answers: BTreeMap::new(),
            rev: 0,
            changes: VecDeque::new(),
            dropped: 0,
            subscribers: Vec::new(),
            page: None,
        }
    }

    /// `specify-core open FILE`: a session starting from the answers stored in the file (a
    /// file Specify wrote resumes them, part by part, as `analyse` reports them).
    pub fn open(file: Opened, in_place: bool) -> Result<Session, Error> {
        let mut session = Session::new(file, in_place)?;
        let written: Vec<usize> = api::parts_of(&session.step)
            .iter()
            .filter(|p| p["written"] == json!(true))
            .filter_map(|p| p["part"].as_u64().map(|k| k as usize))
            .collect();
        for k in written {
            let analysis = session.analysis(k)?;
            if let Some(resumed) = analysis["resume"]["answers"].as_object() {
                session.answers.insert(k, resumed.clone());
            }
        }
        Ok(session)
    }

    /// `specify-core open --diff OLD NEW` (and MCP's `open` with `old`): `diff OLD NEW` compared
    /// as `specify-core diff` compares them (part `part`, or the files' only parts, or their
    /// assemblies), with the old answers `given` (else those stored in OLD); the session starts
    /// from the answers it carried to NEW.
    pub fn diff(
        old: Opened,
        new: Opened,
        part: Option<usize>,
        given: &Map<String, Value>,
        in_place: bool,
    ) -> Result<Session, Error> {
        let mut session = Session::new(new, in_place)?;
        let old_file = part::File::of_shared(&old.path, Arc::clone(&old.bytes))?;
        let compared = crate::revision::compare(
            &old_file,
            &session.step,
            part,
            given,
            crate::revision::DiffOptions::default(),
        )?;
        session.report = Some(compared.report());
        session.answers = compared.answers_by_part();
        session.old = Some(old);
        Ok(session)
    }

    pub fn file(&self) -> &Opened {
        &self.file
    }

    pub fn old(&self) -> Option<&Opened> {
        self.old.as_ref()
    }

    /// `diff` mode: the revision report.
    pub fn report(&self) -> Option<&Value> {
        self.report.as_ref()
    }

    pub fn in_place(&self) -> bool {
        self.in_place
    }

    pub fn set_in_place(&mut self, in_place: bool) {
        self.in_place = in_place;
    }

    /// SHA-256 of the file's bytes as opened.
    pub fn sha(&self) -> &str {
        &self.sha
    }

    /// Whether bytes with this SHA-256 are the file as opened, or as this session's own last
    /// in-place save left it.
    pub fn is_ours(&self, sha: &str) -> bool {
        sha == self.sha || self.written.as_deref() == Some(sha)
    }

    /// What `file` serves: the input as opened, or as the last in-place save left it.
    pub fn current(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.current)
    }

    /// The file as opened, parsed.
    pub fn step(&self) -> &part::File {
        &self.step
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Part `part`'s analysis, made the first time it is asked for (the part is then open in
    /// the session).
    pub fn analysis(&mut self, part: usize) -> Result<Arc<Value>, Error> {
        if let Some(a) = self.analyses.get(&part) {
            return Ok(Arc::clone(a));
        }
        let loaded = self.step.load(Some(part))?;
        let (analysis, _) = api::analyse_reporting_in(&self.step, &loaded)?;
        let analysis = Arc::new(analysis);
        self.analyses.insert(part, Arc::clone(&analysis));
        Ok(analysis)
    }

    /// Part `part`'s analysis, made by the caller (MCP's `open`).
    pub fn keep_analysis(&mut self, part: usize, analysis: Arc<Value>) {
        self.analyses.entry(part).or_insert(analysis);
    }

    /// The parts the file has.
    pub fn part_count(&self) -> usize {
        self.step.definitions().len()
    }

    /// Part `part`'s answers, if the session holds any for it.
    pub fn answers(&self, part: usize) -> Option<&Map<String, Value>> {
        self.answers.get(&part)
    }

    /// Every part's answers, by part index as a string: `answers`' `parts`.
    pub fn parts(&self) -> Value {
        Value::Object(
            self.answers
                .iter()
                .map(|(k, a)| (k.to_string(), Value::Object(a.clone())))
                .collect(),
        )
    }

    /// Starts part `part` from `answers` without recording a change (a session just made,
    /// before anyone has seen it).
    pub fn start_answers(&mut self, part: usize, answers: Map<String, Value>) {
        self.answers.insert(part, answers);
    }

    /// Replaces part `part`'s answers: the new `rev` and the change, or None when nothing
    /// changed. Every subscriber gets an `answers` event.
    pub fn set(
        &mut self,
        part: usize,
        answers: Map<String, Value>,
        source: &'static str,
    ) -> Option<u64> {
        let before = self.answers.get(&part);
        if before == Some(&answers) || (before.is_none() && answers.is_empty()) {
            if before.is_none() {
                self.answers.insert(part, answers);
            }
            return None;
        }
        let empty = Map::new();
        let before = before.unwrap_or(&empty);
        let set: Map<String, Value> = answers
            .iter()
            .filter(|(id, v)| before.get(*id) != Some(*v))
            .map(|(id, v)| (id.clone(), v.clone()))
            .collect();
        let cleared: Vec<String> = before
            .keys()
            .filter(|id| !answers.contains_key(*id))
            .cloned()
            .collect();
        self.rev += 1;
        let rev = self.rev;
        self.changes.push_back(Change {
            rev,
            part,
            source,
            set,
            cleared,
        });
        while self.changes.len() > KEPT_CHANGES {
            if let Some(gone) = self.changes.pop_front() {
                self.dropped = gone.rev;
            }
        }
        let data = json!({"rev": rev, "part": part, "answers": answers, "source": source});
        self.answers.insert(part, answers);
        self.publish(&event("answers", Some(rev), &data));
        Some(rev)
    }

    /// The changes after `since`, oldest first; None when some of them are no longer kept.
    pub fn changes_since(&self, since: u64) -> Option<Vec<&Change>> {
        if since < self.dropped {
            return None;
        }
        Some(self.changes.iter().filter(|c| c.rev > since).collect())
    }

    /// A new subscriber: its first event is the current state (`source: "start"`), then one
    /// per change and save.
    pub fn subscribe(&mut self) -> mpsc::Receiver<String> {
        let (tell, events) = mpsc::channel();
        let data = json!({"rev": self.rev, "parts": self.parts(), "source": "start"});
        let _ = tell.send(event("answers", Some(self.rev), &data));
        self.subscribers.push(tell);
        events
    }

    fn publish(&mut self, text: &str) {
        self.subscribers
            .retain(|s| s.send(text.to_string()).is_ok());
    }

    /// Where the next save goes: the input (in place), or the first free
    /// `<stem>.specified.step` beside it.
    pub fn save_target(&self) -> Option<PathBuf> {
        if self.in_place {
            return Some(self.file.path.clone());
        }
        let dir = self.file.path.parent()?;
        let stem = stem(&self.file.name);
        (1..=MAX_CANDIDATES)
            .map(|n| candidate(dir, stem, n))
            .find(|p| fs::symlink_metadata(p).is_err())
    }

    /// The parts a save writes: every part with answers or open in the session, and a file's
    /// only part.
    fn parts_to_write(&self) -> Vec<usize> {
        let mut parts: BTreeSet<usize> = self.answers.keys().copied().collect();
        parts.extend(self.analyses.keys().copied());
        if self.part_count() == 1 {
            parts.insert(0);
        }
        parts.into_iter().collect()
    }

    /// Writes the session's answers (docs/open.md, `save`), from the file as opened, with
    /// `write`'s writer; `accept_defaults` fills each open question that has a default, as
    /// `write --accept-defaults` does. Open questions never refuse it: what is answered is
    /// written and `missing` says what is still open. Every subscriber gets a `saved` event.
    pub fn save(&mut self, accept_defaults: bool, source: &str) -> Result<Saved, SaveError> {
        let refused = |e: Error| match e {
            Error::Rules(rules::Error::Invalid(m)) | Error::Write(WriteError::Invalid(m)) => {
                SaveError::Refused(m)
            }
            Error::Rules(e) => SaveError::Refused(e.to_string()),
            other => SaveError::Failed(other.to_string()),
        };
        if self.in_place {
            let now = fs::read(&self.file.path)
                .map(|b| sha256(&b))
                .map_err(|e| SaveError::Failed(format!("{}: {e}", self.file.path.display())))?;
            if !self.is_ours(&now) {
                return Err(SaveError::Changed(format!(
                    "{} has changed on disk since it was opened: nothing is written over it",
                    self.file.path.display()
                )));
            }
        }
        let parts = self.parts_to_write();
        let mut loaded = Vec::with_capacity(parts.len());
        let mut intents = Vec::with_capacity(parts.len());
        let mut missing = Vec::new();
        let empty = Map::new();
        for &k in &parts {
            let analysis = self.analysis(k).map_err(refused)?;
            let answers = self.answers.get(&k).unwrap_or(&empty);
            let part = self.step.load(Some(k)).map_err(|e| refused(e.into()))?;
            let (intent, open) =
                intent_for(&part, &analysis, answers, accept_defaults).map_err(|e| {
                    let e = refused(e);
                    match (e, parts.len() > 1) {
                        (SaveError::Refused(m), true) => {
                            SaveError::Refused(format!("part {k}: {m}"))
                        }
                        (e, _) => e,
                    }
                })?;
            missing.extend(open);
            loaded.push(part);
            intents.push(intent);
        }
        let jobs: Vec<crate::writer::Job<'_>> = parts
            .iter()
            .zip(&loaded)
            .zip(&intents)
            .map(|((k, l), i)| (l, i, Some(self.answers.get(k).unwrap_or(&empty))))
            .collect();
        let (text, report) =
            crate::writer::write_parts_bytes(&jobs).map_err(|e| refused(Error::Write(e)))?;
        let mut report = report.to_dict();
        let (path, bytes) = if self.in_place {
            self.replace_input(text)
        } else {
            let bytes = text.len();
            self.write_new(&text).map(|p| (p, bytes))
        }
        .map_err(|e| SaveError::Failed(format!("cannot save: {e}")))?;
        let shown = path.display().to_string();
        report["output"] = json!(shown);
        let saved = Saved {
            path: shown,
            bytes,
            rev: self.rev,
            report,
            complete: missing.is_empty(),
            missing,
        };
        let data = json!({"path": saved.path, "bytes": saved.bytes, "rev": saved.rev,
                          "source": source});
        self.publish(&event("saved", None, &data));
        Ok(saved)
    }

    /// A save beside the input under the first free candidate name, taken with `create_new`
    /// (nothing is overwritten, even in a race), the written temporary file renamed onto it.
    fn write_new(&self, body: &[u8]) -> io::Result<PathBuf> {
        let dir = self
            .file
            .path
            .parent()
            .ok_or_else(|| io::Error::other("the input has no directory"))?;
        let stem = stem(&self.file.name);
        let temp = temp_file(dir, body)?;
        for n in 1..=MAX_CANDIDATES {
            let target = candidate(dir, stem, n);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
            {
                Ok(_) => {
                    if let Err(e) = fs::rename(&temp, &target) {
                        let _ = fs::remove_file(&temp);
                        let _ = fs::remove_file(&target);
                        return Err(e);
                    }
                    return Ok(target);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    let _ = fs::remove_file(&temp);
                    return Err(e);
                }
            }
        }
        let _ = fs::remove_file(&temp);
        Err(io::Error::other("every candidate name is taken"))
    }

    /// In place: the input replaced by renaming a written temporary file onto it. The first
    /// save of the session first copies the original to the first free `.bak` beside the path
    /// as given; later saves keep no other copy. A link is followed (its target replaced, the
    /// link kept). The replacement keeps the original's permissions, and a gzipped input is
    /// written gzipped. Returns the path as given and the bytes written.
    fn replace_input(&mut self, text: Vec<u8>) -> io::Result<(PathBuf, usize)> {
        let step = self.file.path.clone();
        let real = fs::canonicalize(&step)?;
        let dir = real
            .parent()
            .ok_or_else(|| io::Error::other("the input has no directory"))?;
        let stored = if self.gz {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gz.write_all(&text)?;
            gz.finish()?
        } else {
            text
        };
        let temp = temp_file(dir, &stored)?;
        let replaced = (|| {
            fs::set_permissions(&temp, fs::metadata(&real)?.permissions())?;
            if self.backup.is_none() {
                self.backup = Some(back_up(&step, &real)?);
            }
            fs::rename(&temp, &real)
        })();
        if let Err(e) = replaced {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        let bytes = stored.len();
        self.written = Some(sha256(&stored));
        self.current = Arc::new(stored);
        Ok((step, bytes))
    }
}

/// The intent `write` writes for these answers, and the required questions still open. When
/// none is open it is exactly `write`'s (`api::apply`). Otherwise what is answered (with
/// `accept_defaults`, and every default taken): the preview, with what `apply` reports as not
/// written, as the editor writes an incomplete part.
fn intent_for(
    loaded: &part::LoadedPart,
    analysis: &Value,
    answers: &Map<String, Value>,
    accept_defaults: bool,
) -> Result<(Intent, Vec<String>), Error> {
    if answers.contains_key(crate::answer_file::META) {
        crate::answer_file::check(loaded, answers)
            .map_err(|why| Error::Rules(rules::Error::Invalid(why)))?;
    }
    match api::apply(analysis, answers, accept_defaults) {
        Ok(intent) => Ok((intent, Vec::new())),
        Err(Error::Rules(rules::Error::Incomplete(e))) => {
            let given = if accept_defaults {
                rules::with_all_defaults(analysis, answers)?
            } else {
                answers.clone()
            };
            let mut intent = api::preview(analysis, &given)?;
            intent
                .requirements
                .extend(rules::unasked_rows(analysis, &given)?);
            Ok((intent, e.missing))
        }
        Err(e) => Err(e),
    }
}

/// A server-sent event.
pub fn event(name: &str, id: Option<u64>, data: &Value) -> String {
    match id {
        Some(id) => format!("event: {name}\nid: {id}\ndata: {data}\n\n"),
        None => format!("event: {name}\ndata: {data}\n\n"),
    }
}

/// The input's name without `.gz` and `.step`/`.stp`.
fn stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    let mut end = name.len();
    if lower.ends_with(".gz") {
        end -= 3;
    }
    let base = &lower[..end];
    if base.ends_with(".step") {
        end -= 5;
    } else if base.ends_with(".stp") {
        end -= 4;
    }
    &name[..end]
}

/// The `n`th name a save may take: `<stem>.specified.step`, then `<stem>.specified-2.step`, …
fn candidate(dir: &Path, stem: &str, n: u32) -> PathBuf {
    if n == 1 {
        dir.join(format!("{stem}.specified.step"))
    } else {
        dir.join(format!("{stem}.specified-{n}.step"))
    }
}

/// The highest suffix tried before a save gives up.
const MAX_CANDIDATES: u32 = 10_000;

/// `body` written to a new temporary file in `dir`, synced.
fn temp_file(dir: &Path, body: &[u8]) -> io::Result<PathBuf> {
    let tag = crate::open::token().map_err(|e| io::Error::other(e.to_string()))?;
    let path = dir.join(format!(".specify-save-{}.tmp", &tag[..12]));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let written = file.write_all(body).and_then(|_| file.sync_all());
    if let Err(e) = written {
        let _ = fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// `real` (the input's file) copied to the first free name of `<step>.bak`, `<step>.bak-2`, …
/// (never over a file); the name taken.
fn back_up(step: &Path, real: &Path) -> io::Result<PathBuf> {
    for n in 1..=MAX_CANDIDATES {
        let mut name = step.as_os_str().to_os_string();
        if n == 1 {
            name.push(".bak");
        } else {
            name.push(format!(".bak-{n}"));
        }
        let bak = PathBuf::from(name);
        match OpenOptions::new().write(true).create_new(true).open(&bak) {
            Ok(mut out) => {
                let copied = File::open(real)
                    .and_then(|mut input| io::copy(&mut input, &mut out))
                    .and_then(|_| out.sync_all());
                if let Err(e) = copied {
                    let _ = fs::remove_file(&bak);
                    return Err(e);
                }
                return Ok(bak);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("every .bak name is taken"))
}
