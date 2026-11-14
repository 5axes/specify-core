//! First allocation only. Geometry is in LoadedPart's own coordinates, as Draftwright
//! draws a file containing PMI (model::analyse_step_input does not square such a file).
use haecceity::geom::Surface;
use haecceity::pmi::{DimensionKind, PartPmi};

use super::Item;
use crate::part::LoadedPart;
use crate::pyfmt::round_half_even;

#[derive(Clone, Copy)]
struct Box3 {
    lo: [f64; 3],
    hi: [f64; 3],
}

impl Box3 {
    fn of(loaded: &LoadedPart, faces: &[usize]) -> Option<Self> {
        let mut b = Self {
            lo: [f64::INFINITY; 3],
            hi: [f64::NEG_INFINITY; 3],
        };
        for &f in faces {
            loaded.part.faces.get(f)?;
            let face = loaded.part.face_bounds(f);
            if !face.min.into_iter().chain(face.max).all(f64::is_finite) {
                return None;
            }
            for k in 0..3 {
                b.lo[k] = b.lo[k].min(face.min[k]);
                b.hi[k] = b.hi[k].max(face.max[k]);
            }
        }
        (!faces.is_empty()).then_some(b)
    }

    fn centre(self) -> [f64; 3] {
        std::array::from_fn(|k| 0.5 * (self.lo[k] + self.hi[k]))
    }

    fn longest(self) -> usize {
        dominant(std::array::from_fn(|k| self.hi[k] - self.lo[k]))
    }
}

// Strict > gives X, then Y, then Z for equal components. Signs do not change a view.
fn dominant(d: [f64; 3]) -> usize {
    (1..3).fold(0, |k, i| if d[i].abs() > d[k].abs() { i } else { k })
}

fn direction(loaded: &LoadedPart, f: usize, cylinders_only: bool) -> Option<[f64; 3]> {
    match loaded.part.faces.get(f)?.surface {
        Surface::Cylinder { frame, .. } => Some(frame.z),
        Surface::Plane { frame } if !cylinders_only => Some(frame.z),
        Surface::Cone { frame, .. } | Surface::Torus { frame, .. } if !cylinders_only => {
            Some(frame.z)
        }
        _ => None,
    }
}

fn common_axis(loaded: &LoadedPart, faces: &[usize], cylinders_only: bool) -> Option<usize> {
    let k = dominant(direction(loaded, *faces.first()?, cylinders_only)?);
    faces
        .iter()
        .all(|&f| direction(loaded, f, cylinders_only).is_some_and(|d| dominant(d) == k))
        .then_some(k)
}

// Draftwright pmi_lowering::face_on and ::edge_on; 0=front, 1=plan, 2=side.
fn end_on(k: usize) -> u8 {
    [2, 0, 1][k]
}

fn edge_on(k: usize) -> u8 {
    if k == 1 { 2 } else { 0 }
}

fn external_cylinders(loaded: &LoadedPart, faces: &[usize]) -> bool {
    !faces.is_empty()
        && faces.iter().all(|&f| {
            loaded.part.faces.get(f).is_some_and(|face| {
                matches!(face.surface, Surface::Cylinder { .. }) && !face.reversed
            })
        })
}

fn dimension_view(loaded: &LoadedPart, item: &Item, d: &DimensionKind, b: Box3) -> u8 {
    let faces = &item.key.faces;
    let reference = &item.key.reference;
    let cylinder = common_axis(loaded, faces, true);
    match d {
        DimensionKind::Size { angle: None, .. } if cylinder.is_some() => {
            if external_cylinders(loaded, faces) {
                // dims::turned::diameter_views: X/Z steps in front profile, Y end-on
                // in front. This keeps the axial stations of a shaft in the walk.
                0
            } else {
                end_on(cylinder.expect("tested above"))
            }
        }
        DimensionKind::Location { angle: None, .. }
            if cylinder.is_some_and(|k| {
                common_axis(loaded, reference, false) != Some(k)
                    || reference.iter().any(|&f| {
                        loaded
                            .part
                            .faces
                            .get(f)
                            .is_some_and(|face| matches!(face.surface, Surface::Cylinder { .. }))
                    })
            }) =>
        {
            // Plane-to-bore and bore-to-bore locations lie in the end-on view, as in
            // Draftwright pmi_lowering::plane_to_bore. Keep the hole's position for ties.
            end_on(cylinder.expect("tested above"))
        }
        DimensionKind::Size { angle: Some(_), .. }
        | DimensionKind::Location { angle: Some(_), .. } => {
            let mut normals = faces
                .iter()
                .chain(reference)
                .filter_map(|&f| direction(loaded, f, false));
            if let (Some(a), Some(c)) = (normals.next(), normals.next()) {
                let cross = [
                    a[1] * c[2] - a[2] * c[1],
                    a[2] * c[0] - a[0] * c[2],
                    a[0] * c[1] - a[1] * c[0],
                ];
                if cross.iter().any(|v| v.abs() > 1e-9) {
                    return end_on(dominant(cross));
                }
            }
            0
        }
        _ => {
            // Parallel planes measure along their normal. Otherwise the two support
            // centres give a deterministic direction, without using an authored nominal.
            let k = common_axis(loaded, reference, false)
                .or_else(|| common_axis(loaded, faces, false))
                .unwrap_or_else(|| {
                    Box3::of(loaded, reference).map_or(b.longest(), |r| {
                        dominant(std::array::from_fn(|k| b.centre()[k] - r.centre()[k]))
                    })
                });
            edge_on(k)
        }
    }
}

