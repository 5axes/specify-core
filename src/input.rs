//! Checks shared by the JSON bindings and the in-memory session, before cloning or rules.
use serde_json::{Map, Value};
use std::fmt;

pub const JSON_LIMIT: usize = 8 * 1024 * 1024;
pub const STEP_LIMIT: usize = 64 * 1024 * 1024;
pub const TEXT_LIMIT: usize = 64 * 1024;

#[derive(Debug)]
pub enum Error {
    TooLarge { field: &'static str, limit: usize },
    Answer { id: String, why: String },
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { field, limit } => write!(f, "{field} exceeds the {limit} byte limit"),
            Self::Answer { id, why } => write!(f, "Invalid answers: {id}: {why}"),
        }
    }
}
impl std::error::Error for Error {}

pub fn size(field: &'static str, length: usize, limit: usize) -> Result<(), Error> {
    if length > limit {
        Err(Error::TooLarge { field, limit })
    } else {
        Ok(())
    }
}
fn bad(id: &str, why: impl Into<String>) -> Error {
    Error::Answer {
        id: id.into(),
        why: why.into(),
    }
}

/// The `part.*` answer names.
const PART_NAMES: [&str; 9] = [
    "material",
    "general_tolerance",
    "locating",
    "surface_finish",
    "coating",
    "heat_treatment",
    "edges",
    "notes",
    "process",
];
/// The kinds of feature-scoped answer IDs (`kind:feature`).
const FEATURE_KINDS: [&str; 7] = [
    "hole.function",
    "hole.full_thread",
    "hole.frame",
    "hole.position",
    "hole.location",
    "diameter.fit",
    "diameter.runout",
];

/// Whether some question, added requirement or reserved transport member can have `id`.
/// A BOM key is checked with its value (`bom::validate`), keeping that message.
pub fn known_id(id: &str) -> bool {
    if id == crate::answer_file::META
        || id == crate::bom::CARRY
        || id == crate::characteristics::CARRY
        || id.starts_with("bom.")
    {
        return true;
    }
    if let Some(name) = id.strip_prefix("part.") {
        return PART_NAMES.contains(&name);
    }
    if let Some(suffix) = id.strip_prefix("datum.") {
        let (letter, control) = suffix.split_once('.').unwrap_or((suffix, ""));
        return matches!(letter, "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H")
            && (control.is_empty()
                || suffix == "A.flatness"
                || matches!(suffix, "B.control" | "C.control"));
    }
    if let Some((kind, feature)) = id.split_once(':') {
        return FEATURE_KINDS.contains(&kind) && !feature.is_empty();
    }
    id.starts_with("extra.") && id.len() > "extra.".len()
}

/// Refuses an answer ID no question can have, with the same error as [`answers_shape`].
/// The rules call this on every entry, so no front end can have such an answer ignored.
pub fn ids(answers: &Map<String, Value>) -> Result<(), Error> {
    match answers.keys().find(|id| !known_id(id)) {
        Some(id) => Err(bad(id, "unknown answer ID")),
        None => Ok(()),
    }
}

/// Shape checks also apply to editor calls that have no analysis with which to resolve ids.
pub fn answers_shape(answers: &Map<String, Value>) -> Result<(), Error> {
    for (id, value) in answers {
        let invalid = |why| bad(id, why);
        if !known_id(id) {
            return Err(invalid("unknown answer ID"));
        }
        if id == crate::answer_file::META {
            crate::answer_file::validate(answers).map_err(|why| bad(id, why))?;
        } else if id == crate::bom::CARRY {
            if !value.is_object() {
                return Err(invalid("BOM transport must be an object"));
            }
        } else if let Some(key) = id.strip_prefix("bom.") {
            crate::bom::validate(key, value).map_err(|why| bad(id, why))?;
        } else if id == crate::characteristics::CARRY {
            if !value.is_object() {
                return Err(invalid("characteristic number transport must be an object"));
            }
        } else if let Some(name) = id.strip_prefix("part.") {
            if name == "notes" {
                if !value
                    .as_array()
                    .is_some_and(|v| v.iter().all(Value::is_string))
                {
                    return Err(invalid("an answer must be a list of text notes"));
                }
            } else if name == "process" {
                if !value
                    .as_str()
                    .is_some_and(|v| crate::process::PROCESSES.contains(&v))
                {
                    return Err(invalid("not a process"));
                }
            } else if !matches!(name, "material" | "coating" | "heat_treatment")
                && !value.is_string()
            {
                // Text answers retain the core rules' Python truthiness and str_of semantics.
                return Err(invalid("an answer must be text"));
            }
        } else if let Some(suffix) = id.strip_prefix("datum.") {
            if !suffix.contains('.') {
                if !value
                    .as_array()
                    .is_some_and(|v| v.iter().all(|x| x.as_u64().is_some()))
                {
                    return Err(invalid("a datum answer must be a list of face numbers"));
                }
            } else if !value.is_string() {
                return Err(invalid("an answer must be text"));
            }
        } else if id.contains(':') {
            if !value.is_string() {
                return Err(invalid("an answer must be text"));
            }
        } else if !value.is_object() {
            // known_id leaves only an added requirement (`extra.*`).
            return Err(invalid("an added requirement must be an object"));
        }
    }
    Ok(())
}

pub fn answers(analysis: &Value, answers: &Map<String, Value>) -> Result<(), Error> {
    answers_shape(answers)?;
    // Resolve even dormant feature answers: edits may retain a question hidden by a mode.
    for (id, value) in answers {
        if let Some((kind, feature)) = id.split_once(':') {
            let found = analysis
                .get("features")
                .and_then(Value::as_array)
                .and_then(|fs| {
                    fs.iter()
                        .find(|f| f.get("id").and_then(Value::as_str) == Some(feature))
                })
                .ok_or_else(|| bad(id, format!("unknown feature {feature}")))?;
            let family = found.get("family").and_then(Value::as_str);
            let applicable = if kind.starts_with("hole.") {
                matches!(family, Some("holes" | "hole_patterns"))
            } else {
                family == Some("turned_steps")
            };
            if !applicable {
                return Err(bad(id, "unknown answer ID for this feature family"));
            }
        }
        if id.starts_with("datum.")
            && let Some(ids) = value.as_array()
        {
            for face in ids {
                let found = analysis
                    .get("faces")
                    .and_then(Value::as_array)
                    .is_some_and(|fs| fs.iter().any(|f| f.get("id") == Some(face)));
                if !found {
                    return Err(bad(id, format!("unknown face {face}")));
                }
            }
        }
    }
    // Core question generation checks choices during review, preview, defaults and interpret.
    Ok(())
}
