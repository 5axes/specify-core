//! On reopening a file Specify wrote, the PMI is the truth (ADR P9 §3).
//!
//! The stored answers are a private helper for resuming the interview. Reopening reads the
//! file's PMI and part settings first, as for any file, and compares them item by item with
//! what the stored answers would write: the plan of the stored answers (the writer's own
//! intent, as the PMI scene previews it) against the file's own scene, with the revision
//! comparison's item matching ([`crate::revision::against_plan`]) and every face its own.
//!
//! - **Same in both**: the answer resumes; the file's item is the answer's, not `existing`.
//! - **Only in the file**: `existing`, present and kept (new information).
//! - **Different, or only in the answers**: a conflict for the person to decide. Until then
//!   the file's state is shown and is what would be written: the file's items stay in
//!   `existing` tagged with the question, the stored answer is not resumed, and the question
//!   is not asked. Answering the question (with the stored value, to restore it) decides it,
//!   and the tagged items give way; `_specify.keep` listing it keeps the file's.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};

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
    let scene = Scene::with(loaded, Some(features))?;
    let (file, index) = scene.file_comparison(loaded, analysis)?;
    let faces = loaded.binding.face_count;
    // What the answers write: their preview (defaults left as suggestions), as the editor
    // writes it; and with every default taken, as `apply` with defaults writes it. Defaults
    // can merge requirements (one location to a hole and a pattern), so an answer agrees with
    // the file when either plan states its requirements as the file does.
    let plan = scene.comparison(loaded, analysis, Some(&stored))?;
    let mut truth = crate::revision::against_plan(&plan, &file, faces, &stored);
    if !truth.conflicts.is_empty()
        && let Ok(all) = crate::rules::with_all_defaults(analysis, &stored)
        && all.len() > stored.len()
    {
        let planned = scene.comparison(loaded, analysis, Some(&all))?;
        let other = crate::revision::against_plan(&planned, &file, faces, &stored);
        truth.conflicts.retain(|question, _| other.conflicts.contains_key(question));
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
        let contested: BTreeSet<&String> = truth
            .conflicts
            .values()
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
        conflicts.insert(
            question.clone(),
            (!items.is_empty()).then_some(texts),
        );
    }
    // Part settings: each is one scene item named by its question (notes one per note).
    let mut settings: BTreeMap<&str, bool> = BTreeMap::new();
    for id in file["specify"].as_object().into_iter().flatten().map(|(id, _)| id) {
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
            }),
            None => {
                out.answers.insert(id.clone(), answer.clone());
            }
        }
    }
    Ok(out)
}

/// The question a conflict is presented as (`kind: "conflict"`), for review and MCP.
pub fn question(conflict: &Conflict) -> Value {
    let stored = short(&conflict.stored);
    let (text, keep) = match &conflict.file {
        Some(file) => (
            format!(
                "Saved answer {stored}, file says {file}. Keep the file's {file}, or restore {stored}?"
            ),
            format!("Keep the file's {file}"),
        ),
        None => (
            format!(
                "Saved answer {stored}: removed in the file. Restore it, or leave it removed?"
            ),
            "Leave it removed".to_string(),
        ),
    };
    serde_json::json!({
        "id": conflict.id,
        "kind": "conflict",
        "text": text,
        "stored": conflict.stored,
        "file": conflict.file,
        "choices": [
            {"choice": "keep", "label": keep},
            {"choice": "restore", "label": format!("Restore {stored}"), "answer": conflict.stored},
        ],
    })
}

/// The conflicts of `analysis` not decided under `answers`.
pub fn open_conflicts(analysis: &Value, answers: &Map<String, Value>) -> Vec<Conflict> {
    analysis["resume"]["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| serde_json::from_value::<ConflictIn>(c.clone()).ok())
        .filter(|c| !decided(answers, &c.id))
        .map(|c| Conflict {
            id: c.id,
            stored: c.stored,
            file: c.file,
        })
        .collect()
}

#[derive(serde::Deserialize)]
struct ConflictIn {
    id: String,
    stored: Value,
    file: Option<String>,
}

/// Every conflict question of `analysis`, decided or not.
pub(crate) fn conflict_ids(analysis: &Value) -> BTreeSet<&str> {
    analysis["resume"]["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_str())
        .collect()
}

/// `analysis` as the rules see it under `answers`: a conflict answered (restored, or answered
/// anew) gives way, so the file's contested items and setting leave `existing` and `part`.
pub fn restored<'a>(
    analysis: &'a Value,
    answers: &Map<String, Value>,
) -> std::borrow::Cow<'a, Value> {
    let answered: BTreeSet<&str> = conflict_ids(analysis)
        .into_iter()
        .filter(|id| answers.contains_key(*id))
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
        restated: &BTreeSet<(String, usize)>,
    ) -> BTreeSet<(String, usize)> {
        use haecceity::pmi::NoteOwner;
        let keeps = |collection: &str, i: usize| {
            let at = (collection.to_string(), i);
            !self.explained.contains(&at)
                && !restated.contains(&at)
                && !self
                    .contested
                    .get(&at)
                    .is_some_and(|question| answers.contains_key(question))
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
        if pmi.material.is_some()
            && !self.settings.contains("material")
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
    let planned_notes: Vec<Vec<String>> =
        part_notes(planned).into_iter().map(|(_, n)| n).collect();
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
