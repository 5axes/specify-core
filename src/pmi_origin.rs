//! Stored dimension intent, independent of authorship and recognition IDs (#77).
//! Producer/reader contract: `docs/pmi-origin.md`.
use haecceity::pmi::{
    AttributeSet, AttributeValue, DimensionId, DimensionKind, NoteOwner, PartPmi,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{features, part::LoadedPart, resume};

pub const PROPERTY: &str = "specify pmi origin";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    GeneralTolerance,
    Requirement,
    Imported,
}

/// The same geometric namespace and support indices used by characteristic-number keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub schema: u8,
    pub faces: String,
    pub support: Vec<usize>,
    pub parameter: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub origin: Option<Origin>,
    pub source: Option<Source>,
}

/// Read only stored attributes. Absence stays absent, including on legacy Specify files.
pub fn read(pmi: &PartPmi, owner: NoteOwner) -> Metadata {
    let mut sets = pmi
        .attributes
        .iter()
        .filter(|s| s.name == PROPERTY && s.on == Some(owner));
    let Some(set) = sets.next() else {
        return Metadata::default();
    };
    if sets.next().is_some() {
        return Metadata::default();
    }
    let text = |key: &str| {
        let mut values = set.items.iter().filter(|(name, _)| name == key);
        match (values.next(), values.next()) {
            (Some((_, AttributeValue::Text(value))), None) => Some(value),
            _ => None,
        }
    };
    let origin = text("origin").and_then(|v| match v.as_str() {
        "general_tolerance" => Some(Origin::GeneralTolerance),
        "requirement" => Some(Origin::Requirement),
        "imported" => Some(Origin::Imported),
        _ => None,
    });
    let source = text("source")
        .and_then(|v| serde_json::from_str::<Source>(v).ok())
        .filter(|s| {
            s.schema == 2
                && s.faces.len() == 64
                && s.faces
                    .bytes()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                && !s.support.is_empty()
                && s.support.windows(2).all(|w| w[0] < w[1])
                && !s.parameter.is_empty()
                && s.parameter.split('.').all(|p| {
                    !p.is_empty()
                        && p.bytes()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
                })
        });
    Metadata { origin, source }
}

pub(crate) struct Planner {
    fingerprint: String,
    features: Vec<features::Feature>,
    faces: Vec<features::Face>,
}
impl Planner {
    pub fn new(loaded: &LoadedPart) -> Result<Self, String> {
        Ok(Self {
            fingerprint: resume::fingerprint(loaded).map_err(|e| e.to_string())?,
            features: features::recognise(loaded).map_err(|e| e.to_string())?,
            faces: features::describe_faces(loaded).map_err(|e| e.to_string())?,
        })
    }

