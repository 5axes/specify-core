# Cross-engine fixtures

What each engine reads of the other's files, checked by `tests/cross_engine.rs` (verdicts in
`../known_differences/cross_engine.json`). One directory per synthetic or NIST oracle case
whose every part has Python's accept-defaults intent:

| File | Written by | What it is |
|---|---|---|
| `rust.step.gz` | `cargo run --release --example write_fixtures` | Python's intent (each part's `apply.json.gz`) written by the Rust writer, base answers stored. Byte-checked against today's writer. |
| `rust.refused.txt` | the same | Why the case was not written, in place of `rust.step.gz` (NIST CTC-02 and CTC-04: Python's intents name parts the Rust engine does not have, Q7, so the example's part-index check refuses them before the writer is called). |
| `python_readback.json.gz` | `tools/capture.py cross-engine tests/fixtures/cross_engine` | specify-core's read-back of `rust.step.gz`, in the oracle's `readback.json` shape (per part: `read_existing`, `part_settings`, `resume.answers_for`), or `{error: "crash"}` where OpenCascade died. |
| `python_readback_without_symbols.json.gz` | the same | Only where OpenCascade crashed: a probe reading the file without the datum symbols' `MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP` (U15, Q16). Not a file either engine writes. None since haecceity stopped writing that relationship (Q16, quiddity-rust c228f95): every Rust-written file loads. |
| `python_meta.json.gz` | the same | The read file's sha256 (a stale capture fails the test), specify-core's version, and per part the resume evidence: the payload Python's pattern finds, the face count, the faces it fingerprints and specify-core's own hash of them for the schema it writes (`fingerprint`) and for schema 1 (`fingerprint_v1`), the schema specify-core writes, and the schema 2 fingerprint of those faces by docs/design.md's canonical form (`fingerprint_v2`). |
| `python_written_evidence.json.gz` | the same | The same resume evidence for the file Python wrote for the oracle (`../oracle/<case>/written.step.gz`). |

Regenerate in this order, in one commit: the example, then the capture (each `.gz` is written
reproducibly; only `captured` dates change on a rerun). The initial capture was from
specify-core 2cb6499 (cc341dd with its branches `iso286-3150` and `fingerprint-v2` cherry-picked,
loaded with `SPECIFY_CORE=<that checkout>`; docs/questions.md, "Recapture source"), which writes
and reads stored-answer schema 2.

The bytes are the same on every platform CI runs (macOS arm64, Linux x86_64) and were checked
on Linux arm64 and x86_64 (emulated) as well. Before stored-answer schema 2 they were not: schema
1's fingerprint hashed the sign of each zero centroid component, which integration noise gives
differently from one platform's maths library to another's, so the stored payload of 5 cases
differed on Linux (CI run 37920952871). Schema 2 writes every zero unsigned (Q14); it changed
only that line of the committed files (same Cargo.lock), and Linux CI has passed since (run
37962711443).

For #22, regenerate `rust.step.gz` with the example above: every authored semantic
requirement now carries the documented `specify-core:written:v1` token. Recapture the
Python readbacks and SHA-256 metadata with `tools/capture.py cross-engine`; do not edit
compressed bytes or capture hashes. The reference can be reproduced in an isolated checkout
of `pzfreo/specify-core` at `cc341dd` with `origin/iso286-3150` and `origin/fingerprint-v2`
cherry-picked, followed by `uv sync --frozen`. Set `SPECIFY_CORE` to that checkout when
running its `.venv/bin/python tools/capture.py cross-engine tests/fixtures/cross_engine`.
This preserves the original reference source and pinned Python dependencies without needing
another working repository on the host. The capture records the reconstructed commit hash.
The provenance-only readback departures have explicit `rust-correct` verdicts backed by #22;
the original Python oracle is unchanged. See `docs/pmi-provenance.md` for reader matching.

For quiddity-rust#137, regenerated with v0.2.6 (`b9addd9`): dimension/tolerance
callouts, ISO 1302 surface symbols and General notes are now present, multi-face aspects
are composite, locations own their aspects, and datum sizes use separate entities.
The files include #65 characteristic ids and history and are checked byte for byte again.
Python readbacks are recaptured with the documented `cc341dd` + ISO/fingerprint reference;
the Python-written oracle inputs and their evidence remain independent and unchanged.

