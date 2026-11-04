//! Backwards-compatible answer maps: `_specify` carries face evidence and unresolved carry
//! decisions. It is transport metadata, never a question or a requirement.
use serde_json::{Map, Value, json};

use crate::{part::LoadedPart, resume, rules};

pub const META: &str = "_specify";

/// Reject malformed metadata rather than silently treating it as an unverified plain map.
pub fn validate(answers: &Map<String, Value>) -> Result<(), String> {
    let Some(value) = answers.get(META) else {
        return Ok(());
    };
    let meta = value.as_object().ok_or("_specify must be an object")?;
    if let Some(evidence) = meta.get("evidence") {
        resume::validate_evidence(evidence).map_err(|e| e.to_string())?;
    }
    if let Some(unresolved) = meta.get("unresolved")
        && !unresolved.as_object().is_some_and(|u| {
            u.iter()
                .all(|(id, reason)| !id.is_empty() && reason.is_string())
        })
    {
        return Err("_specify.unresolved must map question IDs to reasons".into());
    }
    Ok(())
}

// These IDs keep their meaning across revisions. Feature IDs encode old face indices and
// may already name a different feature in the destination. An added requirement's
// `extra.<name>` is the person's own name, so its block is kept: writing must not drop a
// requirement they added because its faces did not carry.
fn stable_question(id: &str) -> bool {
    ["datum.", "part.", "bom.", crate::extras::PREFIX]
        .iter()
        .any(|prefix| id.starts_with(prefix))
}

/// The refusal for an unanswered, revision-stable question. Never shadow a reused feature ID,
/// including when reading metadata produced before this restriction was introduced.
pub fn block_reason<'a>(answers: &'a Map<String, Value>, id: &str) -> Option<&'a str> {
    if !stable_question(id) || answers.contains_key(id) {
        return None;
    }
    answers.get(META)?.get("unresolved")?.get(id)?.as_str()
}

/// An explicit answer acknowledges the carry refusal for that ID. Defaults never do.
pub fn blocked(answers: &Map<String, Value>, id: &str) -> bool {
    block_reason(answers, id).is_some()
}

/// Whether a block on *id* applies whatever is asked: an added requirement (`extra.*`) has no
/// question to be asked, so its block always refuses until it is answered again (#123-#125).
pub fn always_applies(id: &str) -> bool {
    id.starts_with(crate::extras::PREFIX)
}

/// Refuses the unanswered carry blocks whose question `applies`: a block applies only while
/// its question is asked (#126). One on a question not asked under the current answers is
/// reported in `not_written` instead, and applies again once the question returns. A block
/// on an added requirement always applies ([`always_applies`]).
pub fn require_resolved_where(
    answers: &Map<String, Value>,
    applies: impl Fn(&str) -> bool,
) -> rules::Result<()> {
    validate(answers).map_err(rules::Error::Invalid)?;
    let mut reasons = unresolved(answers);
    reasons.retain(|id, _| always_applies(id) || applies(id));
    if reasons.is_empty() {
        return Ok(());
    }
    let mut error = rules::IncompleteError::new(reasons.keys().cloned().collect(), None);
    error.carried = reasons
        .into_iter()
        .map(|(id, reason)| (id, reason.as_str().unwrap_or_default().to_string()))
        .collect();
    Err(error.into())
}

/// The carry blocks still unanswered, with their diff reasons.
pub(crate) fn unresolved(answers: &Map<String, Value>) -> Map<String, Value> {
    answers
        .get(META)
        .and_then(|m| m.get("unresolved"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(id, _)| stable_question(id) && !answers.contains_key(*id))
        .map(|(id, reason)| (id.clone(), reason.clone()))
        .collect()
}

/// Verify the saved numbering against actual geometry, even when intent used a fresh analysis.
/// Plain maps have no evidence and retain their historical trust-by-index semantics.
pub fn check(loaded: &LoadedPart, answers: &Map<String, Value>) -> Result<(), String> {
    if !matches(loaded, answers)? {
        return Err(
            "stale answers: saved face evidence refers to different geometry, face numbering or authored names; use \
                    `specify-core diff OLD NEW --answers ANSWERS -o CHANGES` to carry them"
                .into(),
        );
    }
    Ok(())
}

/// A diff must start with the revision to which the saved face indices belong.
pub fn check_for_diff(loaded: &LoadedPart, answers: &Map<String, Value>) -> Result<(), String> {
    if !matches(loaded, answers)? {
        return Err("stale answers: the answers file belongs to another revision; pass the matching old part as OLD".into());
    }
    Ok(())
}

fn matches(loaded: &LoadedPart, answers: &Map<String, Value>) -> Result<bool, String> {
    validate(answers)?;
    match answers.get(META).and_then(|m| m.get("evidence")) {
        Some(evidence) => resume::evidence_matches(loaded, evidence).map_err(|e| e.to_string()),
        None => Ok(true),
    }
}

/// Attach evidence for the destination and retain unresolved decisions across repeated diffs.
/// Resolved entries disappear; newly dropped entries carry the exact diff reason.
pub fn attach(
    loaded: &LoadedPart,
    answers: &mut Map<String, Value>,
    previous: &Map<String, Value>,
    dropped: impl IntoIterator<Item = (String, String)>,
) -> Result<(), String> {
    let mut unresolved = unresolved(previous);
    unresolved.extend(
        dropped
            .into_iter()
            .filter(|(id, _)| stable_question(id))
            .map(|(id, reason)| (id, json!(reason))),
    );
    unresolved.retain(|id, _| !answers.contains_key(id));
    answers.insert(
        META.into(),
        json!({
            "evidence": resume::evidence(loaded).map_err(|e| e.to_string())?,
            "unresolved": unresolved,
        }),
    );
    Ok(())
}
