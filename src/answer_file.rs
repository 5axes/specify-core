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

/// An explicit answer acknowledges the carry refusal for that ID. Defaults never do.
pub fn blocked(answers: &Map<String, Value>, id: &str) -> bool {
    !answers.contains_key(id)
        && answers
            .get(META)
            .and_then(|m| m.get("unresolved"))
            .and_then(|u| u.get(id))
            .is_some()
}

pub fn require_resolved(answers: &Map<String, Value>) -> rules::Result<()> {
    validate(answers).map_err(rules::Error::Invalid)?;
    let reasons = unresolved(answers);
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

fn unresolved(answers: &Map<String, Value>) -> Map<String, Value> {
    answers
        .get(META)
        .and_then(|m| m.get("unresolved"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(id, _)| !answers.contains_key(*id))
        .map(|(id, reason)| (id.clone(), reason.clone()))
        .collect()
}

/// Verify the saved numbering against actual geometry, even when intent used a fresh analysis.
/// Plain maps have no evidence and retain their historical trust-by-index semantics.
pub fn check(loaded: &LoadedPart, answers: &Map<String, Value>) -> Result<(), String> {
    validate(answers)?;
    if let Some(evidence) = answers.get(META).and_then(|m| m.get("evidence"))
        && !resume::evidence_matches(loaded, evidence).map_err(|e| e.to_string())?
    {
        return Err(
            "stale answers: saved face evidence refers to different geometry or face numbering; use \
                    `specify-core diff OLD NEW --answers ANSWERS -o CHANGES` to carry them"
                .into(),
        );
    }
    Ok(())
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
    unresolved.extend(dropped.into_iter().map(|(id, reason)| (id, json!(reason))));
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
