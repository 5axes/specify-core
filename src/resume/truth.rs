//! On reopening a file Specify wrote, the PMI is the truth (ADR P9 §3).
//!
//! The stored answers are a private helper for resuming the interview. Reopening reads the
//! file's PMI and part settings first, as for any file, and compares them item by item with
//! what the stored answers would write: the plan of the stored answers (the writer's own
//! intent, as the PMI scene previews it) against the file's own scene, with the revision
//! comparison's item matching ([`crate::revision::against_plan`]) and every face its own.
//!
//! - **Same in both**: the answer resumes; the file's item is the answer's, not `existing`.
//! - **A default the file states** as the defaults would write it (written, never stored as
//!   an answer): `resume.present`, the file answering its question ([`answered`]).
//! - **Only in the file**: `existing`, present and kept (new information).
//! - **Different, or only in the answers**: a conflict for the person to decide. Until then
//!   the file's state is shown and is what would be written: the file's items stay in
//!   `existing` tagged with the question, the stored answer is not resumed, and the question
//!   is not asked. Answering the question (with the stored value, to restore it) decides it,
//!   and the tagged items give way; `_specify.keep` listing it keeps the file's.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::part::LoadedPart;
use crate::pmi_scene::Scene;

/// The answers metadata member listing conflicts decided by keeping the file's PMI.
pub const KEEP: &str = "keep";

/// A stored answer the file's PMI contradicts.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Conflict {
    /// The question.
    pub id: String,
    /// The stored answer.
    pub stored: Value,
    /// What the file states for it (its requirements' notation), `None` when the file has
    /// none: removed in the file.
    pub file: Option<String>,
    /// The file's requirement is another tool's (not marked as Specify's): answering the
    /// question does not replace it; it stands until the conflict is kept.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub foreign: bool,
}

/// The stored answers reconciled with the file's PMI.
#[derive(Clone, Debug, Default)]
pub(crate) struct Reconciled {
    /// The stored answers the file agrees with.
    pub answers: Map<String, Value>,
    pub conflicts: Vec<Conflict>,
    /// The file's items the resumed answers explain, by haecceity collection and index.
    pub explained: BTreeSet<(String, usize)>,
    /// The file's items each conflict's question stands for.
    pub contested: BTreeMap<(String, usize), String>,
    /// Part settings (`part` keys) the resumed answers explain.
    pub settings: BTreeSet<String>,
    /// Questions no stored answer states that the file answers with its own (Specify-marked)
    /// requirements, otherwise than their defaults would (`resume.file`).
    pub file_answered: Vec<String>,
    /// Questions not stored as answers whose defaults the file states, with those defaults:
    /// present in the file, so answered by it (`resume.present`).
    pub present: Map<String, Value>,
    /// The process the file states, with the attribute's provenance.
    pub process: Option<crate::bom::Field>,
    /// The attribute index and identity for a process Specify may keep as replaceable.
    pub process_keep_identity: Option<(usize, Value)>,
    /// The process belongs to another tool, even after its conflict has cleared.
    pub foreign_process: bool,
}

impl Reconciled {
    /// The [`crate::existing::Selection`] that leaves the explained items out of `existing`
    /// and tags the contested ones with their question.
    pub(crate) fn selection(&self) -> crate::existing::Selection {
        crate::existing::Selection {
            skip: self.explained.clone(),
            questions: self.contested.clone(),
        }
    }
}

