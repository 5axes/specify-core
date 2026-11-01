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
// companion uses its interview key as an alias for that scalar (never a new PMI row).
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
        let ids = scene["presentation"]["semantic_ids"]
            .as_array_mut()
            .unwrap();
        if !ids.iter().any(|id| id == MATERIAL) {
            ids.push(json!(MATERIAL));
            ids.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        }
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

/// Extract only part settings, preserving each semantic identity. Normalization is
/// for display ordering only; the exported semantic text/kind stays as the file wrote it.
fn settings(sources: &[&Semantic]) -> Result<Semantic, String> {
    let mut display = wire::identify(PartPmi::default(), None).map_err(|e| e.to_string())?;
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
    Ok(display)
}

pub(super) fn combined(
    scene: &mut Value,
    current: &Semantic,
    original: Option<&Semantic>,
    loaded: &LoadedPart,
) -> Result<(), String> {
    let sources: Vec<_> = std::iter::once(current).chain(original).collect();
    let display = settings(&sources)?;
    let generated = presentation::generated(&display, &loaded.part).map_err(|e| e.to_string())?;
    let mut notes = serde_json::to_value(generated).map_err(|e| e.to_string())?;
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
    let mut replaced = BTreeSet::new();
    for (id, graphics) in scene["presentation"]["items"]
        .as_object()
        .ok_or("invalid graphics")?
    {
        for graphic in graphics.as_array().ok_or("invalid graphic list")? {
            if block(graphic) || part_ids.contains(id.as_str()) || id == MATERIAL {
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
                row["id"].as_str() != Some("generated:general-notes/plane")
                    && row["id"].as_str() != Some("generated:general-notes/plane/view")
            });
        scene["presentation"][field]
            .as_array_mut()
            .unwrap()
            .extend(notes[field].as_array().into_iter().flatten().cloned());
    }
    // Use the same per-requirement labels for imported and new rows.
    note_notation(&display, scene["specify"].as_object_mut().unwrap());
    stack_feature_notes(&mut scene["presentation"]);
    prune_views(&mut scene["presentation"]);
    Ok(())
}

// Imported feature notes without file ink were laid out against the file's shorter
// block. Reserve the combined block's ink before their generated notes column.
fn stack_feature_notes(presentation: &mut Value) {
    let graphics: Vec<_> = presentation["items"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .collect();
    let feature = |g: &Value| {
        g["source"] == "generated"
            && g["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("generated:notes:"))
    };
    let z = |g: &Value| {
        g["polylines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(|p| p[2].as_f64())
            .collect::<Vec<_>>()
    };
    let bottom = graphics
        .iter()
        .filter(|g| block(g))
        .flat_map(|g| z(g))
        .reduce(f64::min);
    let top = graphics
        .iter()
        .filter(|g| feature(g))
        .flat_map(|g| z(g))
        .reduce(f64::max);
    let (Some(bottom), Some(top)) = (bottom, top) else {
        return;
    };
    let height = graphics
        .iter()
        .filter(|g| feature(g))
        .flat_map(|g| g["text_placements"].as_array().into_iter().flatten())
        .filter_map(|p| p["height"].as_f64())
        .fold(0., f64::max);
    let dz = (bottom - 2. * height - top).min(0.);
    if dz == 0. {
        return;
    }
    let planes: BTreeSet<_> = graphics
        .iter()
        .filter(|g| feature(g))
        .filter_map(|g| g["plane_id"].as_str().map(str::to_owned))
        .collect();
    for graphic in presentation["items"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .filter_map(Value::as_array_mut)
        .flatten()
        .filter(|g| feature(g))
    {
        for field in ["polylines", "triangles"] {
            for point in graphic[field]
                .as_array_mut()
                .into_iter()
                .flatten()
                .filter_map(Value::as_array_mut)
                .flatten()
            {
                point[2] = json!(point[2].as_f64().unwrap() + dz);
            }
        }
        for placement in graphic["text_placements"]
            .as_array_mut()
            .into_iter()
            .flatten()
        {
            placement["origin"][2] = json!(placement["origin"][2].as_f64().unwrap() + dz);
        }
    }
    for plane in presentation["planes"].as_array_mut().into_iter().flatten() {
        if plane["id"].as_str().is_some_and(|id| planes.contains(id)) {
            plane["origin"][2] = json!(plane["origin"][2].as_f64().unwrap() + dz);
        }
    }
}
