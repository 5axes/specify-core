# Questions for the maintainer


## Decision (2026-11-02): limit dimensions and combined ISO 2768 classes

Size and distance answers (`extra.<id>` with `kind: "size"` or `"distance"`) accept
absolute lower/upper limits: `"9.8/10.2"`. Bounds must be finite and strictly increasing.
Unsigned slash pairs are limits; signed bounds use `"limits:-10.2/-9.8"` (the prefix
also accepts positive bounds). Existing `±0.05`, `+-0.05`, `+/-0.05`, `0.05`, and signed
deviation pairs such as `+0.1/-0`, `-0.05/-0.1` keep their meanings. Hyphen and en-dash
ranges are refused to avoid ambiguity with signed deviations and negative bounds.
Limits stay `DimTolerance::Limits` in STEP. Write reports use `9.8/10.2`; scene
`specify[id].notation`, editor labels and diff/Markdown text use haecceity's drawn form
`9.8 - 10.2` (diameters `⌀9.8 - ⌀10.2`). ± answers stay deviations.

`part.general_tolerance` offers the four plain `ISO 2768-f/m/c/v` classes and all twelve
combinations of f/m/c/v with H/K/L, for example `"ISO 2768-mK"`. The default remains
`"ISO 2768-m"`. Typed `mK` is accepted as `ISO 2768-mK`. STEP stores the combined class
text and haecceity recognises its geometric class. Linear defaults still use the existing
ISO 2768-1 table for the first letter. Specify has no ISO 2768-2 tables, so H/K/L adds
no numeric geometric defaults; explicitly answered geometric tolerances keep their values.
Imported foreign PMI uses Add mode, retaining original limit entities; the answers are stored
beside it and resume on reopening, with the foreign limits still in `existing`. Bare or
previously authored parts store and resume the exact limit answer strings in STEP as usual.

## Decision (2026-10-15): gzipped STEP

- **Gzipped STEP input:** accepted everywhere a STEP file or its bytes are read, told by gzip's
  magic bytes (`1f 8b`), not the name, and decompressed in memory; the binding and fingerprints
  are of the decompressed bytes, so a `.step` and its `.step.gz` bind identically. Output is STEP
  text unless the destination's name ends in `.gz`, when it is gzipped (a write in place to a
  `.gz` input stays gzipped). Replaces the earlier deliberate refusal (docs/design.md, "Gzipped
  STEP files are read and written").

## Decisions (2026-10-09)

The maintainer answered these; each question below keeps its text for the record.

- **Q2:** move the quiddity and haecceity dependency to `prototype/rust-port` (it now contains
  a16d7af).
- **Q3:** add standard forms for notes and surface finish to haecceity (surface texture as
  `surface_texture_parameter`, other part notes as standard text notes; quiddity-rust work).
  Until they exist, keep listing them in `not_written`.
- **Q4 and Q15:** derive a knurl's missing parameters from a knurling standard (DIN 82 /
  ISO 13444: number of teeth from diameter and pitch, tooth depth, root fillet; 30° helix for a
  diamond knurl) and state them in the intent, so they are visible and editable. Three details
  decided on 2026-10-09: the number of teeth is π·d/P rounded to a whole number, the pitch kept
  at its nominal (tool) value and the diameter as designed; a diamond knurl states a 30° helix
  angle and no hand (two crossed helices; hand is optional in `turned_knurl`), a straight knurl
  neither; intents and stored answers without the new fields are filled in on write from the
  standard, and the write report says so.
- **Q6:** write an assembly with PMI from elsewhere in add mode, keeping every original byte;
  a deliberate improvement on Python.
- **Q8:** extend ISO 286 to 3150 mm (ISO 286-1), in both engines.
- **Q14:** fingerprint schema 2: zeros written unsigned (`0.0`), in both engines (specify-core's
  Python changes too), so stored answers resume across engines.
- **Q16:** haecceity stops writing the `MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP` that
  relates datum feature symbols to the part (OpenCascade 7.9 crashes on it); a deliberate
  departure, to revisit when OpenCascade is fixed. quiddity-rust work.
