use crate::input;
use haecceity::step;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::{
    api, carried,
    features::Feature,
    part::{self, Binding, LOADER_VERSION, LoadedPart},
    resume,
    revision::{self, assembly},
    tolerancing,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

fn check_input(name: &str, bytes: &[u8]) -> Result<(), String> {
    input::size("STEP input", bytes.len(), input::STEP_LIMIT).map_err(|e| e.to_string())?;
    input::size("Input name", name.len(), input::TEXT_LIMIT).map_err(|e| e.to_string())?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err("Decompress the STEP file before opening it.".into());
    }
    Ok(())
}

/// Compares two revisions of a part held in memory, as `specify-core diff` compares two files,
/// carrying `answers` (the old file's stored answers when empty): `{report, markdown, view}`,
/// the report as `report.json` holds it, as Markdown headed *title* (with no picture), and what
/// a viewer needs to draw the change ([`revision::view`]).
///
/// Two revisions of an assembly (either file of several parts, or of a part placed other than
/// once) are compared part by part ([`assembly::diff`]), `answers` then by part name:
/// `{report, markdown, parts}`, the assembly's report and Markdown, and each part's
/// `{name, status, old, new}` with its `report` when it is in both files, and its `markdown`
/// (headed `#### name`) and `view` when it changed: a changed part is shaped as a part's diff.
pub fn diff(
    old: (&str, &[u8]),
    new: (&str, &[u8]),
    answers: &Map<String, Value>,
    title: &str,
) -> Result<Value, String> {
    diff_with_options(old, new, answers, title, revision::DiffOptions::default())
}

/// Include exact comparison scenes when requested, without re-deriving them.
pub fn diff_with_options(
    old: (&str, &[u8]),
    new: (&str, &[u8]),
    answers: &Map<String, Value>,
    title: &str,
    options: revision::DiffOptions,
) -> Result<Value, String> {
    let read = |(name, bytes): (&str, &[u8])| -> Result<part::File, String> {
        check_input(name, bytes)?;
        part::File::of_bytes(Path::new(name), bytes.to_vec())
            .map_err(|error| format!("{name}: {error}"))
    };
    let (old_file, new_file) = (read(old)?, read(new)?);
    if assembly::is_assembly(&old_file) || assembly::is_assembly(&new_file) {
        return diff_assembly(&old_file, &new_file, answers, title, options);
    }
    input::answers_shape(answers).map_err(|e| e.to_string())?;
    let load = |file: &part::File, name: &str| -> Result<LoadedPart, String> {
        file.load(None).map_err(|error| format!("{name}: {error}"))
    };
    let (before, after) = (load(&old_file, old.0)?, load(&new_file, new.0)?);
    let revised = revision::diff_with_options(&before, &after, answers, options)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "report": serde_json::to_value(&revised).map_err(|error| error.to_string())?,
        "markdown": revised.markdown(title, None),
        "view": revision::view(&before, &after, &revised),
    }))
}

fn diff_assembly(
    old: &part::File,
    new: &part::File,
    answers: &Map<String, Value>,
    title: &str,
    options: revision::DiffOptions,
) -> Result<Value, String> {
    let load = |file: &part::File| file.load_all().map_err(|error| error.to_string());
    let (before, after) = (load(old)?, load(new)?);
    let compared = assembly::diff_with_options(&before, &after, answers, options)
        .map_err(|error| error.to_string())?;
    let parts = compared
        .parts
        .iter()
        .map(|p| {
            let mut row = json!({"name": p.name, "status": p.status, "old": p.old, "new": p.new});
            if let Some(r) = &p.revised {
                row["report"] = serde_json::to_value(r).map_err(|error| error.to_string())?;
                if let ("changed", Some(o), Some(n)) = (p.status, &p.old, &p.new) {
                    row["markdown"] = r
                        .markdown_headed(&format!("#### {}", p.name), None, None)
                        .into();
                    row["view"] = revision::view(&before[o.part], &after[n.part], r);
                }
            }
            Ok(row)
        })
        .collect::<Result<Vec<Value>, String>>()?;
    Ok(json!({
        "report": serde_json::to_value(&compared).map_err(|error| error.to_string())?,
        "markdown": compared.markdown(title, None, None),
        "parts": parts,
    }))
}

