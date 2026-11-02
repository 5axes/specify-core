//! Scene-only composition. The writer's lossless append and semantic records are untouched.
use super::*;
use haecceity::p21::{Attribute, RawEntity};
use presentation::{Presentation, Source};
use std::collections::BTreeSet;

const MATERIAL: &str = "part.material";

fn block(graphic: &Value) -> bool {
    graphic["name"]
        .as_str()
        .is_some_and(|name| name.eq_ignore_ascii_case("general notes"))
}

pub(super) fn multiple_blocks(scene: &Value) -> bool {
    scene["presentation"]["items"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .chain(scene["presentation"]["unlinked"].as_array().unwrap())
        .filter(|g| block(g))
        .filter_map(|g| g["id"].as_str())
        .collect::<BTreeSet<_>>()
        .len()
        > 1
}

// Material is a scalar in haecceity.pmi/1, without a PartIdentifiers slot. Specify's
// companion uses its interview key as an alias for that scalar (never a new PMI row),
// so `semantic_ids` still lists only the PMI document's own ids.
pub(super) fn material(
    scene: &mut Value,
    semantic: &Semantic,
    answers: Option<&Map<String, Value>>,
) {
    if let Some(material) = &semantic.pmi.material {
        scene["specify"][MATERIAL] = json!({
            "status": if answers.is_some_and(|a| a.contains_key(MATERIAL)) { "answered" } else { "existing" },
            "question": MATERIAL,
            "notation": format!("MATERIAL: {}", material.id)
        });
    }
}

/// File provenance describes where a graphic is read from, not which program wrote it.
/// Resolve material DMIAs through the reader's part-owned provenance, not by its label.
pub(super) fn file(
    doc: &Document,
    read: &pmi::PmiRead,
    part: usize,
    semantic: &Semantic,
    loaded: &LoadedPart,
    graphics: &mut Presentation,
) -> Result<(), String> {
    let material = &read.provenance.parts[part].material;
    let linked: BTreeSet<_> = doc
        .ids()
        .filter_map(|id| {
            let RawEntity::Simple {
                name, attributes, ..
            } = doc.get(id)?
            else {
                return None;
            };
            if name == "DRAUGHTING_MODEL_ITEM_ASSOCIATION"
                && let Some(Attribute::EntityRef(definition)) = attributes.get(2)
                && material.contains(definition)
                && let Some(Attribute::EntityRef(item)) = attributes.get(4)
            {
                Some(*item)
            } else {
                None
            }
        })
        .collect();
    let mut material_graphics = BTreeMap::new();
    for graphic in graphics
        .items
        .values_mut()
        .flatten()
        .chain(&mut graphics.unlinked)
    {
        graphic.source = Source::File;
        if graphic.step_ids.iter().any(|id| linked.contains(id)) {
            material_graphics.insert(graphic.id.clone(), graphic.clone());
        }
    }
    if !material_graphics.is_empty() {
        graphics
            .unlinked
            .retain(|g| !material_graphics.contains_key(&g.id));
        graphics
            .items
            .insert(MATERIAL.into(), material_graphics.into_values().collect());
    }
    // Foreign semantic feature notes often have no file callout. Keep their owner
    // and provide the same feature-note fallback as upstream's generated scene.
    let missing: BTreeSet<_> = semantic
        .pmi
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.on.is_some())
        .map(|(i, _)| &semantic.ids["notes"][i])
        .filter(|id| !graphics.items.contains_key(*id))
        .collect();
    if !missing.is_empty() {
        let generated =
            presentation::generated(semantic, &loaded.part).map_err(|e| e.to_string())?;
        for id in missing {
            if let Some(rows) = generated.items.get(id) {
                graphics.items.insert(id.clone(), rows.clone());
                for row in rows {
                    graphics.planes.extend(
                        generated
                            .planes
                            .iter()
                            .filter(|p| Some(&p.id) == row.plane_id.as_ref())
                            .cloned(),
                    );
                    graphics.views.extend(
                        generated
                            .views
                            .iter()
                            .filter(|v| row.id.as_ref().is_some_and(|id| v.graphics.contains(id)))
                            .cloned(),
                    );
                }
            }
        }
    }
    Ok(())
}