- **Q17:** write the rest and list each item that names no faces in `not_written`, with the
  reason.
- **Q18 (b):** keep refusing a second, different material or general tolerance in add mode.
- **Q20 (2):** a size or tolerance on a feature of several faces is written as one
  `geometric_item_specific_usage` per face (§5.1), which OpenCascade reads. quiddity-rust work.


Decisions the port does not make on its own. Each says what is being done meanwhile, so no work
waits on it.

**Q1. Analyses across engines.** Face numbering is the same in both engines on every file
checked so far, so the Rust binding keeps `loader_version` 2 and an analysis from either engine
can be written by the other. Feature ids can still differ (constituent against defining faces,
U2), so answers keyed by feature id are only meaningful with the analysis they were given
against, which is how specify-core already works. Should Draftwright Specify be able to mix
engines (analyse with one, write with the other), or should the Rust engine mark its analyses
(e.g. a distinct `loader_version` or an `engine` field) so a mix is refused? *Meanwhile:*
`loader_version` 2, with a test pinning face-by-face equality on every fixture.

**Q2. Moving to `prototype/rust-port`.** Once it contains a16d7af, the dependency moves there,
where recognition is framed (`recognise_placed`: records found in the part's own frame and
reported in the file's coordinates). Should specify-core-rust use the framed recognition (as
quiddity-rust's own default now is) or the caller-space `correspondence::recognise` it uses on
`ap242`? *Meanwhile:* caller-space, the only one on `ap242`. *Status (2026-10-09): done.* Both
dependencies follow branch `prototype/rust-port`, Cargo.lock pinning 5798c59 (5d39eee for
the haecceity writer decisions of 2026-10-09, 10884b8 since, with haecceity's U10 and U14;
oracle comparisons rerun at each, no difference changed at 10884b8); the new pattern
families (slot, pocket and oriented slot patterns) are left out of the analysis as gusset rib
patterns are. At that point recognition stayed caller-space (`correspondence::recognise`): the decision moved
the dependency without choosing the framed form, and caller-space was what Python's entry points
and the oracle use. Every oracle comparison was rerun and each changed difference rejudged
(`features.json`, `anchors.json`; docs/upstream-needs.md U11 and U12).

**Superseded for the v0.1.1 pin (2026-10-23).** Quiddity #61 now has one public
`correspondence::recognise` entry point, returning framed records; `recognise_placed`
and public caller-space algorithms are gone. Specify follows that API and reads fields
marked `local` through `frame`. Quiddity #79 owns grid publication and near-axis handling.
The coordinate audit and saved-answer migration rule are in `design.md`, "Recognition
coordinates at the Specify boundary" and "Saved answers across kernel upgrades".

**Q3. Notes and surface finish.** haecceity writes no notes (its decision 5: standard forms
only, none chosen). Part notes (surface-finish default, coating, heat treatment, edges) have
defaults, so nearly every write has some. *Meanwhile:* they are listed in the report's
`not_written` and the write succeeds. Should such a write instead be refused until a standard
form exists, or should the Rust engine stop asking those questions?
*Status (2026-10-09): done.* haecceity's standard forms are in the pin (quiddity-rust c228f95,
Cargo.lock 10884b8) and the writer uses them: a finish (`Ra 3.2`, on faces or the part's
default) is an ISO 10303-1110 surface texture, the coating, heat treatment and edge note are
'semantic text' on the part (PMI practice §7.4). The report's lines are now Python's (`surface
finish Ra 3.2`, `edges ...`, `surface finish Ra 0.8 on faces [7]`), nothing is listed in
`not_written` for them, and the Rust engine reads them back as the part's settings and a face's
`surface finish` note (src/existing.rs), so the write oracle's ten note entries went stale and
were removed. Three judgement calls the maintainer can overturn: (1) the material removal
condition, which ISO 1302 and the schema require and specify-core's finish does not state, is
'any process allowed' (ISO 1302's basic symbol, which an unqualified `Ra 3.2` means;
haecceity's question 4); (2) each note's 'semantic text' item is named by the kind Python's text
route gives it ('surface treatment', 'heat treatment', 'edge condition', not the answer's key),
so specify-core's `part_settings`, which scans for an item of that name, reads the Rust engine's
notes too (checked: it reads the edge note of every Rust-written single-part file, and a probe
of the plate written with a coating, a heat treatment, the edge note and a finish `Ra 0.8` on
face 2 gave `part_settings` all three notes: OpenCascade's XCAF keeping one 'semantic text'
property does not affect specify-core, which scans the file's text); (3) a finish
that is not `Ra <micrometres>` (not produced by the rules or `extras`, which refuse one) is
listed in `not_written` rather than guessed. specify-core does not read the surface texture, on the part
(`known_differences/cross_engine.json`, rust-correct) or on faces (the same probe: no note for
face 2 in `read_existing`; no cross-engine fixture has a finish on faces, as no oracle intent
does, so this has no entry there).

**Q4. Diamond knurls.** `turned_knurl` requires a helix angle for a diamond knurl (WR9); the
intent has pattern, pitch and diameter only. *Meanwhile:* straight knurls are written (major and
nominal diameter both the turned diameter, diametral pitch the intent's pitch); diamond knurls go
to `not_written`. Should a standard helix angle (e.g. 30°) be assumed and stated in intent?
*Status (2026-10-09): done; see Q15.*

**Q5. CI and the corpus.** *Decision (2026-10-20):* Ubuntu and macOS CI check out
pzfreo/quiddity's `tests/corpus` at `6d544e060cd0eeb4442d2196e5df757b5a15ea55`, matching
quiddity-rust's suite, and run the full workspace tests. Both `QUIDDITY_CORPUS` and
`SPECIFY_CORPUS` point to that checkout; `QUIDDITY_CORPUS_REQUIRED=1` makes a missing corpus
directory or required input fatal. Local runs without the switch may omit the corpus.

**Q6. Assemblies with PMI from elsewhere.** Python refuses to write an assembly that already has
PMI it did not write (an OpenCascade limitation). haecceity's add mode can write it, keeping
every original byte. *Meanwhile:* written by add mode, recorded as a deliberate difference.
Confirm, or keep Python's refusal.

**Q7. A product with surface models beside its solid.** NIST CTC-02, 04 and 05 (RD/RC and the
AP242 editions of 02 and 04) give one product a solid and one or two `SHELL_BASED_SURFACE_MODEL`s
in separate shape representations. OpenCascade's XCAF makes a part of each, so specify-core
lists CTC-02 AP242 as three parts (`SOLID` 635 faces, `SHELL` 1, `SHELL` 1); haecceity has one
part per product definition, so the Rust engine lists one part of 637 faces: the solid's 635 at
Python's indices, then the two surface faces. The binding's `face_count` differs (635 against
637), so an analysis of these files does not cross engines, and Python's parts 1 and 2 do not
exist in Rust. Should the Rust engine keep one part per product (PMI attaches to the product),
split as XCAF does, or number the solid alone and leave the surface models out?
*Meanwhile:* one part per product, faces solid first; the live comparison in `tests/part.rs`
checks the one part's faces against Python's parts' faces in turn.

**Q8. Diameters over 500 mm.** ISO 286 is tabulated here up to 500 mm, but specify-core still
offers fits for a hole or turned diameter above it: `choices.describe` then calls `callout`, whose
`fit_limits` raises `ValueError: nominal size 600.0 outside 0-500 mm`, so `questions` fails for
the whole part (checked with Python: `describe('hole.function:x', ['general', 'fit:H7'], 600.0)`).
Should fits not be offered above 500 mm (the general tolerance only), or should ISO 286 be
extended to 3150 mm? *Meanwhile:* Python's behaviour is kept: `choices::describe` returns the
same message as an error, which its caller reports (no part question list is made up).
*Status (2026-10-09): done in Rust; the comparison with Python above 500 mm is open.* `standards.rs` tabulates ISO 286-1:2010 Tables 1-3 to
3150 mm (IT5-IT11; H, JS/js, f, g, h, k, m, n, p), so a Ø600 hole or turned diameter is offered
fits and described with their limits (`tests/standards.rs`). Beyond 3150 mm the refusal names
`0-3150 mm`; k above grade 7 stays refused at every size, as in Python. specify-core's own
extension (branch `iso286-3150`) did not exist yet when this was done, so the comparison grid
(`tests/fixtures/iso286_oracle.json.gz`) holds Python's 500 mm tables: every class agrees to
500 mm, and the rows above are judged in `known_differences/iso286.json`, by a kind of difference
that only Python refusing the size can produce. When the branch lands, regenerate the fixture
from it (`iso286_oracle.py` takes the source path): any limits the two disagree on then fail
the sweep, and the entries go stale.
*Status (2026-10-10): done in both engines and compared.* specify-core's branch `iso286-3150`
(63e67bd) tabulates the same: `iso286_oracle.json.gz`, regenerated from it, agrees with the Rust
tables on every class and size of the grid to 3150 mm and on every refusal beyond it, so
`known_differences/iso286.json` is empty. `choices_oracle.json.gz` was recaptured from the same
source (`choices_oracle.py` now takes the source path too) and equals the Rust choices at 600 mm
with no exception left in `tests/choices.rs`. The NIST CTC-05 oracle cases (Ø558.8) were
recaptured (see "Recapture source" below): Python now asks, applies and previews them as Rust
does, so the rules and CLI entries for them are gone; Python's write of the AP242 file now fails
its own verification and its write of the corpus file OCCT's writer, which the oracle records.

**Q9 (rules stream). Malformed input Python crashes on.** A few answers or analysis records make
specify-core's rules raise something other than `ValueError`: an `AttributeError` or `TypeError`,
which serve reports as `failed` with a trace (a hole's purpose answered with a non-string while a
position is asked: Python calls `.partition` on it before `choices.check` runs; a datum answer that
is a bare number), or a `KeyError`, which serve reports as `invalid` with only the key as message
(a tapped size that is not tabulated, `'M7'`; a record missing a key the rules read). None occurs
in the oracle. *Meanwhile:* the Rust rules refuse each as `Error::Invalid` (serve's `invalid`)
with a message naming the answer or record; for a hole's purpose, the message `choices.check`
gives once it gets that far. Confirm, or should such input keep failing as `failed`?

**Q10. Face numbering that differs from OpenCascade's (bears on Q1).** On corpus
`inventory_refusal/13975.step` the two engines number the same 17 faces in different orders: Rust
in the `CLOSED_SHELL`'s order (#17, #145, #216, ...), Python in the order OpenCascade's reader gives
(#17, #216, #752, #1166, ...; why its shell differs from the file's order was not established); every face agrees entity for entity. Every other part the
features sweep compares (119) numbers alike: its remaining face differences are integration
noise (U11) or Q7's extra faces. An analysis of such a file made by one engine and written by the other
would put PMI on the wrong faces, and nothing in the binding detects it (same `source_sha256`,
`face_count`, `loader_version`). Should the binding carry something that detects this (e.g. a
hash of the face entity ids in order), or is Q1's answer enough? *Meanwhile:* recorded as an
`undetermined` known difference (`features.json`); no change to the binding.

**Q11. A sightline sample the classifier cannot decide (mesh stream).** haecceity's `Classifier`
can answer `Unknown` (every probe ray ambiguous), which OpenCascade's classifier never does.
specify-core's rule is IN blocks a leader and ON or OUT do not. *Meanwhile:* `Unknown` blocks
too, so a leader is only reported clear when every sample is shown free. Should an undecided
sample count as free instead?

**Q12. Mesh density (mesh stream).** The Rust mesh meets the same deflections (0.001 of the
diagonal, 0.3 rad) but has about 1.8 times Python's triangles over the oracle cases (1.30 M
against 0.70 M; cylinders and planes about the same, spheres, tori and freeform faces 2 to 4
times), because it refines until no triangle leans from the surface by more than the angular
deflection, which BRepMesh does not check. The largest corpus part (cgb202, 2157 faces) meshes
to 11 MB of JSON. *Meanwhile:* kept as is. Is that size acceptable for Draftwright Specify, or
should the Rust engine relax its lean check to approach Python's size?

**Q13. Face numbering where OpenCascade reorders a shell (mesh stream; the same file as Q10, found independently).** On corpus
`inventory_refusal/13975` (one `CLOSED_SHELL` of 17 faces), Rust numbers the faces in the
shell's own list order; OpenCascade's STEP transfer (plain `STEPControl_Reader` +
`TopExp::MapShapes`, checked with specify-core's venv) gives a different order, so faces 1-16 are
a permutation of Python's (Rust 2 = Python 1, 6 = 2, 1 = 15, ...; equal exact areas and centroids
to 1e-9; tests/mesh.rs `renumbered_faces`, known_differences/mesh.json). An analysis of this file
cannot cross engines, contrary to Q1's premise; the part stream's corpus check did not include
this file. Should the part model reproduce OpenCascade's order here (which needs knowing why
its transfer reorders this shell, a file the corpus keeps as one to refuse), or should such files
be detected and marked so a cross-engine analysis is refused? *Meanwhile:* the file order is kept,
and the mesh sweep checks every case's numbering against the oracle analysis, so any other file
where the orders differ fails the sweep until it has an entry.

**Q14 (writer stream). Resuming across engines.** A file specify-core (Python) wrote does not
resume in the Rust engine, nor the reverse: the stored fingerprint hashes
`json.dumps([(kind, [round(c, 3) ...])])`, whose text keeps the sign of a zero, and a zero
centroid component is integration noise whose sign OpenCascade and haecceity give differently
(the plate: Python `[-50.0, -0.0, -0.0]`, Rust `[-50.0, -0.0, 0.0]`; the numbers are equal,
`tests/resume.rs` `a_python_fingerprint_differs_only_in_signs_of_zero`). The stored item itself
crosses: Python's `resume.stored` reads the Rust writer's payload (checked on the Rust-written
plate with only its material written, as the plate with datums crashes OpenCascade, Q16), and
`answers_for` then refuses it on the fingerprint (Python's `6794223e...`, stored `39a96723...`).
Python cannot resume its own corpus outputs either, for the same reason (its rewrite moves
OpenCascade's noise: `flanged_spool_132`, 0 centroids differ when rounded, the hashes differ).
The Rust writer keeps the geometry's bytes, so its own files resume. *Meanwhile:* the fingerprint is Python's exactly;
a Python-written file is analysed with its PMI as the file's own (no resume), and written by add
mode. Options: (a) a schema 2 payload whose fingerprint writes zeros unsigned (`0.0`), in both
engines (specify-core's Python changes too); (b) accept a payload on its face count alone when the
hash does not match (weaker: answers could be resumed onto changed geometry); (c) keep as is.
*Status (2026-10-09): done in the Rust engine (option a).* The writer stores schema 2, whose
fingerprint writes every zero unsigned after rounding (canonical form in docs/design.md, "The
fingerprint"); schema 1 payloads are still read and resume when their own fingerprint matches
(`resume::fingerprint_v1`). On all 34 parts of the cross-engine check the schema 2 fingerprint
computed in Python from Python's own faces equals the Rust engine's
(`tests/cross_engine.rs`, `schema_two_fingerprints_are_the_same_in_both_engines`). specify-core's
side (branch `fingerprint-v2`) did not exist when this was done, so the Python value is the
documented form computed in the capture tool, not specify-core's own function; the captured
specify-core reads schema 1 only and passes the Rust writer's schema 2 payloads over
(`known_differences/cross_engine.json`). Recapture once that branch lands: the test then also
checks specify-core's own schema 2 fingerprint.
*Status (2026-10-10): done in both engines and compared.* The cross-engine fixtures were
recaptured with specify-core's branch `fingerprint-v2` (d97d030; see "Recapture source" below).
specify-core now reads the Rust writer's schema 2 payloads and resumes them: 15 Rust-written parts
resume with the same answers (the other 3 store none). On all 35 parts of the check its own
schema 2 fingerprint (`resume.fingerprint`) is checked against the Rust engine's
(`schema_two_fingerprints_are_the_same_in_both_engines`, which now requires that evidence): equal
on 34; on the Rust-written NIST CTC-05 part (new in the check, as Python now applies its answers,
Q8) eight faces' rounded centroids differ because haecceity's face integrals are off there (U11,
`cross_engine.json`, rust-wrong), so answers stored on that part would not cross until U11 is
met. The Python-written files of the oracle are still schema 1 (written by cc341dd) and fail in
Rust on the signs of zero, as before.

**Recapture source (judgement call, 2026-10-10; the maintainer can overturn it).** The oracle was
captured from specify-core cc341dd (branch `assembly-mates`, which adds assembly mates to main
c1694ec), while `iso286-3150` and `fingerprint-v2` each branch from c1694ec. Capturing from either
branch as it stands would have dropped the mates and rules changes every other oracle artifact
records. So the recaptures of the CTC-05 cases and of the cross-engine fixtures use cc341dd with
both branches' commits cherry-picked onto it (2cb6499, a local commit with a fixed committer:
`git worktree add --detach <dir> cc341dd`, then in `<dir>` `GIT_COMMITTER_NAME='Paul Fremantle'
GIT_COMMITTER_EMAIL=pzfreo@gmail.com GIT_COMMITTER_DATE=2026-10-10T00:00:00Z git cherry-pick
63e67bd d97d030`, which gives 2cb6499 again (checked at integration); the two touch
disjoint files from assembly-mates and from each other), loaded by `SPECIFY_CORE=<dir>`
(`tools/capture/common.py`); each meta records 2cb6499. The two module-level oracles
(`iso286_oracle.json.gz`, `choices_oracle.json.gz`) load `iso286-3150`'s own tree (63e67bd):
`standards`, `choices` and `materials` are the same there as on cc341dd. The rest of the oracle
(cc341dd) was not recaptured: it reaches no size above 500 mm, and `fingerprint-v2` reads its
schema 1 payloads as before. Recaptured, its written files would store schema 2, and the Rust
engine resuming Python-written files could then be checked directly; that full recapture is
outside this change (it rewrites every stream's oracle artifacts).

**Q15 (writer stream). Straight knurls too (extends Q4).** AP242's `turned_knurl` WR2 requires
six to nine parameters: the major and nominal diameters, the diametral pitch and at least three
of the number of teeth, tooth depth, root fillet, helix angle and hand (haecceity refuses a knurl
with three). The intent of a straight knurl has the diameter and pitch only. *Meanwhile:* every
knurl, straight or diamond, is in `not_written` with that reason. Should the intent gain those
parameters (e.g. the number of teeth from the diameter and pitch, a tooth depth and root fillet
from a knurling standard), or knurls stay unwritten until haecceity has a form for them?
*Status (2026-10-09): done, as decided (Q4 and Q15 above, with the three details).* A knurl's
intent states `teeth`, `tooth_depth`, `root_fillet`, a diamond's `helix_angle`, `standard`
("DIN 82") and `derived` (each derived key with its source); the writer writes straight and
diamond knurls through haecceity's `turned_knurl` (six and seven parameters, WR2; the diamond's
helix angle, WR9; major and nominal diameter both the designed diameter, DIN 82's d1, the
diametral pitch the pitch), fills in what an intent lacks with a warning, writes stated values
as stated, and lists a knurl off the standard (a pitch not in DIN 82's series, a pattern other
than straight or diamond) in `not_written`; verified by read-back (src/standards.rs
`knurl_parameters`, src/rules/turned.rs, src/writer.rs `knurl`; tests/threads.rs,
tests/writer.rs "knurls"). Sources: the DIN 82 text and ISO 13444 were not available; the
values were checked in IS 3403:1981 (DIN 82's knurl types, 90° profile, pitches 0.5 to 1.6 mm,
nominal diameter = finished outside diameter) and IS 6776:1980 §4.3 (knurling wheels: crest and
root radii 0.1 mm up to 1 mm pitch, 0.15 mm above). Judgement calls the maintainer can
overturn: (1) no table gives a tooth depth, so it is P/2, the sharp 90° profile's depth
(consistent with IS 3403 §3.4's 0.5 P growth of a formed straight knurl; rounding would make it
0.08 mm shallower at 1 mm pitch); (2) the root fillet is the wheel's crest radius; (3) DIN 82
draws a helical knurl's pitch normal to its grooves, so for a diamond knurl the circumferential
spacing is P/cos 30°, but the number of teeth uses π·d/P for both patterns as decided; (4) a
knurl whose pitch or pattern the standard does not give is listed in `not_written` rather than
refused. Integrated into `main` on 2026-10-10 together with the pin at 10884b8 (Q3's notes and
finish written); the writer sweep's three knurl entries are gone and its 91 differences carry
no undetermined verdict.

**Q16 (writer stream). Files OpenCascade cannot open (upstream need U15).** Every file the Rust
engine writes with a datum crashes OpenCascade 7.9's STEP reader (specify-core's Python, and so
anything OpenCascade-based downstream), because of the `MECHANICAL_DESIGN_AND_DRAUGHTING_
RELATIONSHIP` relating haecceity's datum symbols to the part. *Meanwhile:* written as haecceity
writes it (valid AP242). Should the writer leave out the datum symbols (or that relationship)
until haecceity or OpenCascade is fixed, so Draftwright's OpenCascade side can open the files?
*Status (2026-10-09): done.* haecceity no longer writes the relationship (pinned at 10884b8).
specify-core (OpenCascade 7.9) now loads all 11 Rust-written synthetic files and both NIST
files of the cross-engine check, and reads every datum; the crash entry and the probe captures
(`python_readback_without_symbols.json.gz`) are gone.

**Q17 (writer stream). Requirements on no faces.** On 26 corpus parts (25 mfcadpp and cadgenbench
cgb217) specify-core's rules (both engines, from Python's analysis) give a hole's size,
locations and position with `faces: []` (holes whose bores are not cylinders the rules keep).
Python's write then fails inside OpenCascade ("StepShape_DimensionalLocation: Null Reference",
or "expected one B-rep representation"); the Rust writer refuses the write as invalid, naming
the item ("the size on holes:16 names no faces"). *Meanwhile:* refused, nothing written. Should
the writer instead write the rest and list such items in `not_written`, or should the rules not
make them? *Status (2026-10-09): done.* The writer writes the rest of the intent and lists each
such item in `not_written` ("the size on holes:16 names no faces, so nothing in the file can
carry it (question Q17)"); the write is verified by reading back (src/writer.rs `faceless`;
tests/writer.rs on mfcadpp/1004 and cgb217).

**Q18 (writer stream). Two write-mode choices that differ from docs/design.md or from Python.**
(a) A bare part is written by `Mode::Replace`, not `Mode::Add` as design.md's "Write modes"
says. For a part with no PMI the two give the same file; they differ only where the part
carries Python's form of stored answers that do not resume (haecceity reads them as not
modelled, so the part looks bare): Replace removes them and stores the new answers, Add would
keep them beside the new ones, leaving two answer sets. Python writes a bare file from its
geometry alone (`writer.py`: `if bare or resumed`), which also drops them, so Replace is
Python's behaviour. *Meanwhile:* Replace (src/writer.rs, `write_verified`). Should design.md
say so? (b) In add mode (a part with PMI from elsewhere), a material or general tolerance the
part already states is not written again when equal (a warning says so) and the write is
refused when different ("the part already states material X; adding Y would give it two").
Python's merge transplants its new material beside the file's (merge.py, `materials`), so the
part then states two. *Meanwhile:* refused, as add mode cannot replace the file's own item and
two would contradict each other. Keep the refusal, or write the second one as Python does?

**Q19 (CLI stream). Contacts between parts that cannot be measured.** For a part of an
assembly the analysis gives each flat face resting on another part a `contact` (`mates.py`).
The Rust overlap (U5 stand-in) refuses a pair whose boundary loop does not close or runs along
an unresolved edge (`mates::contacts_with_refusals`); Python never reports such a pair. The
analysis format has no place for the refusal, so it would be dropped silently. *Meanwhile:*
`analyse` prints each as `warning: ...` on stderr and serve's analyse reply carries
`"warnings": [...]` (only when there are any: an additive key, absent on every captured case).
Should the analysis itself record them (e.g. `contact: {"refused": why}` on the face), or is a
warning outside the analysis enough for Draftwright Specify?

**Q20 (cross-engine stream). What specify-core (Python) can read of a Rust-written file, and
which side should change.** `tests/cross_engine.rs` reads each Rust-written synthetic and NIST
file with Python's own read-back (`tests/fixtures/cross_engine/`, verdicts in
`known_differences/cross_engine.json`). Findings: (1) every file with a datum crashes
OpenCascade (Q16); with the datum symbols' one `MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP`
removed (the capture's probe, `python_readback_without_symbols.json.gz`) all seven load and
Python reads the material, general tolerance, every location, every size (a fit without its
limits, 3 below) and tolerance on a single face, and the datums those tolerances cite, so
option (a) of Q16 would make those cross; (2) a size or tolerance
on a feature of several faces, which haecceity writes as one `item_identified_representation_usage`
of a `set_representation_item` (PMI practice §6.5.1), is dropped by OpenCascade (the datums only
such a tolerance cites go with it); haecceity's own form table also allows one
`geometric_item_specific_usage` per face (§5.1), which OpenCascade reads; (3) a fit written as its
ISO 286 class (`limits_and_fits`, §5.2.5) reads in Python with no limits, because specify-core's
`read_existing` asks XCAF only for plus/minus limits; (4) threads (AP242 `thread` + attribute set)
are not read at all: specify-core reads only its own text route. *Meanwhile:* the Rust writer
writes the standard forms and the differences are recorded (rust-correct for 2 and 3,
undetermined for 1 and 4). For Draftwright to move a part from the Rust engine to the Python one,
which should change: haecceity writing per-face usages (and stated limits beside a fit class,
which its fit form allows), or specify-core's Python reader learning the §6.5.1 usage, the fit
class and the `thread` entity? (Renumbered from Q19 at integration: the CLI stream took Q19.)
*Status (2026-10-09): (1) and (2) done.* With haecceity's per-face usages (decision Q20 (2),
pinned at 10884b8) OpenCascade reads every size and tolerance on a feature of several faces, with
the datums they cite (NIST CTC-03's 8-face H11 holes and position, the plate's, assembly's,
corner holes', identical holes' and stack's hole sets); the `set_representation_item` entry and
the datum entry that followed from it are gone. (3) and (4) stand: such a fit, like every fit,
reads without its limits, and threads are not read (`known_differences/cross_engine.json`).

**Q14, evidence from the cross-engine check (cross-engine stream).** Whether Python's resume
reads the Rust-written stored answers: it finds them but does not resume them. On all 5 parts
Python could load (and on every part of the probes of the 7 that crash) Python's
`resume.saved` returns exactly the payload the Rust writer stored, with the same face count,
and `answers_for` returns None only because the fingerprint differs; the faces Python
fingerprints equal the Rust engine's in kind and rounded centroid by value and differ only in
signs of zero (checked per part by `tests/cross_engine.rs`, `faces_gap`). The reverse holds on
all 15 parts of the 11 Python-written synthetic files: the Rust engine reads Python's payload and
refuses it for the same reason, and reads everything else in them (existing PMI, part settings)
exactly as Python does. So option (a) of Q14 (zeros written unsigned in both engines) would make
stored answers cross in both directions on every file checked.

**Q21 (thin slice S6, judgement calls, overturnable). The in-memory API's edges.**
`part::load_bytes(name, bytes)` and `writer::write_bytes(loaded, intent, answers)` are the
file API without the filesystem, for the editor's browser build. Three details were not settled
by the plan and were taken conservatively: (1) single part only, as the plan says: `load_bytes`
takes no part index and refuses a file of several parts as `load` does without one, and
`write_bytes` refuses a part of such a file, since writing one part of an assembly needs the
others loaded (from the path, in `write_parts`); (2) a file of one part placed more than once,
which the editor's own adapter refused, loads as `load` loads it (the same bytes behave the same
either way); (3) the report's `output` is the `name` given to `load_bytes`, there being no
destination (the editor's adapter did the same). Answers stay optional, as in `write`. Say if a
part index, or another `output`, is wanted.