pub struct Session {
    loaded: LoadedPart,
    analysis: Value,
    scene: specify_core::pmi_scene::Scene,
}

impl Session {
    /// Opens a file of one part used once. A file of several parts, or of one part placed more
    /// than once, is refused: list its parts with [`Session::parts`] and open one with
    /// [`Session::open_part`].
    pub fn open(name: &str, bytes: &[u8]) -> Result<Self, String> {
        Self::load(name, bytes, None)
    }

    /// Opens part `part` (an index from [`Session::parts`]) of any file, an assembly's or one
    /// part's placed several times included, as the command line's `--part` does: the part is
    /// analysed in its own frame, the file's other parts consulted for its mates, and
    /// [`Session::write_pmi`] writes it into the original file, keeping every other part and
    /// placement as it was (decision D3). Part 0 of a file of one part used once opens as
    /// [`Session::open`] opens it.
    pub fn open_part(name: &str, bytes: &[u8], part: usize) -> Result<Self, String> {
        Self::load(name, bytes, Some(part))
    }

    /// The distinct parts of the file, as `specify-core parts` lists them (`api::parts`):
    /// `[{part, name, product_id, faces, instances, placements, written, same_as}]`.
    pub fn parts(name: &str, bytes: &[u8]) -> Result<Value, String> {
        check_input(name, bytes)?;
        let file = part::File::of_bytes(Path::new(name), bytes.to_vec())
            .map_err(|error| error.to_string())?;
        Ok(Value::Array(api::parts_of(&file)))
    }

    fn load(name: &str, bytes: &[u8], chosen: Option<usize>) -> Result<Self, String> {
        check_input(name, bytes)?;
        let mut definitions =
            step::read_part_definitions(bytes).map_err(|error| error.to_string())?;
        let part_count = definitions.len();
        let index = match chosen {
            None => {
                if part_count != 1 {
                    return Err(format!(
                        "Specify browser loading without a part chosen requires one distinct part; found {part_count}. List the parts with parts() and open one with openPart()."
                    ));
                }
                // The length check above proves this is the sole definition.
                if definitions[0].placements.len() != 1 {
                    return Err(
                        "Specify browser loading without a part chosen requires one instance of the part. Open it with openPart(name, bytes, 0)."
                            .into(),
                    );
                }
                0
            }
            Some(index) if index < part_count => index,
            Some(index) => {
                return Err(format!(
                    "{name} has {part_count} part(s); there is no part {index}"
                ));
            }
        };
        let definition = definitions.swap_remove(index);
        let placements = definition.placements.iter().map(|p| p.placement).collect();
        // The kernel owns the part/frame/source mapping; refuse any ordered-face mismatch.
        let part = step::read_part(bytes, &definition).map_err(|error| error.to_string())?;
        if part.faces.len() != definition.faces.len()
            || definition.faces.iter().enumerate().any(|(i, entity)| {
                part.face_source(i)
                    .is_none_or(|source| source.entity != *entity)
            })
        {
            return Err(
                "The browser loader cannot preserve this part's ordered STEP face identity.".into(),
            );
        }
        let doc =
            haecceity::p21::Document::parse(bytes.to_vec()).map_err(|error| error.to_string())?;
        let face_names = haecceity::json::Anchors::read(&doc, &definition).names;
        let loaded = LoadedPart {
            face_names,
            path: PathBuf::from(name),
            bytes: Arc::new(bytes.to_vec()),
            binding: Binding {
                source_sha256: Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                face_count: part.faces.len(),
                loader_version: LOADER_VERSION,
                part: index,
            },
            unresolved_faces: part.unresolved_faces().to_vec(),
            unresolved_edges: part.unresolved_edges().to_vec(),
            part,
            // A file of one part used once keeps the name `open` has always given it (the file's);
            // any other part, a part placed twice included, is named as `part::File::load` names
            // it (its product definition's). The name only prefixes report lines of several parts
            // written together, so neither choice changes a session's output.
            name: if part_count == 1 && definition.placements.len() == 1 {
                name.into()
            } else {
                definition.name.clone()
            },
            definition,
            part_count,
            placements,
            rebuilt: false,
        };
        // A part of an assembly is analysed with the file's other parts, read from these bytes.
        let analysis = api::analyse(&loaded).map_err(|error| error.to_string())?;
        let scene = specify_core::pmi_scene::Scene::new(&loaded)?;
        Ok(Self {
            loaded,
            analysis,
            scene,
        })
    }