The pre-numbering Python reader still returns identical readbacks. The newly numbered
fixtures expose 97 number-field differences across 10 parts after existing semantic
ledger entries are applied; `known_differences/cross_engine.json` records this measured
reader limitation. Numbers are compared with their actual standard roots in the native
characteristic tests, rather than removed from this comparison.

After merging #77 (PR #78), recaptured the v0.2.6 writer fixtures again: each newly
written size/location now owns its `specify pmi origin` UDA, with origin and optional
stable source. Twelve STEP outputs changed; CTC-01 and CTC-05 remain byte-identical.
The independent Python reader predates these UDAs; readback departures are recorded
in the cross-engine ledger without removing any fields from the comparison.

For v0.2.7 (`eaa7762`), regenerate all fourteen successful `rust.step.gz` outputs:
haecceity's deviations now use `+upper/lower`, and its notes and frame placements move.
Each output contains generated callouts or notes, so its byte golden changes even though
recognition and semantic intent do not. The two part-index refusals stay unchanged.
Recapture the independent readbacks and byte hashes with the same schema-2 reference
(`84c770e`); retain the original Python-written oracle and its evidence. The three
`characteristic-numbers-legacy.json` settings-only writer hashes change solely because
v0.2.7 moves the General notes block; their analysis and semantic-read hashes stay fixed.

For v0.4.1 (`a2195eed`), regenerate the fourteen successful outputs for the
regenerated ladders/callouts, one numbered GENERAL NOTES block and generated plane
views; AP214 PRODUCT_TYPE entities upgrade when written as AP242. The two refusals
stay unchanged. Recapture Python readbacks and hashes with the same `cc341dd` +
`63e67bd` + `d97d030` source. The main-only recapture preserved all fourteen
Python semantic readbacks; the combined planner recapture is checked separately.
Keep Python-written
inputs/evidence and historical upgrade specimens unchanged. The three macOS
settings-only hashes, the unnamed face-name STEP hash and the current numbered
M2 tap golden change for the new presentation; stored-answer, analysis and semantic
readback assertions remain unchanged.

