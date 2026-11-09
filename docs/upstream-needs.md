# What specify-core-rust needs from quiddity-rust

quiddity-rust (quiddity and haecceity) is owned by other runs and is not modified from here.
Each need below says what is missing, where specify-core uses it, the stand-in this crate uses
meanwhile (honest: it refuses or reports where it cannot be sure), and how to tell the need is
met. Checked against quiddity-rust branch `ap242` at a16d7af and `prototype/rust-port` at
ef87a63 (2026-10-08); U11 and U12 rechecked at `prototype/rust-port` 5798c59, the pin since the
move (2026-10-09, question Q2).

## U1. A part of an assembly as a `Part` in its own frame

**Status (2026-10-15): met** (quiddity-rust PR #10, rev 0d601d20, haecceity's assemblies slice 1,
pinned by rev): `StepFile::part(i)` / `read_part` give the part's own `Part`, faces in the definition's
order, sources and unresolved lists kept. `part.rs` loads every part through `part::File` (on
`StepFile::read`); `extract_part`, the placed-back read and the rebuilt path are deleted, and no
part is rebuilt or degraded any more (`tests/part.rs`,
`parts_of_assemblies_carry_their_instances_unresolved_lists`). The text below is the need as it
was recorded.

**Missing.** `haecceity::step::read_part_definitions` gives each distinct part's face `#N`s in
specify-core's numbering and its placements, and `read_step` gives one `Part` of every placed
instance. Nothing gives the `Part` of one distinct part, in its own coordinates, with its faces
in `PartDefinition.faces` order: what specify-core's `load(path, part=i)` gives (the solid's own
frame, the frame the analysis, mesh, anchors and mates use).

**Used by.** `part.rs` (`load`, `load_all`), and so features, mesh, anchors, mates for every
assembly and every part stored as an assembly holding one solid (all NIST files).

**Stand-in.** `read_step_placed(bytes, inverse of the part's first placement)`, then the faces
whose `face_source` is in that placement's instances, reordered to `PartDefinition.faces` and
rebuilt with `Part::new` (edges and loops re-indexed). Refused, naming the part, if any face of
the definition is not found exactly once or a placement is not invertible.

**Met when** haecceity offers e.g. `read_part(bytes, &PartDefinition) -> Part` (or the
`PartDefinition` carries it), faces in the definition's order, its face sources kept.

**Also lost by the stand-in.** `Part::new` sets `unresolved_faces`, `unresolved_edges`,
`face_sources` and `edge_sources` empty, and `Part::with_sources` / `Part::with_unresolved` are
`pub(super)`, so a crate outside haecceity cannot restore them on a rebuilt `Part`. `solid_is_valid`
(`brep.rs`) reads `unresolved_faces`, and many quiddity recognisers filter on it, so a rebuilt
part with an unresolved surface would claim valid solids. Meanwhile `LoadedPart` carries the
re-indexed unresolved lists and `read_step`'s own `Part` is used whenever its faces already match
the definition; a rebuilt part with unresolved faces or edges is reported degraded and not
recognised. **Also needed:** a public way to build a `Part` keeping its sources and unresolved
lists (e.g. `pub` `with_sources` / `with_unresolved`), or `read_part` above, which makes it moot.

## U2. Constituent faces and pattern members from quiddity

