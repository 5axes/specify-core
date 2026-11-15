//! One STEP file loaded once, its parts told apart and its faces numbered (port of `load.py`).
//!
//! Everything downstream (recognition, existing PMI, intent, the writer and the browser's
//! picking) refers to faces by their position in one part's face list. A part is numbered on its
//! own shape, not on a placed instance, in specify-core's order (OpenCascade's
//! `TopExp::MapShapes`), which haecceity's [`PartDefinition`] reproduces: face `i` is the face
//! read from `definition.faces[i]`. Both engines therefore number the same file's faces alike
//! and the binding keeps [`LOADER_VERSION`] 2 (docs/design.md, "Face numbering is the same in
//! both engines"; question Q1).
//!
//! A file is read once, by haecceity's [`StepFile::read`] ([`File`]): its distinct parts, their
//! names and product ids, and each part's own `Part` on demand ([`StepFile::part`]), in its own
//! coordinates with its faces in the definition's order, its face and edge sources and its
//! unresolved faces and edges as haecceity records them. A part of an assembly, or a part placed
//! away from the origin, is therefore haecceity's own `Part` like any other (upstream needs U1
//! and U9, met; the stand-ins that read the file placed back and rebuilt the part are gone).
//!
//! What differs from Python: a gzipped file (by its magic bytes) is read decompressed, and bound
//! by its STEP text's hash, where specify-core refuses it (maintainer decision 2026-10-15); a file's parts are haecceity's product definitions, so a product whose
//! shape is a solid plus surface models (NIST CTC-02, 04, 05) is one part here where
//! OpenCascade's XCAF splits it into a part per shape (see `parts`, question Q7); a file with
//! several root shapes, which Python refuses ("expected one part"), is read; part names follow
//! Python's rule over `NEXT_ASSEMBLY_USAGE_OCCURRENCE`s (haecceity's `PartDefinition.name`), not
//! over XCAF's labels.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use haecceity::Part;
use haecceity::step::{PartDefinition, Placement, StepError, StepFile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Bumped whenever a change here could renumber faces for the same bytes. 2 is specify-core's
/// (an assembly holding one solid is numbered on the solid), kept because haecceity numbers faces
/// as specify-core does (question Q1).
pub const LOADER_VERSION: u32 = 2;

/// What face indices are only valid against (Python's `Binding`): the file's bytes, the part's
/// face count, the loader that numbered them and which of the file's parts it is. Serialises in
/// `Binding.to_dict`'s key order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// SHA-256 of the file's bytes, lowercase hex.
    pub source_sha256: String,
    /// How many faces the part has.
    pub face_count: usize,
    /// [`LOADER_VERSION`] of the loader that numbered the faces.
    pub loader_version: u32,
    /// Which of the file's parts (see [`parts`]); 0 for a file of one part.
    pub part: usize,
}

/// One part of a STEP file, loaded (Python's `LoadedPart`). Not `Clone`, as `haecceity::Part`
/// is not; everything downstream takes it by reference.
///
/// Fields are public for callers that construct a part themselves (the editor's session).
/// Such callers must populate `face_names` with haecceity's `Anchors::read` in face order.
#[derive(Debug)]
pub struct LoadedPart {
    /// The file it was read from (or the name standing for it, for bytes held in memory).
    pub path: PathBuf,
    /// The file's bytes, read once and shared by every part of the file.
    pub bytes: Arc<Vec<u8>>,
    pub binding: Binding,
    /// This part's own solid(s) in its own frame, face `i` read from `definition.faces[i]`.
    pub part: Part,
    /// haecceity's account of the part: its product definition, face and edge `#N`s, placements,
    /// name and product id.
    pub definition: PartDefinition,
    /// Authored STEP representation-item names, indexed exactly like `part.faces`.
    /// Read by haecceity `Anchors::read`; unnamed faces are `None`.
    pub face_names: Vec<Option<String>>,
    /// How many parts the file holds; this is part `binding.part` of them.
    pub part_count: usize,
    /// The part's name in the file (Python's `_part_name`; see [`parts`]).
    pub name: String,
    /// Where each instance of the part is placed in the file, rows of `[R | t]` in millimetres.
    pub placements: Vec<Placement>,
    /// Kept for callers that build a `LoadedPart` themselves: true says the `Part` was rebuilt
    /// from other geometry and has lost haecceity's record of what did not resolve (see
    /// [`LoadedPart::degraded`]). This loader never rebuilds a part (upstream need U1 is met),
    /// so it is always false here.
    pub rebuilt: bool,
    /// The faces (this part's indices) whose surface haecceity could not resolve: the part's own
    /// `Part::unresolved_faces`.
    pub unresolved_faces: Vec<usize>,
    /// The edges (indices into `part.edges`) whose curve haecceity could not resolve: the part's
    /// own `Part::unresolved_edges`.
    pub unresolved_edges: Vec<usize>,
    /// quiddity's recognition of `part`, made on first use and kept ([`LoadedPart::recognition`]).
    /// Callers that build a `LoadedPart` themselves start it as `Default::default()`.
    pub recognition: OnceLock<quiddity::correspondence::Recognition>,
}

