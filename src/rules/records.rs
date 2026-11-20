//! The analysis as the rules read it: faces, features, the file's own PMI, and the datums
//! chosen.
//!
//! The analysis is JSON from either engine (or an older version of one), so records are read
//! by key, as Python reads its dicts: an optional key that is absent is tolerated where Python
//! tolerates it, and a key the rules need but cannot find is an [`Error::Invalid`] naming the
//! face or feature and the key, never a panic.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use super::py;
use super::{Error, Result, invalid};

/// A face of the analysis (`features.Face.to_dict()`): what a datum, a bore or a distance
/// needs of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub id: i64,
    /// `plane`, `cylinder`, `cone`, `torus` or `other`.
    pub kind: String,
    pub area: Option<f64>,
    pub centroid: Option<[f64; 3]>,
    /// A plane's outward normal; the axis of a surface of revolution.
    pub direction: Option<[f64; 3]>,
    /// A point on the axis of a surface of revolution.
    pub origin: Option<[f64; 3]>,
    pub radius: Option<f64>,
    /// In an assembly, the other part this face rests on (`mates.contacts`).
    pub contact: Option<Value>,
}

impl Face {
    /// A face record; `id` (an integer) and `kind` (a string) are required, the rest is read
    /// when present.
    pub fn from_value(value: &Value) -> Result<Face> {
        let Some(record) = value.as_object() else {
            return invalid(format!("a face is not an object: {}", py::repr(value)));
        };
        let id = match record.get("id") {
            Some(Value::Number(n)) if n.as_i64().is_some() => n.as_i64().unwrap_or_default(),
            Some(other) => return invalid(format!("a face's 'id' is {}", py::repr(other))),
            None => return invalid("a face has no 'id'"),
        };
        let kind = match record.get("kind") {
            Some(Value::String(s)) => s.clone(),
            _ => return invalid(format!("face {id} has no 'kind'")),
        };
        let number = |key: &str| -> Result<Option<f64>> {
            match record.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Number(n)) => Ok(n.as_f64()),
                Some(other) => invalid(format!(
                    "face {id}: '{key}' is not a number: {}",
                    py::repr(other)
                )),
            }
        };
        let vector = |key: &str| -> Result<Option<[f64; 3]>> {
            match record.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Array(items)) if items.len() == 3 => {
                    let mut out = [0.0; 3];
                    for (slot, item) in out.iter_mut().zip(items) {
                        *slot = item.as_f64().ok_or_else(|| {
                            Error::Invalid(format!("face {id}: '{key}' is not three numbers"))
                        })?;
                    }
                    Ok(Some(out))
                }
                Some(_) => invalid(format!("face {id}: '{key}' is not three numbers")),
            }
        };
        Ok(Face {
            id,
            kind,
            area: number("area")?,
            centroid: vector("centroid")?,
            direction: vector("direction")?,
            origin: vector("origin")?,
            radius: number("radius")?,
            contact: record.get("contact").filter(|c| !c.is_null()).cloned(),
        })
    }

    fn missing<T>(&self, key: &str) -> Result<T> {
        invalid(format!("face {} has no '{key}'", self.id))
    }

    pub fn area(&self) -> Result<f64> {
        self.area.map_or_else(|| self.missing("area"), Ok)
    }

    pub fn centroid(&self) -> Result<[f64; 3]> {
        self.centroid.map_or_else(|| self.missing("centroid"), Ok)
    }

    pub fn direction(&self) -> Result<[f64; 3]> {
        self.direction.map_or_else(|| self.missing("direction"), Ok)
    }

    pub fn origin(&self) -> Result<[f64; 3]> {
        self.origin.map_or_else(|| self.missing("origin"), Ok)
    }

    pub fn radius(&self) -> Result<f64> {
        self.radius.map_or_else(|| self.missing("radius"), Ok)
    }

    pub fn is_plane(&self) -> bool {
        self.kind == "plane"
    }

    pub fn is_cylinder(&self) -> bool {
        self.kind == "cylinder"
    }
}

/// The analysis's faces by id, in the analysis's order (Python's `{f["id"]: f for f in
/// analysis["faces"]}`: a repeated id keeps its first place and its last record).
#[derive(Debug, Clone, Default)]
pub struct Faces {
    list: Vec<Face>,
    index: HashMap<i64, usize>,
}

impl Faces {
    /// The faces of `analysis["faces"]`.
    pub fn from_analysis(analysis: &Value) -> Result<Faces> {
        match analysis.get("faces") {
            Some(Value::Array(items)) => Faces::from_values(items),
            _ => invalid("the analysis has no 'faces' list"),
        }
    }

