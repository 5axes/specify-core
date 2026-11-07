#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

use serde_json::{Map, Value, json};
use specify_core::input;
use specify_core::{api, choices};
pub mod session;

/// Decode diff answers before dispatching a single part or an assembly.
pub fn diff_answers(text: &str) -> Result<Map<String, Value>, String> {
    input::size("Answers", text.len(), input::JSON_LIMIT).map_err(|e| e.to_string())?;
    let answers = serde_json::from_str(text).map_err(|e| format!("Invalid answers: {e}"))?;
    // Part names are valid keys here. Validate each answer map after dispatch,
    // when the part owning an invalid question id is known.
    Ok(answers)
}

/// Accept every unanswered default to the same fixed point as CLI/serve apply.
pub fn accept_defaults(
    analysis: &Value,
    answers: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    input::answers(analysis, answers).map_err(|e| e.to_string())?;
    specify_core::rules::with_all_defaults(analysis, answers).map_err(|error| error.to_string())
}

pub fn interpret(
    analysis: &Value,
    answers: &Map<String, Value>,
    question_id: &str,
    text: &str,
) -> Result<Value, String> {
    input::answers(analysis, answers).map_err(|e| e.to_string())?;
    let questions = api::questions(analysis, answers).map_err(|error| error.to_string())?;
    let question = questions
        .iter()
        .find(|question| question.id == question_id && question.kind == "choice")
        .ok_or_else(|| format!("No choice question {question_id}"))?;
    let value = choices::interpret(
        &question.id,
        text,
        &question.options,
        question.diameter,
        question.depth,
    )
    .map_err(|error| error.0)?;
    let shown = if question.id.starts_with("hole.function:") {
        choices::with_default_full_thread(&value, question.depth)
    } else {
        value.clone()
    };
    // result is an object constructed here, so adding its callout key cannot panic.
    let mut result = json!({"value": value, "code": choices::code(&shown)});
    if let Some(diameter) = question.diameter
        && (question.id.starts_with("hole.function:") || question.id.starts_with("diameter.fit:"))
    {
        result["callout"] = json!(choices::callout(
            &shown,
            diameter,
            question.id.starts_with("hole."),
        )?);
    }
    Ok(result)
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use specify_core::tolerancing;
    use wasm_bindgen::prelude::*;

    /// Types for the JSON report returned by `diff`; ids/numbers match `pmiScene`.
    #[wasm_bindgen(typescript_custom_section)]
    const PMI_DIFF_TYPES: &str = r#"
/** Faces in analysis/snapshot JSON; `name` is a unique authored STEP `f:` identity. */
export interface AnalysisFace {
  id: number;
  name?: string;
  kind: string;
  area: number;
  centroid: [number, number, number];
  direction?: [number, number, number];
  origin?: [number, number, number];
  radius?: number;
  contact?: unknown;
}
export interface GeometryCarrierExemption {
  part: number;
  condition: "shared-carrier-controlled-pair" | "shared-carrier-datum-target-support" | "kept-carrier-datum-member" | "kept-carrier-independent-feature" | "kept-carrier-datum-feature" | "kept-carrier-datum-composite";
  aspect: number;
  feature: number;
}
export interface PreservedNormalization {
  id: number;
  normalization: string;
}
/** A kernel finding about the default Notes saved view. */
export interface SavedViewFinding {
  kind: string;
  part?: number;
  ids: number[];
  entity: string;
  detail: string;
}
/** Report in writePmi JSON; optional diagnostic keys are absent when empty. */
export interface WriteReport {
  output: string;
  written: string[];
  not_written: string[];
  warnings: string[];
  geometry_carrier_exemptions?: GeometryCarrierExemption[];
  presentation_warnings?: string[];
  preserved_normalizations?: PreservedNormalization[];
  retained_shared?: number[];
  saved_view_findings?: SavedViewFinding[];
}
/** JSON returned by evidence(), also embedded alongside stored answers. */
export interface AnswerEvidence {
  schema: 1 | 2 | 3;
  face_count: number;
  faces: string;
  face_names?: (string | null)[];
  geometry?: unknown;
}
/** Optional reserved `_specify` member of an otherwise ordinary answer map. */
export interface AnswerFileMetadata {
  evidence?: AnswerEvidence;
  unresolved?: Record<string, string>;
}
export interface PmiItemChange {
  class: "added" | "removed" | "changed" | "renumbered";
  number?: number;
  id: string;
  old_id?: string;
  faces: { old: { face: number }[]; new: { face: number }[] };
  kind: string;
  status: "answered" | "default" | "general" | "existing";
  on: string;
  old?: string;
  new?: string;
  reason?: string;
}
export interface DiffPmi {
  items: PmiItemChange[];
  scenes?: { old: PmiScene; new: PmiScene };
  /** @deprecated Legacy answer comparison, retained for one release. */
  stored: unknown[];
  /** @deprecated Legacy CAD comparison, retained for one release. */
  existing: unknown[];
  existing_count: [number, number];
  answers_from: "given" | "stored" | "none";
  new_stored: boolean;
}
"#;

    /// Types for the JSON returned by both scene entry points.
    #[wasm_bindgen(typescript_custom_section)]
    const PMI_SCENE_TYPES: &str = r#"
/** Document-local semantic or graphic identity; never a mesh face index. */
export type PmiSceneId = string;
export type PmiSceneVector = [number, number, number];
export type PmiSceneStatus = "answered" | "default" | "general" | "existing";
/** A lossless semantic/unit row. Values follow the packaged haecceity.pmi/1 schema;
 * decimal quantities are strings and semantic references use PmiSceneId. */
export interface PmiSceneItem {
  identifier?: string;
  id: PmiSceneId;
  value: unknown;
}
export interface PmiSceneItems {
  standards: unknown[];
  decimal_places: number | null;
  features: PmiSceneItem[];
  datum_targets: PmiSceneItem[];
  datums: PmiSceneItem[];
  dimensions: PmiSceneItem[];
  tolerances: PmiSceneItem[];
  tolerance_relations: PmiSceneItem[];
  general: PmiSceneItem[];
  threads: PmiSceneItem[];
  knurls: PmiSceneItem[];
  holes: unknown[];
  material: unknown;
  notes: PmiSceneItem[];
  surface_textures: PmiSceneItem[];
  attributes: PmiSceneItem[];
  geometry: PmiSceneItem[];
  derived_geometry: PmiSceneItem[];
}
/** Placement in model-space millimetres. A placeholder can omit text_index. */
export interface PmiSceneTextPlacement {
  text_index?: number;
  /** Lower-left / upper-right in plane mm relative to origin. */
  cell_box?: [[number, number], [number, number]];
  /** True when this run's glyphs are already drawn by the graphic's polylines.
   * Absent or false means the host may render the text literal. */
  stroked?: boolean;
  origin: PmiSceneVector;
  x_axis: PmiSceneVector;
  normal: PmiSceneVector;
  height?: number;
  line_spacing?: number;
  alignment?: string;
}
export interface PmiSceneGraphic {
  leader?: PmiSceneVector;
  id?: PmiSceneId;
  plane_id?: PmiSceneId;
  text_placements?: PmiSceneTextPlacement[];
  source: "generated" | "file";
  name: string;
  text: string[];
  polylines: PmiSceneVector[][];
  polyline_roles?: ("text" | "frame" | "leader" | "extension" | "symbol" | "dimension")[];
  triangles: [PmiSceneVector, PmiSceneVector, PmiSceneVector][];
  step_ids: number[];
  unsupported: string[];
}
export interface PmiScenePlane {
  id: PmiSceneId;
  name: string;
  origin: PmiSceneVector;
  x_axis: PmiSceneVector;
  normal: PmiSceneVector;
}
/** Saved/group or generated plane view; direction points into the scene. */
export interface PmiSceneView {
  id: PmiSceneId;
  name: string;
  direction?: PmiSceneVector;
  up?: PmiSceneVector;
  graphics: PmiSceneId[];
}
export interface PmiSceneMetadata {
  /** This requirement's semantic note wording, independent of shared graphics. */
  notation?: string;
  status: PmiSceneStatus;
  question?: string;
  number?: number;
  origin?: "requirement" | "general_tolerance" | "imported";
  source?: {schema: number; faces: string; support: number[]; parameter: string};
  /** A basic (theoretically exact) dimension; absent otherwise. */
  basic?: true;
}
/** JSON.parse of either pmiScene call. Supporting rows may lack a specify entry. */
export interface PmiScene {
  pmi: {
    schema: "haecceity.pmi/1";
    units: {length: PmiSceneItem[]; angle: PmiSceneItem[]; density: PmiSceneItem[]; other: PmiSceneItem[]};
    pmi: PmiSceneItems;
  };
  presentation: {
    schema: "haecceity.presentation/1";
    length_unit: string;
    semantic_ids: PmiSceneId[];
    items: Record<PmiSceneId, PmiSceneGraphic[]>;
    unlinked: PmiSceneGraphic[];
    planes?: PmiScenePlane[];
    views?: PmiSceneView[];
  };
  specify: Record<PmiSceneId, PmiSceneMetadata>;
}
"#;

    #[wasm_bindgen]
    pub struct IntentEngine(session::Session);

    #[wasm_bindgen]
    impl IntentEngine {
        #[wasm_bindgen(constructor)]
        pub fn open(name: &str, bytes: &[u8]) -> Result<IntentEngine, JsValue> {
            session::Session::open(name, bytes)
                .map(IntentEngine)
                .map_err(|error| JsValue::from_str(&error))
        }
        /// Part `part` (an index from `IntentEngine.parts`) of any file, an assembly included.
        #[wasm_bindgen(js_name = openPart)]
        pub fn open_part(name: &str, bytes: &[u8], part: usize) -> Result<IntentEngine, JsValue> {
            session::Session::open_part(name, bytes, part)
                .map(IntentEngine)
                .map_err(|error| JsValue::from_str(&error))
        }

        /// The file's distinct parts, as `specify-core parts` lists them: `[{part, name,
        /// product_id, faces, instances, placements, written, same_as}]`.
        pub fn parts(name: &str, bytes: &[u8]) -> Result<String, JsValue> {
            session::Session::parts(name, bytes)
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
        /// Assembly BOM JSON; each field has value and file/answered/default source.
        pub fn bom(bytes: &[u8]) -> Result<String, JsValue> {
            input::size("STEP", bytes.len(), input::STEP_LIMIT).map_err(js_error)?;
            specify_core::bom::read_bytes(bytes.to_vec())
                .and_then(|rows| serde_json::to_string(&rows).map_err(|e| e.to_string()))
                .map_err(js_error)
        }
        /// `PmiScene` JSON for current answers or `{answers, auto?, gdt?}` review state.
        /// Parse with `JSON.parse`; ids join semantic rows, graphics and Specify statuses.
        #[wasm_bindgen(js_name = pmiScene)]
        pub fn pmi_scene(&self, answers: &str) -> Result<String, JsValue> {
            let value = json("Answers or review state", answers)?;
            let scene = if value.get("answers").is_some_and(Value::is_object) {
                self.0.pmi_scene_state(&self::state(answers)?)
            } else {
                self.0.pmi_scene(&self::answers(answers)?)
            };
            scene.map(|value| value.to_string()).map_err(js_error)
        }

        pub fn snapshot(&self) -> String {
            self.0.snapshot().to_string()
        }

        /// `project` is the editor's base64 project; `payload` is `{name, value}` JSON. At most one.
        /// Rewriting replaces what a previous write carried: without either, the written file
        /// carries no project, payload or original record, even if the opened one did.
        #[wasm_bindgen(js_name = writePmi)]
        pub fn write_pmi(
            &self,
            answers: &str,
            project: Option<String>,
            payload: Option<String>,
            bom: Option<bool>,
        ) -> Result<String, JsValue> {
            let answers = self::answers(answers)?;
            let payload = payload
                .as_deref()
                .map(|text| json("Payload", text))
                .transpose()?;
            self.0
                .write_pmi_with_bom(
                    &answers,
                    project.as_deref(),
                    payload.as_ref(),
                    bom.unwrap_or(false),
                )
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        /// `AnswerEvidence` JSON: keep it with answers saved outside the STEP.
        pub fn evidence(&self) -> Result<String, JsValue> {
            self.0
                .evidence()
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        /// Saved answers matched to this part: `{answers, unresolved?, migrated?, unverified?,
        /// automatic?}`. `evidence` is what `evidence()` gave when they were saved; `automatic`
        /// a JSON list of the IDs the default path answered. Without evidence answers are
        /// matched by feature ID. Schema 3 matches exact authored names first; a missing or
        /// duplicate name is unresolved without geometric fallback. Unnamed faces use
        /// verified indices or saved geometric correspondence. Schema 1/2 retain their
        /// fingerprint checks. Extras and datum lists follow their face supports.
        #[wasm_bindgen(js_name = rematchAnswers)]
        pub fn rematch_answers(
            &self,
            answers: &str,
            evidence: Option<String>,
            automatic: Option<String>,
        ) -> Result<String, JsValue> {
            let answers = self::answers(answers)?;
            let evidence = evidence
                .as_deref()
                .map(|text| json("Evidence", text))
                .transpose()?;
            let automatic: Option<Vec<String>> = automatic
                .as_deref()
                .map(|text| {
                    serde_json::from_value(json("Automatic", text)?)
                        .map_err(|error| JsValue::from_str(&format!("Invalid automatic: {error}")))
                })
                .transpose()?;
            self.0
                .rematch_answers(&answers, evidence.as_ref(), automatic.as_deref())
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        /// The JSON payload `name` the opened STEP carries, if any.
        pub fn payload(&self, name: &str) -> Result<Option<String>, JsValue> {
            limit("Payload name", name, input::TEXT_LIMIT)?;
            Ok(self.0.payload(name).map(|value| value.to_string()))
        }

        /// The part the opened STEP was first written from, if it records it.
        pub fn original(&self) -> Option<Vec<u8>> {
            self.0.original()
        }
        /// The review settled to the default path, and the settled `{answers, auto, gdt}`.
        pub fn settle(&self, state: &str) -> Result<String, JsValue> {
            let state = self::state(state)?;
            self.0
                .settle(&state)
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
        pub fn review(&self, answers: &str) -> Result<String, JsValue> {
            let answers = self::answers(answers)?;
            self.0
                .review(&answers)
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        /// Complete answer map, with defaults explicit and `_specify.evidence`, for saving.
        /// Throws if any required answer or carry block remains unanswered.
        #[wasm_bindgen(js_name = exportAnswers)]
        pub fn export_answers(&self, answers: &str) -> Result<String, JsValue> {
            self.0
                .export_answers(&self::answers(answers)?)
                .map(|answers| Value::Object(answers).to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        #[wasm_bindgen(js_name = acceptDefaults)]
        pub fn accept_defaults(&self, answers: &str) -> Result<String, JsValue> {
            self.0
                .accept_defaults(&self::answers(answers)?)
                .map(|answers| Value::Object(answers).to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
    }

    fn parse(analysis: &str, answers: &str) -> Result<(Value, Map<String, Value>), JsValue> {
        limit("Analysis", analysis, input::JSON_LIMIT)?;
        let analysis = serde_json::from_str(analysis)
            .map_err(|error| JsValue::from_str(&format!("Invalid analysis: {error}")))?;
        let answers = self::answers(answers)?;
        Ok((analysis, answers))
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }

    /// Two revisions of a part compared (`session::diff`): `{report, markdown, view}` JSON; of
    /// an assembly, `{report, markdown, parts}`, each changed part with its own `markdown` and
    /// `view`. `answers` is a JSON object (by part name for an assembly), `{}` to carry the old
    /// file's stored answers.
    #[wasm_bindgen(js_name = diff)]
    pub fn diff_json(
        old_name: &str,
        old: &[u8],
        new_name: &str,
        new: &[u8],
        answers: &str,
        title: &str,
        scenes: Option<bool>,
    ) -> Result<String, JsValue> {
        limit("Title", title, input::TEXT_LIMIT)?;
        let answers = crate::diff_answers(answers).map_err(js_error)?;
        session::diff_with_options(
            (old_name, old),
            (new_name, new),
            &answers,
            title,
            specify_core::revision::DiffOptions {
                scenes: scenes.unwrap_or(false),
            },
        )
        .map(|value| value.to_string())
        .map_err(|error| JsValue::from_str(&error))
    }

    /// View-only `PmiScene` JSON without an interview; `part` selects an assembly part.
    /// Parse with `JSON.parse`; file graphics retain their text placement and saved views.
    #[wasm_bindgen(js_name = pmiScene)]
    pub fn pmi_scene(bytes: &[u8], part: Option<usize>) -> Result<String, JsValue> {
        input::size("STEP input", bytes.len(), input::STEP_LIMIT).map_err(js_error)?;
        specify_core::pmi_scene::pmi_scene(bytes, part)
            .map(|value| value.to_string())
            .map_err(js_error)
    }

    #[wasm_bindgen(js_name = review)]
    pub fn review_json(analysis: &str, answers: &str) -> Result<String, JsValue> {
        let (analysis, answers) = parse(analysis, answers)?;
        input::answers(&analysis, &answers).map_err(js_error)?;
        api::review(&analysis, &answers)
            .map(|value| value.to_string())
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = acceptDefaults)]
    pub fn accept_defaults_json(analysis: &str, answers: &str) -> Result<String, JsValue> {
        let (analysis, answers) = parse(analysis, answers)?;
        accept_defaults(&analysis, &answers)
            .map(|answers| Value::Object(answers).to_string())
            .map_err(|error| JsValue::from_str(&error))
    }

    fn js_error(error: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&error.to_string())
    }
    fn limit(field: &'static str, text: &str, limit: usize) -> Result<(), JsValue> {
        input::size(field, text.len(), limit).map_err(js_error)
    }
    fn state(json: &str) -> Result<tolerancing::State, JsValue> {
        limit("State", json, input::JSON_LIMIT)?;
        let state: tolerancing::State = serde_json::from_str(json)
            .map_err(|error| JsValue::from_str(&format!("Invalid state: {error}")))?;
        // Saved auto IDs may be stale or repeated; core transforms handle them safely.
        input::answers_shape(&state.answers).map_err(js_error)?;
        Ok(state)
    }
    fn json(field: &'static str, text: &str) -> Result<Value, JsValue> {
        limit(field, text, input::JSON_LIMIT)?;
        serde_json::from_str(text).map_err(|error| {
            JsValue::from_str(&format!("Invalid {}: {error}", field.to_lowercase()))
        })
    }
    fn answers(json: &str) -> Result<Map<String, Value>, JsValue> {
        limit("Answers", json, input::JSON_LIMIT)?;
        let answers = serde_json::from_str(json)
            .map_err(|error| JsValue::from_str(&format!("Invalid answers: {error}")))?;
        input::answers_shape(&answers).map_err(js_error)?;
        Ok(answers)
    }
    fn out(state: tolerancing::State) -> Result<String, JsValue> {
        serde_json::to_string(&state).map_err(js_error)
    }

    /// The person's answers replace the state's (`tolerancing::answer_edit`).
    #[wasm_bindgen(js_name = answerEdit)]
    pub fn answer_edit(state: &str, answers: &str) -> Result<String, JsValue> {
        out(tolerancing::answer_edit(
            &self::state(state)?,
            &self::answers(answers)?,
        ))
    }

    /// A change of tolerancing mode (`plusminus`, `geometric`, or none for the default path).
    #[wasm_bindgen(js_name = tolerancingEdit)]
    pub fn tolerancing_edit(
        state: &str,
        mode: Option<String>,
        answers: &str,
    ) -> Result<String, JsValue> {
        if let Some(text) = mode.as_deref() {
            limit("Mode", text, input::TEXT_LIMIT)?;
        }
        let mode = mode
            .map(|mode| serde_json::from_value(Value::String(mode)))
            .transpose()
            .map_err(|error| JsValue::from_str(&format!("Invalid mode: {error}")))?;
        out(tolerancing::tolerancing_edit(
            &self::state(state)?,
            mode,
            &self::answers(answers)?,
        ))
    }

    /// The state a project opens with, given the answers a STEP Specify wrote resumes, if any.
    #[wasm_bindgen(js_name = openedTolerancing)]
    pub fn opened_tolerancing(state: &str, resume: Option<String>) -> Result<String, JsValue> {
        let resume = resume.as_deref().map(self::answers).transpose()?;
        out(tolerancing::opened(&self::state(state)?, resume.as_ref()))
    }

    #[wasm_bindgen(js_name = interpret)]
    pub fn interpret_json(
        analysis: &str,
        answers: &str,
        question_id: &str,
        text: &str,
    ) -> Result<String, JsValue> {
        limit("Question ID", question_id, input::TEXT_LIMIT)?;
        limit("Interpretation", text, input::TEXT_LIMIT)?;
        let (analysis, answers) = parse(analysis, answers)?;
        interpret(&analysis, &answers, question_id, text)
            .map(|value| value.to_string())
            .map_err(|error| JsValue::from_str(&error))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn blind_thread_interpretation_summary_matches_the_intent() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/spindle.step");
        let analysis = api::analyse_file(&path, None).unwrap();
        let questions = api::questions(&analysis, &Map::new()).unwrap();
        let hole = questions
            .iter()
            .find(|q| {
                q.id.starts_with("hole.function:")
                    && q.default == Some(json!("tapped:M5"))
                    && q.depth.is_some()
            })
            .unwrap();
        let result = interpret(&analysis, &Map::new(), &hole.id, "M5").unwrap();
        assert_eq!(result["value"], "tapped:M5");
        // #20: the proposed tap is written only once its interpreted answer is supplied.
        let answers = Map::from_iter([(hole.id.clone(), result["value"].clone())]);
        let answers = accept_defaults(&analysis, &answers).unwrap();
        let intent = api::preview(&analysis, &answers).unwrap();
        let thread = intent
            .requirements
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap();
        let full = thread["full_thread"].as_f64().unwrap();
        assert_eq!(result["code"], format!("M5 tapped, {full} mm full thread"));
        assert!(
            result["callout"]
                .as_str()
                .unwrap()
                .contains(&format!("{full} mm min full thread"))
        );
    }
}
