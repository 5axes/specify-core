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
    settings: &Map<String, Value>,
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
    let (part_pmi, unmarked) = scene.file_pmi();
    // A question the person kept earlier (`_specify.keep`, stored with the answers) is the
    // file's whatever its marker: the kept requirement was written unmarked.
    let kept_earlier = kept(&stored);
    let mut restated = BTreeSet::new();
    let plan = scene.reopening_plan(loaded, analysis, &stored, &mut restated)?;
    let mut truth = crate::revision::against_plan(&plan, &file, faces, &stored, &drift);
    // The questions whose answers the plans state PMI for; any other stored answer writes none.
    let mut writes = writing(&plan);
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
        let planned = scene.reopening_plan(loaded, analysis, &all, &mut restated)?;
        writes.extend(writing(&planned));
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
    // A stored answer that writes no PMI has nothing in the file to agree with: it resumes,
    // unless the file states PMI for its question, which contradicts it.
    let silent: Vec<&String> = stored
        .keys()
        .filter(|id| {
            !id.starts_with('_')
                && !writes.contains(*id)
                && id.as_str() != crate::process::QUESTION
                && id.as_str() != "part.material"
                && !kept_earlier.contains(id.as_str())
                && !truth.conflicts.contains_key(*id)
        })
        .collect();
    let rows = crate::revision::scene_items(&file);
    if !silent.is_empty() {
        use haecceity::pmi::{ToleranceKind, ToleranceTarget};
        let positioned: BTreeSet<usize> = part_pmi
            .tolerances
            .iter()
            .filter(|t| t.kind == ToleranceKind::Position)
            .filter_map(|t| match t.target {
                ToleranceTarget::Feature(f) => Some(f),
                _ => None,
            })
            .flat_map(|f| crate::existing::faces_of(part_pmi, &[f]))
            .collect();
        let contested: BTreeSet<String> = truth
            .conflicts
            .values()
            .chain(file_answered.values())
            .flatten()
            .map(|(id, _)| id.clone())
            .collect();
        let mut rows = rows.clone();
        rows.retain(|id, _| !truth.explained.contains(id) && !contested.contains(id));
        let mut face_rows = rows.clone();
        face_rows.retain(|id, _| {
            !index.get(id).is_some_and(|at| {
                at.0 == "dimensions"
                    && !unmarked.contains(at)
                    && derived_location(part_pmi, at.1, restated.contains(at), &positioned)
            })
        });
        for (id, items) in contradicted(
            analysis, settings, &stored, &silent, &rows, &face_rows, &index,
        ) {
            truth.conflicts.insert(id, items);
        }
    }
    // A list can also disagree with the file when it writes some entries but leaves
    // another tool's entry unexplained. Keep the whole setting, including its matched
    // entries. After a stale answer is dropped on save, the marked entries still
    // answer the question on reopening, including settings with no writing default.
    let mut part_rows: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for (id, (_, text)) in &rows {
        if let Some(key) = setting(id) {
            part_rows
                .entry(key)
                .or_default()
                .push((id.clone(), text.clone()));
        }
    }
    let is_unmarked = |id: &str| index.get(id).is_some_and(|at| unmarked.contains(at));
    for (key, mut items) in part_rows {
        let question = format!("part.{key}");
        if stored.contains_key(&question)
            && !kept_earlier.contains(question.as_str())
            && (settings.get(key) != stored.get(&question)
                || truth.conflicts.contains_key(&question))
            && items
                .iter()
                .any(|(id, _)| is_unmarked(id) && !truth.explained.contains(id))
        {
            truth.explained.retain(|id| setting(id) != Some(key));
            truth.conflicts.insert(question, items);
        } else if !stored.contains_key(&question) && !present.contains_key(&question) {
            items.retain(|(id, _)| !is_unmarked(id) && !truth.explained.contains(id));
            if !items.is_empty() {
                file_answered.entry(question).or_default().extend(items);
            }
        }
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
                foreign: !kept_earlier.contains(id.as_str())
                    && out
                        .contested
                        .iter()
                        .any(|(at, q)| q == id && unmarked.contains(at)),
            }),
            None => {
                out.answers.insert(id.clone(), answer.clone());
            }
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