    pub fn pmi_scene(&self, answers: &Map<String, Value>) -> Result<Value, String> {
        input::answers(&self.analysis, answers).map_err(|e| e.to_string())?;
        self.scene
            .scene(&self.loaded, Some(&self.analysis), Some(answers))
    }

    pub fn pmi_scene_state(&self, state: &tolerancing::State) -> Result<Value, String> {
        input::answers(&self.analysis, &state.answers).map_err(|e| e.to_string())?;
        self.scene
            .scene_with_state(&self.loaded, Some(&self.analysis), state)
    }

    pub fn snapshot(&self) -> Value {
        json!({"analysis": self.analysis, "mesh": api::mesh(&self.loaded),
            "provenance": {"source_sha256": self.loaded.binding.source_sha256, "face_entities": self.loaded.definition.faces, "part": self.loaded.binding.part}})
    }

    /// Write and verify the answered PMI, with the answers stored. `project` is the editor's
    /// project already in base64 (stored as `draftwright editor project`); `payload` is any
    /// application's `{name, value}`, stored as base64 JSON under `name` (`carried`). Either
    /// also records the original part, which [`Session::original`] gives back. Without either,
    /// the written file carries no project, payload or original, even if the opened one did.
    /// For a part of an assembly ([`Session::open_part`]) all of it is written on that part
    /// only, into the original file: the other parts, their placements, PMI, answers and
    /// payloads are kept byte for byte, and the original recorded is the whole file.
    pub fn write_pmi(
        &self,
        answers: &Map<String, Value>,
        project: Option<&str>,
        payload: Option<&Value>,
    ) -> Result<Value, String> {
        self.write_pmi_with_bom(answers, project, payload, false)
    }

    pub fn write_pmi_with_bom(
        &self,
        answers: &Map<String, Value>,
        project: Option<&str>,
        payload: Option<&Value>,
        bom: bool,
    ) -> Result<Value, String> {
        // A complete old sidecar may also contain feature IDs that disappeared. Diagnose
        // its geometry evidence before interpreting any of those IDs on this part.
        specify_core::answer_file::check(&self.loaded, answers)?;
        input::answers(&self.analysis, answers).map_err(|e| e.to_string())?;
        let mut intent =
            api::preview(&self.analysis, answers).map_err(|error| error.to_string())?;
        // Preview leaves out what apply reports as not written (#126); a write lists it.
        intent.requirements.extend(
            specify_core::rules::unasked_rows(&self.analysis, answers)
                .map_err(|error| error.to_string())?,
        );
        if bom {
            specify_core::bom::accept_suggestions(&self.analysis, answers, &mut intent)?;
        }
        let extra = carried_attributes(&self.loaded.bytes, project, payload)?;
        let (bytes, report) =
            specify_core::writer::write_bytes_with(&self.loaded, &intent, Some(answers), &extra)
                .map_err(|error| error.to_string())?;
        Ok(
            json!({"bytes": bytes, "source_sha256": self.loaded.binding.source_sha256, "report": report.to_dict()}),
        )
    }

    /// The face evidence to keep with answers held outside the STEP, for
    /// [`Session::rematch_answers`]: `AnswerEvidence` JSON (`resume::evidence`).
    pub fn evidence(&self) -> Result<Value, String> {
        resume::evidence(&self.loaded).map_err(|error| error.to_string())
    }

    /// Answers held outside the STEP matched to this part's features (`resume::rematch_answers`):
    /// `{answers, unresolved?, migrated?, unverified?, automatic?}`. Does not change the session.
    pub fn rematch_answers(
        &self,
        answers: &Map<String, Value>,
        evidence: Option<&Value>,
        automatic: Option<&[String]>,
    ) -> Result<Value, String> {
        let features: Vec<Feature> = serde_json::from_value(self.analysis["features"].clone())
            .map_err(|error| error.to_string())?;
        let resumed =
            resume::rematch_answers(&self.loaded, &features, answers, evidence, automatic)
                .map_err(|error| error.to_string())?;
        serde_json::to_value(resumed).map_err(|error| error.to_string())
    }