    /// Faces from their records.
    pub fn from_values(values: &[Value]) -> Result<Faces> {
        let mut faces = Faces::default();
        for value in values {
            // A face that could not be measured keeps its place in the analysis's list but is
            // not one the rules know: nothing is asked of it, and nothing refers to it.
            if value.get("unmeasured").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let face = Face::from_value(value)?;
            match faces.index.get(&face.id) {
                // index is private and records only positions appended to list; list is never shortened.
                Some(&i) => faces.list[i] = face,
                None => {
                    faces.index.insert(face.id, faces.list.len());
                    faces.list.push(face);
                }
            }
        }
        Ok(faces)
    }

    /// The face `id`, if the analysis has it.
    pub fn get(&self, id: i64) -> Option<&Face> {
        // index is private and records only positions appended to list; list is never shortened.
        self.index.get(&id).map(|&i| &self.list[i])
    }

    /// The face `id`, which the analysis is expected to have (a feature's face, a datum's).
    pub fn at(&self, id: i64) -> Result<&Face> {
        self.get(id)
            .ok_or_else(|| Error::Invalid(format!("no face {id} in the analysis")))
    }

    pub fn contains(&self, id: i64) -> bool {
        self.index.contains_key(&id)
    }

    /// Every face, in the analysis's order.
    pub fn iter(&self) -> impl Iterator<Item = &Face> {
        self.list.iter()
    }
}

/// A recognised feature (`features.Feature.to_dict()`), read by key.
#[derive(Debug, Clone)]
pub struct Feature<'a> {
    pub id: &'a str,
    pub family: &'a str,
    faces: Option<Vec<i64>>,
    /// A pattern's member hole ids; empty when absent.
    pub members: Vec<&'a str>,
    record: Option<&'a Map<String, Value>>,
}

impl<'a> Feature<'a> {
    pub fn from_value(value: &'a Value) -> Result<Feature<'a>> {
        let Some(map) = value.as_object() else {
            return invalid(format!("a feature is not an object: {}", py::repr(value)));
        };
        let Some(id) = map.get("id").and_then(Value::as_str) else {
            return invalid("a feature has no 'id'");
        };
        let Some(family) = map.get("family").and_then(Value::as_str) else {
            return invalid(format!("feature {id} has no 'family'"));
        };
        let faces = match map.get("faces") {
            None | Some(Value::Null) => None,
            Some(Value::Array(items)) => Some(
                items
                    .iter()
                    .map(|i| {
                        i.as_i64().ok_or_else(|| {
                            Error::Invalid(format!("feature {id}: 'faces' are not face numbers"))
                        })
                    })
                    .collect::<Result<Vec<i64>>>()?,
            ),
            Some(_) => return invalid(format!("feature {id}: 'faces' is not a list")),
        };
        let members = match map.get("members") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|m| {
                    m.as_str().ok_or_else(|| {
                        Error::Invalid(format!("feature {id}: 'members' are not feature ids"))
                    })
                })
                .collect::<Result<Vec<&str>>>()?,
            Some(_) => return invalid(format!("feature {id}: 'members' is not a list")),
        };
        let record = match map.get("record") {
            None | Some(Value::Null) => None,
            Some(Value::Object(r)) => Some(r),
            Some(_) => return invalid(format!("feature {id}: 'record' is not an object")),
        };
        Ok(Feature {
            id,
            family,
            faces,
            members,
            record,
        })
    }

    /// Its faces (`feature["faces"]`).
    pub fn faces(&self) -> Result<&[i64]> {
        self.faces
            .as_deref()
            .ok_or_else(|| Error::Invalid(format!("feature {} has no 'faces'", self.id)))
    }

    /// Its record (`feature["record"]`).
    pub fn record(&self) -> Result<&'a Map<String, Value>> {
        self.record
            .ok_or_else(|| Error::Invalid(format!("feature {} has no 'record'", self.id)))
    }

    /// `record.get(key)`: None when absent or null.
    pub fn get(&self, key: &str) -> Result<Option<&'a Value>> {
        Ok(self.record()?.get(key).filter(|v| !v.is_null()))
    }

    /// `float(record[key])`.
    pub fn number(&self, key: &str) -> Result<f64> {
        match self.get(key)? {
            Some(v) => py::float_of(v)
                .map_err(|e| Error::Invalid(format!("feature {}: record '{key}': {e}", self.id))),
            None => invalid(format!("feature {}: record has no '{key}'", self.id)),
        }
    }

    /// `record.get(key)` as an optional number (a hole's depth).
    pub fn optional_number(&self, key: &str) -> Result<Option<f64>> {
        match self.get(key)? {
            None => Ok(None),
            Some(v) => py::float_of(v)
                .map(Some)
                .map_err(|e| Error::Invalid(format!("feature {}: record '{key}': {e}", self.id))),
        }
    }

    /// `record[key]` as three numbers (a hole's axis or location).
    pub fn vector(&self, key: &str) -> Result<[f64; 3]> {
        let bad = || {
            Error::Invalid(format!(
                "feature {}: record '{key}' is not three numbers",
                self.id
            ))
        };
        match self.get(key)? {
            Some(value @ Value::String(_)) if key == "axis" || key.ends_with("_axis") => {
                crate::features::axis_direction(value).ok_or_else(bad)
            }
            Some(Value::Array(items)) if items.len() == 3 => {
                let mut out = [0.0; 3];
                for (slot, item) in out.iter_mut().zip(items) {
                    *slot = item.as_f64().ok_or_else(bad)?;
                }
                Ok(out)
            }
            Some(_) => Err(bad()),
            None => invalid(format!("feature {}: record has no '{key}'", self.id)),
        }
    }
}