**Missing.** Python quiddity's evidence view gives a feature's *constituent* faces (the faces
that physically belong to it: a hole's bore and floor) and a pattern's member features.
`quiddity::Features.defining` gives *defining* faces only (`pin.step`: Python `holes:3.5`, Rust
`[3]`), and `HolePattern` holds copies of its member `HoleRecord`s, not their indices. Python's
cross-family reconciliation (which drops `bosses` on turned parts and adds `step_levels`) is not
ported either (`features/mod.rs`: "the Python aggregate's cross-family reconciliation is not
ported").

**Used by.** `features.rs` (feature faces and ids, hole sets), `general.rs` (a hole's floors and
counterbores are excluded from the part's plane levels: with defining faces only, a blind hole's
floor becomes a plane level and gains a general dimension), `rules` (`_bores` filters to the
bore anyway, so hole questions are unaffected).

**Stand-in.** Defining faces; pattern members matched to hole records by equality (the first
not yet taken), as quiddity's own `fingerprint::derived_faces` does. The extra plane levels are
known differences with verdicts.

**Met when** quiddity exposes per record its constituent faces (or its `Occurrence.context`
split into belonging and consulted) and per pattern its member indices.

**Status (2026-10-09, pin 5d39eee; unchanged at 10884b8): reconciliation partly met upstream.** quiddity-rust's
`recognise()` now reconciles the aggregate (its a02b758): with no change here, 252 features
entries went stale, every one a feature Rust reported and Python did not (158 blend chains
Python supersedes by its fillets, 45 bosses, 24 fillets, 13 chamfers, 7 prismatic pockets, 3
slots, 2 plates); no new difference appeared. Constituent faces and pattern members are still
missing.

## U3. Notes and surface finish in `pmi::write`

**Missing.** `pmi::write` refuses notes (`docs/step-ap242.md` decision 5: standard forms only,
none chosen yet). specify-core writes a part's surface-finish default, coating, heat treatment
and edge note, and a finish on faces.

**Used by.** `writer.rs`. The part-note questions have defaults, so almost every intent has
them.

**Stand-in.** Not written; each is listed in the write report's `not_written` (the field Python
has for intent with no place in the file), never dropped silently. See questions.md Q3.

**Met when** `PartPmi.notes` (and a surface texture on faces) are written in a standard form and
read back.

**Status (2026-10-09): met** (quiddity-rust c228f95, pinned at 10884b8): `SurfaceTexture` and
'semantic text' attribute sets are written and read; src/writer.rs writes every finish and part
note through them and nothing is listed in `not_written` for them.

## U4. Face triangulation

**Missing.** haecceity has point clouds on faces (`Part::face_cloud`) but no triangulation.
specify-core's `mesh` (and `serve`'s `analyse`, which returns the mesh) needs triangles per face,
wound outward, at a chordal deflection of 0.001 of the part's diagonal and 0.3 rad.

**Used by.** `mesh.rs`.