/// Retain the file's support graph so feature-note owners keep their identities.
/// Specify authors part settings only; the last source owns the file's feature notes.
/// Normalization affects display ordering, never the exported semantic text/kind.
fn settings(sources: &[&Semantic], feature_notes: &BTreeSet<String>) -> Result<Semantic, String> {
    let owner = sources.last().unwrap();
    // Re-identify the typed projection rather than cloning transport observations:
    // newer kernels retain source identifiers whose row counts would become stale.
    let mut display = wire::identify(owner.pmi.clone(), None).map_err(|e| e.to_string())?;
    display.ids = owner.ids.clone();
    display.pmi.material = None;
    display.pmi.general.clear();
    display.pmi.surface_textures.clear();
    display.pmi.attributes.clear();
    display.pmi.notes.clear();
    for collection in ["general", "surface_textures", "attributes", "notes"] {
        display.ids.get_mut(collection).unwrap().clear();
    }
    for source in sources {
        if display.pmi.material.is_none() {
            display.pmi.material = source.pmi.material.clone();
        }
        for (i, row) in source.pmi.general.iter().enumerate() {
            display.pmi.general.push(row.clone());
            display
                .ids
                .get_mut("general")
                .unwrap()
                .push(source.ids["general"][i].clone());
        }
        for (i, row) in source
            .pmi
            .surface_textures
            .iter()
            .enumerate()
            .filter(|(_, r)| r.on.is_none())
        {
            display.pmi.surface_textures.push(row.clone());
            display
                .ids
                .get_mut("surface_textures")
                .unwrap()
                .push(source.ids["surface_textures"][i].clone());
        }
        for (i, row) in source
            .pmi
            .attributes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.on.is_none() && r.name == writer::SEMANTIC_TEXT)
        {
            display.pmi.attributes.push(row.clone());
            display
                .ids
                .get_mut("attributes")
                .unwrap()
                .push(source.ids["attributes"][i].clone());
        }
        for (i, row) in source
            .pmi
            .notes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.on.is_none())
        {
            let mut row = row.clone();
            if crate::existing::edge_break_note(&row.text) {
                row.kind = "edge condition".into();
            } else if row.kind == writer::SEMANTIC_TEXT {
                row.kind = "general note".into();
            }
            display.pmi.notes.push(row);
            display
                .ids
                .get_mut("notes")
                .unwrap()
                .push(source.ids["notes"][i].clone());
        }
    }
    for (i, row) in owner.pmi.notes.iter().enumerate() {
        let id = &owner.ids["notes"][i];
        if row.on.is_some() && feature_notes.contains(id) {
            display.pmi.notes.push(row.clone());
            display.ids.get_mut("notes").unwrap().push(id.clone());
        }
    }
    Ok(display)
}