/// What a datum is: coplanar faces, or the coaxial cylinders of one hole or boss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatumKind {
    Plane,
    Axis,
}

impl DatumKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DatumKind::Plane => "plane",
            DatumKind::Axis => "axis",
        }
    }
}

/// One datum: its faces (sorted, for a chosen one; as the file has them, for the file's own),
/// its kind, and whether the file already states it.
#[derive(Debug, Clone, PartialEq)]
pub struct Datum {
    pub faces: Vec<i64>,
    pub kind: DatumKind,
    pub existing: bool,
}

/// The datums, in letter order (Python's `_datums` dict).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Datums(pub Vec<(char, Datum)>);

impl Datums {
    pub fn get(&self, letter: char) -> Option<&Datum> {
        self.0.iter().find(|(x, _)| *x == letter).map(|(_, d)| d)
    }

    pub fn contains(&self, letter: char) -> bool {
        self.get(letter).is_some()
    }

    pub fn letters(&self) -> Vec<char> {
        self.0.iter().map(|(x, _)| *x).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(char, Datum)> {
        self.0.iter()
    }
}

/// The analysis read once for a run of the rules: every public entry builds one and the
/// question passes (several, to a fixed point) share it.
pub struct Ctx<'a> {
    pub analysis: &'a Map<String, Value>,
    pub faces: Faces,
    pub features: Vec<Feature<'a>>,
    /// Feature by id (a repeated id: the last, as Python's dict).
    by_id: HashMap<&'a str, usize>,
    /// Faces the file's sizes and notes are on: not asked about again.
    pub covered: HashSet<i64>,
    /// Faces the file's position tolerances are on.
    pub positioned: HashSet<i64>,
    /// Faces any geometric tolerance of the file is on.
    pub toleranced: HashSet<i64>,
    /// The file's datum letters and their faces (`_file_datums`), in the order first seen.
    pub in_file: Vec<(String, Vec<i64>)>,
    /// What the file states of the part (`analysis["part"]`).
    pub stated: Option<&'a Map<String, Value>>,
    /// Suggested answers from the assembly (`analysis["mates"]`).
    pub mates: Option<&'a Map<String, Value>>,
    default_datums: OnceCell<Result<DefaultDatums>>,
}

/// The default datum faces: A, B and C (`_default_datums`).
pub type DefaultDatums = (Vec<i64>, Vec<i64>, Vec<i64>);

/// One entry of the file's PMI (`existing`), as far as the rules read it.
struct Existing<'a> {
    kind: &'a str,
    kind_type: Option<&'a str>,
    faces: Option<Vec<i64>>,
}