/// The conflicts decided by keeping the file's PMI (`_specify.keep`).
pub fn kept(answers: &Map<String, Value>) -> BTreeSet<&str> {
    answers
        .get(crate::answer_file::META)
        .and_then(|meta| meta.get(KEEP))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

/// Whether the conflict on `id` is decided under `answers`: answered (restored, or answered
/// anew), or kept.
pub fn decided(answers: &Map<String, Value>, id: &str) -> bool {
    answers.contains_key(id) || kept(answers).contains(id)
}

/// The settings key of a part question's scene id (`part.notes:2` → `notes`).
fn setting(id: &str) -> Option<&str> {
    let key = id.strip_prefix("part.")?;
    Some(key.split_once(':').map_or(key, |(key, _)| key))
}

/// Each face's kind, by face index: the analysis lists one entry per face (an unmeasured face
/// keeps its place), so a face's kind is found at its own index.
fn face_kinds(analysis: &Value) -> Vec<String> {
    analysis["faces"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| f["kind"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[cfg(test)]
mod face_kind_tests {
    use super::face_kinds;
    use serde_json::json;

    #[test]
    fn an_unmeasured_face_keeps_its_place_in_the_kinds() {
        let analysis = json!({"faces": [
            {"id": 0, "kind": "cylinder", "unmeasured": true},
            {"id": 1, "kind": "plane", "area": 1.0, "centroid": [0, 0, 0]},
            {"id": 2, "kind": "cone", "area": 1.0, "centroid": [0, 0, 0]},
        ]});
        assert_eq!(face_kinds(&analysis), ["cylinder", "plane", "cone"]);
    }
}

/// Compare `analysis`'s stored answers (`resume.answers`) with the file's PMI.
/// `analysis` is the analysis as it was before reconciling (the stored answers resumed).
pub(crate) fn reconcile(
    loaded: &LoadedPart,
    analysis: &Value,
    features: &[crate::features::Feature],
) -> Result<Reconciled, String> {
    let stored = analysis["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let kept_earlier = kept(&stored);
    let scene = Scene::for_resume(loaded, features)?;
    let (file, index) = scene.file_comparison(loaded, analysis)?;
    let faces = loaded.binding.face_count;
    // What the answers write: their preview (defaults left as suggestions), as the editor
    // writes it; and with every default taken, as `apply` with defaults writes it. Defaults
    // can merge requirements (one location to a hole and a pattern), so an answer agrees with
    // the file when either plan states its requirements as the file does.
    // Answers matched to features recognised differently since they were stored (migrated,
    // or matched by ID without face evidence): only their requirements may sit on other faces.
    // Recognition changed since the file was written when its answers were matched without
    // face evidence or moved to features recognised anew: then any answer's requirement may
    // sit on the faces the old recognition gave it. In a file today's recognition reproduces,
    // only a face of another kind (a hole's cone or floor) is drift.
    let recognised_anew = analysis["resume"]["unverified"].is_string()
        || analysis["resume"]["migrated"]
            .as_array()
            .is_some_and(|m| !m.is_empty());
    let drift = crate::revision::Drift {
        questions: if recognised_anew {
            stored.keys().cloned().collect()
        } else {
            BTreeSet::new()
        },
        kinds: face_kinds(analysis),
    };
    // Which of the file's requirements are not marked as Specify's: another tool's, or one the
    // person kept (written unmarked).
    let (unmarked, process, kept_process) = {
        let doc =
            haecceity::p21::Document::parse(loaded.bytes.to_vec()).map_err(|e| e.to_string())?;
        let defs =
            haecceity::step::read_part_definitions(&loaded.bytes).map_err(|e| e.to_string())?;
        let read = haecceity::pmi::read(&doc, &defs).map_err(|e| e.to_string())?;
        let part = loaded.binding.part;
        let mut unmarked: BTreeSet<(String, usize)> =
            crate::provenance::unmarked(&doc, &read, part)
                .into_iter()
                .collect();
        let pmi = &read.parts[part];
        // Process answers and attributes also identify files whose property links carry
        // provenance when authored by Specify.
        let marked_attributes = super::scoped(&doc, part)
            || stored.contains_key(crate::process::QUESTION)
            || pmi.attributes.iter().any(crate::process::is_process_set);
        for (i, ids) in read.provenance.parts[part]
            .attributes
            .iter()
            .enumerate()
            .filter(|_| marked_attributes)
        {
            if !super::is_answers(&pmi.attributes[i])
                && !crate::provenance::marked(&doc, ids, "attribute")
            {
                unmarked.insert(("attributes".to_string(), i));
            }
        }
        let kept_process = if kept_earlier.contains(crate::process::QUESTION) {
            super::stored_in(&doc)
                .remove(&part)
                .and_then(|mut payload| payload.remove(super::KEPT_PROCESS))
        } else {
            None
        };
        let process =
            crate::process::file_pmi(&doc, pmi, &read.provenance.parts[part]).map(|(i, field)| {
                let set = &pmi.attributes[i];
                let identity = json!({"name": set.name, "items": set.items});
                (i, field, identity)
            });
        (unmarked, process, kept_process)
    };
    let plan = scene.comparison(loaded, analysis, Some(&stored))?;
    let mut truth = crate::revision::against_plan(&plan, &file, faces, &stored, &drift);
    let mut present = Map::new();
    let mut file_answered: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    // The defaults plan asks every question, those kept earlier too (`_specify.keep` would
    // leave them unasked), so their requirements can be found in the file.
    let mut unkept = stored.clone();
    if let Some(meta) = unkept
        .get_mut(crate::answer_file::META)
        .and_then(Value::as_object_mut)
    {
        meta.remove(KEEP);
    }
    if let Ok(all) = crate::rules::with_all_defaults(analysis, &unkept)
        && all.len() > stored.len()
    {
        let planned = scene.comparison(loaded, analysis, Some(&all))?;
        let other = crate::revision::against_plan(&planned, &file, faces, &stored, &drift);
        truth
            .conflicts
            .retain(|question, _| other.conflicts.contains_key(question));
        truth.explained.extend(other.explained);
        // A question both plans contradict keeps the file's items either names.
        for (question, items) in other.conflicts {
            if let Some(mine) = truth.conflicts.get_mut(&question) {
                for item in items {
                    if !mine.contains(&item) {
                        mine.push(item);
                    }
                }
            }
        }
        // A default the file states as the defaults would write it is present: the file
        // answers its question (`resume.present`), which stays answerable, not a suggestion.
        // Only Specify's own (marked) requirements are its defaults: one written unmarked is
        // another tool's, or one the person kept, and is simply the file's (`existing`).
        for (question, (unchanged, ids)) in other.defaults {
            // The process is read from the file with its own source (an accepted suggestion
            // stays one, `process::read`), so it is not taken as an answer here.
            let own = ids
                .iter()
                .all(|id| index.get(id).is_none_or(|at| !unmarked.contains(at)));
            if unchanged
                && own
                && !ids.is_empty()
                && question != crate::process::QUESTION
                && let Some(value) = all.get(&question)
            {
                truth.explained.extend(ids);
                present.insert(question, value.clone());
            }
        }
        // A question no stored answer states whose requirement Specify's own PMI states
        // otherwise than its default (changed elsewhere, or written from an intent): the file
        // answers it. Not a conflict, and its default is not restated: its requirements are
        // the file's, kept on writing, until the question is answered.
        // A question the person kept earlier (`_specify.keep`, stored with the answers) is the
        // file's whatever its marker: the kept requirement was written unmarked.
        for (question, items) in other.changed_defaults {
            let own = kept_earlier.contains(question.as_str())
                || items
                    .iter()
                    .any(|(id, _)| index.get(id).is_some_and(|at| !unmarked.contains(at)));
            if own && question != crate::process::QUESTION && !present.contains_key(&question) {
                file_answered.insert(question, items);
            }
        }
        let contested: BTreeSet<&String> = truth
            .conflicts
            .values()
            .chain(file_answered.values())
            .flatten()
            .map(|(id, _)| id)
            .collect();
        truth.explained.retain(|id| !contested.contains(id));
    }
    let item = |id: &str| index.get(id).cloned();
    let mut out = Reconciled {
        explained: truth.explained.iter().filter_map(|id| item(id)).collect(),
        ..Reconciled::default()
    };
    let mut conflicts: BTreeMap<String, Option<Vec<String>>> = BTreeMap::new();
    for (question, items) in &truth.conflicts {
        let mut texts = Vec::new();
        for (id, text) in items {
            if let Some(at) = item(id) {
                out.contested.insert(at, question.clone());
            }
            if !text.is_empty() && !texts.contains(text) {
                texts.push(text.clone());
            }
        }
        conflicts.insert(question.clone(), (!items.is_empty()).then_some(texts));
    }
    // Process affects the questions themselves and has no graphical requirement to match.
    // Compare its selected attribute directly and keep its index for the normal write rules.
    let question = crate::process::QUESTION;
    let saved_process = stored.get(question).filter(|_| {
        crate::rules::Ctx::new(analysis).is_ok_and(|ctx| crate::process::applies(&ctx, &stored))
    });
    let file_process = process.as_ref().map(|(_, field, _)| &field.value);
    let process_conflict = saved_process.is_some() && saved_process != file_process;
    if process_conflict {
        conflicts.insert(
            question.into(),
            file_process
                .and_then(Value::as_str)
                .map(|text| vec![text.into()]),
        );
    }
    if let Some((i, mut field, identity)) = process {
        let at = ("attributes".to_string(), i);
        let kept = kept_process.as_ref() == Some(&identity);
        out.foreign_process = field.source == "file" && !kept;
        if !out.foreign_process {
            out.process_keep_identity = Some((i, identity));
        }
        if !out.foreign_process || process_conflict {
            out.contested.insert(at, question.into());
        }
        if process_conflict && field.source == "default" {
            field.source = "answered".into();
        }
        if saved_process.is_none() && !out.foreign_process && field.source != "default" {
            out.file_answered.push(question.into());
        }
        out.process = Some(field);
    }
    // Part settings: each is one scene item named by its question (notes one per note).
    let mut settings: BTreeMap<&str, bool> = BTreeMap::new();
    for id in file["specify"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, _)| id)
    {
        if let Some(key) = setting(id) {
            let explained = truth.explained.contains(id);
            settings
                .entry(key)
                .and_modify(|all| *all &= explained)
                .or_insert(explained);
        }
    }
    out.settings = settings
        .into_iter()
        .filter(|(_, explained)| *explained)
        .map(|(key, _)| key.to_string())
        .collect();
    // Material is a part setting outside the item collections.
    const MATERIAL: &str = "part.material";
    if let Some(answer) = stored.get(MATERIAL) {
        let planned = &plan["pmi"]["pmi"]["material"];
        let actual = &file["pmi"]["pmi"]["material"];
        if !planned.is_null() && planned == actual {
            out.settings.insert("material".into());
        } else if !planned.is_null() {
            let text = actual["id"].as_str().map(str::to_owned);
            conflicts.insert(MATERIAL.into(), text.map(|t| vec![t]));
        } else {
            // The answer writes no material (e.g. an empty one): nothing to contradict.
            let _ = answer;
        }
    }
    for (id, answer) in &stored {
        match conflicts.get(id) {
            Some(file) => out.conflicts.push(Conflict {
                id: id.clone(),
                stored: answer.clone(),
                file: file.as_ref().map(|texts| texts.join("; ")),
                // Another tool's requirement (not marked as Specify's, and not one the person
                // kept): an answer never overrides it unasked; only keeping decides.
                foreign: if id == question {
                    out.foreign_process
                } else {
                    !kept_earlier.contains(id.as_str())
                        && out
                            .contested
                            .iter()
                            .any(|(at, q)| q == id && unmarked.contains(at))
                },
            }),
            None if id != crate::process::QUESTION || !out.foreign_process => {
                out.answers.insert(id.clone(), answer.clone());
            }
            None => {}
        }
    }
    if out.foreign_process
        && let Some(Value::Object(meta)) = out.answers.get_mut(crate::answer_file::META)
        && let Some(Value::Array(ids)) = meta.get_mut(KEEP)
    {
        ids.retain(|id| id != question);
        if ids.is_empty() {
            meta.remove(KEEP);
        }
        if meta.is_empty() {
            out.answers.remove(crate::answer_file::META);
        }
    }
    for (question, items) in file_answered {
        for (id, _) in &items {
            if let Some(at) = item(id) {
                out.contested.insert(at, question.clone());
            }
        }
        out.file_answered.push(question);
    }
    out.present = present;
    Ok(out)
}

/// The question a conflict is presented as (`kind: "conflict"`), for review and MCP.
pub fn question(conflict: &Conflict) -> Value {
    let stored = short(&conflict.stored);
    let (text, keep) = match &conflict.file {
        Some(file) if conflict.foreign => (
            format!(
                "Saved answer {stored}, file says {file}, another tool's requirement. Keep the \
                 file's {file}?"
            ),
            format!("Keep the file's {file}"),
        ),
        Some(file) => (
            format!(
                "Saved answer {stored}, file says {file}. Keep the file's {file}, or restore {stored}?"
            ),
            format!("Keep the file's {file}"),
        ),
        None => (
            format!("Saved answer {stored}: removed in the file. Restore it, or leave it removed?"),
            "Leave it removed".to_string(),
        ),
    };
    let restore = format!("Restore {stored}");
    // Another tool's requirement is not replaced by an answer: keeping is the only choice.
    let choices = if conflict.foreign {
        serde_json::json!([{"choice": "keep", "label": keep}])
    } else {
        serde_json::json!([
            {"choice": "keep", "label": keep},
            {"choice": "restore", "label": restore, "answer": conflict.stored},
        ])
    };
    let mut out = serde_json::json!({
        "id": conflict.id,
        "kind": "conflict",
        "text": text,
        "stored": conflict.stored,
        "file": conflict.file,
        "choices": choices,
    });
    if conflict.foreign {
        out["foreign"] = Value::Bool(true);
    }
    out
}

/// The conflicts of `analysis` not decided under `answers`.
pub fn open_conflicts(analysis: &Value, answers: &Map<String, Value>) -> Vec<Conflict> {
    analysis["resume"]["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| serde_json::from_value::<ConflictIn>(c.clone()).ok())
        // Another tool's requirement is decided only by keeping it: an answer, such as one
        // from answers saved before the file was edited, does not override it.
        .filter(|c| {
            if c.foreign {
                !kept(answers).contains(c.id.as_str())
            } else {
                !decided(answers, &c.id)
            }
        })
        .map(|c| Conflict {
            id: c.id,
            stored: c.stored,
            file: c.file,
            foreign: c.foreign,
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct ConflictIn {
    id: String,
    stored: Value,
    file: Option<String>,
    #[serde(default)]
    foreign: bool,
}

/// Every conflict question of `analysis`, decided or not.
/// The conflicts whose file requirement is another tool's (`foreign`): their question is not
/// asked, answered or not, and the file's requirement stands.
pub(crate) fn foreign_ids(analysis: &Value) -> BTreeSet<&str> {
    analysis["resume"]["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["foreign"] == Value::Bool(true))
        .filter_map(|c| c["id"].as_str())
        .collect()
}

pub(crate) fn conflict_ids(analysis: &Value) -> BTreeSet<&str> {
    analysis["resume"]["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_str())
        .collect()
}

/// The questions a reopened file's own PMI stands for until they are answered: its
/// conflicts and the questions it answers itself (`resume.file`). Not asked; answering one
/// gives the file's items way.
pub fn file_questions(analysis: &Value) -> BTreeSet<&str> {
    let mut out = conflict_ids(analysis);
    out.extend(
        analysis["resume"]["file"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str),
    );
    out
}

/// The answers the rules see under `analysis`: `answers`, and for each question they leave
/// unanswered the default a reopened file states (`resume.present`, ADR P9).
pub fn answered<'a>(
    analysis: &Value,
    answers: &'a Map<String, Value>,
) -> std::borrow::Cow<'a, Map<String, Value>> {
    let present = analysis["resume"]["present"].as_object();
    let process = crate::process::QUESTION;
    // An old process answer must not drive the rules when the file's foreign process wins.
    let foreign_process = answers.contains_key(process) && crate::process::foreign(analysis);
    if !foreign_process && present.is_none_or(|p| p.keys().all(|id| answers.contains_key(id))) {
        return std::borrow::Cow::Borrowed(answers);
    }
    let mut out = answers.clone();
    for (id, value) in present.into_iter().flatten() {
        if !out.contains_key(id) {
            out.insert(id.clone(), value.clone());
        }
    }
    if foreign_process {
        out.remove(process);
    }
    std::borrow::Cow::Owned(out)
}

/// `analysis` as the rules see it under `answers`: a conflict answered (restored, or answered
/// anew) gives way, so the file's contested items and setting leave `existing` and `part`.
pub fn restored<'a>(
    analysis: &'a Value,
    answers: &Map<String, Value>,
) -> std::borrow::Cow<'a, Value> {
    let foreign = foreign_ids(analysis);
    let answered: BTreeSet<&str> = file_questions(analysis)
        .into_iter()
        .filter(|id| answers.contains_key(*id) && !foreign.contains(id))
        .collect();
    if answered.is_empty() {
        return std::borrow::Cow::Borrowed(analysis);
    }
    let mut out = analysis.clone();
    if let Some(existing) = out["existing"].as_array_mut() {
        existing.retain(|e| !e["question"].as_str().is_some_and(|q| answered.contains(q)));
    }
    if let Some(part) = out["part"].as_object_mut() {
        for id in &answered {
            if let Some(key) = setting(id) {
                part.remove(key);
            }
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Short display of an answer value.
fn short(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Reconciled {
    /// The items of the file's PMI `pmi` a rewrite under `answers` keeps (ADR P9): every
    /// requirement the resumed answers do not explain, but those of a conflict `answers` now
    /// answers (restored, or answered anew), which give way. Specify's own bookkeeping (stored
    /// answers, characteristic numbers, dimension origins and plans) is never kept: the
    /// rewrite states it again. An attribute on a requirement follows that requirement.
    pub(crate) fn kept_items(
        &self,
        pmi: &haecceity::pmi::PartPmi,
        answers: &Map<String, Value>,
        writes_process: bool,
        restated: &BTreeSet<(String, usize)>,
    ) -> BTreeSet<(String, usize)> {
        use haecceity::pmi::NoteOwner;
        // Only the file's items of an undecided conflict or of a question the file answers
        // need keeping here: the rewrite replaces Specify's own (marked) requirements and
        // protects every other one, which is another tool's.
        let keeps = |collection: &str, i: usize| {
            let at = (collection.to_string(), i);
            !self.explained.contains(&at)
                && !restated.contains(&at)
                && self
                    .contested
                    .get(&at)
                    .is_some_and(|question| !self.replaces(question, answers, writes_process))
        };
        let mut out = BTreeSet::new();
        let lengths = [
            ("datums", pmi.datums.len()),
            ("datum_targets", pmi.datum_targets.len()),
            ("dimensions", pmi.dimensions.len()),
            ("tolerances", pmi.tolerances.len()),
            ("general", pmi.general.len()),
            ("threads", pmi.threads.len()),
            ("knurls", pmi.knurls.len()),
            ("notes", pmi.notes.len()),
            ("surface_textures", pmi.surface_textures.len()),
        ];
        for (collection, len) in lengths {
            for i in 0..len {
                if keeps(collection, i) {
                    out.insert((collection.to_string(), i));
                }
            }
        }
        // A datum the rewrite states with the same letter on other faces cannot be kept beside
        // it, nor therefore a tolerance citing the file's datum of that letter: the rewrite's
        // datum stands for the letter, as the answers state it.
        let cites = |tolerance: &haecceity::pmi::GeometricTolerance| -> Vec<usize> {
            tolerance
                .datums
                .iter()
                .flat_map(|system| system.compartments())
                .flat_map(|c| &c.references)
                .map(|r| r.datum.0)
                .collect()
        };
        for (i, tolerance) in pmi.tolerances.iter().enumerate() {
            if cites(tolerance)
                .iter()
                .any(|d| restated.contains(&("datum letters".to_string(), *d)))
            {
                out.remove(&("tolerances".to_string(), i));
            }
        }
        for (i, set) in pmi.attributes.iter().enumerate() {
            if super::is_answers(set)
                || [
                    crate::characteristics::PROPERTY,
                    crate::pmi_origin::PROPERTY,
                    "specify dimension plan",
                ]
                .contains(&set.name.as_str())
            {
                continue;
            }
            let owner = match set.on {
                Some(NoteOwner::Dimension(d)) => Some(("dimensions", d.0)),
                Some(NoteOwner::Tolerance(t)) => Some(("tolerances", t.0)),
                Some(NoteOwner::DatumTarget(t)) => Some(("datum_targets", t.0)),
                Some(NoteOwner::Feature(f)) => pmi
                    .threads
                    .iter()
                    .position(|t| t.feature == f)
                    .map(|t| ("threads", t))
                    .or_else(|| {
                        pmi.knurls
                            .iter()
                            .position(|k| k.feature == f)
                            .map(|k| ("knurls", k))
                    }),
                None => None,
            };
            let kept = match owner {
                Some((collection, index)) => out.contains(&(collection.to_string(), index)),
                None => keeps("attributes", i),
            };
            if kept {
                out.insert(("attributes".to_string(), i));
            }
        }
        // A kept tolerance keeps the datums it cites, and a kept datum its feature (linked to
        // the datum only from the feature's side, so not kept by the datum's own references):
        // the rewrite's equal datum is then that one, not a second of the same letter.
        for (i, tolerance) in pmi.tolerances.iter().enumerate() {
            if out.contains(&("tolerances".to_string(), i)) {
                // The rewrite's equal datum (same letter and faces) is then the kept one.
                for d in cites(tolerance) {
                    out.insert(("datums".to_string(), d));
                }
            }
        }
        let kept_datums: Vec<usize> = out
            .iter()
            .filter(|(c, _)| c == "datums")
            .map(|(_, i)| *i)
            .collect();
        for d in kept_datums {
            let Some(datum) = pmi.datums.get(d) else {
                continue;
            };
            let mut features: Vec<usize> = datum.feature().into_iter().map(|f| f.0).collect();
            for target in datum.targets() {
                out.insert(("datum_targets".to_string(), target.0));
                if let Some(t) = pmi.datum_targets.get(target.0) {
                    if let haecceity::pmi::TargetShape::Area(f)
                    | haecceity::pmi::TargetShape::Curve(f) = t.shape()
                    {
                        features.push(f.0);
                    }
                    features.extend(t.on().map(|f| f.0));
                }
            }
            for f in features {
                out.insert(("features".to_string(), f));
            }
        }
        if pmi.material.is_some()
            && self.conflicts.iter().any(|c| c.id == "part.material")
            && !restated.contains(&("material".to_string(), 0))
            && !answers.contains_key("part.material")
        {
            out.insert(("material".to_string(), 0));
        }
        out
    }
}

/// The items of the file's PMI `before` that the rewrite's own PMI `planned` states again: a
/// requirement with the same characteristic key, the general tolerance, the material, or a
/// part note of the same kind. A rewrite keeps the file's PMI, but does not state it twice.
pub(crate) fn restated(
    before: &haecceity::pmi::PartPmi,
    planned: &haecceity::pmi::PartPmi,
) -> BTreeSet<(String, usize)> {
    let keys: BTreeSet<String> = crate::characteristics::keyed(planned)
        .into_iter()
        .map(|(_, key)| key)
        .collect();
    let mut out: BTreeSet<(String, usize)> = crate::characteristics::keyed(before)
        .into_iter()
        .filter(|(_, key)| keys.contains(key))
        .map(|((collection, i), _)| (collection.to_string(), i))
        .collect();
    // A datum of a letter the rewrite states on other faces: the two cannot coexist.
    let letters: BTreeSet<&str> = planned.datums.iter().map(|d| d.label().as_str()).collect();
    for (i, datum) in before.datums.iter().enumerate() {
        if letters.contains(datum.label().as_str()) && !out.contains(&("datums".to_string(), i)) {
            out.insert(("datums".to_string(), i));
            out.insert(("datum letters".to_string(), i));
        }
    }
    if !planned.general.is_empty() {
        out.extend((0..before.general.len()).map(|i| ("general".to_string(), i)));
    }
    if planned.material.is_some() {
        out.insert(("material".to_string(), 0));
    }
    let part_notes = |p: &haecceity::pmi::PartPmi| -> Vec<(usize, Vec<String>)> {
        p.attributes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.on.is_none() && a.name == crate::writer::SEMANTIC_TEXT)
            .map(|(i, a)| (i, a.items.iter().map(|(name, _)| name.clone()).collect()))
            .collect()
    };
    let planned_notes: Vec<Vec<String>> = part_notes(planned).into_iter().map(|(_, n)| n).collect();
    for (i, names) in part_notes(before) {
        if planned_notes.iter().any(|n| {
            n.iter()
                .any(|name| names.contains(name) && name != crate::writer::NOTE_INDEX)
        }) {
            out.insert(("attributes".to_string(), i));
        }
    }
    out
}

impl Reconciled {
    /// The STEP entities of the file's items kept by a decision to keep them (a conflict whose
    /// question `answers` leave unanswered while `_specify.keep` lists it): the requirements
    /// themselves and the attributes on them, never the datums or features they rest on.
    pub(crate) fn decided_kept(
        &self,
        pmi: &haecceity::pmi::PartPmi,
        prov: &haecceity::pmi::PartProvenance,
        kept: &BTreeSet<(String, usize)>,
        answers: &Map<String, Value>,
    ) -> BTreeSet<u64> {
        use haecceity::pmi::NoteOwner;
        let decided = kept_ids(answers);
        let own: BTreeSet<&(String, usize)> = self
            .contested
            .iter()
            .filter(|(at, question)| kept.contains(*at) && decided.contains(question.as_str()))
            .map(|(at, _)| at)
            .collect();
        let ids = |collection: &str, i: usize| -> Vec<u64> {
            let list = match collection {
                "dimensions" => &prov.dimensions,
                "tolerances" => &prov.tolerances,
                "datums" => &prov.datums,
                "threads" => &prov.threads,
                "knurls" => &prov.knurls,
                "surface_textures" => &prov.surface_textures,
                "attributes" => &prov.attributes,
                "general" => &prov.general,
                "notes" => &prov.notes,
                _ => return Vec::new(),
            };
            list.get(i).cloned().unwrap_or_default()
        };
        let mut out = BTreeSet::new();
        for (collection, i) in &own {
            out.extend(ids(collection, *i));
        }
        for (i, set) in pmi.attributes.iter().enumerate() {
            let owner = match set.on {
                Some(NoteOwner::Dimension(d)) => Some(("dimensions", d.0)),
                Some(NoteOwner::Tolerance(t)) => Some(("tolerances", t.0)),
                Some(NoteOwner::Feature(f)) => pmi
                    .threads
                    .iter()
                    .position(|t| t.feature == f)
                    .map(|t| ("threads", t)),
                _ => None,
            };
            if owner.is_some_and(|(c, j)| own.contains(&(c.to_string(), j)))
                && kept.contains(&("attributes".to_string(), i))
            {
                out.extend(ids("attributes", i));
            }
        }
        if decided.contains("part.material") && kept.contains(&("material".to_string(), 0)) {
            out.extend(prov.material.iter().copied());
        }
        out
    }
}

fn kept_ids(answers: &Map<String, Value>) -> BTreeSet<&str> {
    kept(answers)
        .into_iter()
        .filter(|id| !answers.contains_key(*id))
        .collect()
}

/// The datums the tolerances `tolerances` of `pmi` cite, with the features and targets they
/// rest on (a datum's feature is linked from the feature's side, so keeping the datum alone
/// would orphan it): kept with another tool's tolerance, the rewrite's equal datum is that one.
pub(crate) fn datum_support(
    pmi: &haecceity::pmi::PartPmi,
    tolerances: &BTreeSet<usize>,
) -> BTreeSet<(String, usize)> {
    let mut out = BTreeSet::new();
    let mut datums = BTreeSet::new();
    for &i in tolerances {
        if let Some(system) = pmi.tolerances.get(i).and_then(|t| t.datums.as_ref()) {
            for compartment in system.compartments() {
                for reference in &compartment.references {
                    datums.insert(reference.datum.0);
                }
            }
        }
    }
    for d in datums {
        let Some(datum) = pmi.datums.get(d) else {
            continue;
        };
        out.insert(("datums".to_string(), d));
        let mut features: Vec<usize> = datum.feature().into_iter().map(|f| f.0).collect();
        for target in datum.targets() {
            out.insert(("datum_targets".to_string(), target.0));
            if let Some(t) = pmi.datum_targets.get(target.0) {
                if let haecceity::pmi::TargetShape::Area(f)
                | haecceity::pmi::TargetShape::Curve(f) = t.shape()
                {
                    features.push(f.0);
                }
                features.extend(t.on().map(|f| f.0));
            }
        }
        for f in features {
            out.insert(("features".to_string(), f));
        }
    }
    out
}

/// The reconciliations made most recently, by file (`source_sha256`) and part: a write reuses
/// the one the caller's analysis made of the same bytes instead of analysing the part again.
type Recent = Vec<((String, usize), Option<Reconciled>)>;

static RECENT: std::sync::Mutex<Recent> = std::sync::Mutex::new(Vec::new());

/// How many reconciliations [`RECENT`] holds.
const RECENT_LEN: usize = 8;

pub(crate) fn remember(loaded: &LoadedPart, reconciled: Option<&Reconciled>) {
    let key = (loaded.binding.source_sha256.clone(), loaded.binding.part);
    let mut recent = RECENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    recent.retain(|(k, _)| *k != key);
    recent.push((key, reconciled.cloned()));
    if recent.len() > RECENT_LEN {
        recent.remove(0);
    }
}

pub(crate) fn recall(loaded: &LoadedPart) -> Option<Option<Reconciled>> {
    let key = (loaded.binding.source_sha256.clone(), loaded.binding.part);
    RECENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, r)| r.clone())
}

impl Reconciled {
    /// The file's items of the conflicts and file-answered questions `answers` now answer
    /// (restored, or answered anew), with the attributes on them: they give way on writing,
    /// marked as Specify's or not.
    pub(crate) fn give_way(
        &self,
        pmi: &haecceity::pmi::PartPmi,
        answers: &Map<String, Value>,
        writes_process: bool,
    ) -> BTreeSet<(String, usize)> {
        use haecceity::pmi::NoteOwner;
        let mut out: BTreeSet<(String, usize)> = self
            .contested
            .iter()
            .filter(|(_, question)| self.replaces(question, answers, writes_process))
            .map(|(at, _)| at.clone())
            .collect();
        for (i, set) in pmi.attributes.iter().enumerate() {
            let owner = match set.on {
                Some(NoteOwner::Dimension(d)) => Some(("dimensions", d.0)),
                Some(NoteOwner::Tolerance(t)) => Some(("tolerances", t.0)),
                _ => None,
            };
            if owner.is_some_and(|(c, j)| out.contains(&(c.to_string(), j))) {
                out.insert(("attributes".to_string(), i));
            }
        }
        out
    }
}

impl Reconciled {
    fn replaces(&self, question: &str, answers: &Map<String, Value>, writes_process: bool) -> bool {
        let answered = if question == crate::process::QUESTION {
            writes_process
        } else {
            answers.contains_key(question)
        };
        answered && !self.is_foreign(question)
    }

    /// Whether `question` is a conflict whose file requirement is another tool's.
    fn is_foreign(&self, question: &str) -> bool {
        if question == crate::process::QUESTION {
            self.foreign_process
        } else {
            self.conflicts.iter().any(|c| c.id == question && c.foreign)
        }
    }
}