**Stand-in.** `triangulate.rs`: each face triangulated in its parameter space from its boundary
loops (`uv_loops`), refined until the deflections hold, mapped to 3-D on the exact surface,
wound by the face's outward normal. A face it cannot triangulate is listed in the mesh's
`missing` (the format's own way of saying so). Checked by properties: every triangle's centroid
maps into the face's domain, the triangles' area is within 1% of `face_moments` area, normals
outward.

**Met when** haecceity offers `Part::triangulate(face, deflection, angular) -> (points,
triangles)`.

## U5. Overlap area of two coplanar faces

**Missing.** `mates.contacts` needs the area two facing coplanar faces of different parts share
(Python: `BRepAlgoAPI_Common` and its surface area). haecceity has no boolean of faces.

**Used by.** `mates.rs` (contacts, and so datum A's default in an assembly).

**Stand-in.** The two faces' boundary loops (edge samples, outer and inner) projected into their
common plane and intersected as polygons with holes (a polygon-boolean crate); the area of the
result. Curved boundaries are sampled at the edge samples' density, so the area is approximate
(reported to 3 decimals as in Python; differences are known differences with verdicts).

**Met when** haecceity offers the common area of two planar faces.

## U6. A point proved on a face's trimmed domain

**Missing.** specify-core's anchors use Python quiddity's `inspect_face(face).anchor`, a point
proved to lie on the face's real trim. haecceity's `Part::face_centre` is build123d's
`Face.center()` (the middle of the parameter range on a curved face), which can fall outside a
trimmed face.

**Used by.** `anchors.rs`.

**Stand-in.** `face_centre`, accepted only if the face's `FaceDomain::contains` its parameters;
otherwise the centroid of the face's largest triangle from the U4 stand-in; otherwise no anchor
for that face (the page falls back to its own guess, as specify-core documents).

**Met when** haecceity offers a point on the trimmed face with its parameters.

## U7. Read-back verification as a library function

**Missing.** `quiddity pmi write`'s check (add: the part's PMI before plus exactly the items
written, a feature or datum equal to an existing one being that one; replace: exactly the items
written; other parts and findings unchanged) is in `src/bin/quiddity.rs` (`verify`,
`item_keys`), not in the library.

**Used by.** `writer.rs`.

**Stand-in.** The same comparison rebuilt here from `pmi::read` and
`pmi::write::differences`, with the add-mode merge rule restated and tested.

**Met when** quiddity or haecceity exports it.

## U8. Thread pitch and 'number of threads'

**Missing.** `pmi::Thread` has no pitch, and what 'number of threads' (a ratio) counts is open
in haecceity (`docs/step-ap242.md`, stage 2: "Open: what a thread's 'number of threads'
counts"). haecceity's own test of specify-core intents writes `number_of_threads` 1, `form`
"M", the designation as `qualifier`, and the pitch in an attribute set.

**Used by.** `writer.rs` (internal and external threads).

**Stand-in.** haecceity's test mapping, exactly; the pitch, tapping drill and depths in an
`AttributeSet` on the thread's feature; `through` as the string "true" (Python's UDA form; the
writer refuses booleans).

**Met when** haecceity settles the meaning of 'number of threads' and where the pitch goes.

## U9. Part names over the assembly structure, and one read for parts and geometry

**Status (2026-10-15): met** (quiddity-rust PR #10, rev 0d601d20): `PartDefinition.name` is the
XCAF-style display name and `PartDefinition.product_id` the product's id; `StepFile::read`
parses the file once for the parts and their geometry. `part_names` is deleted; part loading
parses the file once (it parsed it twice and read it a third time as Part 21). The analysis
(`api::analyse_reporting_in`, so `analyse --part N` and serve) now takes the part definitions
from that `part::File` and parses the file as Part 21 once for both the stored answers and the
PMI (`resume::answers_for_parsed`, `existing::read_parsed`); it read the definitions again and
parsed the file up to three more times. The Part 21 parse remains because `StepFile` does not
expose its `p21::Document`, which `pmi::read` takes. The text below is the need as it was
recorded.

**Missing.** `PartDefinition.name` is the product's `name` as written (escapes unresolved), and
haecceity exposes no assembly structure: which product definitions are assemblies and which
components each holds. specify-core names a part after an assembly that holds it alone (an
exporter's wrapper: `tests/fixtures/parts/wrapped.step` is "assembly", not "plate"), and
OpenCascade names a product by its `id` when its `name` is empty (NIST FTC-08 and CTC-04 RD:
"NIST PMI FTC 08 ASME1"). Separately, `read_part_definitions` reads the whole file and drops the
`Part` it built, so loading a part reads the file twice (`read_part_definitions`, then
`read_step` or `read_step_placed`).

**Used by.** `part.rs` (`parts`, `load`, `load_all`: every part's name; every load's time).

**Stand-in.** Names from a `haecceity::p21::Document` of the same bytes: the
`NEXT_ASSEMBLY_USAGE_OCCURRENCE`s give each assembly's components (depth first from the roots,
components in file order), `PRODUCT_DEFINITION` → `PRODUCT_DEFINITION_FORMATION` → `PRODUCT`
gives the name, the id when the name is empty, escapes resolved by `p21::decode`. A component
that XCAF makes of a product's several shape representations (NIST files, named `SOLID` by
OpenCascade) is not an assembly here: such a part is named by its product. The file is read
twice, plus the `p21` parse.

**Met when** `PartDefinition` carries its display name (or its product's id and the assembly
structure with names), and one call returns the `Part` with the definitions (or `read_part`
of U1, reading once).

## U5, as built (mates stream)

The stand-in in `src/mates.rs` (`overlap`, `edge_points`) intersects the two faces' loops with
the `i_overlay` crate (even-odd fill, outer and inner loops alike), in the first face's plane.
Circular arcs are resampled from their exact curves to a sagitta of 1e-9 of their radius (made
only for faces whose boxes already meet), so straight and circular boundaries give the exact
area to Python's 3 decimals (the stack's 2254.701 and 28.274, checked against hand-computed
areas in `tests/mates.rs`); ellipses and splines use haecceity's edge samples (2e-4 mm chord
tolerance) and may differ in the last digit. A loop that does not close (consecutive edges or its
ends more than 0.01 mm apart) or runs along an unresolved edge is refused: no contact for that
pair, the reason from `mates::contacts_with_refusals`. Nothing more is needed from haecceity
than U5 as stated above.

## U10. specify-core's notes on faces, as anchored items

