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
    /// Stored answers withheld while the file has unexplained PMI in their scope.
    pub withheld: Map<String, Value>,
    /// File items in that scope which the ordinary reconciliation does not manage.
    pub retained: BTreeSet<(String, usize)>,
    /// The writer's plan could not be compared with the file.
    pub uncompared: bool,
    pub conflicts: Vec<Conflict>,
    /// The file's items the resumed answers explain, by haecceity collection and index.
    pub explained: BTreeSet<(String, usize)>,
    /// Explained tolerances whose questions may be suppressed by imported PMI.
    masked_tolerances: BTreeMap<usize, String>,
    /// The file's items each conflict's question stands for.
    pub contested: BTreeMap<(String, usize), String>,
    /// Part settings (`part` keys) the resumed answers explain.
    pub settings: BTreeSet<String>,
    /// Questions no stored answer states that the file answers with its own (Specify-marked)
    /// requirements, otherwise than their defaults would (`resume.file`).
    pub file_answered: Vec<String>,
    /// Questions of `file_answered` another tool's (unmarked, never kept) requirement holds
    /// (`resume.foreign`): as for a foreign conflict, no answer replaces it.
    pub foreign_file: BTreeSet<String>,
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
    /// Without a usable plan, there is no safe basis for resuming answers or
    /// replacing any of the file's requirements.
    pub(crate) fn uncompared(analysis: &Value, pmi: &haecceity::pmi::PartPmi) -> Self {
        let withheld = analysis["resume"]["answers"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(id, _)| !id.starts_with('_'))
            .map(|(id, answer)| (id.clone(), answer.clone()))
            .collect();
        let mut retained: BTreeSet<_> = [
            ("features", pmi.features.len()),
            ("datums", pmi.datums.len()),
            ("datum_targets", pmi.datum_targets.len()),
            ("dimensions", pmi.dimensions.len()),
            ("tolerances", pmi.tolerances.len()),
            ("general", pmi.general.len()),
            ("threads", pmi.threads.len()),
            ("knurls", pmi.knurls.len()),
            ("notes", pmi.notes.len()),
            ("surface_textures", pmi.surface_textures.len()),
            ("material", usize::from(pmi.material.is_some())),
        ]
        .into_iter()
        .flat_map(|(collection, len)| (0..len).map(move |i| (collection.to_string(), i)))
        .collect();
        retained.extend(pmi.attributes.iter().enumerate().filter_map(|(i, set)| {
            (!super::is_answers(set) && set.name != crate::characteristics::PROPERTY)
                .then_some(("attributes".into(), i))
        }));
        Self {
            withheld,
            retained,
            uncompared: true,
            ..Self::default()
        }
    }

    /// The [`crate::existing::Selection`] that leaves the explained items out of `existing`
    /// and tags the contested ones with their question.
    pub(crate) fn selection(&self) -> crate::existing::Selection {
        crate::existing::Selection {
            skip: self.explained.clone(),
            questions: self.contested.clone(),
            retained: self.retained.clone(),
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
    reading: &crate::existing::Reading,
) -> Result<Reconciled, String> {
    let mut stored = analysis["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut occupied_settings = BTreeSet::new();
    reading.each_setting(false, |key, actual| {
        let id = format!("part.{key}");
        if !blank(&actual)
            && stored
                .get(&id)
                .is_some_and(|saved| !contains(saved, &actual))
        {
            occupied_settings.insert(id);
        }
    });
    let mut withheld = Map::new();
    // A competing list cannot be planned in Add mode. Its entries have already
    // been compared to the full stored list, so withhold it before planning.
    for id in &occupied_settings {
        if stored[id].is_array() && !kept(&stored).contains(id.as_str()) {
            withheld.insert(
                id.clone(),
                stored.shift_remove(id).expect("occupied stored answer"),
            );
        }
    }
    let scene = Scene::for_resume(loaded, features)?;
    let ((file, index), mut plan) = scene.reopening_comparison(loaded, analysis, &stored)?;
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
        kinds: analysis["faces"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| f["kind"].as_str().unwrap_or_default().to_owned())
            .collect(),
    };
    // Which of the file's requirements are not marked as Specify's: another tool's, or one the
    // person kept (written unmarked).
    // Process answers and attributes also identify files whose property links carry
    // provenance when authored by Specify.
    let (part_pmi, unmarked) = scene.file_pmi(stored.contains_key(crate::process::QUESTION));
    let compare = |plan: &Value, stored: &Map<String, Value>| {
        crate::revision::against_plan(plan, &file, faces, stored, &drift)
    };
    let preview =
        |answers: &Map<String, Value>| scene.comparison_with_file(loaded, analysis, answers, &file);
    let defaults = |answers: &Map<String, Value>| -> Result<_, String> {
        let mut unkept = answers.clone();
        if let Some(meta) = unkept
            .get_mut(crate::answer_file::META)
            .and_then(Value::as_object_mut)
        {
            meta.remove(KEEP);
        }
        match crate::rules::with_all_defaults(analysis, &unkept) {
            Ok(all) if all.len() > answers.len() => {
                let planned = preview(&all)?;
                Ok(Some((all, planned)))
            }
            _ => Ok(None),
        }
    };
    let rows = crate::revision::scene_items(&file);
    let mut occupied = occupied_answers(
        analysis,
        &occupied_settings,
        &stored,
        &rows,
        &index,
        part_pmi,
    )?;
    let mut blocked: BTreeMap<_, Vec<_>> = withheld
        .keys()
        .filter_map(|id| {
            let key = setting(id)?;
            Some((
                id.clone(),
                rows.keys()
                    .filter(|row| setting(row) == Some(key))
                    .cloned()
                    .collect(),
            ))
        })
        .collect();
    let mut all_planned = defaults(&stored)?;
    let mut truth = compare(&plan, &stored);
    let mut other = all_planned.as_ref().map(|(_, plan)| compare(plan, &stored));
    let mut writes = writing(&plan);
    if let Some((_, defaults)) = &all_planned {
        writes.extend(writing(defaults));
    }
    // A full-thread answer supplies depth to its hole function's thread PMI.
    // Printed hole functions write semantic notes. Neither has a standalone
    // scene row tagged with the question.
    writes.extend(
        stored
            .iter()
            .filter(|(id, value)| {
                id.starts_with("hole.full_thread:")
                    || (id.starts_with("hole.function:")
                        && value
                            .as_str()
                            .is_some_and(|value| crate::choices::PRINTED_HOLES.contains(&value)))
            })
            .map(|(id, _)| id.clone()),
    );
    let exact = crate::revision::exactly_restated(
        &file,
        std::iter::once(&plan).chain(all_planned.iter().map(|(_, plan)| plan)),
    );
    occupied.retain(|id, items| {
        if setting(id).is_none() {
            items.retain(|item| !exact.contains(item));
            if items.is_empty() {
                return false;
            }
        }
        !kept(&stored).contains(id.as_str())
            && (!writes.contains(id) || setting(id).is_some())
            && !(truth.conflicts.contains_key(id)
                && other.as_ref().is_none_or(|t| t.conflicts.contains_key(id)))
    });
    for id in occupied.keys() {
        if let Some(value) = stored.shift_remove(id) {
            withheld.insert(id.clone(), value);
        }
    }
    // Apply #204 with the withheld answers absent, so the file can answer the question
    // exactly as it would if no answer had been stored for it.
    if !occupied.is_empty() {
        plan = preview(&stored)?;
        all_planned = defaults(&stored)?;
        truth = compare(&plan, &stored);
        other = all_planned.as_ref().map(|(_, plan)| compare(plan, &stored));
    }
    blocked.extend(occupied);
    let kept_earlier = kept(&stored);
    let kept_process = kept_earlier
        .contains(crate::process::QUESTION)
        .then(|| scene.kept_process())
        .flatten();
    let process = scene.file_process();
    let mut present = Map::new();
    let mut file_answered: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    if let (Some((all, _)), Some(other)) = (all_planned, other) {
        truth
            .conflicts
            .retain(|question, _| other.conflicts.contains_key(question));
        truth.explained.extend(other.explained);
        truth.explained_by.extend(other.explained_by);
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
        // A legacy Replace file predates attribute marks, so an exact default match still
        // explains its part attribute.
        let legacy_attributes = scene.legacy_part_attributes();
        for (question, (unchanged, ids)) in other.defaults {
            // The process is read from the file with its own source (an accepted suggestion
            // stays one, `process::read`), so it is not taken as an answer here.
            let own = ids.iter().all(|id| {
                index.get(id).is_none_or(|at| {
                    !unmarked.contains(at) || (at.0 == "attributes" && legacy_attributes)
                })
            });
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
    // A marked part setting also answers its question when there is no writing default
    // to match it to (for example, general notes).
    for (id, (_, text)) in &rows {
        if let Some(key) = setting(id) {
            let question = format!("part.{key}");
            if !stored.contains_key(&question)
                && !present.contains_key(&question)
                && !truth.explained.contains(id)
                && index.get(id).is_some_and(|at| !unmarked.contains(at))
            {
                file_answered
                    .entry(question)
                    .or_default()
                    .push((id.clone(), text.clone()));
            }
        }
    }
    let managed: BTreeSet<&String> = truth
        .conflicts
        .values()
        .chain(file_answered.values())
        .flatten()
        .map(|(id, _)| id)
        .collect();
    let mut retained = BTreeSet::new();
    for (question, items) in &blocked {
        for id in items.iter().filter(|id| !managed.contains(*id)) {
            // #204 can explain a requirement through recognition drift. It still
            // needs preserving when the full plan did not restate it exactly.
            if setting(question).is_none() {
                truth.explained.remove(id);
            }
            if !truth.explained.contains(id)
                && let Some(at) = index.get(id).filter(|at| !unmarked.contains(*at))
            {
                retained.insert(at.clone());
            }
        }
    }
    let item = |id: &str| index.get(id).cloned();
    // Rules can omit a tolerance question when imported PMI already controls
    // its faces. An explained, marked tolerance still needs keeping if the
    // actual save does not write it again.
    let imported_tolerance_faces: BTreeSet<_> = rows
        .iter()
        .filter(|(id, _)| {
            index
                .get(*id)
                .is_some_and(|at| at.0 == "tolerances" && unmarked.contains(at))
        })
        .flat_map(|(_, (faces, _))| faces.iter().copied())
        .collect();
    let masked_tolerances = truth
        .explained
        .iter()
        .filter_map(|id| {
            let at = index.get(id)?;
            let question = truth.explained_by.get(id)?;
            (at.0 == "tolerances"
                && !unmarked.contains(at)
                && rows.get(id).is_some_and(|row| {
                    row.0
                        .iter()
                        .any(|face| imported_tolerance_faces.contains(face))
                }))
            .then(|| (at.1, question.clone()))
        })
        .collect();
    let mut out = Reconciled {
        withheld,
        retained,
        masked_tolerances,
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
                // Another tool's requirement among the question's items (a foreign conflict
                // kept by a save, beside Specify's own location dimensions): keep-only.
                if unmarked.contains(&at) && !kept_earlier.contains(question.as_str()) {
                    out.foreign_file.insert(question.clone());
                }
                out.contested.insert(at, question.clone());
            }
        }
        out.file_answered.push(question);
    }
    out.present = present;
    Ok(out)
}

/// The questions whose answers `plan`, a comparison scene of answers, states PMI for: a
/// requirement or part setting tagged with the question, or the material.
pub(crate) fn writing(plan: &Value) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = plan["specify"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, meta)| meta["status"] != "existing")
        .filter_map(|(_, meta)| meta["question"].as_str().map(str::to_owned))
        .collect();
    if !plan["pmi"]["pmi"]["material"].is_null() {
        out.insert("part.material".into());
    }
    out
}