impl LoadedPart {
    /// quiddity's recognition of the part: made on the first call and shared by every later one,
    /// so a part analysed and then diffed (or diffed against several revisions) is recognised
    /// once. The part is never changed after it is loaded, so the result cannot go stale.
    pub fn recognition(&self) -> &quiddity::correspondence::Recognition {
        self.recognition
            .get_or_init(|| quiddity::correspondence::recognise(&self.part))
    }

    /// The `#N` of the `ADVANCED_FACE` (or `FACE_SURFACE`) face `index` was read from (Python's
    /// `face_ranks` and `requirements.face_entities_of`). Panics if `index` is not a face of the
    /// part, as indexing does.
    pub fn face_entity(&self, index: usize) -> u64 {
        // This native helper is not called by the WASM crate; its caller must supply a definition face index.
        self.definition.faces[index]
    }

    /// Why the part cannot be trusted as a whole solid, if it cannot: it was rebuilt
    /// (`rebuilt`, set only by a caller that builds the part itself) and has unresolved faces or
    /// edges, which a rebuilt `Part` no longer records, so its `solid_is_valid` and the
    /// recognisers that skip unresolved faces would take it for a valid solid. Recognition,
    /// meshing and anchors consult this and `unresolved_faces`, and refuse or report rather than
    /// run on such a part. A part this module loads is never degraded.
    pub fn degraded(&self) -> Option<String> {
        if !self.rebuilt || (self.unresolved_faces.is_empty() && self.unresolved_edges.is_empty()) {
            return None;
        }
        Some(format!(
            "part {} ({}) was rebuilt without haecceity's record of what did not resolve \
             (upstream need U1); unresolved faces {:?}, unresolved edges {:?}",
            self.binding.part, self.name, self.unresolved_faces, self.unresolved_edges
        ))
    }
}

/// One distinct part of a file as `parts` lists it: Python's `{part, name, faces, instances,
/// placements}` in that key order, with the product's id after the name (decision D1: a part is
/// identified by its index; its name and product id are reported beside it). The API adds
/// `written`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartSummary {
    pub part: usize,
    pub name: String,
    /// `PRODUCT.id` as the file states it (escapes decoded); need not be unique, and often
    /// equals the name. Not in Python's listing.
    pub product_id: String,
    pub faces: usize,
    pub instances: usize,
    /// Each instance's placement, rows of `[R | t]`, every value rounded to 9 decimals.
    pub placements: Vec<Placement>,
}