impl<'a> Ctx<'a> {
    pub fn new(analysis: &'a Value) -> Result<Ctx<'a>> {
        let Some(map) = analysis.as_object() else {
            return invalid("the analysis is not an object");
        };
        let faces = Faces::from_analysis(analysis)?;
        let features: Vec<Feature<'a>> = match map.get("features") {
            Some(Value::Array(items)) => items
                .iter()
                .map(Feature::from_value)
                .collect::<Result<_>>()?,
            _ => return invalid("the analysis has no 'features' list"),
        };
        let mut by_id = HashMap::new();
        for (i, f) in features.iter().enumerate() {
            by_id.insert(f.id, i);
        }
        let existing = read_existing(map)?;
        let mut covered = HashSet::new();
        let mut positioned = HashSet::new();
        let mut toleranced = HashSet::new();
        let mut in_file: Vec<(String, Vec<i64>)> = Vec::new();
        for (n, e) in existing.iter().enumerate() {
            let need =
                |what: &str| Error::Invalid(format!("existing PMI entry {n} has no '{what}'"));
            match e.kind {
                "dimension" | "note" | "geometric_tolerance" | "datum" => {
                    let kind_type = e.kind_type.ok_or_else(|| need("type"))?;
                    let ids = e.faces.as_ref().ok_or_else(|| need("faces"))?;
                    match e.kind {
                        "dimension" if kind_type.starts_with("Size_") => covered.extend(ids),
                        "dimension" => {}
                        "note" => covered.extend(ids),
                        "geometric_tolerance" => {
                            toleranced.extend(ids);
                            if kind_type.starts_with("Position") {
                                positioned.extend(ids);
                            }
                        }
                        _ => {
                            // A datum: the first with faces wins over one without.
                            let known = in_file.iter().position(|(t, _)| t == kind_type);
                            if !ids.is_empty() || known.is_none() {
                                match known {
                                    // i is a position in this same in_file vector, which is not shortened.
                                    Some(i) => in_file[i].1 = ids.clone(),
                                    None => in_file.push((kind_type.to_string(), ids.clone())),
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let stated = match map.get("part") {
            None => None,
            Some(Value::Object(m)) => Some(m),
            Some(_) => return invalid("the analysis's 'part' is not an object"),
        };
        let mates = match map.get("mates") {
            None | Some(Value::Null) => None,
            Some(Value::Object(m)) => Some(m),
            Some(_) => return invalid("the analysis's 'mates' is not an object"),
        };
        Ok(Ctx {
            analysis: map,
            faces,
            features,
            by_id,
            covered,
            positioned,
            toleranced,
            in_file,
            stated,
            mates,
            default_datums: OnceCell::new(),
        })
    }

    /// The feature `id`; the analysis is expected to have it (a pattern's member).
    pub fn feature(&self, id: &str) -> Result<&Feature<'a>> {
        self.by_id
            .get(id)
            // by_id is constructed by enumerating features, which is immutable thereafter.
            .map(|&i| &self.features[i])
            .ok_or_else(|| Error::Invalid(format!("no feature {id} in the analysis")))
    }

    /// The file's datum letter `letter`, if it has one.
    pub fn file_datum(&self, letter: char) -> Option<&[i64]> {
        let mut buf = [0u8; 4];
        let key = letter.encode_utf8(&mut buf);
        self.in_file
            .iter()
            .find(|(t, _)| t == key)
            .map(|(_, f)| f.as_slice())
    }

    /// `_default_datums(faces, features)`, worked out once.
    pub fn default_datums(&self) -> Result<&DefaultDatums> {
        self.default_datums
            .get_or_init(|| super::datums::defaults_with(self, &Map::new()))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Datum defaults recomputed with the current diameter answers. Keep the no-answer
    /// cache separate: editing a thread or knurl must immediately update every consumer.
    pub fn default_datums_with(&self, answers: &Map<String, Value>) -> Result<DefaultDatums> {
        if answers.keys().any(|key| key.starts_with("diameter.fit:")) {
            super::datums::defaults_with(self, answers)
        } else {
            self.default_datums().cloned()
        }
    }

    /// Whether any feature is of `family`.
    pub fn has_family(&self, family: &str) -> bool {
        self.features.iter().any(|f| f.family == family)
    }
}

fn read_existing(map: &Map<String, Value>) -> Result<Vec<Existing<'_>>> {
    let items = match map.get("existing") {
        None => return Ok(Vec::new()),
        Some(Value::Array(items)) => items,
        Some(_) => return invalid("the analysis's 'existing' is not a list"),
    };
    items
        .iter()
        .enumerate()
        .map(|(n, e)| {
            let kind = e
                .get("kind")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid(format!("existing PMI entry {n} has no 'kind'")))?;
            let faces = match e.get("faces") {
                None => None,
                Some(Value::Array(ids)) => Some(
                    ids.iter()
                        .map(|i| {
                            i.as_i64().ok_or_else(|| {
                                Error::Invalid(format!(
                                    "existing PMI entry {n}: 'faces' are not face numbers"
                                ))
                            })
                        })
                        .collect::<Result<Vec<i64>>>()?,
                ),
                Some(_) => {
                    return invalid(format!("existing PMI entry {n}: 'faces' is not a list"));
                }
            };
            Ok(Existing {
                kind,
                kind_type: e.get("type").and_then(Value::as_str),
                faces,
            })
        })
        .collect()
}