    /// The payload `name` the opened STEP carries, if any (`carried::payload`); for a part of
    /// an assembly, the one written on this part (`carried::part_payload`).
    pub fn payload(&self, name: &str) -> Option<Value> {
        if self.loaded.part_count == 1 {
            carried::payload(&self.loaded.bytes, name)
        } else {
            carried::part_payload(&self.loaded.bytes, self.loaded.binding.part, name)
        }
    }

    /// The part the opened STEP was first written from, if it records it and the bytes
    /// verify (`carried::original`).
    pub fn original(&self) -> Option<Vec<u8>> {
        carried::original(&self.loaded.bytes)
    }

    /// Return ordinary editable answers, preserving supplied values and leaving questions
    /// without defaults open. This does not mutate the session or write PMI.
    pub fn accept_defaults(
        &self,
        answers: &Map<String, Value>,
    ) -> Result<Map<String, Value>, String> {
        crate::accept_defaults(&self.analysis, answers)
    }

    /// A complete answer file with accepted defaults and this part's evidence. Unlike
    /// accept_defaults, refuses incomplete sets; never writes or changes the session.
    pub fn export_answers(
        &self,
        answers: &Map<String, Value>,
    ) -> Result<Map<String, Value>, String> {
        specify_core::answer_file::check(&self.loaded, answers)?;
        input::answers(&self.analysis, answers).map_err(|e| e.to_string())?;
        api::export_answers(&self.loaded, &self.analysis, answers).map_err(|e| e.to_string())
    }

    /// The review settled to the default path (`specify_core::tolerancing::settle`), with the
    /// label anchors [`Session::review`] adds: `{review, state}`.
    pub fn settle(&self, state: &tolerancing::State) -> Result<Value, String> {
        input::answers(&self.analysis, &state.answers).map_err(|e| e.to_string())?;
        let (review, settled) =
            tolerancing::settle(&self.analysis, state).map_err(|error| error.to_string())?;
        let review = self.with_anchors(review);
        Ok(json!({"review": review, "state": settled}))
    }

    pub fn review(&self, answers: &Map<String, Value>) -> Result<Value, String> {
        input::answers(&self.analysis, answers).map_err(|e| e.to_string())?;
        let review = api::review(&self.analysis, answers).map_err(|error| error.to_string())?;
        Ok(self.with_anchors(review))
    }

    fn with_anchors(&self, mut review: Value) -> Value {
        // JSON reads return Null on a missing key; api::review always constructs an object intent.
        let groups = review["intent"]["datums"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(
                review["intent"]["requirements"]
                    .as_array()
                    .into_iter()
                    .flatten(),
            )
            .filter_map(|item| item["faces"].as_array())
            .map(|faces| {
                faces
                    .iter()
                    .filter_map(|face| face.as_u64().map(|face| face as usize))
                    .collect()
            })
            .collect::<Vec<Vec<usize>>>();
        review["intent"]["anchors"] = json!(api::anchors(&self.loaded, &groups));
        review
    }
}

/// The editor's base64 `project`, or an application's `{name, value}` `payload`, as the
/// attributes the writer adds; at most one of them.
fn carried_attributes(
    source: &[u8],
    project: Option<&str>,
    payload: Option<&Value>,
) -> Result<Vec<haecceity::pmi::AttributeSet>, String> {
    const PROJECT: &str = "draftwright editor project";
    if let Some(text) = project {
        input::size("Project", text.len(), input::JSON_LIMIT).map_err(|e| e.to_string())?;
    }
    match (project, payload) {
        (Some(_), Some(_)) => Err("give a project or a payload, not both".into()),
        (Some(text), None) => carried::attributes_encoded(source, PROJECT, text)
            .map_err(|_| "the project to store is not base64".to_string()),
        (None, Some(payload)) => {
            let (Some(name), Some(value)) = (payload["name"].as_str(), payload.get("value")) else {
                return Err("Invalid payload: expected {name, value}".into());
            };
            input::size("Payload name", name.len(), input::TEXT_LIMIT)
                .map_err(|e| e.to_string())?;
            carried::attributes(source, name, value).map_err(|e| e.to_string())
        }
        (None, None) => Ok(vec![]),
    }
}