/// Why a file or part could not be loaded. The messages are specify-core's where it has one.
#[derive(Debug)]
pub enum LoadError {
    /// The file could not be read or parsed as STEP (Python: `cannot read STEP file {path}`);
    /// the reason follows.
    Unreadable { path: PathBuf, reason: String },
    /// The file holds no shape with faces.
    NoFaces { path: PathBuf },
    /// haecceity refused the file's geometry or structure, for the reason given.
    Step { path: PathBuf, reason: String },
    /// A file of several parts was loaded without saying which; the parts' names, in order.
    Assembly { path: PathBuf, names: Vec<String> },
    /// The part asked for is not one of the file's `count`.
    NoSuchPart {
        path: PathBuf,
        count: usize,
        part: usize,
    },
    /// haecceity could not read the part's own shape (`StepFile::part`), for the reason given.
    Extract {
        path: PathBuf,
        part: usize,
        name: String,
        reason: String,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Unreadable { path, reason } => {
                write!(f, "cannot read STEP file {}: {reason}", path.display())
            }
            LoadError::NoFaces { path } => write!(f, "{}: no shape with faces", path.display()),
            LoadError::Step { path, reason } => write!(f, "{}: {reason}", path.display()),
            LoadError::Assembly { path, names } => {
                let names: Vec<String> = names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| format!("{i}: {name}"))
                    .collect();
                write!(
                    f,
                    "{} is an assembly of {} parts; choose one ({})",
                    path.display(),
                    names.len(),
                    names.join(", ")
                )
            }
            LoadError::NoSuchPart { path, count, part } => write!(
                f,
                "{} has {count} part(s); there is no part {part}",
                path.display()
            ),
            LoadError::Extract {
                path,
                part,
                name,
                reason,
            } => write!(
                f,
                "{}: cannot read part {part} ({name}): {reason}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// Load `path` and take its `part`-th part; a file of several parts must say which (Python's
/// `load`; decision D4). Only the part asked for is built.
#[cfg(not(target_arch = "wasm32"))]
pub fn load(path: &Path, part: Option<usize>) -> Result<LoadedPart, LoadError> {
    File::read(path)?.load(part)
}

/// [`load`] of a file held in memory, for a caller with no filesystem (the editor's browser
/// build): `bytes` are the file's, `name` stands in for its path in the [`LoadedPart`] and in
/// errors. Single part: a file of several parts is refused as [`load`] refuses it without a
/// part ([`LoadError::Assembly`]); [`File::of_bytes`] then [`File::load`] takes a part of an
/// assembly. The part is loaded exactly as [`load`] would load the same bytes from a file,
/// binding and face numbering included.
pub fn load_bytes(name: &str, bytes: Vec<u8>) -> Result<LoadedPart, LoadError> {
    File::of_bytes(Path::new(name), bytes)?.load(None)
}

/// Every part of `path`, loaded, in [`parts`]' order (Python's `load_all`).
#[cfg(not(target_arch = "wasm32"))]
pub fn load_all(path: &Path) -> Result<Vec<LoadedPart>, LoadError> {
    File::read(path)?.load_all()
}

/// The distinct parts of `path` (Python's `parts`): each one's name, product id, face count and
/// where each of its instances is placed. A part placed twice is one part with two placements.
/// No part's own geometry is built.
///
/// The parts are haecceity's: one per product definition whose shape has faces, in
/// specify-core's order. Where OpenCascade's XCAF makes a part of each shape representation of
/// one product (a solid with surface models beside it, as in NIST CTC-02, 04 and 05), this lists
/// the product once with all its faces, the solid's first.
#[cfg(not(target_arch = "wasm32"))]
pub fn parts(path: &Path) -> Result<Vec<PartSummary>, LoadError> {
    Ok(File::read(path)?.parts())
}

/// A STEP file read once (haecceity's [`StepFile`]): its bytes, their hash and its parts, from
/// which any part, or every part, is loaded without reading or parsing the file again. What
/// [`load`], [`load_all`] and [`parts`] read through; a caller that needs several parts of one
/// file (the analysis of a part of an assembly, the writer) holds one.
pub struct File {
    path: PathBuf,
    bytes: Arc<Vec<u8>>,
    sha256: String,
    step: StepFile,
    face_names: Vec<Vec<Option<String>>>,
}

impl File {
    /// Reads the file at `path`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(path: &Path) -> Result<File, LoadError> {
        let bytes = std::fs::read(path).map_err(|e| LoadError::Unreadable {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
        File::of_bytes(path, bytes)
    }

    /// The file whose bytes are `bytes`, `path` standing for where it came from (in the parts
    /// loaded and in errors); nothing is read from `path`.
    pub fn of_bytes(path: &Path, bytes: Vec<u8>) -> Result<File, LoadError> {
        File::of_shared(path, Arc::new(bytes))
    }

    /// [`File::of_bytes`] over bytes already shared (a [`LoadedPart`]'s), not copied.
    pub fn of_shared(path: &Path, bytes: Arc<Vec<u8>>) -> Result<File, LoadError> {
        // Gzip (its magic bytes, whatever the file is called) is decompressed in memory, and the
        // file is its STEP text from here on: its hash, its parts and what the writer edits, so
        // a `.step` and its `.step.gz` bind identically (maintainer decision 2026-10-15).
        let bytes = match gunzip(&bytes) {
            Some(text) => Arc::new(text.map_err(|e| LoadError::Unreadable {
                path: path.to_path_buf(),
                reason: format!("the file is gzip but cannot be decompressed: {e}"),
            })?),
            None => bytes,
        };
        let step = StepFile::read(&bytes).map_err(|e| step_error(path, e))?;
        if step.parts.is_empty() {
            return Err(LoadError::NoFaces {
                path: path.to_path_buf(),
            });
        }
        let doc =
            haecceity::p21::Document::parse(bytes.to_vec()).map_err(|e| LoadError::Unreadable {
                path: path.to_path_buf(),
                reason: e.to_string(),
            })?;
        let face_names = step
            .parts
            .iter()
            .map(|definition| haecceity::json::Anchors::read(&doc, definition).names)
            .collect();
        let sha256 = Sha256::digest(bytes.as_slice())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(File {
            path: path.to_path_buf(),
            bytes,
            sha256,
            step,
            face_names,
        })
    }

    /// Where the file came from, as given.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's bytes, shared with every part loaded from it.
    pub fn bytes(&self) -> &Arc<Vec<u8>> {
        &self.bytes
    }

    /// SHA-256 of the file's bytes, lowercase hex (each part's `binding.source_sha256`).
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// haecceity's account of each distinct part, in [`parts`]' order.
    pub fn definitions(&self) -> &[PartDefinition] {
        &self.step.parts
    }

    /// The distinct parts, as [`parts`] lists them.
    pub fn parts(&self) -> Vec<PartSummary> {
        self.step
            .parts
            .iter()
            .enumerate()
            .map(|(index, definition)| PartSummary {
                part: index,
                name: definition.name.clone(),
                product_id: definition.product_id.clone(),
                faces: definition.faces.len(),
                instances: definition.placements.len(),
                placements: definition
                    .placements
                    .iter()
                    .map(|p| rounded(&p.placement))
                    .collect(),
            })
            .collect()
    }

    /// The `part`-th part; with no part, the file's only part, and a file of several is refused
    /// naming its parts ([`LoadError::Assembly`], decision D4).
    pub fn load(&self, part: Option<usize>) -> Result<LoadedPart, LoadError> {
        let count = self.step.parts.len();
        let index = match part {
            Some(index) => index,
            None if count > 1 => {
                return Err(LoadError::Assembly {
                    path: self.path.clone(),
                    names: self.step.parts.iter().map(|d| d.name.clone()).collect(),
                });
            }
            None => 0,
        };
        if index >= count {
            return Err(LoadError::NoSuchPart {
                path: self.path.clone(),
                count,
                part: index,
            });
        }
        self.load_index(index)
    }

    /// Every part, in [`parts`]' order.
    pub fn load_all(&self) -> Result<Vec<LoadedPart>, LoadError> {
        (0..self.step.parts.len())
            .map(|index| self.load_index(index))
            .collect()
    }

    /// Builds part `index`'s [`LoadedPart`]: haecceity's own `Part` of it, in its own frame.
    fn load_index(&self, index: usize) -> Result<LoadedPart, LoadError> {
        // load() validates the requested part index against this definitions list before indexing.
        let definition = &self.step.parts[index];
        let part = self.step.part(index).map_err(|e| LoadError::Extract {
            path: self.path.clone(),
            part: index,
            name: definition.name.clone(),
            reason: e.to_string(),
        })?;
        let unresolved_faces = part.unresolved_faces().to_vec();
        let unresolved_edges = part.unresolved_edges().to_vec();
        Ok(LoadedPart {
            path: self.path.clone(),
            bytes: Arc::clone(&self.bytes),
            binding: Binding {
                source_sha256: self.sha256.clone(),
                face_count: definition.faces.len(),
                loader_version: LOADER_VERSION,
                part: index,
            },
            part,
            definition: definition.clone(),
            face_names: self.face_names[index].clone(),
            part_count: self.step.parts.len(),
            name: definition.name.clone(),
            placements: definition.placements.iter().map(|p| p.placement).collect(),
            rebuilt: false,
            unresolved_faces,
            unresolved_edges,
            recognition: OnceLock::new(),
        })
    }
}

/// A haecceity refusal as a [`LoadError`]: a file that does not parse is unreadable (Python's
/// `cannot read STEP file`), a file with no shape with faces is [`LoadError::NoFaces`], anything
/// else is haecceity's own reason.
/// `bytes` decompressed when they are gzip (they begin with its magic bytes `1f 8b`), else none.
pub fn gunzip(bytes: &[u8]) -> Option<std::io::Result<Vec<u8>>> {
    bytes.starts_with(&[0x1f, 0x8b]).then(|| {
        let mut text = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::MultiGzDecoder::new(bytes), &mut text)
            .map(|_| text)
    })
}

fn step_error(path: &Path, error: StepError) -> LoadError {
    let path = path.to_path_buf();
    match error {
        StepError::Parse(_) => LoadError::Unreadable {
            path,
            reason: error.to_string(),
        },
        // read_step's refusal of a file with no faces at all (step.rs, `read_placed`).
        StepError::Incomplete(ref why) if why == "no solid or shell with faces" => {
            LoadError::NoFaces { path }
        }
        _ => LoadError::Step {
            path,
            reason: error.to_string(),
        },
    }
}

/// A placement with every value rounded to 9 decimals, as Python's `_matrix` does
/// (`round(x, 9)`): Rust's `{:.9}` rounds the exact binary value correctly, as Python's `round`
/// does, so the two agree.
fn rounded(p: &Placement) -> Placement {
    p.map(|row| row.map(|x| format!("{x:.9}").parse().unwrap_or(x)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_round_to_nine_decimals() {
        let p: Placement = [
            [1.0, 2e-17, 0.0, -30.0000000004],
            [0.0, 1.0, 0.0, 0.1234567896],
            [0.0, 0.0, 1.0, -1e-12],
        ];
        let r = rounded(&p);
        assert_eq!(r[0], [1.0, 0.0, 0.0, -30.0]);
        assert_eq!(r[1][3], 0.12345679);
        assert!(
            r[2][3] == 0.0 && r[2][3].is_sign_negative(),
            "Python: round(-1e-12, 9) is -0.0"
        );
    }
}