For the shared planner (#75), regenerate Rust outputs with v0.4.1 and recapture
with `tools/capture.py cross-engine tests/fixtures/cross_engine --rust-only`.
This option retains the independent, unchanged Python-written specimens and their
original resume evidence. Use the same `cc341dd` + `iso286-3150` + `fingerprint-v2`
reference described above (this combined recapture uses the clean reconstruction
`7e2125d`, recorded by the capture tool in every `python_meta.json.gz`).
Planner counts/roles now have dimension-owned metadata;
synthetic support points use the source context's length unit, including inches.

The combined v0.4.1 integration recapture preserves thirteen semantic readbacks.
CTC-05 additionally omits three unreconciled General offsets (171.45, 206.375 and
241.3 from face 8) that conflict with existing position requirements; the writer
reports them. Their three stale per-item cross-engine ledger entries are removed.
New shared source UDAs name the complete measured target; retained thread drilling
attributes also prevent duplicate drill rows in Add mode. These are integration
fixes, rather than changes to v0.4.1's shared planner or the 42-part count table.

For v0.4.2 (`d7112fd`), after merging main `3936c84` (#91 planner dimensions and
#94 opt-in BOM), rerun `cargo run --release --locked --example write_fixtures`
and `SPECIFY_CORE=/tmp/specify-python-oracle75
/Users/paul/repos/specify-core/.venv/bin/python tools/capture.py cross-engine
tests/fixtures/cross_engine --rust-only` (one command). The reference remains the
clean reconstruction `7e2125d` above. All fourteen successful STEP byte goldens,
independent Python readbacks and capture metadata remain byte-identical to main;
both recorded refusals also remain unchanged. No capture or cross-engine ledger
change is needed. These cases do not exercise the temporary #168 datum-constituent
adjustment. Keep historical inputs and Python-written evidence frozen.

For v0.4.3 (`56a4cbc0`), rerun the same Rust writer generation and independent
`--rust-only` Python capture using reference `7e2125d`. All fourteen STEP byte
goldens, semantic readbacks, metadata and both refusals remain unchanged. Although
upstream #171/#172 change some presentation and long-text outputs, these cases
keep their existing presentation and short descriptive values. No golden or
known-difference changes are needed. The packaged Python PMI schema also matches
v0.4.3 byte for byte, so the schema mirror is retained.

For v0.4.4 (`53cf3604`), rerun the writer and the independent `--rust-only`
Python capture using the same clean reference `7e2125d`, with dependencies from
its frozen `uv.lock` in an isolated Python 3.12 environment. All fourteen
successful files are refreshed: main's schema-2 stable-key number histories
replace schema 1, and thread-bearing presentation gains the new callouts and
packing. All fourteen decoded Python semantic readbacks are identical to main;
metadata records the actual new STEP hashes. The two refusals and all
Python-written inputs/evidence remain frozen. No cross-engine verdict changes
are needed. Both packaged Python schemas are copied byte for byte from the
locked v0.4.4 dependency and checked by `python_schemas_match_the_pinned_kernel`.

For Add-mode answer storage, only these byte goldens change:

- `nist/nist_ctc_01_asme1_ap242`: persist the supplied base answers with
  `pmi_scope = specify-core:written:v1` beside the original foreign PMI.
- `nist/nist_ctc_03_asme1_ap242`: persist the supplied per-part base answers with the
  same provenance scope; the previous Add writer omitted them.
- `nist/nist_ctc_05_asme1_ap242`: persist the supplied base answers with that scope,
  including the existing explicit deferrals; no deferred requirement gains PMI.

The new answer UDA precedes number history, so subsequent newly authored entity ids move.
Original foreign entity spans are independently checked byte for byte by
`tests/add_mode_answers.rs`. All other byte goldens and both refusals stay unchanged.
The three Python hash/resume metadata files and two changed semantic readbacks are
independently recaptured with Python 3.12.13 and the reference's frozen `uv.lock`, using the
documented clean reconstruction `2cb6499` (cc341dd + 63e67bd + d97d030; the source of the later
reconstruction `7e2125d`). CTC-05's semantic readback remains unchanged: its existing U11
face-integral mismatch now also prevents Python from resuming the newly stored answers;
the ledger keeps both consequences visible as `rust-wrong`. Historical Python-written
oracle files and their evidence stay frozen.

For v0.4.5 (`845a8919`), regenerate all sixteen cases and independently recapture
all fourteen successful files with Python 3.12.13, the frozen `uv.lock` dependencies
and clean reference `7e2125d` (`--rust-only`). This reference has the same Python
source as the preceding `2cb6499` reconstruction. Preserve both part-index refusals
and all Python-written specimens/evidence. The ten changed Rust outputs have these
reviewed reasons; each still compares every decompressed STEP byte:

| Case | Reason for changed STEP bytes in v0.4.5 |
|---|---|
| `nist/nist_ctc_01_asme1_ap242` | #181 reuses neutral per-face carriers and directly references datum features for new requirements. This input has no SHAPE_ASPECT('DIMENSION') wrapper; no foreign-wrapper restoration was involved. |
| `nist/nist_ctc_03_asme1_ap242` | #181 removes new forwarding location endpoints in favour of direct datum/face carriers. This input has no SHAPE_ASPECT('DIMENSION') wrapper; no foreign-wrapper restoration was involved. |
| `nist/nist_ctc_05_asme1_ap242` | #181 shares the new location/feature carriers and their GISUs instead of adding private dimension wrappers. |
| `parts/a_hole_along_x_is_labelled_from_its_rim` | The new linear location directly references its existing datum/face endpoint instead of a DIMENSION wrapper. |
| `parts/assembly` | New locations on both parts reuse their datum/face carriers; subsequent new entity ids move. |
| `parts/four_corner_holes_are_quiddity_rectangle_with_members` | Locations reuse the pattern/datum support carriers and no longer own forwarding aspects. |
| `parts/identical_holes_no_pattern_claims_form_a_set` | Locations reuse the set members' shared face/datum carriers. |
| `parts/pin` | New locations directly reference datum features instead of forwarding DIMENSION aspects. |
| `parts/plate` | New locations reuse the plate's shared datum/face carriers. |
| `parts/stack` | Locations on all three parts reuse their datum/face carriers; subsequent new ids move. |

New complex datum/composite carriers also receive Specify's inherited shape-aspect
ownership stamp. Four byte goldens (`a_file_with_two_parts_must_say_which`,
`a_placed_part_is_meshed_in_the_frame_the_analysis_uses`,
`default_datum_a_takes_every_coplanar_face`, `wrapped`) remain unchanged.
All fourteen decoded independent Python semantic readbacks remain identical;
metadata records the actual written hashes and clean reference. No known-difference
entry is removed or changed. Original input spans, foreign dependencies, shared
carrier removal and stable characteristic numbers are checked in native tests.
