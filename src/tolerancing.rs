//! A drawing by default, GD&T on request: the editor's tolerancing rules, shared by every
//! front end so a person and an AI agent make the same changes (moved from draftwright-editor's
//! `src/model/intent.ts`, docs/WORKFLOW.md there).
//!
//! Until GD&T is asked for, some questions are answered on the person's behalf so Specify
//! proposes measurement datums and holes located by ± dimensions (no position tolerances),
//! plain holes except a proposed tap, and plain diameters except a proposed thread or knurl.
//! Those answers are recorded in `auto` so they can be given back. `gdt` is the tolerancing
//! asked for: none yet (the default path), ± only, or geometric.
//!
//! The state is the project's own `{answers, auto, gdt}`; `auto` absent (not empty) marks a
//! project saved before the default path existed.
// JSON Value indexing below is read-only and returns Null for absent or ill-typed keys.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::api;

/// How the tolerancing is specified, once asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// ± only: holes located by ± dimensions.
    Plusminus,
    /// Geometric (GD&T): Specify proposes position tolerances from the measurement datums.
    Geometric,
}

/// A project's tolerancing state, as the editor stores it in `edits`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub answers: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gdt: Option<Mode>,
}

const LOCATING: &str = "part.locating";
const PLUS_MINUS: &str = "± dimensions";
// A hole's tap and a diameter's thread or knurl are facts about the part; their other options
// (clearance, fits) are tolerancing.
const FACT_OPTIONS: [&str; 3] = ["tapped:", "thread:", "knurl:"];

fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn is_hole_or_diameter(id: &str) -> bool {
    id.starts_with("hole.function:") || id.starts_with("diameter.fit:")
}

/// A question's default as the editor applies it: ± locating where Specify's own is position
/// tolerances. `question` is a presented question (`api::review`'s `questions`).
pub fn editor_default(question: &Value) -> Value {
    if question["id"] == LOCATING {
        Value::String(PLUS_MINUS.into())
    } else {
        question.get("default").cloned().unwrap_or(Value::Null)
    }
}

/// The answers the default path adds to `questions` not yet answered.
pub fn default_path_answers(
    questions: &[Value],
    answers: &Map<String, Value>,
) -> Map<String, Value> {
    let mut added = Map::new();
    for question in questions {
        let Some(id) = question["id"].as_str() else {
            continue;
        };
        if answers.contains_key(id) || crate::answer_file::blocked(answers, id) {
            continue;
        }
        if id == LOCATING {
            added.insert(id.into(), editor_default(question));
        } else if is_hole_or_diameter(id) {
            let default = text(question.get("default"));
            if !FACT_OPTIONS
                .iter()
                .chain(&["general"])
                .any(|option| default.starts_with(option))
            {
                added.insert(id.into(), Value::String("general".into()));
            }
        }
    }
    added
}

/// The mode of a project saved before the default path: Specify's defaults were then the
/// geometric scheme, so it keeps it, unless it was already located by ± dimensions.
pub fn inferred_mode(answers: &Map<String, Value>) -> Mode {
    if answers.get(LOCATING).and_then(Value::as_str) == Some(PLUS_MINUS) {
        Mode::Plusminus
    } else {
        Mode::Geometric
    }
}