    pub fn dimension(
        &self,
        pmi: &mut PartPmi,
        req: &Map<String, Value>,
        general: bool,
        features: &[haecceity::pmi::Feature],
    ) {
        let owner = NoteOwner::Dimension(DimensionId(pmi.dimensions.len() - 1));
        let mut support: Vec<usize> = req
            .get("faces")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()))
            .collect();
        support.sort_unstable();
        support.dedup();
        if req.contains_key("planner") {
            // A shared survivor can target a complete coincident group rather than
            // the representative source row. Store the measured target, as #77
            // requires; inspection allocation and carried sources are unchanged.
            let target = match pmi.dimensions.last().expect("dimension just appended").kind {
                DimensionKind::Size { feature, .. } => feature,
                DimensionKind::Location { to, .. } => to,
            };
            support = crate::existing::faces_in_arena(features, &[target]);
        }
        let size = req.get("kind").and_then(Value::as_str) == Some("size");
        // Aggregate bodies, risers and plane-level features can contain a support without
        // being the feature this measurement states. Only match families whose parameter
        // the writer can identify in Draftwright's measurement vocabulary.
        let eligible = |f: &features::Feature| {
            if size {
                matches!(
                    f.family.as_str(),
                    "holes" | "hole_patterns" | "bosses" | "turned_steps" | "turned_bodies"
                )
            } else {
                matches!(f.family.as_str(), "holes" | "hole_patterns" | "bosses")
            }
        };
        let named = req
            .get("feature")
            .and_then(Value::as_str)
            .and_then(|id| self.features.iter().find(|f| f.id == id && eligible(f)));
        // Face-picked extras can name a recognised feature too. Prefer a unique exact
        // support; otherwise a unique containing feature. Never resolve ambiguity by ID.
        let candidates = |exact: bool| {
            self.features
                .iter()
                .filter(|f| {
                    eligible(f)
                        && !support.is_empty()
                        && if exact {
                            f.faces == support
                        } else {
                            support.iter().all(|i| f.faces.contains(i))
                        }
                })
                .collect::<Vec<_>>()
        };
        let exact = candidates(true);
        let containing = candidates(false);
        let feature = named.or(match exact.as_slice() {
            [f] => Some(*f),
            [] => match containing.as_slice() {
                [f] => Some(*f),
                _ => None,
            },
            _ => None,
        });
        let planned_parameter = feature.filter(|_| !support.is_empty()).and_then(|_| {
            req.get("planner")
                .and_then(|p| {
                    serde_json::from_value::<crate::dimensioning::Planned>(p.clone()).ok()
                })
                .map(|p| match p.required.kind {
                    quiddity::dimensioning::Kind::Size { role, .. } => role,
                    quiddity::dimensioning::Kind::Location { axis, .. } => {
                        format!("location.length.{}", ["x", "y", "z"][axis])
                    }
                })
        });
        let parameter = planned_parameter.or_else(|| {
            feature.and_then(|f| {
                if size {
                    Some(
                        match f.family.as_str() {
                            "holes" | "hole_patterns" => "bore.diameter",
                            "bosses" => "boss.diameter",
                            "turned_steps" => "step.diameter",
                            "turned_bodies" => "turned_body.diameter",
                            // Other feature families do not name a diameter measurement in the
                            // current Specify writer. Do not invent a Draftwright parameter.
                            _ => return None,
                        }
                        .to_string(),
                    )
                } else {
                    let direction = req
                        .get("reference")
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(Value::as_u64)
                        .and_then(|i| self.faces.get(i as usize))
                        .and_then(|f| f.direction)
                        .unwrap_or([1., 0., 0.]);
                    let axis = (0..3)
                        .max_by(|&a, &b| {
                            direction[a]
                                .abs()
                                .total_cmp(&direction[b].abs())
                                .then_with(|| b.cmp(&a))
                        })
                        .unwrap_or(0);
                    Some(format!("location.length.{}", ["x", "y", "z"][axis]))
                }
            })
        });
        let mut items = vec![(
            "origin".into(),
            AttributeValue::Text(
                if general {
                    "general_tolerance"
                } else {
                    "requirement"
                }
                .into(),
            ),
        )];
        if let Some(parameter) = parameter {
            let source = Source {
                schema: 2,
                faces: self.fingerprint.clone(),
                support,
                parameter,
            };
            items.push((
                "source".into(),
                AttributeValue::Text(serde_json::to_string(&source).expect("source serializes")),
            ));
        }
        pmi.attributes.push(AttributeSet {
            name: PROPERTY.into(),
            on: Some(owner),
            items,
            description: None,
            general_property: None,
        });
    }
}

/// Preserve an established source through the number allocator's unique support-growth
/// match. Recognition IDs and newly added defining faces cannot rename that requirement.
pub(crate) fn carry_source(pmi: &mut PartPmi, index: usize, old: &Source, fingerprint: &str) {
    let owner = NoteOwner::Dimension(DimensionId(index));
    if old.faces == fingerprint
        && let Some(set) = pmi
            .attributes
            .iter_mut()
            .find(|s| s.name == PROPERTY && s.on == Some(owner))
    {
        let value = AttributeValue::Text(serde_json::to_string(old).expect("source serializes"));
        if let Some((_, current)) = set.items.iter_mut().find(|(key, _)| key == "source") {
            *current = value;
        } else {
            set.items.push(("source".into(), value));
        }
    }
}