/// A total order, retaining semantic model order for equal geometric keys. Quantize
/// projected coordinates to 1 µm so numerical noise does not split a shared drawing row.
pub(super) fn order(loaded: &LoadedPart, p: &PartPmi, items: &[Item]) -> Vec<usize> {
    let keys: Vec<_> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            if item.key.kind == "datum" {
                return (0, item.key.r#type.as_str(), 0, 0.0, 0.0, 0);
            }
            let dimension_index = i.checked_sub(p.datums.len());
            let dimension = dimension_index.and_then(|i| p.dimensions.get(i));
            // A depth's RP-correct endpoint aspects do not change the inspection
            // walk of its underlying hole quantity. Retain the bore-based sort
            // position/rank used before its semantic form became a location.
            let depth = dimension_index.and_then(|i| p.attributes.iter().find(|a|
                a.name == "specify dimension plan" && a.on == Some(haecceity::pmi::NoteOwner::Dimension(haecceity::pmi::DimensionId(i)))))
                .and_then(|a| a.items.iter().find_map(|(k,v)| match v {
                    haecceity::pmi::AttributeValue::Text(s) if k == "requirement" => serde_json::from_str::<crate::dimensioning::Planned>(s).ok(),
                    _ => None,
                })).filter(|p| matches!(&p.required.kind, quiddity::dimensioning::Kind::Size { kind: quiddity::dimensioning::SizeKind::Length, role, .. } if role.ends_with(".depth")));
            let surrogate = depth.as_ref().map(|p| {
                let mut key = item.key.clone();
                key.faces = p.supports.iter().filter(|s| p.required.covers.contains(&s.feature))
                    .flat_map(|s| s.faces.iter().copied()).collect();
                key.faces.sort_unstable();
                key.faces.dedup();
                key.reference.clear();
                super::Item { key, entity: 0, value_hint: super::ValueHint::None }
            });
            let item = surrogate.as_ref().unwrap_or(item);
            let depth_kind = DimensionKind::Size {
                feature: haecceity::pmi::FeatureId(0), kind: haecceity::pmi::SizeKind::CurveLength, path: None, angle: None,
            };
            let kind = if depth.is_some() { Some(&depth_kind) } else { dimension.map(|d| &d.kind) };
            let rank = match kind {
                Some(DimensionKind::Size { .. }) => 0,
                Some(DimensionKind::Location { .. }) => 1,
                None if item.key.kind == "geometric_tolerance" => 2,
                None => 3,
            };
            let Some(b) = Box3::of(loaded, &item.key.faces) else {
                return (2, "", 0, 0.0, 0.0, rank);
            };
            let view = if let Some(kind) = kind {
                dimension_view(loaded, item, kind, b)
            } else {
                match item.key.kind.as_str() {
                    "surface_texture" => {
                        edge_on(common_axis(loaded, &item.key.faces, false).unwrap_or(b.longest()))
                    }
                    "thread" | "knurl" if external_cylinders(loaded, &item.key.faces) => 0,
                    _ => end_on(common_axis(loaded, &item.key.faces, true).unwrap_or(b.longest())),
                }
            };
            let c = b.centre();
            // model::VIEWS: front=(X,Z), plan=(X,Y), right side=(Y,Z).
            let (x, y) = match view {
                0 => (c[0], c[2]),
                1 => (c[0], c[1]),
                _ => (c[1], c[2]),
            };
            (
                1,
                "",
                view,
                -round_half_even(y, 3),
                round_half_even(x, 3),
                rank,
            )
        })
        .collect();
    let mut order: Vec<_> = (0..items.len()).collect();
    order.sort_by(|&a, &b| {
        let a = keys[a];
        let b = keys[b];
        (a.0, a.1, a.2)
            .cmp(&(b.0, b.1, b.2))
            .then_with(|| a.3.partial_cmp(&b.3).expect("finite position"))
            .then_with(|| a.4.partial_cmp(&b.4).expect("finite position"))
            .then_with(|| a.5.cmp(&b.5))
    });
    order
}
