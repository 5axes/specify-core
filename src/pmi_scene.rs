//! Haecceity's semantic/presentation documents with Specify's interview metadata.
//! `Scene` caches the parsed source; answering never reloads STEP or geometry.
use std::collections::BTreeMap;

use haecceity::{
    json::{self as wire, Semantic, presentation},
    p21::Document,
    pmi::{self, NoteOwner, PartId, PartPmi},
    step,
};
use serde_json::{Map, Value, json};

use crate::{api, characteristics, part::LoadedPart, pmi_origin, resume, writer};

mod notes;

type PlannedScene = (Semantic, Map<String, Value>, BTreeMap<String, usize>);

type Bindings = BTreeMap<(String, usize), (String, Value, usize)>;

const NUMBERED: [&str; 6] = [
    "datums",
    "dimensions",
    "tolerances",
    "surface_textures",
    "threads",
    "knurls",
];

pub struct Scene {
    doc: Document,
    read: pmi::PmiRead,
    index: usize,
    stored: Option<Map<String, Value>>,
    foreign: Option<(Document, pmi::PmiRead)>,
}

impl Scene {
    pub fn new(loaded: &LoadedPart) -> Result<Self, String> {
        let doc = Document::parse(loaded.bytes.to_vec()).map_err(|e| e.to_string())?;
        let defs = step::read_part_definitions(&loaded.bytes).map_err(|e| e.to_string())?;
        let read = pmi::read(&doc, &defs).map_err(|e| e.to_string())?;
        let stored = resume::answers_for(loaded).map_err(|e| e.to_string())?;
        let foreign = if stored.is_some() && resume::scoped(&doc, loaded.binding.part) {
            let bytes = crate::provenance::without_owned(&doc, &read, &[loaded.binding.part])?;
            let doc = Document::parse(bytes).map_err(|e| e.to_string())?;
            let read = pmi::read(&doc, &defs).map_err(|e| e.to_string())?;
            Some((doc, read))
        } else {
            None
        };
        Ok(Self {
            doc,
            read,
            index: loaded.binding.part,
            stored,
            foreign,
        })
    }

    pub(crate) fn carries_numbers(&self, answers: &Map<String, Value>) -> Result<bool, String> {
        characteristics::carries_history(&self.read.parts[self.index], answers)
    }

    /// `None` shows only the file; `Some` shows the current answers. Stored origin/source are authoritative when showing the file.
    pub fn scene(
        &self,
        loaded: &LoadedPart,
        analysis: Option<&Value>,
        answers: Option<&Map<String, Value>>,
    ) -> Result<Value, String> {
        self.scene_mode(loaded, analysis, answers, false)
    }

    // Revision comparisons need current semantic notation, never stale As Drawn text.
    pub(crate) fn comparison(
        &self,
        loaded: &LoadedPart,
        analysis: &Value,
        answers: Option<&Map<String, Value>>,
    ) -> Result<Value, String> {
        self.scene_mode(loaded, Some(analysis), answers, true)
    }