pub(super) fn combined(
    scene: &mut Value,
    current: &Semantic,
    original: Option<&Semantic>,
    loaded: &LoadedPart,
) -> Result<(), String> {
    let sources: Vec<_> = std::iter::once(current).chain(original).collect();
    let owner = sources.last().unwrap();
    // Preserve file ink. Missing feature-note ink (including the file-view fallback)
    // must be generated together with the combined block, in haecceity's layout.
    let feature_notes: BTreeSet<String> = owner
        .pmi
        .notes
        .iter()
        .enumerate()
        .filter(|(_, note)| note.on.is_some())
        .map(|(i, _)| owner.ids["notes"][i].clone())
        .filter(|id| {
            !scene["presentation"]["items"][id]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|g| g["source"] == "file"))
        })
        .collect();
    let display = settings(&sources, &feature_notes)?;
    let mut generated =
        presentation::generated(&display, &loaded.part).map_err(|e| e.to_string())?;
    let part_ids: BTreeSet<_> = haecceity::presentation::notes::general_notes(&display.pmi)
        .iter()
        .filter_map(|row| {
            use haecceity::presentation::notes::Source;
            let (collection, index) = match row.source {
                Source::General(i) => ("general", i),
                Source::Texture(i) => ("surface_textures", i),
                Source::Attribute(i) => ("attributes", i),
                Source::Note(i) => ("notes", i),
                Source::Material => return None,
            };
            Some(display.ids[collection][index].as_str())
        })
        .collect();
    // The support graph also carries drawable requirements. Keep their existing
    // graphics; this projection replaces only the part block and missing notes.
    generated
        .items
        .retain(|id, _| part_ids.contains(id.as_str()) || feature_notes.contains(id));
    generated
        .unlinked
        .retain(|g| g.name.eq_ignore_ascii_case("general notes"));
    let planes: BTreeSet<_> = generated
        .items
        .values()
        .flatten()
        .chain(&generated.unlinked)
        .filter_map(|g| g.plane_id.clone())
        .collect();
    generated.planes.retain(|p| planes.contains(&p.id));
    generated.views.retain(|v| {
        generated
            .planes
            .iter()
            .any(|p| v.id == format!("{}/view", p.id))
    });
    let mut notes = serde_json::to_value(generated).map_err(|e| e.to_string())?;
    let mut replaced = BTreeSet::new();
    for (id, graphics) in scene["presentation"]["items"]
        .as_object()
        .ok_or("invalid graphics")?
    {
        for graphic in graphics.as_array().ok_or("invalid graphic list")? {
            if block(graphic)
                || part_ids.contains(id.as_str())
                || feature_notes.contains(id)
                || id == MATERIAL
            {
                replaced.insert(graphic["id"].to_string());
            }
        }
    }
    for graphic in scene["presentation"]["unlinked"]
        .as_array()
        .ok_or("invalid unlinked graphics")?
    {
        if block(graphic) {
            replaced.insert(graphic["id"].to_string());
        }
    }
    for graphics in scene["presentation"]["items"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        graphics
            .as_array_mut()
            .unwrap()
            .retain(|g| !replaced.contains(&g["id"].to_string()));
    }
    scene["presentation"]["items"]
        .as_object_mut()
        .unwrap()
        .retain(|_, gs| !gs.as_array().unwrap().is_empty());
    scene["presentation"]["unlinked"]
        .as_array_mut()
        .unwrap()
        .retain(|g| !replaced.contains(&g["id"].to_string()));
    // Alias material to the very same block rather than leaving material-only notes unlinked.
    if display.pmi.material.is_some() {
        let graphic = notes["items"]
            .as_object()
            .unwrap()
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .chain(notes["unlinked"].as_array().unwrap())
            .find(|g| block(g))
            .cloned();
        if let Some(graphic) = graphic {
            notes["items"][MATERIAL] = json!([graphic]);
            notes["unlinked"] = json!([]);
        }
        material(scene, &display, None);
        if current.pmi.material.is_some() {
            scene["specify"][MATERIAL]["status"] = json!("answered");
        }
    }
    scene["presentation"]["items"]
        .as_object_mut()
        .unwrap()
        .extend(notes["items"].as_object().unwrap().clone());
    scene["presentation"]["unlinked"]
        .as_array_mut()
        .unwrap()
        .extend(notes["unlinked"].as_array().unwrap().clone());
    for field in ["planes", "views"] {
        scene["presentation"]
            .as_object_mut()
            .unwrap()
            .entry(field)
            .or_insert_with(|| json!([]));
        scene["presentation"][field]
            .as_array_mut()
            .unwrap()
            .retain(|row| {
                !notes[field]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|new| new["id"] == row["id"])
            });
        scene["presentation"][field]
            .as_array_mut()
            .unwrap()
            .extend(notes[field].as_array().into_iter().flatten().cloned());
    }
    // Use the same per-requirement labels for imported and new rows.
    note_notation(&display, scene["specify"].as_object_mut().unwrap());
    prune_views(&mut scene["presentation"]);
    Ok(())
}
