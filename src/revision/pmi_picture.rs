//! Keep haecceity's presentation geometry; only choose revision colours here.
use super::{PmiItemChange, PmiScenes};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub(super) struct Graphic {
    pub polylines: Vec<Vec<[f64; 3]>>,
    pub triangles: Vec<[[f64; 3]; 3]>,
    pub colour: [f64; 3],
    pub alpha: f64,
    pub old: bool,
}

impl Graphic {
    pub fn points(&self) -> impl Iterator<Item = &[f64; 3]> {
        self.polylines
            .iter()
            .flatten()
            .chain(self.triangles.iter().flatten())
    }

    pub fn placed(&self, place: &dyn Fn([f64; 3]) -> [f64; 3]) -> Self {
        Self {
            polylines: self
                .polylines
                .iter()
                .map(|line| line.iter().map(|&p| place(p)).collect())
                .collect(),
            triangles: self.triangles.iter().map(|t| t.map(place)).collect(),
            ..self.clone()
        }
    }
}

pub(super) fn graphics(items: &[PmiItemChange], scenes: &PmiScenes) -> Vec<Graphic> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    // Ghosts first, current requirements last. Shared note blocks are drawn once per side.
    for class in ["removed", "added", "changed", "renumbered"] {
        for item in items.iter().filter(|item| item.class == class) {
            let old = class == "removed";
            let scene = if old { &scenes.old } else { &scenes.new };
            for graphic in scene["presentation"]["items"][&item.id]
                .as_array()
                .into_iter()
                .flatten()
            {
                let key = graphic["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| graphic.to_string());
                if !seen.insert((old, key)) {
                    continue;
                }
                let polylines: Vec<Vec<[f64; 3]>> =
                    serde_json::from_value(graphic["polylines"].clone()).unwrap_or_default();
                let triangles: Vec<[[f64; 3]; 3]> =
                    serde_json::from_value(graphic["triangles"].clone()).unwrap_or_default();
                if polylines.is_empty() && triangles.is_empty() {
                    continue;
                }
                result.push(Graphic {
                    polylines,
                    triangles,
                    old,
                    colour: match class {
                        "removed" => [0.84, 0.16, 0.16],
                        "added" => [0.04, 0.56, 0.24],
                        _ => [0.88, 0.48, 0.0],
                    },
                    alpha: if old { 0.45 } else { 1.0 },
                });
            }
        }
    }
    result
}