/// A locating answer chooses the same mode as the tolerancing controls; removing it returns to
/// the default path.
pub fn mode_after_answers(
    mode: Option<Mode>,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Option<Mode> {
    if before.get(LOCATING) == after.get(LOCATING) {
        return mode;
    }
    after.get(LOCATING)?;
    Some(inferred_mode(after))
}

/// What the default path answered and the person has since left alone.
pub fn still_auto(
    auto: Option<&[String]>,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Option<Vec<String>> {
    auto.map(|ids| {
        ids.iter()
            .filter(|id| after.get(*id) == before.get(*id))
            .cloned()
            .collect()
    })
}

/// The answers once GD&T is asked for: the default path's are given back, but for ± locating.
/// Either mode can follow the other: ± locates by dimensions whatever was answered before, and
/// geometric lets Specify propose the locating again.
pub fn gdt_answers(
    answers: &Map<String, Value>,
    auto: &[String],
    mode: Mode,
) -> Map<String, Value> {
    let mut kept = answers.clone();
    for id in auto {
        if !(mode == Mode::Plusminus && id == LOCATING) {
            kept.shift_remove(id);
        }
    }
    match mode {
        Mode::Plusminus => {
            kept.insert(LOCATING.into(), Value::String(PLUS_MINUS.into()));
        }
        Mode::Geometric => {
            if kept.get(LOCATING).and_then(Value::as_str) == Some(PLUS_MINUS) {
                kept.shift_remove(LOCATING);
            }
        }
    }
    kept
}

/// A change of tolerancing to `mode` (none: back to the default path) with `answers`: automatic
/// answers left alone are given back to Specify, and nothing stays automatic.
pub fn tolerancing_edit(state: &State, mode: Option<Mode>, answers: &Map<String, Value>) -> State {
    let auto = still_auto(state.auto.as_deref(), &state.answers, answers).unwrap_or_default();
    let answers = match mode {
        Some(mode) => gdt_answers(answers, &auto, mode),
        None => {
            let mut next = answers.clone();
            for id in &auto {
                next.shift_remove(id);
            }
            next
        }
    };
    State {
        answers,
        auto: Some(vec![]),
        gdt: mode,
    }
}

/// The person's answers replace the state's: an answer they change is theirs (no longer given
/// back), and a changed locating answer changes the mode as the tolerancing controls would.
pub fn answer_edit(state: &State, answers: &Map<String, Value>) -> State {
    let mode = mode_after_answers(state.gdt, &state.answers, answers);
    if mode != state.gdt {
        return tolerancing_edit(state, mode, answers);
    }
    State {
        answers: answers.clone(),
        auto: still_auto(state.auto.as_deref(), &state.answers, answers),
        gdt: state.gdt,
    }
}

/// The state a project opens with. A STEP Specify wrote (`resume`) resumes its answers, and with
/// them the tolerancing they had, when the project has none of its own; a project from before
/// the default path keeps the tolerancing it had.
pub fn opened(state: &State, resume: Option<&Map<String, Value>>) -> State {
    let mut next = state.clone();
    if let Some(answers) = resume.filter(|_| next.answers.is_empty()) {
        next = State {
            answers: answers.clone(),
            auto: Some(vec![]),
            gdt: Some(inferred_mode(answers)),
        };
    }
    if next.gdt.is_none() && next.auto.is_none() {
        next.gdt = Some(inferred_mode(&next.answers));
        next.auto = Some(vec![]);
    }
    next
}

/// Specify's review settled to the default path: while no tolerancing is asked for, the default
/// path's answers are added and recorded, until none are new (a few rounds: some questions only
/// exist once an earlier one is answered). Returns the review and the settled state.
pub fn settle(analysis: &Value, state: &State) -> api::Result<(Value, State)> {
    let mut state = state.clone();
    let mut review = api::review(analysis, &state.answers)?;
    for _ in 0..3 {
        if state.gdt.is_some() {
            break;
        }
        let questions = review["questions"].as_array().cloned().unwrap_or_default();
        let added = default_path_answers(&questions, &state.answers);
        if added.is_empty() {
            break;
        }
        let mut auto = state.auto.take().unwrap_or_default();
        for id in added.keys() {
            if !auto.contains(id) {
                auto.push(id.clone());
            }
        }
        state.answers.extend(added);
        state.auto = Some(auto);
        review = api::review(analysis, &state.answers)?;
    }
    Ok((review, state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn mode_changes_give_automatic_answers_back_and_keep_manual_ones() {
        let state = State {
            answers: map(
                json!({"part.locating": "position tolerances", "hole.function:a": "general", "part.material": "steel"}),
            ),
            auto: Some(vec!["hole.function:a".into()]),
            gdt: Some(Mode::Geometric),
        };
        let mut plus_answers = state.answers.clone();
        plus_answers.insert(LOCATING.into(), json!(PLUS_MINUS));
        let plus = answer_edit(&state, &plus_answers);
        assert_eq!(
            plus,
            tolerancing_edit(&state, Some(Mode::Plusminus), &state.answers)
        );
        assert_eq!(plus.auto, Some(vec![]));
        assert!(
            !plus.answers.contains_key("hole.function:a"),
            "automatic plain-hole answers return to Specify"
        );
        assert_eq!(
            plus.answers["part.material"], "steel",
            "manual answers are kept"
        );

        let mut manual = plus_answers.clone();
        manual.insert("hole.function:a".into(), json!("fit:H7"));
        let kept = tolerancing_edit(&state, Some(Mode::Plusminus), &manual);
        assert_eq!(
            kept.answers["hole.function:a"], "fit:H7",
            "an answer changed with the mode is not given back"
        );

        let reset = tolerancing_edit(&state, None, &map(json!({"part.material": "steel"})));
        assert_eq!(reset.gdt, None);
    }

    #[test]
    fn a_locating_answer_sets_the_mode_and_removing_it_returns_to_the_default_path() {
        let plus = map(json!({"part.locating": PLUS_MINUS}));
        let geometric = map(json!({"part.locating": "position tolerances"}));
        assert_eq!(
            mode_after_answers(None, &plus, &geometric),
            Some(Mode::Geometric)
        );
        assert_eq!(
            mode_after_answers(Some(Mode::Geometric), &geometric, &plus),
            Some(Mode::Plusminus)
        );
        assert_eq!(
            mode_after_answers(Some(Mode::Plusminus), &plus, &Map::new()),
            None
        );
        assert_eq!(
            mode_after_answers(Some(Mode::Plusminus), &plus, &plus),
            Some(Mode::Plusminus)
        );
    }

    #[test]
    fn the_default_path_plains_holes_but_keeps_proposed_taps_and_locates_by_dimensions() {
        let questions = vec![
            json!({"id": "part.locating", "default": "position tolerances"}),
            json!({"id": "hole.function:a", "default": "clearance:normal"}),
            json!({"id": "hole.function:b", "default": "tapped:M6"}),
            json!({"id": "diameter.fit:c", "default": "knurl:straight"}),
            json!({"id": "diameter.fit:d", "default": null}),
            json!({"id": "part.material", "default": null}),
        ];
        let added = default_path_answers(&questions, &map(json!({"hole.function:a": "fit:H7"})));
        assert_eq!(
            added,
            map(json!({"part.locating": PLUS_MINUS, "diameter.fit:d": "general"}))
        );
    }

    #[test]
    fn a_project_opens_with_the_tolerancing_it_had() {
        let legacy = State {
            answers: map(json!({"part.locating": PLUS_MINUS})),
            auto: None,
            gdt: None,
        };
        assert_eq!(opened(&legacy, None).gdt, Some(Mode::Plusminus));
        let current = State {
            answers: Map::new(),
            auto: Some(vec![]),
            gdt: None,
        };
        assert_eq!(
            opened(&current, None),
            current,
            "a default-path project stays on it"
        );
        let resumed = opened(
            &current,
            Some(&map(json!({"part.locating": "position tolerances"}))),
        );
        assert_eq!(resumed.gdt, Some(Mode::Geometric));
    }
}