    fn scene_mode(
        &self,
        loaded: &LoadedPart,
        analysis: Option<&Value>,
        answers: Option<&Map<String, Value>>,
        comparison: bool,
    ) -> Result<Value, String> {
        let before = &self
            .foreign
            .as_ref()
            .map_or(&self.read, |(_, read)| read)
            .parts[self.index];
        let foreign = (self.stored.is_none() || self.foreign.is_some()) && has_pmi(before);
        if let Some(answers) = answers {
            let owned;
            let analysis = match analysis {
                Some(a) => a,
                None => {
                    owned = api::analyse(loaded).map_err(|e| e.to_string())?;
                    &owned
                }
            };
            let (semantic, metadata, counts) = self.planned(loaded, analysis, answers, foreign)?;
            // Reusing a foreign datum does not author a new item or redraw its callout.
            if foreign
                && semantic.pmi.material.is_none()
                && !metadata
                    .values()
                    .any(|meta| matches!(meta["status"].as_str(), Some("answered" | "default")))
            {
                return if let Some((doc, read)) = &self.foreign {
                    self.file_from(loaded, None, comparison, Some((doc, read)))
                } else {
                    self.file(loaded, None, comparison)
                };
            }
            let mut graphics =
                presentation::generated(&semantic, &loaded.part).map_err(|e| e.to_string())?;
            if foreign {
                let mut planes = std::collections::BTreeSet::new();
                for id in &semantic.ids["datums"] {
                    if metadata
                        .get(id)
                        .is_some_and(|meta| meta["status"] == "existing")
                        && let Some(rows) = graphics.items.remove(id)
                    {
                        planes.extend(rows.into_iter().filter_map(|graphic| graphic.plane_id));
                    }
                }
                graphics.planes.retain(|plane| !planes.contains(&plane.id));
            }
            for (id, count) in counts {
                if !comparison
                    && count > 1
                    && let Some(items) = graphics.items.get_mut(&id)
                {
                    for item in items {
                        item.name = format!("{count}× {}", item.name);
                        if let Some(first) = item.text.first_mut() {
                            *first = format!("{count}× {first}");
                        }
                    }
                }
            }
            let mut result = encode(&semantic, graphics, metadata)?;
            if foreign {
                let file = if let Some((doc, read)) = &self.foreign {
                    self.file_from(loaded, None, comparison, Some((doc, read)))?
                } else {
                    self.file(loaded, None, comparison)?
                };
                merge(&mut result, file)?;
            } else if comparison {
                // Replace plans rebuild Specify requirements; keep CAD requirements even
                // when the source also contains saved interview answers.
                let mut file = self.scene_mode(loaded, Some(analysis), None, true)?;
                let existing: std::collections::BTreeSet<String> = file["specify"]
                    .as_object()
                    .ok_or("invalid metadata")?
                    .iter()
                    .filter(|(_, meta)| meta["status"] == "existing")
                    .map(|(id, _)| id.clone())
                    .collect();
                let replaced: std::collections::BTreeSet<String> = file["specify"]
                    .as_object()
                    .ok_or("invalid metadata")?
                    .keys()
                    .filter(|id| !existing.contains(*id))
                    .cloned()
                    .collect();
                for rows in file["pmi"]["pmi"]
                    .as_object_mut()
                    .ok_or("invalid PMI")?
                    .values_mut()
                {
                    if let Some(rows) = rows.as_array_mut() {
                        rows.retain(|row| {
                            row["id"].as_str().is_some_and(|id| !replaced.contains(id))
                        });
                    }
                }
                file["specify"]
                    .as_object_mut()
                    .ok_or("invalid metadata")?
                    .retain(|id, _| existing.contains(id));
                file["presentation"]["items"]
                    .as_object_mut()
                    .ok_or("invalid graphics")?
                    .retain(|id, _| existing.contains(id));
                // This is a scene of the retained CAD requirements, not the whole
                // file. Remove replaced identities along with their semantic rows
                // before merging it into the rebuilt answer scene.
                file["presentation"]["semantic_ids"]
                    .as_array_mut()
                    .ok_or("invalid semantic ids")?
                    .retain(|id| id.as_str().is_some_and(|id| !replaced.contains(id)));
                // Only notation is consumed here; the two generated layouts may place
                // the same part note differently when the file has extra foreign PMI.
                file["presentation"]["planes"] = json!([]);
                file["presentation"]["views"] = json!([]);
                merge(&mut result, file)?;
            }
            let original = foreign
                .then(|| {
                    wire::identify(
                        before.clone(),
                        Some(&self.read.identifiers.parts[self.index]),
                    )
                    .map_err(|e| e.to_string())
                })
                .transpose()?;
            notes::combined(&mut result, &semantic, original.as_ref(), loaded)?;
            return Ok(result);
        }
        // Reconstruct only the interview identities/numbers of Specify-written files.
        // PMI values and #77 metadata below always come from the stored model.
        let planned = if let Some(saved) = &self.stored {
            let owned;
            let analysis = match analysis {
                Some(a) => a,
                None => {
                    owned = api::analyse(loaded).map_err(|e| e.to_string())?;
                    &owned
                }
            };
            Some(self.planned(loaded, analysis, saved, false)?.1)
        } else {
            None
        };
        self.file(loaded, planned.as_ref(), comparison)
    }

    /// Use the editor's review state to distinguish automatic defaults from answers.
    /// The state's `auto` list is authoritative; equal values alone never imply a default.
    pub fn scene_with_state(
        &self,
        loaded: &LoadedPart,
        analysis: Option<&Value>,
        state: &crate::tolerancing::State,
    ) -> Result<Value, String> {
        let mut result = self.scene(loaded, analysis, Some(&state.answers))?;
        if let Some(automatic) = &state.auto {
            for meta in result["specify"]
                .as_object_mut()
                .ok_or("invalid scene metadata")?
                .values_mut()
            {
                if meta["status"] == "answered"
                    && meta["question"]
                        .as_str()
                        .is_some_and(|q| automatic.iter().any(|id| id == q))
                {
                    meta["status"] = json!("default");
                }
            }
        }
        Ok(result)
    }