**Missing.** specify-core (Python) writes a thread, knurl or surface finish on faces by its own
text route: a part-level `PROPERTY_DEFINITION('manufacturing requirement', <kind>, ...)` (which
`pmi::read` reads as a `Note` with no owner) and a `SHAPE_ASPECT(<kind>)` whose
`GEOMETRIC_ITEM_SPECIFIC_USAGE`s, named `<kind>` ('internal thread', 'external thread', 'knurl',
'surface finish', 'surface texture'), name the faces. Nothing semantic refers to that aspect (only
a presentation link and, for threads and knurls, a user defined attribute), so `pmi::read`
reports no note on those faces.

**Used by.** `existing.rs` (`read_existing`'s notes): the rules treat a face with a thread or knurl
note as covered and do not ask for its size.

**Stand-in.** The `GEOMETRIC_ITEM_SPECIFIC_USAGE`s of those names are read from the
`haecceity::p21::Document` (name and identified item), each face mapped through
`PartDefinition::face_index`, exactly as Python's `existing.notes` reads its own text; threads
and knurls `pmi::read` holds semantically (`Thread`, `Knurl`) are added. Checked against
Python's read-back of every file it wrote (`tests/existing.rs`) and a live write of the plate
with a face finish.

**Met when** `pmi::read` reports these notes with their faces (for instance a `Note` on the
aspect's feature), or specify-core-rust no longer needs to read Python-written files.

**Status (2026-10-09): met upstream, stand-in still in use** (quiddity-rust 86b11ba, pinned at
10884b8): `pmi::read` puts each such note on the faces of the aspect written after it
(`Note.on`). `existing.rs` still reads the usages itself; moving it onto `Note.on` is the
existing-reader's change, not made by the writer stream that moved the pin (all of
`tests/existing.rs` and `tests/cross_engine.rs` pass unchanged at 10884b8).

## U11. `Part::face_moments` on every face, to 1e-6

**Missing.** `describe_faces` gives each face's area and centroid from `Part::face_moments`.
(a) At a16d7af it returned `None` for corpus `cadgenbench_inputs/cgb217.step` face 29 (`#2586`,
a B-spline surface), so `describe_faces` refused that part; at 5798c59 it gives a value (area
49.9296; Python 49.9176; adaptive reference 50.0848: neither within 1e-6), so (a) is met and
cgb217's faces are compared like every other part's. (b) Its accuracy: against OpenCascade's
adaptive integration (`BRepGProp.SurfaceProperties` with eps 1e-9), on 1407 face areas or
centroids (B-spline, torus, cylinder and sphere faces of cgb202, cgb203, cgb207, cgb217, cgb242,
cgb243, threaded_connector_109 and NIST CTC/FTC files) haecceity is off by 1e-6 to 1e-3
relative where OpenCascade's default integral is within 1e-6; on 876 others haecceity is within
and OpenCascade not; on 1167 neither is (`tests/fixtures/known_differences/features.json`, each
with both numbers and the reference; regenerated by `tools/known_differences/judge_faces.py`
and `features.py`).

**Used by.** `features.rs` (`describe_faces`: the analysis's faces, which rules use to pick datum
faces and top faces), and so every analysis.

**Stand-in.** A face without moments refuses the part's faces, naming the face (none in the
oracle's cases since 5798c59). (b) is reported value by value in the known differences; no
correction is applied.

**Met when** `face_moments` gives every face of the corpus a value within 1e-6 relative of an
adaptive reference.

## U12. The feature families quiddity-rust does not recognise

**Missing.** At a16d7af ten families of Python quiddity's evidence view were missing from
`correspondence::recognise`. At 5798c59 seven are there (`slots`, `oriented_slots`,
`polygonal_bosses`, `pads`, `sheet_metal_bodies`, `freeform_surfaces`, `blends`), and every
feature Python reports in them is matched by id on the oracle's cases (records equal, or within
the bounds `features.json` states; a polygonal boss lacks its top face, U2). Three remain:
`section_recesses` (Python's aggregate over the recess source families; quiddity-rust has the
source families, `pockets`, `channels`, `prismatic_pockets`, `section_passages` and others, but
not the aggregate), `risers` (`recognise_risers` exists but is outside `Features`) and
`step_levels` (made by Python's cross-family reconciliation, U2). On the oracle's cases that is
853 features Python reports and Rust does not (317, 128 and 408;
`tests/fixtures/known_differences/features.json`, one `rust-wrong` entry per family). The
recess source families, which Python's evidence view leaves out, are Rust-only features
(undetermined), and without the reconciliation Rust also lists 158 blend chains whose faces are
all Python's fillets, which Python drops (`BLEND_SUPERSEDED_BY_FILLET`; rust-wrong).

**Used by.** `features.rs` (`recognise`): the analysis's features, which Draftwright's picker
lists. specify-core's rules read only `holes`, `hole_patterns` and `turned_steps`, so no question
or default changes.

**Stand-in.** None: the families are absent from the analysis (an absence, not a guess); each
missing feature is a known difference.

**Met when** `correspondence::recognise` returns these three families with their defining (or,
with U2, constituent) faces.

## U13. Parameter-space loops that are simple polygons at degenerate corners (mesh stream)

**Missing.** `Part::uv_loops` is what the U4 stand-in triangulates. On a few freeform faces it
is not a simple polygon even before any thinning: cadgenbench `cgb202` face 399 (a narrow
neck: samples 283 and 353 of 673 cross near (0.110, 0.425)). At a16d7af `cgb242` faces 715 and
726 were refused too (the loop ran from (1.0, 0.9997) straight to (0.0, 0.0) across a
degenerate corner, a side whose image leaves the face); at 5798c59 they triangulate, within
0.06% of the exact area. Closed B-spline surfaces are also left non-periodic, so their loops
jump at the seam.

**Used by.** `triangulate.rs` (and so `mesh.rs` and the anchors' largest-triangle fallback).

**Stand-in.** Closed freeform directions are detected (the surface's two parameter sides are
the same points) and loops unwrapped across them locally; a polygon side along a parameter line
is sampled; a loop that still crosses itself unthinned, or a side that jumps, refuses the face,
which the mesh lists in `missing` (known differences `mesh.json`, verdict rust-wrong).

**Met when** `uv_loops` gives simple, continuous loops on these faces (routing degenerate
corners along their parameter lines, as poles are), or U4 is met.

## U14. Replace of a thread haecceity wrote, and the thread's runout aspect (writer stream)

**Missing.** Two gaps in haecceity at a16d7af that meet whenever a file haecceity wrote with a
`thread` is written again by `Mode::Replace` (every resumed specify-core file with a tapped hole
or a turned thread):

- `pmi::read` lists among the part's consumed instances the `product_definition_shape` of the
  thread's feature definition (the 'applied shape' construct, thread WR13), and the removal plan
  (`removal.rs`) refuses any `product_definition_shape` seed as shared infrastructure, so
  `pmi::write(.., Mode::Replace, ..)` fails: "removal refused (RemovePresentation); seed #355
  product_definition_shape is shared infrastructure" (pin with an internal thread, written then
  rewritten; `tests/resume.rs`
  `a_written_file_resumes_its_answers_and_is_rewritten_not_added_to`).
- The bare 'thread runout' `shape_aspect` thread WR16 requires is not consumed by the reader
  (docs/step-ap242.md, "Reader gaps the writer found"), so after a write `pmi::read` reports it
  as an unconsumed finding, and a replace's removal plan finds it blocking (it references the
  feature definition's shape).

**Used by.** `writer.rs` (`verify`, and every replace of a part with a thread).

**Stand-in.** `writer::verify` passes over a new unconsumed-`shape_aspect` finding only when it
is an instance the edit added and is exactly a `SHAPE_ASPECT` described 'thread runout'
(haecceity's own tests make the same exception). When a replace is refused only because of such
feature-definition shapes (none a part's own shape, nothing else blocking), `writer::clear`
removes the part's PMI first in a scratch copy: the refused `product_definition_shape`s are
restated as the `property_definition` they are a subtype of (only in the copy; they are removed,
so the restatement never reaches the file), `removal::plan` runs over the part's provenance as
the writer runs it (presentation it would empty goes too; the bare runout aspects are added),
and the replace is then made on the cleared copy. Anything else that blocks refuses the write,
naming it. The output is verified as any replace is (exactly the items written; no consumed
instance survives, compared by its text, since what is added is numbered from the cleared
file's largest id).

**Met when** the reader consumes the runout aspect with its thread, and a replace (and remove)
of a part with a thread haecceity wrote succeeds (the removal plan takes a feature definition's
`product_definition_shape` that only the removed PMI uses).

**Status (2026-10-09): met** (quiddity-rust 86b11ba, pinned at 10884b8): the reader consumes the
bare runout aspect with its thread, and the removal plan takes a feature definition's own shape
with its feature. The stand-in is gone: `writer::clear` and the runout exception in
`writer::verify` were removed, and every rewrite of a part with a thread
(`tests/resume.rs`, `a_resumed_files_thread_can_be_answered_away` and
`a_written_file_resumes_its_answers_and_is_rewritten_not_added_to`) is now an ordinary replace,
verified as any is.

## U15. Files haecceity writes with datum symbols crash OpenCascade's STEP reader (writer stream)

**Found.** specify-core (Python, OpenCascade 7.9 in its venv) dies with SIGSEGV (exit 139) in
`load.load` (`STEPCAFControl_Reader`, with and without GD&T mode) on any file haecceity wrote
with a datum: the plate with only its three datums written crashes, the plate with only a
material, a thread, a location, a size or a general tolerance does not. Bisected on the Rust
writer's output: removing the one `MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP('','',#shape
representation,#draughting model)` that relates the datum feature symbols' `DRAUGHTING_MODEL` to
the part's shape representation makes the file load; removing the three
`DRAUGHTING_MODEL_ITEM_ASSOCIATION`s instead does not. (docs/step-ap242.md reports OpenCascade
reading the written files through `tools/check_pmi_occt.py`; specify-core's reader does not.)

**Used by.** Nothing here reads with OpenCascade; it matters to stage 4's cross-engine check
(Python reading Rust-written files) and to anything OpenCascade-based opening the output
(Draftwright's Python side). Question Q16.

**Stand-in.** None: the writer writes what haecceity writes (decision 6, the symbols) and the
file is valid AP242 (every written file passes `express::validate_document` and
`express_rules::check_all` in `tests/writer.rs`).

**Met when** OpenCascade 7.9's reader loads a haecceity-written file with datum symbols (or
haecceity relates the draughting model in a form it reads).

**Status (2026-10-09): met** (quiddity-rust c228f95, pinned at 10884b8): haecceity no longer
writes the relationship (Q16), and specify-core loads all 11 Rust-written synthetic files and
both NIST files of the cross-engine check, reading every datum (`known_differences/
cross_engine.json` has no crash entry). haecceity's investigation places the crash in
specify-core's `load.py` name lookup through OCP rather than in OpenCascade's reader
(docs/step-ap242.md in quiddity-rust, decision 1).

## U15, evidence from the cross-engine check (cross-engine stream)

All 7 Rust-written synthetic files with datums crash specify-core in `load.load_all`; with the
one `MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP` naming the 'datum feature symbols'
`DRAUGHTING_MODEL` removed, all 7 load (`tools/capture/crossengine.py`,
`without_symbol_relationship`; `python_readback_without_symbols.json.gz`). Swapping its rep_1 and
rep_2 still crashes (the plate, by hand). All six NIST CAx-IF files in `tests/fixtures/nist` use
this relationship only between two `DRAUGHTING_MODEL`s (saved views), never with a shape
representation; haecceity does not evaluate the relationship's WR1-WR3 (`express_rules.rs`).
What would meet the need for OpenCascade-based readers: relating the symbols' draughting model
in a form those files use, or leaving the relationship out (Q16).

## U16. Closest-point projection across a closed NURBS trim seam

**Missing.** A trimmed-curve closest-point query that considers both sides of a
closed NURBS seam. `Curve::parameter` can select one endpoint as its sampled seed
and clamp there, missing a nearer point just inside the other endpoint.

**Used by.** Requested mesh boundary-deviation measurement (`mesh.rs`), including
cgb217 face 29's closed NURBS trim.

**Stand-in.** `trim_projection` tries both one-sided seam seeds with bounded Newton
steps, retaining only closer exact `Curve::value` evaluations.

**Met when** haecceity projects onto a supplied trimmed parameter interval across
both seam branches, returning the closest point or an explicit failure. The local
seam-specific inversion can then be removed.
