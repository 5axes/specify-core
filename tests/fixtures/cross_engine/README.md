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