/// Whether a part setting's value states nothing: absent, empty text or an empty list.
fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

/// Every file entry must match a saved entry. Each saved list entry can explain
/// one file entry, in the order the writer would state it.
fn contains(saved: &Value, actual: &Value) -> bool {
    if let (Some(saved), Some(actual)) = (saved.as_array(), actual.as_array()) {
        let mut saved = saved.iter();
        actual.iter().all(|entry| saved.any(|value| value == entry))
    } else {
        saved == actual
    }
}

/// Find file PMI in a stored answer's scope, regardless of authorship or whether a
/// plan also states it. For a datum question, either version of the datum may be its target.
fn occupied_answers(
    analysis: &Value,
    occupied_settings: &BTreeSet<String>,
    stored: &Map<String, Value>,
    rows: &BTreeMap<String, (Vec<usize>, String)>,
    index: &BTreeMap<String, (String, usize)>,
    pmi: &haecceity::pmi::PartPmi,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut bare = analysis.clone();
    bare["existing"] = Value::Array(Vec::new());
    bare["part"] = Value::Object(Map::new());
    let questions = crate::rules::questions(&bare, stored).map_err(|e| e.to_string())?;
    let mut out = BTreeMap::new();
    for id in stored.keys() {
        if id.starts_with('_') || id == crate::process::QUESTION {
            continue;
        }
        if let Some(key) = setting(id) {
            if occupied_settings.contains(id) {
                out.insert(
                    id.clone(),
                    rows.keys()
                        .filter(|row| setting(row) == Some(key))
                        .cloned()
                        .collect(),
                );
            }
            continue;
        }
        let mut faces: BTreeSet<usize> = questions
            .iter()
            .filter(|q| q.id == *id)
            .flat_map(|q| &q.faces)
            .filter_map(|&f| usize::try_from(f).ok())
            .collect();
        if let Some(letter) = id
            .strip_prefix("datum.")
            .and_then(|suffix| suffix.split('.').next())
        {
            faces.extend(
                rows.iter()
                    .filter(|(row, _)| {
                        index.get(*row).is_some_and(|(collection, i)| {
                            collection == "datums" && pmi.datums[*i].label().as_str() == letter
                        })
                    })
                    .flat_map(|(_, (faces, _))| faces.iter().copied()),
            );
        }
        let items: Vec<_> = rows
            .iter()
            .filter(|(_, (on, _))| on.iter().any(|f| faces.contains(f)))
            .map(|(id, _)| id.clone())
            .collect();
        if !items.is_empty() {
            out.insert(id.clone(), items);
        }
    }
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

/// The questions another tool's requirement holds: the conflicts whose file requirement is
/// another tool's (`foreign`) and the file's own answers that include one (`resume.foreign`).
/// Their question is not asked, answered or not, and the file's requirement stands.
pub(crate) fn foreign_ids(analysis: &Value) -> BTreeSet<&str> {
    foreign_in(&analysis["resume"])
}

/// [`foreign_ids`] of an analysis's `resume`.
pub(crate) fn foreign_in(resume: &Value) -> BTreeSet<&str> {
    resume["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["foreign"] == Value::Bool(true))
        .filter_map(|c| c["id"].as_str())
        .chain(
            resume["foreign"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .collect()
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
    // A foreign process the file keeps is the one the rules see, so the answer changes nothing.
    let process_changed = !foreign_process
        && answers.get(process).is_some_and(|process| {
            analysis["process"]
                .get("value")
                .is_some_and(|old| old != process)
        });
    let mut out = answers.clone();
    for (id, value) in present.into_iter().flatten() {
        // A printed tolerance read back as a default belongs to the file's
        // process. Let an explicitly changed process supply its own default.
        if process_changed
            && id == "part.general_tolerance"
            && value
                .as_str()
                .is_some_and(|text| crate::general::plus_minus(text).is_some())
        {
            continue;
        }
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
        if self.uncompared {
            return self.retained.clone();
        }
        // Only the file's items of an undecided conflict or of a question the file answers
        // need keeping here: the rewrite replaces Specify's own (marked) requirements and
        // protects every other one, which is another tool's.
        let keeps = |collection: &str, i: usize| {
            let at = (collection.to_string(), i);
            self.retained.contains(&at)
                || (collection == "tolerances"
                    && self.masked_tolerances.get(&i).is_some_and(|question| {
                        self.answers
                            .get(question)
                            .is_some_and(|value| answers.get(question) == Some(value))
                    })
                    && !restated.contains(&at))
                || (!self.explained.contains(&at)
                    && !restated.contains(&at)
                    && self
                        .contested
                        .get(&at)
                        .is_some_and(|question| !self.replaces(question, answers, writes_process)))
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
            if !self.retained.contains(&("tolerances".into(), i))
                && cites(tolerance)
                    .iter()
                    .any(|d| restated.contains(&("datum letters".to_string(), *d)))
            {
                out.remove(&("tolerances".to_string(), i));
            }
        }
        for (i, set) in pmi.attributes.iter().enumerate() {
            if super::is_answers(set)
                || set.name == crate::characteristics::PROPERTY
                || ([crate::pmi_origin::PROPERTY, "specify dimension plan"]
                    .contains(&set.name.as_str())
                    && !matches!(set.on, Some(NoteOwner::Dimension(d)) if self.retained.contains(&("dimensions".into(), d.0))))
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
    pub(crate) fn is_foreign(&self, question: &str) -> bool {
        if question == crate::process::QUESTION {
            self.foreign_process
        } else {
            self.conflicts.iter().any(|c| c.id == question && c.foreign)
                || self.foreign_file.contains(question)
        }
    }
}