/// A location the planner measures from the part geometry can share a question's faces
/// without stating its answer. Older writers emitted separate locations where the current
/// planner merges them, so those locations need not share a characteristic key.
fn derived_location(
    part: &haecceity::pmi::PartPmi,
    i: usize,
    restated: bool,
    positioned: &BTreeSet<usize>,
) -> bool {
    use haecceity::pmi::{DimTolerance, DimensionKind, LocationKind};
    let d = &part.dimensions[i];
    let DimensionKind::Location {
        to,
        kind: LocationKind::LinearDistance,
        path: None,
        angle: None,
        directed: false,
        ..
    } = d.kind
    else {
        return false;
    };
    if !d.modifiers.is_empty() || d.qualifier.is_some() || d.principle.is_some() {
        return false;
    }
    match d.tolerance {
        DimTolerance::Basic if !restated => {
            // Legacy files place each hole individually. The basic dimensions define the
            // true position of the faces covered by the file's position tolerance.
            let faces = crate::existing::faces_of(part, &[to]);
            !faces.is_empty() && faces.iter().all(|face| positioned.contains(face))
        }
        DimTolerance::Basic => true,
        DimTolerance::None => restated || !part.general.is_empty(),
        _ => false,
    }
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

/// Of the stored answers `silent`, which write no PMI, those the file contradicts, each with
/// the file's requirements for it (scene id, notation). Conservative: asking again is better
/// than resuming an answer the file may say otherwise.
/// - A question about faces: any unexplained file requirement on the question's faces.
/// - A part setting (`part.notes`, `part.coating`, ...): the file's setting, stated and
///   different from the answer, with each unexplained entry tagged individually.
fn contradicted(
    analysis: &Value,
    settings: &Map<String, Value>,
    stored: &Map<String, Value>,
    silent: &[&String],
    rows: &BTreeMap<String, (Vec<usize>, String)>,
    face_rows: &BTreeMap<String, (Vec<usize>, String)>,
    index: &BTreeMap<String, (String, usize)>,
) -> BTreeMap<String, Vec<(String, String)>> {
    let mut out = BTreeMap::new();
    if rows.is_empty()
        && !silent
            .iter()
            .any(|id| setting(id).is_some_and(|key| settings.get(key).is_some_and(|v| !blank(v))))
    {
        return out;
    }
    // Find the original question's faces. Keep the file's measurement frame so a control
    // on a stated datum is checked on that datum, while other PMI cannot hide its question.
    let mut bare = analysis.clone();
    if let Some(existing) = bare["existing"].as_array_mut() {
        existing.retain(|item| item["kind"] == "datum");
    }
    bare["part"] = Value::Object(Map::new());
    let questions = crate::rules::questions(&bare, stored).unwrap_or_default();
    for id in silent {
        if let Some(key) = setting(id).filter(|key| settings.get(*key).is_some_and(|v| !blank(v))) {
            let actual = &settings[key];
            let mut items: Vec<(String, String)> = rows
                .iter()
                .filter(|(row, _)| setting(row) == Some(key))
                .map(|(row, (_, text))| (row.clone(), text.clone()))
                .collect();
            if *actual == stored[id.as_str()] && items.len() <= 1 {
                continue;
            }
            if items.is_empty() {
                // No scene item to tag: the setting's value is what the file says.
                let text = match actual {
                    Value::String(s) => s.clone(),
                    Value::Array(list) => list
                        .iter()
                        .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
                        .collect::<Vec<_>>()
                        .join("; "),
                    other => other.to_string(),
                };
                items.push((String::new(), text));
            }
            out.insert((*id).clone(), items);
            continue;
        }
        let Some(question) = questions.iter().find(|q| &q.id == *id) else {
            continue;
        };
        let faces: Vec<usize> = question
            .faces
            .iter()
            .filter_map(|&f| usize::try_from(f).ok())
            .collect();
        if faces.is_empty() {
            continue;
        }
        let datum_control =
            id.starts_with("datum.") && (id.ends_with(".flatness") || id.ends_with(".control"));
        let items: Vec<(String, String)> = face_rows
            .iter()
            .filter(|(row, (on, _))| {
                // A datum declaration establishes the reference for the question's control.
                let datum = index
                    .get(*row)
                    .is_some_and(|at| matches!(at.0.as_str(), "datums" | "datum_targets"));
                !(datum_control && datum) && on.iter().any(|f| faces.contains(f))
            })
            .map(|(row, (_, text))| (row.clone(), text.clone()))
            .collect();
        if !items.is_empty() {
            out.insert((*id).clone(), items);
        }
    }
    out
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
    let Some(present) = analysis["resume"]["present"].as_object() else {
        return std::borrow::Cow::Borrowed(answers);
    };
    if present.keys().all(|id| answers.contains_key(id)) {
        return std::borrow::Cow::Borrowed(answers);
    }
    let mut out = answers.clone();
    for (id, value) in present {
        if !out.contains_key(id) {
            out.insert(id.clone(), value.clone());
        }
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
                && self.contested.get(&at).is_some_and(|question| {
                    !answers.contains_key(question) || self.is_foreign(question)
                })
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
    ) -> BTreeSet<(String, usize)> {
        use haecceity::pmi::NoteOwner;
        let mut out: BTreeSet<(String, usize)> = self
            .contested
            .iter()
            .filter(|(_, question)| answers.contains_key(*question) && !self.is_foreign(question))
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
    /// Whether `question` is a conflict whose file requirement is another tool's.
    fn is_foreign(&self, question: &str) -> bool {
        self.conflicts.iter().any(|c| c.id == question && c.foreign)
    }
}
