# specify-core-rust: design of the port

specify-core (Python, `/Users/paul/repos/specify-core`, AGPL-3.0-or-later) is interview-driven
AP242 PMI authoring: it loads a STEP part, recognises its features, works out what manufacturing
intent is missing, proposes defaults with reasons, and writes the answers back as AP242 semantic
PMI, verified by reading the file back. This crate is its port to Rust, with no OpenCascade and
no Python: geometry, STEP and PMI come from haecceity and features from quiddity, both in
[pzfreo/quiddity](https://github.com/pzfreo/quiddity).

Ground rules, from the maintainer's brief:

- **Accuracy over parity.** Python's outputs are the oracle, not the specification. Every
  difference gets a verdict with evidence (`rust-correct`, `rust-wrong`, `equivalent`,
  `undetermined`, `not-applicable`). Behaviour that exists only to work round OpenCascade is not
  copied where haecceity is more correct.
- **Compatible interfaces.** The command line (`parts`, `analyse`, `mesh`, `questions`, `write`,
  `serve`) and the JSON formats (analysis, answers, intent, mesh, write report, serve protocol)
  stay specify-core's, so Draftwright Specify can run either engine. A deliberate change is
  recorded in the known differences with its reason.
- **Honest failure.** Refuse or report; never silently drop or guess. Deterministic output.
- **Upstream is read-only.** quiddity-rust and draftwright-rust belong to other runs. What this
  crate needs from them and lacks is recorded in [upstream-needs.md](upstream-needs.md), with a
  local, honest stand-in or a refusal meanwhile. Decisions only the maintainer can make are in
  [questions.md](questions.md).

## What specify-core asks of its dependencies

Read from `src/specify_core/*.py` at specify-core `main`:

| Need | Where in Python | OpenCascade / Quiddity call |
|---|---|---|
| Load a STEP once, distinct parts of an assembly, a part placed twice is one part | `load.py` | `STEPCAFControl_Reader`, XCAF labels, `GetFreeShapes`, references, components |
| Face numbering per part, on the part's own solid | `load.py` `_faces` | `TopExp::MapShapes(FACE)` |
| Which `ADVANCED_FACE` each face was read from | `LoadedPart.face_ranks`, `requirements.face_entities_of` | transfer reader `EntityFromShapeResult` |
| Placements of each instance (3x4) and part names | `load.py` `_instances`, `_part_name` | `XCAFDoc_ShapeTool.GetLocation`, `TDataStd_Name` |
| Face kind, area, centroid, plane normal (outward), axis, origin, radius | `features.describe_faces` | `BRepGProp.SurfaceProperties`, `BRepAdaptor_Surface` |
| Recognised features with constituent faces, records, pattern members | `features.recognise` | `quiddity.evidence.build_recognition_evidence` |
| Existing semantic PMI on the part's faces | `existing.py` | XCAF DimTol reader, plus text scans for what it loses (`magnitudes.py`, `datum_faces`, `notes`, `part_settings`) |
| Mesh by face, outward winding, face-boundary polylines without seams | `mesh.py` | `BRepMesh_IncrementalMesh`, `GCPnts_QuasiUniformDeflection`, `Bnd_Box` |
| Mates: cylinders placed per instance, coaxial and overlapping; flat contacts with overlap area | `mates.py` | `BRepAdaptor_Surface`, `BRepTools.UVBounds`, `BRepAlgoAPI_Common`, `Bnd_Box` |
| Label anchors: point on the trimmed face, normal, clear leader direction | `anchors.py`, `sightlines.py` | `quiddity.inspection.inspect_face`, `BRepClass3d_SolidClassifier` |
| Write datums, sizes, locations, geometric tolerances, material | `writer.py` | `STEPCAFControl_Writer` (XCAF DimTol, material tool), then text fixes |
| Write threads, knurls, general tolerance, notes, datum symbols, stored answers | `requirements.py`, `resume.py` | raw Part 21 text appended |
| Add PMI to a file that has some, byte for byte | `merge.py` | OCCT scratch write + text transplant |
| Verify by reading back | `writer.verify`, `_verify_appended` | XCAF reader + text scans |

## The haecceity / quiddity API that replaces it

quiddity-rust is used as a library (Rust to Rust), not through its `quiddity` command line.

| Need | Replacement (branch `prototype/rust-port`, commit 10884b8) |
|---|---|
| Load, parts, names, placements, face `#N` | `haecceity::step::read_part_definitions(&bytes) -> Vec<PartDefinition>` (`product_definition`, `shape`, `name`, `faces: Vec<u64>` in specify-core's numbering, `edges`, `placements`, `face_index(#N)`) |
| The part's B-rep | `haecceity::read_step(&bytes) -> Part` (every placed instance; faces carry `face_source(i) -> Source { entity, instance }`) |
| Face geometry | `Part.faces[i].surface` (`Surface::{Plane, Cylinder, Cone, Sphere, Torus, ..}` with `Frame`), `Face.reversed`, `Part::face_moments(i) -> FaceMoments { area, centroid, area_vector }`, `face_normal`, `uv_bounds`, `face_centre`, `bounds`, `face_edges`, `edge_faces`, `neighbours` |
| Features | `quiddity::correspondence::recognise(&Part) -> Recognition { features: PublishedFeatures, fingerprints }`; `recognition::document` records (published defining faces and hole-pattern record types) |
| Point classification for sightlines | `haecceity::classify::Classifier::new(&part).classify(p) -> State` |
| Edge polylines | `Part.edges[e].samples`, `Part::edge_faces()` (a seam lists its face once) |
| Existing PMI | `haecceity::p21::Document`, `haecceity::pmi::read(&doc, &parts) -> PmiRead { parts: Vec<PartPmi>, findings, provenance, accounting, callouts, saved_views }` (anchors are face indices of the part) |
| Writing PMI | `haecceity::pmi::write(doc, parts, &[(PartId, PartPmi)], Mode::{Add, Replace, Remove}, PresentationPolicy) -> (Edit, WriteReport)`; every instance validated against the AP242 schema table, rule checks, datum feature symbols for added datums |
| Verification | `haecceity::pmi::read` of the output and `haecceity::pmi::write::differences(&written, &read)` |

Everything in Python that exists only because of OpenCascade is not ported: `p21.py` (escapes:
haecceity writes them), `merge.py` (haecceity's `Mode::Add` keeps every original byte),
`magnitudes.py` and the text fallbacks in `existing.py` (haecceity reads magnitudes and datum
features itself), `locations.mark_basic` (`DimTolerance::Basic`), `writer.py`'s text fixes
(`_drop_runout_zones`, `_type_limits`, `_simple_tolerances`, `_part_datums`, the metre scale
for tolerances), `lettering.py` and the datum symbol drawing (haecceity draws datum symbols,
decision 6 of `docs/step-ap242.md`).

**Face numbering is the same in both engines.** haecceity numbers faces in OpenCascade's
`TopExp::MapShapes` order (`brep.rs`, `PartDefinition`). Checked before this design on
`plate.step`, `pin.step` and the corpus spool `flanged_spool_132.step`: all 113 faces have the
same kind and centroid (to 1e-9) at the same index. The Rust binding therefore keeps
`loader_version` 2, so an analysis made by either engine can be written by the other; the
features stream pins the face-by-face equality on every fixture, and a file where it fails is a
known difference whose analyses must not cross engines (question Q1).

**Features differ by design between the two quiddities.** On `pin.step` Python reports
`holes:3.5` (the bore and its floor: *constituent* faces) and Rust `holes/0 [3]` (*defining*
faces only); Python reports `step_levels` and suppresses `bosses` on a turned part (its
cross-family reconciliation). Rust has no `step_levels`; since quiddity #123 in v0.2.3,
its proved `turned_bodies` suppress nearly full-length OD bosses while retaining shorter
local bosses. Feature ids in this crate are specify-core's (`family:faces`, from the faces the port uses), so ids for the same
feature can differ between engines. An analysis is a stored snapshot, but saved STEP answers are re-recognised on reopening;
the resume correspondence rule below handles feature-ID changes.

## Dependency plan

- `quiddity` and `haecceity` by git from `https://github.com/pzfreo/quiddity`,
  pinned to tag `v0.4.9` (`17a883aa`) in both manifests and Cargo.lock; the root crate takes
  quiddity with `default-features = false, features = ["presentation"]`, so clap stays out.
  Update only these packages with `cargo update -p quiddity -p haecceity`, then run
  the named local profile below and the full suite in CI, and rejudge any departures from the frozen Python captures with
  independent evidence. `correspondence::recognise` returns published file-coordinate records (#92);
  Specify serialises `PublishedFeatures` (see the axis consumer audit below).
  Specify consumes the document for defining faces and pattern types only: modelled
  threads are additive features; their claimed freeforms become thread faces. The
  upstream document is recognition/6 with complete face accounting (#66). Specify has no
  unclaimed/unrecognised-face display, so it does not add a new UI for that accounting.
  Specify keeps its own analysis schema and binding because face numbering
  is unchanged. Derived PMI geometry is followed through its deriving faces by
  `existing::faces_of`; Specify reads PMI anew from STEP, never persists the upstream
  PMI JSON (version 2, now reader binding `haecceity-pmi-read/19`; older readings must be reread).
  It builds its own write report, so upstream parts/write-report version 2 and the
  graphic-only `Accounting::presentation` do not change its existing keys. The v0.4.6
  `geometry_carrier_exemptions`, `presentation_warnings` and `preserved_normalizations`
  the v0.4.7 `retained_shared` and the v0.4.8 `saved_view_findings` pass through as
  optional report fields.
- **Cargo.toml and Cargo.lock across parallel streams.** A stream that adds a crate adds its
  line in alphabetical order within its table (`[dependencies]` or `[dev-dependencies]`) and
  regenerates the lock with `cargo update -p <crate>` only (never a bare `cargo update`, which
  would move other pins). The integrator resolves a Cargo.lock conflict by taking either side
  and rerunning `cargo update -p <crate>` for each crate the merged streams added (or `cargo
  check`, which adds missing entries without moving existing ones), never by editing the lock
  by hand.
- `serde`, `serde_json` (`preserve_order`, so objects keep Python's key order; `float_roundtrip`),
  `sha2` (the binding's `source_sha256`), `base64` (stored answers), `regex` (typed specs and
  added requirements, as Python reads them), `flate2` (gzipped STEP read and written, maintainer
  decision 2026-10-15; also the gzipped fixtures). A stream that needs another crate adds one line to `Cargo.toml` and says why in its
  commit; the mates stream is expected to add a polygon-boolean crate for flat contacts
  (upstream need U5).
- Toolchain 1.99 (`rust-toolchain.toml`), edition 2024. No OpenCascade, no Python at run time.
  Python is used by the oracle capture tool and optional live checks. Live tests require
  an explicit `SPECIFY_TEST_PYTHON` path and never discover another host checkout;
  additional plain NIST inputs use `SPECIFY_TEST_NIST_AP242`. Frozen captures are unchanged.

## Module map

| Python (`specify_core/`) | Rust (`src/`) | Notes |
|---|---|---|
| `load.py` | `part.rs` | `LoadedPart { path, bytes, binding, part (haecceity `Part`, the part's own solid in its own frame, faces in specify-core's numbering), definition (`PartDefinition`), part_count, name, placements }`; `load`, `load_all`, `parts`. `File` reads a file once with haecceity's `StepFile::read` (parts, names, product ids) and loads any part from it with `StepFile::part`: haecceity's own `Part` in the part's own frame, faces in the definition's order, sources and unresolved lists kept (U1 and U9, met at quiddity-rust 0d601d2). `LoadedPart` still carries `rebuilt`, `unresolved_faces` and `unresolved_edges` for callers that build one themselves; a part this crate loads is never rebuilt or degraded. `LoadedPart` is not `Clone` (`haecceity::Part` is only `Debug`); APIs take it by reference. `Binding` with `loader_version` 2. |
| `features.py` | `features.rs` | `Face` records (`describe_faces`) from `Surface` and `face_moments`; `recognise` from quiddity: holes, patterns (members matched to hole records by equality, as quiddity's fingerprints do), every other family, and specify-core's own hole sets. |
| `existing.py` | `existing.rs` | `read_existing` maps `PartPmi` to specify-core's `existing` records (kind, OCCT type name, faces, faces2, value, upper, lower, datums: the names are a JSON format, kept); `part_settings` from material, general tolerance class and notes; `notes` from threads, knurls, finish. No text scans. |
| `magnitudes.py`, `p21.py`, `merge.py`, `lettering.py` | none | OpenCascade workarounds; haecceity does it right (above). |
| `standards.py` | `standards.rs` | ISO 286, ISO 273, tap drills, pitches, ISO 2768 classes: tables as `const` arrays in Python's order. |
| `materials.py` | `materials.rs` | Suggestion tables, Python's order. |
| (Python's `:g`, `_fmt`, `repr`) | `pyfmt.rs` | Number formatting shared by choices, rules, problems and the report. |
| `choices.py` | `choices.rs` | `describe`, `split`, `code`, `callout`, `interpret`, `check`, `SpecError`. |
| `rules.py` | `rules/` (`mod.rs` Question, Intent, questions, apply, preview, with_all_defaults, IncompleteError; `datums.rs`; `holes.rs`; `turned.rs`) | Pure functions of the analysis JSON and the answers JSON; no geometry. |
| `extras.py` | `extras.rs` | Added requirements, checked and turned into requirements. |
| `general.py` | `general.rs` | ISO 2768-1 deviations and the general dimensions. |
| `locations.py` | `locations.rs` | `along`, `references`, `distance`, `dimensions`, `parse`. `mark_basic` is not ported (basic is `DimTolerance::Basic`). |
| `problems.py` | `problems.rs` | |
| `mates.py` | `mates.rs` | Cylinders and planes per instance, `mates`, `contacts`, `suggestion`. Overlap area of facing planes by a local polygon intersection of the faces' boundaries (stand-in for U5). |
| `mesh.py` | `mesh.rs`, `triangulate.rs` | Mesh format `pmi-assist-mesh` v2 kept. Triangulation is a local stand-in (U4): each face triangulated in its parameter space from its boundary loops and refined to the deflection; a face it cannot do is listed in `missing`, as the format allows. Edges from the edges' samples, seams left out. |
| `anchors.py`, `sightlines.py` | `anchors.rs`, `sightlines.rs` | Anchor point: `face_centre` checked to lie on the trimmed face (stand-in for quiddity's `inspect_face`, U6); sightlines by `Classifier`. The reach (0.12 of the diagonal) uses the exact bbox diagonal, where Python's `serve._diagonal` uses OCCT's loose `BRepBndLib` box: anchor points differ systematically by that ratio, recorded once as a `rust-correct` pattern. |
| `writer.py`, `requirements.py` | `writer.rs` | Intent → one `PartPmi` per part → `pmi::write` (Replace for a bare/legacy resumed part, Add for foreign or mixed PMI) → atomic replace of the output → read back and compare. Report format kept. |
| `resume.py` | `resume.rs`, `pyjson.rs` | Stored answers as a CAx-IF user defined attribute on the part (`AttributeSet` 'pmi-assist answers'); Python-written files are read too. The fingerprint hashes Python's exact `json.dumps` text (`pyjson.rs`); schema 2 written, zeros unsigned (Q14), schema 1 read. |
| `api.py` | `api.rs` | |
| `cli.py` | `main.rs` | Same subcommands, arguments, exit codes (0 ok, 1 write failed, 2 refused/invalid), stdout summaries. |
| `serve.py` | `serve.rs` | Protocol 1, same ops and error codes. Replies on stdout; nothing else writes to stdout. |

## Data types

The JSON formats are the contract with Draftwright Specify, so types serialise to exactly
specify-core's keys, key order and omission rules (`serde_json` with `preserve_order`; fields
`skip_serializing_if` where Python leaves them out). Numbers: integers stay integers, floats are
`f64`; JSON text need not be byte-identical to Python's (`1e-05` against `1e-5` reads the same),
except where a hash is taken (resume fingerprint, `pyjson.rs`).

- `Binding { source_sha256, face_count, loader_version: 2, part }`.
- Analysis (`schema` 1): `{schema, binding, faces: [Face], features: [Feature], mates?,
  existing: [ExistingPmi], part: {material?, general_tolerance?, surface_finish?, coating?,
  heat_treatment?, edges?}, resume?: {answers}}`. `Face { id, kind: plane|cylinder|cone|torus|
  other, area, centroid, direction?, origin?, radius?, contact? }`. `Feature { id, family, record
  (JSON object), faces, members?, notes?, description? }`. Analysis snapshots add plain
  `description` text for turned bodies, face grooves, cylindrical profiles and concave
  fillets. The measured record and source-face indices remain the matching/PMI inputs;
  these descriptions introduce no feature questions.
- Answers: `{question id: JSON value}` (strings, face lists, `extra.*` objects), with
  optional reserved `_specify` evidence and unresolved carry blocks. See
  [answer files](answers.md) for validation and plain-map limitations.
- `Question` (to_dict order and omissions as Python's), `Intent { schema: 1, rules_version: 3,
  binding, part, datums, requirements, general }` with requirements as JSON objects whose kinds
  are `size`, `location`, `position`, `flatness`, `perpendicularity`, `parallelism`, `profile`,
  `runout`, `total_runout`, `thread`, `knurl`, `finish` (Python's keys). The rules work on
  `serde_json::Value` for the analysis and answers, as Python works on dicts, with typed helpers
  for faces and features: the analysis may come from either engine and from older versions.
- Mesh: `{format: "pmi-assist-mesh", version: 2, binding, bbox, positions, indices, faces,
  missing, edges}`.
- Write report: `{output, written, not_written, warnings}`. An answer whose question the rules
  do not ask is an intent row of kind `unasked` with a `not_written_reason`, listed in
  `not_written` (#126; docs/answers.md).
- Errors: `IncompleteError { missing, part }`, invalid answer (`ValueError` in Python) as a typed
  `Invalid(String)`, `SpecError`, verification and write failures; the CLI and `serve` map them to
  Python's exit codes and error codes (`incomplete`, `invalid`, `failed`).
- Stored answers: `{schema, answers, part, face_count, faces}`, compact JSON
  (`separators=(",", ":")`), base64, in a `DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist
  answers','<base64>')`, or haecceity v0.4.3's marked ordered text compound when
  it exceeds the encoded string bound. Chunk text is joined in list order. Named
  parts write schema 3; wholly unnamed parts retain schema 2. Schemas 1–3 are read (compared as Python's
  `==` compares: `1.0` and `true` are schema 1), any other is passed over. A payload resumes
  when its `face_count` is the part's and its `faces` is the part's fingerprint *of the
  payload's own schema* (Q14).

### Saved answers across kernel upgrades

Specify's payload schema remains 2 (schema 1 still reads); it is independent of
quiddity's fingerprint v3. Alpha.6 saved no quiddity feature fingerprints. Do not feed
its data to quiddity's v3 correspondence or assume positional record IDs survive.

On opening a Specify-written STEP, `resume::report_for_parsed` first checks the
payload's part, face count and **ordered geometric face fingerprint**, using its
original schema. This proves the saved face indices still denote the same ordered
surface kinds and rounded centroids. No face-index correspondence is inferred when
that check fails: the file opens as a foreign file, retaining its existing PMI and
part settings, as before this migration. This legacy evidence is deliberately
conservative: it does not migrate topology edits or reordered faces, and cannot distinguish
geometry edits that preserve all the legacy fingerprint's rounded surface kinds/centroids.

For a feature question `question:family:faces`, an exact same-family face support is
preferred. If it disappeared, carry the answer only to a **unique same-family feature
containing all the original geometric support faces**. This permits a recogniser to add
supporting faces (such as a floor) without guessing by dimensions, proximity, family
changes or partial overlap. Hole-pattern kinds must also agree. Multiple candidates,
no candidate, malformed support, multiple old features mapping to one new feature
(even across different question types), or multiple old answers mapping to one new question
are unresolved; colliding answers are never merged, even if their values agree.
Non-feature answers and literal notes keep their values and insertion order. Current
IDs are rederived on every reopening, never reused as quiddity positional IDs.

**Schema 1 under another kernel.** Schema 1 hashes the sign of each zero centroid
component, which is integration noise, and quiddity v0.1.1 gives other signs than the
kernel that wrote the pre-alpha browser exports (the mounting plate: 14 of its 23 zero
components, face 1 `[-0.0, -30.0, 0.0]` then and `[0.0, -30.0, 0.0]` now; its schema 2
fingerprint is the same under both). The stored hash cannot be searched for every sign
(2^23 there), so it cannot be checked. A schema 1 payload stored as this port stores it (a
user defined attribute, not Python's own `PROPERTY_DEFINITION('pmi-assist answers','schema
1',...)`), with the part's face count, whose hash is not reproduced is therefore read as
answers **without face evidence**, as below: they are trusted by feature ID.
`analysis.resume.unverified` says so, the analysis warnings and review `problems`
(`saved_answers_unverified`, and `saved_answer_unresolved` for any answer whose ID this part
lacks) repeat it. Python-written schema 1 files still open as foreign files (Q14). Fixture:
`tests/fixtures/legacy-schema1` (`tests/shared_answers.rs`): `part.material = 6082-T6` and
`hole.function:holes:9 = fit:H7` both resume.

**Answers held outside the STEP.** A caller that keeps answers itself (the editor's
project, draftwright-mcp's hosted part) uses `resume::rematch_answers(loaded, features,
answers, evidence, automatic)` (WASM `rematchAnswers`) instead of its own list of
feature-keyed answer kinds. The sound evidence is the part's face evidence when the answers
were saved: `resume::evidence(loaded)`, `{schema: 2, face_count, faces}`, the same fields
and fingerprint the writer stores with answers in the file (a stored payload serves too).
The fingerprint, not the source sha256, is what proves the saved face indices still denote
the same ordered faces across loader and kernel changes. With matching evidence the answers
are matched exactly as answers stored in a STEP are, by `rematch`.

**Without evidence** (no evidence given: a project saved before it kept evidence, or the
schema 1 case above) the answers are trusted by ID, as the maintainer decided for legacy
projects: no face correspondence is inferred, a feature answer is kept when this part has a
feature with that exact ID, a datum when its faces exist on the part, and anything else is
`unresolved` with its value and a reason. The part's own answers (`part.*`) are kept.
`unverified` says the match was by ID, so callers can surface it, but the kept answers are in
`answers`.

**With evidence that does not match** (another part's, or the part before an edit) the
evidence contradicts the part, so a feature ID or face index that still exists is not taken
as evidence of the same feature: IDs renumber between kernels for the same geometry, and
another part's or an edited part's IDs can coincide. Every answer on a feature (the
feature-keyed kinds) or a datum (`datum.X` and the settings that follow it, `datum.X.flatness`
and `datum.X.control`) is `unresolved` with its value and a reason; only the part's own answers
(`part.*`) are kept, and `unverified` gives the reason for the whole. Such an answer is never
dropped, applied, or attached to another feature: it is reported and left open.

In both cases `automatic` (the editor's `auto`) marks unresolved answers the default path
gave, which need not be reported, and is returned with migrated IDs renamed.

Low-level `answers_for` remain payload readers: they retain the
original keys for clients holding the original analysis and byte-preserving round trips.
Use `api::analyse`/the session, or `report_for_parsed`, to resume against new recognition.

`analysis.resume` contains `answers`, optional `migrated` old/new question pairs and
optional `unresolved` records (`id`, `feature`, `answer`, `reason`). Only `answers` are
applied. CLI/serve warnings and the existing review/session `problems` list also name
each unresolved old ID and reason (`saved_answer_unresolved`). The analysis carries
`existing: []` for a saved session, so old written PMI cannot mask unanswered questions.
Editor users should review these warnings and answer the open questions before saving;
original unresolved values remain available in the resume report for manual recovery.
No defaults are automatically accepted by migration. Rewriting stores the user's
current answers through the existing schema-2 payload. **Unresolved old answers are not
persisted when saving again**; recover their values from the resume report before saving.
Answers left open on contradicting evidence are never applied (above), so a rewrite cannot
launder them: an answer left open becomes verified only by the person answering it, which is
the explicit acceptance. Answers trusted by ID without evidence are applied, and a rewrite
stores them under the fresh schema 2 fingerprint.
Unchanged answer payloads retain
the same compact JSON/base64 bytes.

Compatibility inputs in `tests/fixtures/resume-alpha6` were written and committed with
the alpha.6/alpha.8 build before the dependency bump: five files, 87 accepted answers,
including a genuinely rotated/translated plate. Those 87 answers retain their IDs.
Two additional fixtures were written from the exact alpha.6 tag: FTC-10 keeps all
29 answers (27 exact, two migrated to an expanded linear pattern); CTC-02 keeps 32
of 34 answers and reports two unmatched linear-pattern answers. FTC-10 also opens
two newly recognised-pattern questions, without accepting their defaults. The original
fixtures cover literal Unicode, quotes and STEP-escape-looking backslashes too.

### Recognition coordinates at the Specify boundary

Quiddity 0.2.0-alpha.2 publishes recognition schema 6 (#92). Specify serialises
`PublishedFeatures` for its record payload and reads the published document for defining
faces and hole-pattern variant names; there is no typed `Deref` view. The formerly
oriented slot, chamfer and slot-pattern families are merged into `slots`, `chamfers`
and `slot_patterns` (#92), with file-coordinate letter/vector axes. Plain records come
first. Specify has no separate oriented-family readers. No geometry
reads recognition's top-level `frame`. `_local` and `_frame` are no longer emitted.
Fingerprints remain `quiddity-rust/fingerprint/3`; merged records change fingerprint
values, so re-recognise on upgrade. Golden's 13 pre-upgrade saved answers are all part/datum
settings, not `oriented_slots:*` answers: they resume unchanged by part/face evidence.
Its three old oriented-slot supports are checked against the merged slots by face support
and end-wall geometry; no dimension/order-based answer migration is introduced.

Axis/coordinate consumer audit (native and `crates/specify-core-wasm/src`):

- `features::end_planes` reads slot/channel `long_axis`, `lo`, `hi` and `origin`.
  Without `origin`, letters mean absolute positive file axes. Converted records
  (including mixed letter/vector axes) mean `origin + distance * direction`.
  Missing/invalid axes or origins return `FeatureError::Records`, never omit an end plane.
  Derived `end_direction`, `end_lo`, `end_hi` are also in file coordinates.
  With #100/#91 end-wall claims, `features::refine_owned_end_planes` selects unique
  parallel planes from the feature's own faces near those ends and uses their native
  normals/origins. This avoids both legacy position snapping and sampled prismatic-section
  direction error. Unclaimed or ambiguous ends retain the published-record fallback;
  no plane is borrowed from another feature.
- `rules::datums::recess_planes` uses those published end planes, including when reading
  older analysis JSON without derived fields. Blind-hole floors use file `axis`, `location`
  and scalar `depth`. Both letter and vector axes are accepted. End-plane comparisons allow
  half the published 0.01-mm step plus the geometry floor (0.005001 mm).
- `features::hole_sets`, `rules::holes::hole_key`, `rules::records::Feature::vector`,
  `rules::holes` and `locations` accept letters and vectors for hole `axis`. Hole `location`
  is a file point; grouping and questions read scalar sizes/depths unchanged.
- `rules::turned` reads `hi - lo` only for thread length: a distance invariant under the
  common origin and unit direction. It does not interpret either endpoint as file Z.
- `rules::datums` turned detection, `extras`, `general`, `anchors`, `sightlines`, `mates`
  and the writer use file-space face centroids, normals, cylinder axes/origins or the
  original kernel geometry. They do not decode recognition-frame coordinates.
- `width_axis`, `depth_axis`, `other_axis`, `height_axis`, `thickness_axis`, `x_axis`,
  `y_axis`, `section_axes`, `basis_axes`, nested profile/support/mirror axes, `w_center`,
  `d_lo`/`d_hi`, `z_lo`/`z_hi`, riser positions, levels, spans and body bounds are opaque
  published record data in Specify. They are serialised intact (vectors included), not
  interpreted geometrically. The WASM session/input boundary delegates all recognition
  and rules to these same functions and has no independent axis decoder.

The kernel owns one-time publication, grid rounding and near-axis handling. Specify does
not re-round positions or infer an epsilon frame. Equal-area datum candidates within
max(1e-6 mm², 1e-6 times the larger area) keep face order under rigid motion.
`tests/schema6.rs` checks all three golden 30° slots against actual end walls, rotates
by 30° about Z plus translation and by a generic rotation, and compares questions,
accepted defaults, written/read-back PMI and its face geometry under the same motion.
Converted golden endpoints agree with actual end-wall planes within 3e-6 mm; its published
slot-pattern direction is exactly −X. The same tests check the 18.694-mm, 45° chamfer on
mfcadpp/10252 face 2 against its two overlapping support-edge intervals, and compare all
20 formerly oriented supports in the frozen capture across the family merge. Body integral
uncertainties remain independent of that support-geometry proof. Of the earlier 319
undetermined schema-6 additions, physical support/extent checks reclassify 256 additive
body-level frame fields and 14 oriented boxes as not applicable, and ten 3e-9-mm axis-origin
differences as equivalent; 39 axis/level-selection and body-integral comparisons stay open.

Local upgrade checks select the touched binaries plus features, resume, cross-engine and
WASM, with both corpus paths exported, `CARGO_BUILD_JOBS=6`, `RUST_TEST_THREADS=6`.
The full workspace suite remains CI's check.

Part-21 strings are decoded text in step-io 0.3 (#62): do not decode an attribute twice;
haecceity's `Document::span` continues to provide checked `usize` slices of its own bytes.

### The fingerprint (both engines, byte for byte)

For each face in face order (`describe_faces`), the pair `[kind, [x, y, z]]`: `kind` the face's
kind string, `x, y, z` its centroid's components each rounded to 3 decimals by Python's
`round(c, 3)` (correctly rounded, half to even; `pyfmt::round_half_even`). Then:

- **Schema 2** (written): every component that is zero after rounding is written unsigned:
  `0.0`, never `-0.0` (Python `0.0 if r == 0 else r`; Rust `if r == 0.0 { 0.0 } else { r }`).
  Rounding comes first, so `-0.0004` becomes `0.0`.
- **Schema 1** (read only): the components as rounded, signs of zero kept. Under a kernel
  whose signs of zero differ it is not reproduced; see "Saved answers across kernel upgrades".

The list is written as Python's `json.dumps(faces)` writes it with its defaults: `[` `]`
brackets, `", "` between items and `": "` (unused here), strings as JSON strings with
`ensure_ascii`, each float as `float.__repr__` (shortest round-trip digits, always a `.0` or an
exponent: `5.0`, `-0.109`, `1e+16`); Rust writes the same text with `pyjson::dumps`. The
fingerprint is the lowercase hex SHA-256 of that text's UTF-8 bytes (Python
`hashlib.sha256(text.encode()).hexdigest()`). Example, the plate's first two faces:
`[["plane", [-50.0, 0.0, 0.0]], ["plane", [0.0, -30.0, 0.0]], ...]` where schema 1 had
`[-50.0, -0.0, -0.0]` from OpenCascade and `[-50.0, -0.0, 0.0]` from haecceity.
`resume::fingerprint_of` implements both schemas over such a list; the cross-engine check
(`tests/cross_engine.rs`, `schema_two_fingerprints_are_the_same_in_both_engines`) compares it
with specify-core's own `resume.fingerprint` (branch `fingerprint-v2`) and with the hash computed
in the capture from Python's own faces (`tools/capture/crossengine.py`, `_fingerprint_v2`) on
every part of every file in the check: equal wherever the two engines' rounded faces are (all but
NIST CTC-05, whose face integrals differ, U11).

## What is written, and how it differs

| Intent | Python writes | Rust writes (through haecceity) |
|---|---|---|
| Datums A–H | XCAF datums, one per tolerance; datum symbol appended | One `datum` per letter per part, its `datum_feature` on the faces (a hole's cylinders for an axis datum); datum feature symbol (decision 6) |
| Size with fit | limits only (OCCT writes the grade wrongly) | `DimTolerance::Fit` (`limits_and_fits`), the class as stated |
| Size ± / limits, location ±, location basic | XCAF, then `mark_basic` text | `Deviations`, `Basic` |
| Position (Ø, Ⓜ), flatness, perpendicularity, parallelism, profile, runout | XCAF; text fixes | `GeometricTolerance` with zone, modifier, datum system |
| Material | XCAF material tool | `Material` ('material name', decision 2) |
| General tolerance | appended 'default tolerances' text + 'manufacturing requirement' text | `GeneralTolerance::Class` |
| Threads | appended text route + UDA | `Thread` (side, major Ø, form, class, hand, partial area) + `AttributeSet` (pitch, tapping drill, depths, as haecceity's tests map them) |
| Knurls | appended text route + UDA | `turned_knurl`, straight or diamond: the parameters beyond diameter and pitch (number of teeth, tooth depth, root fillet; a diamond's 30° helix angle, no hand) derived from DIN 82 and stated in the intent with their sources, filled in on write for an intent without them (Q4, Q15) |
| Surface finish on faces, part notes (finish default, coating, heat treatment, edges) | appended text route | a finish as an ISO 10303-1110 `SurfaceTexture` ('any process allowed', Ra in µm) on its faces or the part; coating, heat treatment and edges as 'semantic text' attribute sets on the part, each item named by Python's kind (Q3) |
| Leaders for threads, knurls, finish | appended presentation | none (presentation is out of scope except datum symbols) |
| Stored answers | `PROPERTY_DEFINITION('pmi-assist answers')` text | `AttributeSet` 'pmi-assist answers' on the part, one string item (base64 JSON, payload as Python's) |

Write modes are decided per part. A bare part uses `Mode::Replace` (also dropping stale
Python-form answers, Q18). A part with foreign PMI uses `Mode::Add` and stores supplied
answers and characteristic numbers. Its stored payload adds `pmi_scope` equal to the
[PMI provenance token](pmi-provenance.md), without changing the answer schema. On resume,
marked requirements belong to Specify; unmarked PMI remains in `existing`, answers its
usual questions, and retains every entity byte. A mixed rewrite still uses Add: remove
only marked Specify requirements through the kernel removal planner, then append the edited
intent and saved answers. Foreign dependencies, including shared features and datums, are
protected. Specify's numbers survive value edits; foreign numbers reserve their values.
Legacy resumed payloads without `pmi_scope` still use Replace. Old Add-mode files without
stored answers still open as existing PMI. A marker alone grants no deletion permission.
One assembly part may be written alone, leaving the others byte for byte (D3); parts in a
combined write may use different modes. Assemblies with foreign PMI are supported (Q6).
An AP214/AP203 file gaining PMI becomes AP242 only if every
instance validates (haecceity decision 1); otherwise the write is refused, naming the instances.

Verification: the output is written beside the destination, read back with `pmi::read`, and
compared with what was written by `pmi::write::differences` (add: the part's PMI before plus the
items written; replace: exactly the items written), the stored answers read back, then renamed
over the destination (symlink written through, permissions kept), as Python does. Any difference
leaves the destination and the source untouched (exit 1).

## Oracle and tests

### Capture tool

`tools/capture.py`, run with `/Users/paul/repos/specify-core/.venv/bin/python` (specify-core's
271 tests pass there). It writes compact checked fixtures under `tests/fixtures/oracle/`, gzipped
JSON, one directory per case, each with `meta.json` (input path relative to its root, the input's
sha256, specify-core's and quiddity's versions and git revision, the command run). For each
input part (each part of an assembly):

- `analysis` (`api.analyse`), `parts` for assemblies;
- `questions` with no answers and after `with_all_defaults`, with defaults, bases, reasons and
  attention; `open` ids; `problems`;
- `preview` intent, `apply(accept_defaults)` intent, `general` dimensions;
- scenarios: answer sets taken from specify-core's tests (extras, datums D–H and frames, typed
  specs, ± locating, runouts, fits, threads, knurls, mates) with their questions, intents,
  problems and refusals (the error kind and message);
- `interpret` cases from `test_choices.py` (typed text → value or refusal);
- the write report and the written STEP read back by Python. **The read-back calls Python's
  functions directly, not `api.analyse`**: for a file with stored answers it can resume,
  `api.analyse` returns `existing: []` and `part: {}` (`api.py`, the `answers_for` branch), and
  Python stores answers on every write of a bare part, so every synthetic part's read-back
  would be empty. The capture records, for every written part, `readback.json` = `{existing:
  [e.to_dict() for e in read_existing(load(out, part=k))], part: part_settings(that loaded
  part), resume: resume.answers_for(that loaded part)}` (a call that raises gives `{error,
  message}` in place of its value). The same artifact shape comes from `capture.py readback`
  (stage 4's cross-engine check). For synthetic parts and NIST files only, the written STEP is
  committed gzipped as `written.step.gz` (stages 3 and 4 resume and read Python-written files);
  its bytes are not a parity target;
- mesh summary: face count, triangles per face, `missing`, bbox, per-face mesh area, edge count
  (not the triangles: meshes are checked by properties, not parity);
- anchors for preview groups; a `serve` transcript of one session per synthetic part.

Inputs: the synthetic parts of specify-core's `tests/conftest.py` and of individual tests, built by
`tools/make_parts.py` and committed under `tests/fixtures/parts/`; NIST's AP242 CTC-01 to 05
(public-domain US government works, gzipped and committed as haecceity does); the quiddity corpus
(`QUIDDITY_CORPUS`, default `/Users/paul/repos/quiddity/tests/corpus`, never searched for
elsewhere), not committed: its outputs are committed, its inputs are not. A `readback` mode runs
the same direct read-back (above) on a STEP the Rust port wrote, for the cross-engine check.

**Gzipped corpus files.** Six corpus inputs are gzipped (`cadgenbench_inputs/cgb{202,217,242,
243}.step.gz`, `inventory_refusal/{13975,14052}.step.gz`); Python's engine does not read gzip, and
haecceity's `corpus_upgrade.json` pins the `cadgenbench_inputs` ones as upgradable, so keeping
them as refusals would lose real cases. Decision: both engines see the decompressed file. The
capture tool decompresses to a temporary `.step` and runs Python on it; `meta.json` records the
sha256 of both the `.gz` and the decompressed bytes (the binding's `source_sha256` is the
decompressed one); the Rust harness's `case.input()` decompresses to a temporary file and
returns its path.

**Gzipped STEP files are read and written (maintainer decision 2026-10-15).** The Rust engine no
longer refuses gzip, where specify-core does. A file is gzip when it begins with gzip's magic
bytes (`1f 8b`), whatever it is called; it is decompressed in memory in `part::File::of_shared`,
through which every file and byte load goes (the CLI, serve, `api`, `part::load_bytes`), and is
its STEP text from then on: `binding.source_sha256` is the decompressed bytes' hash, and the
stored-answers fingerprint (of the faces) is unchanged, so a `.step` and its `.step.gz` bind and
resume identically. A gzip that does not decompress is refused as unreadable ("the file is gzip
but cannot be decompressed: …"). The writer writes STEP text unless the destination's name ends
in `.gz`, when the verified text is gzipped (no name, no time stamp); a write in place to a `.gz`
input therefore stays gzipped. `write_bytes` returns STEP text. Tests: `tests/gzip.rs`.

### Comparison harness and known differences

`tests/common/` (oracle stream): load a case, compare a Rust JSON value with the oracle's by path
with numeric tolerances (relative 1e-9 by default, per-field overrides such as areas 1e-6),
collect differences as `{case, path, python, rust}`. Each difference must match an entry in
`tests/fixtures/known_differences/<area>.json`, one file per area (`analysis`, `features`, `existing`,
`rules`, `mates`, `mesh`, `anchors`, `writer`, `cli`), each owned by the stream for that area:

```json
{"case": "corpus/cadgenbench/flanged_spool_132", "path": "features[*].faces",
 "pattern": "holes faces exclude floors", "verdict": "rust-correct | rust-wrong | equivalent | undetermined | not-applicable",
 "evidence": "what was checked: the geometry, the standard, the file text, a test"}
```

An unmatched difference fails the test, and so does an entry that matches nothing (stale). A
`rust-wrong` entry is a bug kept visible, not a pass. `check_known(area, diffs)` is the
asserting form, used once per area by the area's own sweep. The harness also has a
non-asserting `explained(areas, diffs) -> Vec<Difference>` returning only the differences no
entry of the named areas' files matches, with no stale check and no tally: a sweep that crosses
areas filters through earlier areas' files with it, because running their `check_known` there
would report every entry not met in that sweep as stale.

**Feature ids cascade.** Feature ids differ between engines by design (defining versus
constituent faces, U2), and question ids, answer keys, intent requirements and report lines
embed them. So comparisons with Python are made where the inputs are Python's (Python's
analysis and answers into the Rust rules, writer, CLI and serve); Rust-produced analyses are
checked by composition (the CLI equals the library) and by smoke tests (the rules run on them
without panic or refusal), not entry by entry against Python. The cascade is recorded once as a
documented rule in the `cli` area, not per difference. Local tests whose input is a corpus file
skip when `QUIDDITY_CORPUS` does not have it. CI checks out the pinned corpus and sets
`QUIDDITY_CORPUS_REQUIRED=1`, making a missing corpus or captured input fatal.

Python's unit tests are ported where they test behaviour (named after the Python test, citing
it), not where they test an OpenCascade workaround (`test_p21.py`, `test_magnitudes.py`,
`test_conformance.py`'s text fixes, `test_merge.py`'s transplant: their intent is re-expressed
against haecceity, e.g. "the original file is kept byte for byte" for `Mode::Add`). No test is
weakened: a Python assertion that does not hold in Rust becomes a known difference with a verdict.

Each of specify-core's 32 test files appears once below (the stage porting it in brackets).

Ported: `test_load.py` [1 part; its reading parts 2 features], `test_standards.py`,
`test_materials.py`, `test_choices.py` [1 tables], `test_features.py` [2 features],
`test_rules.py`, `test_datums.py`, `test_general.py`, `test_locations.py` (minus `mark_basic`),
`test_locating_problems_extras.py`, `test_attention.py`, `test_basis.py`, `test_call_out.py`,
`test_same_as.py`, `test_profile_runout.py` [2 rules], `test_threads.py` [2 rules; writing 3],
`test_mmc_finish_notes.py` [2 rules; reading 2 features; writing 3], `test_preview.py` [2 rules;
anchors 4],
`test_mates.py` [2 mates], `test_mesh.py`, `test_anchors.py` [2 mesh-anchors; preview and serve
parts 4], `test_writer.py`, `test_requirements.py` (semantic content), `test_resume.py`,
`test_merge.py` (re-expressed for `Mode::Add`), `test_drawn.py` (datum symbols),
`test_conformance.py` (re-expressed, below) [3 writer], `test_assembly.py` [1 parts listing;
2 mates; 3 writes; 4 API and CLI], `test_serve.py` [4].

Not ported: `test_p21.py` (escapes: haecceity writes them), `test_magnitudes.py` (OCCT magnitude
text scans), `test_nist_coverage.py` (tests `tools/nist/coverage.py`, tooling: revisit with
that tool, see "later").

`test_conformance.py`: kept are "every written file passes the schema check" (haecceity
validates every written instance; the test asserts the write report and a clean express check)
and "what was asked is what is read back". Dropped: its assertions on OpenCascade text fixes
(runout zones dropped, type limits, simple tolerances, part datums, the metre scale) and on the
text routes for notes, threads and knurls, which this port does not write (each a
`not-applicable` entry in `writer.json` naming the assertion), and its use of
`tools/conformance`'s generated corpus (that tool is in "later").

**JSON text.** Documents are compared parsed, not as text. Two text-level differences are
expected and recorded by the CLI stream up front: Python writes the analysis file with
`json.dumps(indent=1)` and `ensure_ascii` on, escaping µ, × and ⌀, where `serde_json` writes
UTF-8 (`equivalent`: same parsed value); Python can write `NaN`/`Infinity` tokens where a value
escapes `features._plain`, and `serde_json` writes `null` (`rust-correct`: RFC 8259 has no such
tokens and a JavaScript client's `JSON.parse` rejects them).

## Out of scope for now

- **Presentation** other than datum feature symbols (leaders for threads, knurls and finish):
  haecceity decision 6; the semantics are written, and Draftwright draws from semantics.
- **PMI on assembly occurrences**: haecceity reports it as a finding; specify-core never writes it.
- **The Draftwright round-trip tool** (`tools/draftwright_roundtrip.py`): it runs in
  Draftwright's environment and reads Python's text route, which this port does not write.
- **NIST coverage tooling** (`tools/nist/coverage.py`) and the conformance corpus maker: tools
  around specify-core, not its engine; revisit after stage 4.
- **Inch-specific defaults**: specify-core gives metric defaults only; kept.

## Upstream needs

Recorded precisely, with stand-ins, in [upstream-needs.md](upstream-needs.md): U1 a part of an
assembly as a `Part` in its own frame; U2 constituent faces and pattern members from quiddity;
U3 a standard form for notes and surface finish in `pmi::write`; U4 face triangulation; U5
overlap area of two coplanar faces; U6 a point proved on a face's trimmed domain; U7 the
read-back verification of `quiddity pmi write` as a library function; U8 thread semantics for
pitch and 'number of threads'; U9 part names over the assembly structure and one read; U10
specify-core's own notes on faces as anchored items; U11 `face_moments` on every face to 1e-6;
U12 the feature families quiddity-rust does not recognise (ten at a16d7af, three since the move); U13 simple parameter-space loops
at degenerate corners.

## Status

All five planned stages are delivered: every module of specify-core is ported, composed by the
API, the command line and `serve`, and checked against outputs captured from specify-core
(Python). What follows states what the tests verify, as of stage 5 (consolidation). The known
differences are generated into [known-differences.md](known-differences.md) by
`tests/known_differences_doc.rs`, which fails when the document is not the rendering of
`tests/fixtures/known_differences/*.json`.

Stage 5 was integrated into `main` on 2026-10-09. It delivered the generated known-differences
document, this Status section and the README's usage and differences. On the merged tree
`cargo fmt --all --check` and `cargo clippy --all-targets --release -- -D warnings` are clean,
and `SPECIFY_CORPUS=... cargo test --release` passes all 302 tests. The dependencies have since
moved from rev a16d7af to `prototype/rust-port` (5798c59, Q2), with every oracle comparison
rerun and the changed differences rejudged; that move was integrated into `main` on 2026-10-09,
where fmt and clippy are clean and all 302 tests pass with `SPECIFY_CORPUS` set. Three of the maintainer's
decisions were then integrated into `main` on 2026-10-09: stored answers schema 2 (Q14, Rust
engine), ISO 286 to 3150 mm (Q8, Rust engine; the comparison with Python above 500 mm waits for
specify-core's extension) and writing the rest of an intent whose items name no faces (Q17).
On that tree fmt and clippy are clean and all 312 tests pass with `SPECIFY_CORPUS` set. Two
more were integrated on 2026-10-10: the knurl derivation (Q4 and Q15; straight and diamond
knurls are written through `turned_knurl`, questions.md Q15 status) and haecceity's writer
decisions with the pin moved to 10884b8 (notes and finish written in standard forms, Q3;
OpenCascade opens every Rust-written file, Q16; U14's stand-in removed). On that tree fmt and
clippy are clean and all 317 tests pass with `SPECIFY_CORPUS` set. On 2026-10-10 the
comparisons were recaptured against specify-core's branches `iso286-3150` and `fingerprint-v2`
(questions.md Q8 and Q14 status, "Recapture source"): the ISO 286 and choices grids, the NIST
CTC-05 oracle cases and the cross-engine fixtures. That recapture was integrated into `main` on 2026-10-10;
on that tree fmt and clippy are clean and all 318 tests pass with `SPECIFY_CORPUS` set.

### Implemented and verified, per module

Counts of differences are those each area's sweep prints with `SPECIFY_CORPUS` set (corpus
cases skip without it); each difference matches an entry with a verdict, and an unmatched
difference or a stale entry fails the sweep.

| Module | Verified by | Against Python |
|---|---|---|
| Oracle and harness (`tools/capture.py`, `tools/capture/`, `tests/common`) | `tests/oracle_harness.rs` (the harness on hand-made values; every case's input is the file it was captured from) | Fixtures for the 11 synthetic parts, the quiddity corpus cases and the NIST AP242 files, plus scenarios transcribed from specify-core's tests |
| `part.rs` (load, parts, binding at `loader_version` 2) | `tests/part.rs` | Face numbering equals Python's on every synthetic, corpus and NIST part checked except corpus `inventory_refusal/13975` (Q10, Q13); NIST products with surface models are one part (Q7) |
| `standards.rs`, `materials.rs`, `choices.rs`, `pyfmt.rs`, `pyjson.rs` | `tests/standards.rs`, `tests/materials.rs`, `tests/choices.rs`, `tests/pyfmt.rs` | Equal to Python-generated tables and sweeps (`choices_oracle.json.gz`, `pyfmt.json`) |
| `features.rs` | `tests/features.rs` | 120 corpus and NIST parts, none refused as degraded: 19945 differences since the move to 5798c59 (14661 undetermined, most of them the vertices and triangles of three sheet-metal flat patterns; 3900 rust-wrong, 1161 rust-correct, 211 equivalent, 12 not-applicable; 8652 on a16d7af), each face value judged against OpenCascade's adaptive integral; at 5d39eee quiddity-rust's reconciliation removed 252 of them (Rust-only features, 158 of them rust-wrong blend chains; U2) and added none; unchanged at 10884b8 |
| `existing.rs` | `tests/existing.rs` | 120 inputs and 17 written parts: 230 differences (208 rust-correct, 22 not-applicable) |
| `rules/`, `extras.rs`, `general.rs`, `locations.rs`, `problems.rs` | `tests/rules_oracle.rs`, `tests/rules.rs`, `tests/datums.rs`, `tests/general.rs`, `tests/locations.rs`, `tests/extras.rs`, `tests/problems.rs`, `tests/attention.rs`, `tests/basis.rs`, `tests/call_out.rs`, `tests/same_as.rs`, `tests/profile_runout.rs`, `tests/threads.rs`, `tests/mmc_finish_notes.rs`, `tests/preview.rs`, `tests/rust_analyses.rs` | Equal to Python on all 994 captured artifacts (from Python's inputs: 252 question sets, 116 file writes, 98 scenarios); `rules.json` holds only the knurl parameters Python's intent lacks (Q4, Q15); the fits above 500 mm (Q8) are compared with specify-core's `iso286-3150` (NIST CTC-05 recaptured). The rules also run on the Rust engine's own analyses of every synthetic part (`tests/rust_analyses.rs`) |
| `mates.rs` | `tests/mates.rs` | 18 multi-part parts: 7 differences (6 not-applicable, Q7; 1 rust-correct) |
| `mesh.rs`, `triangulate.rs` | `tests/mesh.rs` | 120 cases, 1 312 080 triangles: 1048 differences (553 equivalent, 419 rust-correct, 37 undetermined, 27 not-applicable, 12 rust-wrong); meshes judged by properties (areas, boxes, edges, missing faces), not triangle by triangle |
| `anchors.rs`, `sightlines.rs` | `tests/anchors.rs` | 126 cases, 719 anchors: 265 differences (133 equivalent, 125 rust-correct, 6 not-applicable, 1 undetermined; 259 before NIST CTC-05 was recaptured) |
| `writer.rs`, `resume.rs` | `tests/writer.rs`, `tests/resume.rs`, `tests/assembly_write.rs`, `tests/write_oracle.rs` | 138 writes, from Python's intents, read back by the Rust reader: 93 differences after NIST CTC-05 was recaptured (78 rust-correct, 15 not-applicable, none undetermined or rust-wrong; 91 at 10884b8 with knurls written; 95 before the knurls, 713 before the Q17 integration and the notes written in standard forms, Q3); every file `tests/writer.rs` writes passes haecceity's EXPRESS schema and rule checks |
| `api.rs`, `main.rs`, `serve.rs` | `tests/api.rs`, `tests/cli.rs`, `tests/serve.rs` | The binary's `analyse`, `mesh`, `questions` and `write` against the library's results (exactly) and against Python's command line and serve transcripts (`cli_oracle.json.gz`): 50 differences (24 rust-correct, 15 equivalent, 11 not-applicable; 55 before NIST CTC-05 was recaptured); `test_serve.py` and `test_assembly.py`'s API and command-line tests ported |
| Cross-engine (`examples/write_fixtures.rs`, `tools/capture/crossengine.py`) | `tests/cross_engine.rs` | Python reading every Rust-written synthetic and NIST file, and the Rust engine reading the 11 Python-written synthetic files: 53 differences with specify-core's `fingerprint-v2` (44 rust-correct, 6 undetermined, 2 not-applicable, 1 rust-wrong: NIST CTC-05's fingerprint, U11), 15 Rust-written parts resumed by Python with the same answers; 66 at 10884b8 before specify-core read schema 2 |

Known-difference entries by verdict (entries, not differences): 3898 in all, 950 rust-correct,
1431 rust-wrong, 77 equivalent, 1392 undetermined, 48 not-applicable, after the recapture against
specify-core's `iso286-3150` and `fingerprint-v2` (3904 at 10884b8 with the notes and knurls
written; 4164 after the Q8, Q14 and Q17 integration; 4152 after the move to `prototype/rust-port` 5798c59; 3634 at stage 5, on a16d7af). The whole suite
(`SPECIFY_CORPUS=/Users/paul/repos/quiddity/tests/corpus cargo test --release`, 318 tests
since the recapture against specify-core's branches) passed at stage 5, again after the move to
`prototype/rust-port` 5798c59, after the Q8, Q14 and Q17 integration, and after the knurl and
notes integration on 2026-10-10; and after the recapture against specify-core's branches was
integrated on 2026-10-10. The command line was also run by hand at stage 5 on `tests/fixtures/parts/plate.step` and `assembly.step`
(every README command; exit codes as documented).

### Stand-ins in use

What quiddity-rust does not yet provide, with the local stand-in (details and the "met when"
in [upstream-needs.md](upstream-needs.md)) and the parts each affects in the oracle:

| Need | Stand-in | Affects in the oracle |
|---|---|---|
| U1 part of an assembly in its own frame | None: met at quiddity-rust 0d601d2 (`StepFile::part`); the stand-in is gone | The 5 parts of `parts/assembly` and `parts/stack` are haecceity's own `Part`s, checked against their first instance in the whole read (`tests/part.rs`, `parts_of_assemblies_carry_their_instances_unresolved_lists`) |
| U2 constituent faces, pattern members | Defining faces; members matched to hole records by equality | Feature ids can still differ from Python's (the cascade, `cli.json`); remaining constituent-face and member-link departures are in `features.json`. #100 now claims drill-point cones, rectangular slot end walls and proved polygonal-boss free-end caps |
| U3 notes and surface finish | None: met at 10884b8, written in standard forms | specify-core does not read the surface texture (`cross_engine.json`, rust-correct) |
| U4 face triangulation | `triangulate.rs` (constrained Delaunay in parameter space, `spade`) | Every mesh; faces listed in `missing`: cgb202 face 399 (U13), the zero-area thread end faces of `threaded_connector_109` and cgb242 face 483, and `inventory_refusal/14052` face 1 (a hole crossing its outer loop) (`mesh.json`) |
| U5 overlap area of coplanar faces | Polygon intersection of the faces' loops (`i_overlay`), arcs resampled exactly | Contacts of every assembly part: the 18 multi-part parts the mates sweep compares; equal to Python's 3 decimals on `parts/stack` |
| U6 a point on the trimmed face | `face_centre` if inside the face's domain, else the largest triangle's centroid, else no anchor | Every anchor (719 on 126 cases); NIST FTC-07 faces 185-187, where Python's point is off the trim (`anchors.json`, rust-correct); NIST CTC-05 AP242's two cylinders labelled from another point on them (`anchors.json`, equivalent) |
| U7 read-back verification as a library function | The comparison rebuilt from `pmi::read` and `pmi::write::differences` | Every write (136 oracle writes, `tests/write_oracle.rs`) |
| U8 thread pitch and 'number of threads' | haecceity's own test mapping; pitch, drill and depths in an `AttributeSet` | Every written thread |
| U9 names over the assembly structure, one read | None: met at quiddity-rust 0d601d2 (`PartDefinition.name`, `product_id`, `StepFile` read once); the stand-in is gone | Every load; `parts/wrapped` named "assembly"; NIST FTC-08 and CTC-04 RD named by the product id |
| U10 specify-core's notes on faces | Its `GEOMETRIC_ITEM_SPECIFIC_USAGE`s read from the `p21::Document` | Reading the 11 Python-written synthetic files (`tests/existing.rs`, `tests/cross_engine.rs`) |
| U11 `face_moments` on every face | None: a face without moments refuses the part; accuracy reported value by value | None refused since 5798c59 (cgb217 face 29, refused at a16d7af, now has moments); 1407 face values `rust-wrong`, 1167 `undetermined` (`features.json`); the schema 2 fingerprint of NIST CTC-05 AP242, whose rounded centroids differ on eight faces (`cross_engine.json`, rust-wrong) |
| U12 families not recognised | None: absent from the analysis | One `rust-wrong` entry per remaining family (`section_recesses`, `risers`, `step_levels`: 853 features) in `features.json`; 2740 features at a16d7af, ten families |
| U13 simple parameter-space loops | Closed freeform directions unwrapped; a loop still crossing or jumping refuses the face | cgb202 face 399, listed in `missing` (`mesh.json`, rust-wrong); cgb242 faces 715 and 726 triangulate since 5798c59 |
| U14 replace of a thread haecceity wrote | None: met at 10884b8, an ordinary replace | Every rewrite of a resumed part with a thread (`tests/resume.rs`) |
| U15 OpenCascade crashes on datum symbols | None: met at 10884b8 (no relationship written, Q16) | None: specify-core loads every Rust-written file |

### Open rust-wrong and undetermined work

Listed entry by entry in [known-differences.md](known-differences.md), "Open work"; in short:

- **features** (1418 rust-wrong, 1386 undetermined entries): face areas and centroids off the
  adaptive integral (U11: 1407 rust-wrong, 1167 where neither engine is within 1e-6); the three
  missing families (U12); 158 blend chains Python supersedes by its fillets (U2's
  reconciliation); constituent faces (U2); families or features one engine reports and the
  other does not (among them the recess source families Python's evidence view leaves out),
  and small differences in feature records (locations up to 1.85e-3 mm, body keys up to 2e-5
  relative, 3e-4 on cgb217, blend points up to 0.023 mm, sheet-metal flat patterns tessellated
  more coarsely), undetermined; corpus `inventory_refusal/13975`'s face order (Q10).
- **mesh** (12 rust-wrong, 4 undetermined): a freeform face not triangulated (cgb202 face 399,
  U13); single inward triangles of at most 7.3e-6 mm2 on eight faces at degenerate corners
  (cgb202, cgb207, cgb242); refinement stopping at its round cap on cgb243 face 537; NIST CTC-05 AP242
  edges, the folded surface and the face order of `inventory_refusal/13975` (Q13), undetermined.
- **anchors** (1 undetermined): a leader from `inventory_refusal/14052`'s invalid face.
- **cross_engine** (1 rust-wrong, 1 undetermined): the schema 2 fingerprint of NIST CTC-05,
  whose rounded face centroids differ where haecceity's face integrals are off (U11); Python
  does not read AP242 threads (Q20). Resume (Q14): specify-core (`fingerprint-v2`) resumes the
  Rust writer's schema 2 answers, and its own schema 2 fingerprint equals the Rust engine's on
  every other part; the oracle's schema 1 files fail in Rust on signs of zero, as the rule says.
- **rules, existing, mates, writer, cli**: none (the writer's three knurl entries went with Q4
  and Q15).

### Open questions

The remaining questions and interim behaviour are recorded in [questions.md](questions.md):

- **Interchange between engines:** Q1 (mixing engines on one analysis), Q10 and Q13 (a file whose
  face order OpenCascade changes: corpus `inventory_refusal/13975`), Q14 (resume across engines:
  schema 2 in both engines, compared), Q20 (what Python reads of a
  Rust-written file: fits without limits and threads remain).
- **What is written:** Q6 (an assembly with foreign PMI written by add mode), Q17
  (requirements on no faces listed in `not_written`, the rest written), Q18 (bare parts by `Mode::Replace`; a second material in
  add mode refused).
- **Engine behaviour:** Q2 (historical branch move; superseded by the public framed API in v0.1.1),
  Q7 (a product with surface models: one part), Q8 (ISO 286 extended to 3150 mm, in both engines, compared), Q9
  (malformed input refused as invalid), Q11 (an undecided sightline sample blocks), Q12 (mesh
  density, about 1.8 times Python's triangles), Q19 (unmeasurable contacts as warnings).

### What remains

- **Out of scope, as above:** presentation other than datum feature symbols; PMI on assembly occurrences; the Draftwright
  round-trip tool; the NIST coverage tooling and `test_nist_coverage.py`; the conformance corpus
  maker; inch-specific defaults.
- **Waiting on quiddity-rust:** each need in [upstream-needs.md](upstream-needs.md); when one
  is met, its stand-in is replaced and the affected area files' entries go stale, which the
  sweeps report.
- **Not captured by the oracle** (`tests/fixtures/oracle/README.md`): scenarios whose input is a
  file Python wrote (built instead from the committed `written.step.gz` files by
  `tests/resume.rs`, `tests/writer.rs` and `tests/cross_engine.rs`), an assembly write with
  different answers per part through Python, and tests of internals on hand-made face dicts.
- **CI and the corpus:** Ubuntu and macOS test the full workspace with Quiddity's corpus pinned
  to `6d544e060cd0eeb4442d2196e5df757b5a15ea55`, matching quiddity-rust's suite. Both
  `QUIDDITY_CORPUS` and `SPECIFY_CORPUS` point to the sparse checkout;
  `QUIDDITY_CORPUS_REQUIRED=1` prevents silent skips. Local runs without the switch can still
  omit the corpus (Q5, decided 2026-10-20). Live Python/OpenCascade checks remain opt-in.