    fn planned(
        &self,
        loaded: &LoadedPart,
        analysis: &Value,
        answers: &Map<String, Value>,
        foreign: bool,
    ) -> Result<PlannedScene, String> {
        let intent = api::preview(analysis, answers).map_err(|e| e.to_string())?;
        let mut bindings = Bindings::new();
        let mut observe = |collection: &str, index: usize, req: &Map<String, Value>| {
            let id = requirement_id(req);
            let question = req.get("question").and_then(Value::as_str);
            let status = if req.get("general") == Some(&Value::Bool(true)) {
                "general"
            } else if question.is_some_and(|q| answers.contains_key(q)) {
                "answered"
            } else {
                "default"
            };
            let mut meta = json!({"status":status});
            if let Some(q) = question {
                meta["question"] = json!(q);
            }
            let count = req
                .get("planner")
                .and_then(|p| p.get("required"))
                .and_then(|r| {
                    r.get("pattern")
                        .and_then(|p| p.get("pitch"))
                        .and_then(|p| p.get("intervals"))
                        .or_else(|| r.get("count"))
                })
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or_else(|| {
                    analysis["features"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|feature| {
                            feature["id"] == req.get("feature").cloned().unwrap_or(Value::Null)
                        })
                        .and_then(|feature| feature["members"].as_array())
                        .map_or(1, |members| {
                            let on = req.get("faces").and_then(Value::as_array);
                            members
                                .iter()
                                .filter(|id| {
                                    analysis["features"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .find(|f| f["id"] == **id)
                                        .and_then(|f| f["faces"].as_array())
                                        .is_some_and(|faces| {
                                            faces
                                                .iter()
                                                .any(|face| on.is_some_and(|on| on.contains(face)))
                                        })
                                })
                                .count()
                                .max(1)
                        })
                });
            bindings.insert((collection.into(), index), (id, meta, count));
        };
        let (baseline_doc, baseline_read) = self
            .foreign
            .as_ref()
            .map_or((&self.doc, &self.read), |(doc, read)| (doc, read));
        let before = &baseline_read.parts[self.index];
        let mut p = writer::scene_model(
            loaded,
            &intent,
            before,
            if foreign {
                pmi::write::Mode::Add
            } else {
                pmi::write::Mode::Replace
            },
            &mut observe,
        )
        .map_err(|e| e.to_string())?;
        characteristics::allocate(
            loaded,
            &mut p,
            &self.read.parts[self.index],
            &self.read.provenance.parts[self.index],
            &self.doc,
            Some(answers),
            if self.foreign.is_some() {
                characteristics::Allocation::Scoped
            } else if foreign {
                characteristics::Allocation::Add
            } else {
                characteristics::Allocation::Replace
            },
        )?;
        let numbers = characteristics::planned_numbers(&p)?;
        let mut semantic = wire::identify(p, None).map_err(|e| e.to_string())?;
        let mut metadata = Map::new();
        let mut counts = BTreeMap::new();
        let mut number_index = 0;
        // Same collection order as characteristics::items.
        for collection in [
            "datums",
            "dimensions",
            "tolerances",
            "surface_textures",
            "threads",
            "knurls",
        ] {
            let len = semantic.ids[collection].len();
            for i in 0..len {
                let number = numbers.get(number_index).copied().flatten();
                number_index += 1;
                if let Some((id, mut meta, count)) = bindings.remove(&(collection.into(), i)) {
                    if let Some(n) = number {
                        meta["number"] = json!(n);
                    }
                    stored_metadata(&semantic.pmi, collection, i, &mut meta);
                    semantic
                        .ids
                        .get_mut(collection)
                        .ok_or("missing id collection")?[i] = id.clone();
                    if metadata.insert(id.clone(), meta).is_some() {
                        return Err(format!("ambiguous Specify requirement id {id}"));
                    }
                    if matches!(collection, "dimensions" | "tolerances") {
                        counts.insert(id, count);
                    }
                } else if let Some(number) = number {
                    // Part-level surface textures are numbered too; their question/id
                    // is assigned below alongside the other part settings.
                    metadata.insert(
                        semantic.ids[collection][i].clone(),
                        json!({"number":number}),
                    );
                }
            }
        }
        if foreign {
            let original = wire::identify(
                before.clone(),
                Some(&baseline_read.identifiers.parts[self.index]),
            )
            .map_err(|e| e.to_string())?;
            let recorded = recorded_numbers(
                &original,
                &baseline_read.provenance.parts[self.index],
                baseline_doc,
            )?;
            for (i, datum) in semantic.pmi.datums.iter().enumerate() {
                if let Some(old) = before
                    .datums
                    .iter()
                    .position(|old| old.label() == datum.label())
                {
                    let id = original.ids["datums"][old].clone();
                    let previous = std::mem::replace(
                        &mut semantic.ids.get_mut("datums").ok_or("missing datum ids")?[i],
                        id.clone(),
                    );
                    metadata.remove(&previous);
                    counts.remove(&previous);
                    let mut meta = json!({"status":"existing"});
                    if let Some(number) = recorded
                        .get(&("datums", old))
                        .filter(|n| id.parse::<u64>().ok() == Some(**n))
                    {
                        meta["number"] = json!(number);
                    }
                    metadata.insert(id, meta);
                }
            }
        }
        part_metadata(&mut semantic, &mut metadata, answers)?;
        attribute_ids(&mut semantic, &mut metadata)?;
        fill_metadata(&semantic, &mut metadata, "default");
        Ok((semantic, metadata, counts))
    }

    fn file(
        &self,
        loaded: &LoadedPart,
        planned: Option<&Map<String, Value>>,
        comparison: bool,
    ) -> Result<Value, String> {
        self.file_from(loaded, planned, comparison, None)
    }

    fn file_from(
        &self,
        loaded: &LoadedPart,
        planned: Option<&Map<String, Value>>,
        comparison: bool,
        source: Option<(&Document, &pmi::PmiRead)>,
    ) -> Result<Value, String> {
        let (doc, read) = source.unwrap_or((&self.doc, &self.read));
        let stored = if source.is_some() {
            None
        } else {
            self.stored.as_ref()
        };
        let p = &read.parts[self.index];
        // identify (without named anchor bindings) deliberately exports mesh indices.
        let mut semantic = wire::identify(p.clone(), Some(&read.identifiers.parts[self.index]))
            .map_err(|e| e.to_string())?;
        let recorded = recorded_numbers(&semantic, &read.provenance.parts[self.index], doc)?;
        let mut metadata = Map::new();
        for (collection, ids) in &mut semantic.ids {
            for (i, id) in ids.iter_mut().enumerate() {
                if !is_requirement(p, collection, i) {
                    continue;
                }
                let mut meta = json!({"status":"existing"});
                if let Some(&n) = recorded
                    .get(&(collection.as_str(), i))
                    .filter(|n| id.parse::<u64>().ok() == Some(**n))
                {
                    meta["number"] = json!(n);
                    if let Some((stable, saved)) =
                        planned.and_then(|m| m.iter().find(|(_, v)| v["number"] == n))
                    {
                        *id = stable.clone();
                        meta = saved.clone();
                        if let Some(object) = meta.as_object_mut() {
                            object.remove("origin");
                            object.remove("source");
                        }
                    }
                }
                stored_metadata(p, collection, i, &mut meta);
                metadata.insert(id.clone(), meta);
            }
        }
        if let Some(answers) = stored {
            part_metadata(&mut semantic, &mut metadata, answers)?;
            attribute_ids(&mut semantic, &mut metadata)?;
            // Drop obsolete content ids after renaming the part settings.
            let ids: std::collections::BTreeSet<_> = semantic.ids.values().flatten().collect();
            metadata.retain(|id, _| ids.contains(id));
        }
        let mut graphics = if comparison {
            presentation::generated(&semantic, &loaded.part)
        } else {
            presentation::read(doc, read, PartId(self.index), &semantic)
        }
        .map_err(|e| e.to_string())?;
        if !comparison {
            notes::file(doc, read, self.index, &semantic, loaded, &mut graphics)?;
        }
        let mut result = encode(&semantic, graphics, metadata)?;
        // Append keeps the original callout records. Reopening must still draw
        // just one block, including the requirements from both generations.
        if notes::multiple_blocks(&result) {
            notes::combined(&mut result, &semantic, None, loaded)?;
        }
        notes::material(&mut result, &semantic, stored);
        Ok(result)
    }
}

fn part_metadata(
    semantic: &mut Semantic,
    metadata: &mut Map<String, Value>,
    answers: &Map<String, Value>,
) -> Result<(), String> {
    let questions = crate::rules::part_questions();
    for collection in ["general", "surface_textures", "attributes"] {
        for i in 0..semantic.ids[collection].len() {
            let key = match collection {
                "general" => Some("general_tolerance"),
                "surface_textures" if semantic.pmi.surface_textures[i].on.is_none() => {
                    crate::existing::PART_NOTES
                        .iter()
                        .find(|(_, kind)| *kind == "surface texture")
                        .map(|(key, _)| *key)
                }
                "attributes"
                    if semantic.pmi.attributes[i].name == writer::SEMANTIC_TEXT
                        && semantic.pmi.attributes[i].on.is_none() =>
                {
                    semantic.pmi.attributes[i]
                        .items
                        .first()
                        .and_then(|(kind, _)| {
                            crate::existing::PART_NOTES
                                .iter()
                                .find(|(_, stored)| *stored == kind)
                                .map(|(key, _)| *key)
                        })
                }
                _ => None,
            };
            if let Some(key) = key {
                let question = questions
                    .iter()
                    .find(|q| q.id.strip_prefix("part.") == Some(key))
                    .ok_or_else(|| format!("no part question for {key}"))?
                    .id
                    .clone();
                let id = if key == "notes" {
                    let index = semantic.pmi.attributes[i]
                        .items
                        .iter()
                        .find_map(|(k, v)| {
                            if k == writer::NOTE_INDEX {
                                if let pmi::AttributeValue::Integer(n) = v {
                                    Some(n.to_string())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| i.to_string());
                    format!("{question}:{index}")
                } else {
                    question.clone()
                };
                let old = std::mem::replace(
                    &mut semantic
                        .ids
                        .get_mut(collection)
                        .ok_or("missing id collection")?[i],
                    id.clone(),
                );
                let mut meta = metadata.remove(&old).unwrap_or_else(|| json!({}));
                meta["status"] = json!(if answers.contains_key(&question) {
                    "answered"
                } else {
                    "default"
                });
                meta["question"] = json!(question);
                metadata.insert(id, meta);
            }
        }
    }
    Ok(())
}

// Metadata/history are semantic JSON items too. Their identities follow their
// requirement owner and property name, rather than changing with the property's value.
fn attribute_ids(semantic: &mut Semantic, metadata: &mut Map<String, Value>) -> Result<(), String> {
    let mut occurrences = BTreeMap::new();
    for (index, attribute) in semantic.pmi.attributes.iter().enumerate() {
        if attribute.name == writer::SEMANTIC_TEXT && attribute.on.is_none() {
            continue;
        }
        let owner = match attribute.on {
            Some(NoteOwner::Feature(id)) => {
                let thread = semantic
                    .pmi
                    .threads
                    .iter()
                    .position(|t| t.feature == id)
                    .map(|i| ("threads", i));
                let knurl = semantic
                    .pmi
                    .knurls
                    .iter()
                    .position(|k| k.feature == id)
                    .map(|i| ("knurls", i));
                if attribute.name.starts_with("knurl") {
                    knurl.or(thread)
                } else {
                    thread.or(knurl)
                }
                .or(Some(("features", id.0)))
            }
            Some(NoteOwner::Dimension(id)) => Some(("dimensions", id.0)),
            Some(NoteOwner::Tolerance(id)) => Some(("tolerances", id.0)),
            Some(NoteOwner::DatumTarget(id)) => Some(("datum_targets", id.0)),
            None => None,
        }
        .map(|(collection, index)| {
            semantic
                .ids
                .get(collection)
                .and_then(|ids| ids.get(index))
                .cloned()
                .ok_or("unknown attribute owner")
        })
        .transpose()?
        .unwrap_or_else(|| "part".into());
        let base = format!("attribute:{owner}:{}", attribute.name);
        let occurrence = occurrences.entry(base.clone()).or_insert(0usize);
        let id = format!("{base}:{}", *occurrence);
        *occurrence += 1;
        let parent = metadata.get(&owner).map(|parent| {
            let mut meta = json!({"status":parent["status"]});
            if let Some(question) = parent.get("question") {
                meta["question"] = question.clone();
            }
            meta
        });
        let old = std::mem::replace(
            &mut semantic
                .ids
                .get_mut("attributes")
                .ok_or("missing attribute ids")?[index],
            id.clone(),
        );
        let old_meta = metadata.remove(&old);
        if let Some(meta) = parent.or(old_meta) {
            metadata.insert(id, meta);
        }
    }
    Ok(())
}

fn has_pmi(p: &PartPmi) -> bool {
    let mut p = p.clone();
    p.attributes.retain(|a| !resume::is_answers(a));
    p != PartPmi::default()
}

fn requirement_id(req: &Map<String, Value>) -> String {
    if let Some(id) = req
        .get("planner")
        .and_then(|p| p.get("required"))
        .and_then(|r| r.get("id"))
        .and_then(Value::as_str)
    {
        return format!("planner:{id}");
    }
    if let Some(letter) = req.get("letter").and_then(Value::as_str) {
        return format!("datum:{letter}");
    }
    let kind = req.get("kind").and_then(Value::as_str).unwrap_or("item");
    let question = req.get("question").and_then(Value::as_str).unwrap_or("");
    let target = if question.starts_with("extra.") {
        question.to_string()
    } else if let Some(feature) = req
        .get("feature")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        feature.to_owned()
    } else {
        format!("faces:{}", req.get("faces").unwrap_or(&Value::Null))
    };
    let mut id = format!("{kind}:{target}");
    if kind == "location" {
        id.push_str(&format!(
            ":{}",
            req.get("reference").unwrap_or(&Value::Null)
        ));
        // A member hole and its pattern can both state this measurement. The
        // question identifies which requirement it is, independently of value.
        if !question.is_empty() {
            id.push(':');
            id.push_str(question);
        }
    }
    id
}

fn stored_metadata(p: &PartPmi, collection: &str, i: usize, meta: &mut Value) {
    if collection != "dimensions" {
        return;
    }
    let stored = pmi_origin::read(p, NoteOwner::Dimension(pmi::DimensionId(i)));
    if let Some(origin) = stored.origin {
        meta["origin"] = json!(origin);
        match origin {
            pmi_origin::Origin::GeneralTolerance => meta["status"] = json!("general"),
            pmi_origin::Origin::Imported => meta["status"] = json!("existing"),
            pmi_origin::Origin::Requirement if meta["status"] == "existing" => {
                meta["status"] = json!("answered")
            }
            _ => {}
        }
    }
    if let Some(source) = stored.source {
        meta["source"] = json!(source);
    }
}

fn fill_metadata(semantic: &Semantic, metadata: &mut Map<String, Value>, status: &str) {
    for (collection, ids) in &semantic.ids {
        for (i, id) in ids.iter().enumerate() {
            if is_requirement(&semantic.pmi, collection, i) {
                metadata
                    .entry(id.clone())
                    .or_insert_with(|| json!({"status":status}));
            }
        }
    }
}

// The documents retain all support rows, but the interview overlay describes PMI only.
fn is_requirement(pmi: &PartPmi, collection: &str, index: usize) -> bool {
    match collection {
        "attributes" => {
            let a = &pmi.attributes[index];
            !resume::is_answers(a) && a.name != characteristics::PROPERTY
        }
        "datums" | "datum_targets" | "dimensions" | "tolerances" | "surface_textures"
        | "threads" | "knurls" | "notes" | "general" => true,
        _ => false,
    }
}

// This walk returns numbers only from validated Specify history, never arbitrary
// numeric ID_ATTRIBUTE values. The caller also checks the actual id on each item.
fn recorded_numbers(
    semantic: &Semantic,
    prov: &pmi::PartProvenance,
    doc: &haecceity::p21::Document,
) -> Result<BTreeMap<(&'static str, usize), u64>, String> {
    let mut numbers = characteristics::recorded_numbers(&semantic.pmi, prov, doc)?.into_iter();
    let mut result = BTreeMap::new();
    for collection in NUMBERED {
        for i in 0..semantic.ids[collection].len() {
            if let Some(number) = numbers.next().flatten() {
                result.insert((collection, i), number);
            }
        }
    }
    Ok(result)
}

// A graphic can render several semantic requirements once (GENERAL NOTES).
// Labels and comparisons belong to each requirement, never to that shared block.
fn manufacturing_notation(
    semantic: &Semantic,
    metadata: &mut Map<String, Value>,
    graphics: &presentation::Presentation,
) {
    for (id, attribute) in semantic.ids["attributes"]
        .iter()
        .zip(&semantic.pmi.attributes)
    {
        if [
            characteristics::PROPERTY,
            pmi_origin::PROPERTY,
            "specify dimension plan",
        ]
        .contains(&attribute.name.as_str())
        {
            continue;
        }
        let Some(meta) = metadata.get_mut(id) else {
            continue;
        };
        let thread = (attribute.name == "thread manufacturing")
            .then(|| semantic.pmi.threads.iter().find(|thread| {
                matches!(attribute.on, Some(NoteOwner::Feature(feature)) if semantic.pmi.same_target(
                    &pmi::ToleranceTarget::Feature(feature),
                    &pmi::ToleranceTarget::Feature(thread.feature)))
            }))
            .flatten();
        if thread.is_none()
            && graphics
                .items
                .get(id)
                .is_some_and(|items| items.iter().any(|g| !g.name.is_empty()))
        {
            continue;
        }
        let notation = if let Some(thread) = thread {
            haecceity::presentation::thread(thread, &semantic.pmi).notation()
        } else {
            // Feature-owned attributes (knurl manufacturing, inspection methods,
            // and other CAD notes) have no GENERAL NOTES row or callout name.
            attribute
                .items
                .iter()
                .map(|(key, value)| {
                    use pmi::AttributeValue as A;
                    let value = match value {
                        A::Text(s) => s.clone(),
                        A::Integer(n) => n.to_string(),
                        A::Real(n) => n.to_string(),
                        A::Boolean(b) => b.to_string(),
                        A::Measure(v) => format!(
                            "{} {}",
                            crate::existing::value(v),
                            if matches!(v.quantity, pmi::Quantity::Length(_)) {
                                "mm"
                            } else {
                                "°"
                            }
                        ),
                        A::OtherMeasure { value, unit, .. } => format!("{value} {unit}"),
                    };
                    format!("{key}: {value}")
                })
                .collect::<Vec<_>>()
                .join("; ")
        };
        meta["notation"] = json!(notation);
    }
    // Manufacturing/finish requirements can exist without any presentation.
    // Use their own semantics only where there is no drawing wording to retain.
    for (id, knurl) in semantic.ids["knurls"].iter().zip(&semantic.pmi.knurls) {
        if graphics
            .items
            .get(id)
            .is_some_and(|items| items.iter().any(|g| !g.name.is_empty()))
        {
            continue;
        }
        let pattern = match knurl.pattern {
            pmi::KnurlPattern::Straight => "straight",
            pmi::KnurlPattern::Diamond => "diamond",
            pmi::KnurlPattern::Diagonal => "diagonal",
        };
        if let Some(meta) = metadata.get_mut(id) {
            meta["notation"] = json!(format!(
                "{pattern} knurl, pitch {} mm, major ⌀{} mm",
                knurl.diametral_pitch.mm(),
                knurl.major_diameter.mm()
            ));
        }
    }
    for (id, texture) in semantic.ids["surface_textures"]
        .iter()
        .zip(&semantic.pmi.surface_textures)
    {
        if !graphics
            .items
            .get(id)
            .is_some_and(|items| items.iter().any(|g| !g.name.is_empty()))
            && let Some(meta) = metadata.get_mut(id)
        {
            meta["notation"] =
                json!(haecceity::presentation::notes::texture_parameters(texture).join(", "));
        }
    }
}

fn note_notation(semantic: &Semantic, metadata: &mut Map<String, Value>) {
    use haecceity::presentation::notes::{Source, general_notes};

    for (id, note) in semantic.ids["notes"].iter().zip(&semantic.pmi.notes) {
        if let Some(meta) = metadata.get_mut(id) {
            meta["notation"] = json!(note.text);
            if note.on.is_none() && crate::existing::edge_break_note(&note.text) {
                meta["question"] = json!("part.edges");
            }
        }
    }
    let mut texts: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for row in general_notes(&semantic.pmi) {
        let (collection, index) = match row.source {
            Source::General(i) => ("general", i),
            Source::Texture(i) => ("surface_textures", i),
            Source::Attribute(i) => ("attributes", i),
            Source::Note(i) => ("notes", i),
            Source::Material => continue,
        };
        if let Some(id) = semantic.ids.get(collection).and_then(|ids| ids.get(index)) {
            texts.entry(id).or_default().push(row.text);
        }
    }
    for (id, text) in texts {
        if let Some(meta) = metadata.get_mut(id) {
            meta["notation"] = json!(text.join("; "));
        }
    }
}

fn encode(
    semantic: &Semantic,
    graphics: presentation::Presentation,
    mut metadata: Map<String, Value>,
) -> Result<Value, String> {
    manufacturing_notation(semantic, &mut metadata, &graphics);
    note_notation(semantic, &mut metadata);
    // Serialize the upstream document directly, including optional stroked flags.
    let mut result = json!({"pmi": wire::to_value(semantic).map_err(|e|e.to_string())?, "presentation": graphics, "specify":metadata});
    prune_views(&mut result["presentation"]);
    Ok(result)
}

fn prune_views(presentation: &mut Value) {
    // Filtering or replacing semantic graphics can leave generated plane views
    // behind. An alias or unlinked graphic still keeps its membership alive.
    let ids: std::collections::BTreeSet<String> = presentation["items"]
        .as_object()
        .into_iter()
        .flat_map(|items| items.values())
        .filter_map(Value::as_array)
        .flatten()
        .chain(presentation["unlinked"].as_array().into_iter().flatten())
        .filter_map(|graphic| graphic["id"].as_str().map(str::to_owned))
        .collect();
    if let Some(views) = presentation.get_mut("views").and_then(Value::as_array_mut) {
        for view in views.iter_mut() {
            if let Some(graphics) = view["graphics"].as_array_mut() {
                graphics.retain(|id| id.as_str().is_some_and(|id| ids.contains(id)));
            }
        }
        views.retain(|view| {
            view["graphics"]
                .as_array()
                .is_some_and(|ids| !ids.is_empty())
        });
    }
}

fn merge(target: &mut Value, other: Value) -> Result<(), String> {
    for (collection, rows) in other["pmi"]["pmi"]
        .as_object()
        .ok_or("invalid PMI document")?
    {
        if let Some(rows) = rows.as_array() {
            let output = target["pmi"]["pmi"][collection]
                .as_array_mut()
                .ok_or("missing PMI collection")?;
            for row in rows {
                if let Some(existing) = output.iter_mut().find(|r| r["id"] == row["id"]) {
                    // The source owns reused datums, including their targets/supports.
                    *existing = row.clone();
                } else {
                    output.push(row.clone());
                }
            }
        } else if target["pmi"]["pmi"][collection].is_null() {
            target["pmi"]["pmi"][collection] = rows.clone();
        }
    }
    for table in ["length", "angle", "density", "other"] {
        let output = target["pmi"]["units"][table]
            .as_array_mut()
            .ok_or("missing unit table")?;
        for unit in other["pmi"]["units"][table]
            .as_array()
            .ok_or("invalid unit table")?
        {
            if !output.iter().any(|u| u["id"] == unit["id"]) {
                output.push(unit.clone());
            }
        }
    }
    for key in ["specify", "presentation"] {
        if key == "specify" {
            target[key]
                .as_object_mut()
                .ok_or("invalid metadata")?
                .extend(other[key].as_object().ok_or("invalid metadata")?.clone());
        } else {
            target[key]["items"]
                .as_object_mut()
                .ok_or("invalid graphics")?
                .extend(
                    other[key]["items"]
                        .as_object()
                        .ok_or("invalid graphics")?
                        .clone(),
                );
            for field in ["semantic_ids", "unlinked"] {
                target[key][field]
                    .as_array_mut()
                    .ok_or("invalid graphics")?
                    .extend(
                        other[key][field]
                            .as_array()
                            .ok_or("invalid graphics")?
                            .clone(),
                    );
            }
            let ids = target[key]["semantic_ids"]
                .as_array_mut()
                .ok_or("invalid ids")?;
            ids.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            ids.dedup();
        }
    }
    // Both inputs are trusted haecceity exports. Keep import/schema validation out
    // of production builds; package tests validate the complete merged documents.
    for field in ["planes", "views"] {
        if let Some(rows) = other["presentation"][field].as_array() {
            let presentation = target["presentation"]
                .as_object_mut()
                .ok_or("invalid graphics")?;
            let output = presentation
                .entry(field)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("invalid presentation collection")?;
            for row in rows {
                if let Some(old) = output.iter().find(|old| old["id"] == row["id"]) {
                    if old != row {
                        return Err(format!("conflicting presentation {field} id {}", row["id"]));
                    }
                } else {
                    output.push(row.clone());
                }
            }
        }
    }
    prune_views(&mut target["presentation"]);
    Ok(())
}

/// Raw STEP view-only form. Select an assembly part by its `parts()` index.
pub fn pmi_scene(bytes: &[u8], part: Option<usize>) -> Result<Value, String> {
    let file = crate::part::File::of_bytes(std::path::Path::new("scene.step"), bytes.to_vec())
        .map_err(|e| e.to_string())?;
    let loaded = file.load(part).map_err(|e| e.to_string())?;
    Scene::new(&loaded)?.scene(&loaded, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use presentation::{Graphic, Source, TextPlacement, View};

    fn graphic(id: &str, stroked: Option<bool>) -> Graphic {
        Graphic {
            id: Some(id.into()),
            plane_id: None,
            source: Source::File,
            name: "Text".into(),
            text: vec!["Text".into()],
            polyline_roles: None,
            polylines: vec![],
            triangles: vec![],
            step_ids: vec![],
            unsupported: vec![],
            text_placements: vec![TextPlacement {
                text_index: Some(0),
                cell_box: None,
                stroked,
                origin: [0.; 3],
                x_axis: [1., 0., 0.],
                normal: [0., 0., 1.],
                height: Some(2.),
                line_spacing: None,
                alignment: None,
            }],
        }
    }

    #[test]
    fn geometry_roles_and_cell_boxes_survive_scene_encoding() {
        let mut g = graphic("geometry", Some(true));
        g.polylines = vec![vec![[0., 0., 0.], [1., 0., 0.]]];
        g.polyline_roles = Some(vec![presentation::PolylineRole::Text]);
        g.text_placements[0].cell_box = Some([[-2., -1.], [2., 1.]]);
        let scene = encoded(presentation::Presentation {
            unlinked: vec![g],
            ..Default::default()
        });
        let g = &scene["presentation"]["unlinked"][0];
        assert_eq!(g["polyline_roles"], json!(["text"]));
        assert_eq!(
            g["text_placements"][0]["cell_box"],
            json!([[-2., -1.], [2., 1.]])
        );
    }

    fn encoded(graphics: presentation::Presentation) -> Value {
        let semantic = wire::identify(PartPmi::default(), None).unwrap();
        encode(&semantic, graphics, Map::new()).unwrap()
    }

    #[test]
    fn text_placement_stroked_survives_true_absent_and_false() {
        for stroked in [Some(true), None, Some(false)] {
            let scene = encoded(presentation::Presentation {
                unlinked: vec![graphic("text", stroked)],
                ..Default::default()
            });
            let placement = &scene["presentation"]["unlinked"][0]["text_placements"][0];
            assert_eq!(placement.get("stroked"), stroked.map(Value::Bool).as_ref());
            assert_eq!(placement["text_index"], 0);
        }
    }

    #[test]
    fn merging_replaced_graphics_prunes_obsolete_views() {
        let document = |id: &str| {
            encoded(presentation::Presentation {
                items: BTreeMap::from([("requirement".into(), vec![graphic(id, None)])]),
                views: vec![View {
                    id: format!("{id}/view"),
                    name: id.into(),
                    direction: None,
                    up: None,
                    graphics: vec![id.into()],
                }],
                ..Default::default()
            })
        };
        let mut scene = document("generated");
        let file = document("file");
        merge(&mut scene, file.clone()).unwrap();
        assert_eq!(
            scene["presentation"]["views"],
            file["presentation"]["views"]
        );
        assert_eq!(
            scene["presentation"]["items"],
            file["presentation"]["items"]
        );
    }

    #[test]
    fn filtered_graphics_are_pruned_from_views_but_aliases_and_unlinked_survive() {
        let mut graphics = presentation::Presentation {
            items: BTreeMap::from([
                ("removed".into(), vec![graphic("filtered", None)]),
                ("original".into(), vec![graphic("aliased", None)]),
                ("alias".into(), vec![graphic("aliased", None)]),
            ]),
            unlinked: vec![graphic("unlinked", None)],
            views: vec![
                View {
                    id: "mixed".into(),
                    name: "Mixed view".into(),
                    direction: Some([0., 0., -1.]),
                    up: Some([0., 1., 0.]),
                    graphics: vec!["filtered".into(), "aliased".into(), "unlinked".into()],
                },
                View {
                    id: "empty".into(),
                    name: "Filtered view".into(),
                    direction: None,
                    up: None,
                    graphics: vec!["filtered".into()],
                },
            ],
            ..Default::default()
        };
        graphics.items.remove("removed");
        graphics.items.remove("original");
        let scene = encoded(graphics);
        assert_eq!(scene["presentation"]["views"].as_array().unwrap().len(), 1);
        let view = &scene["presentation"]["views"][0];
        assert_eq!(view["id"], "mixed");
        assert_eq!(view["graphics"], json!(["aliased", "unlinked"]));
        assert_eq!(view["direction"], json!([0., 0., -1.]));
        assert_eq!(view["up"], json!([0., 1., 0.]));
    }
}
