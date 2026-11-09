# The Python specify-core

Answer maps accept optional `_specify` geometry evidence and unresolved carry blocks.
Bulk defaults cannot replace dropped answers; write rejects stale evidence. Plain maps
remain compatible but cannot detect stale in-range face indices. See
[answer files and changed geometry](answers.md).

## Mesh tolerance

`api.mesh(step, part=None, *, max_deviation_mm=None)` accepts paths and loaded parts.
For example `api.mesh("curved.step", max_deviation_mm=0.05)` returns mesh v2 with
`max_deviation_mm` and measured `achieved_deviation_mm` (both mm). Omitting the
keyword or passing `None` preserves the previous output. Non-positive/non-finite
values and faces that cannot meet the measured bound raise `ValueError`.
See [mesh tolerance](mesh-tolerance.md) for sampling and refusal semantics.

## Export complete answers

`api.export_answers(step, analysis, answers)` accepts PMI defaults to a fixed point and
returns a complete dict with `_specify.evidence` for the selected part. `step` can be a
path or a `LoadedPart`; the analysis binding selects an assembly part. It writes no STEP.
Missing required answers or unresolved carry blocks raise `IncompleteError`; stale
saved evidence or a mismatched analysis raises `ValueError`.

```python
import json
from pathlib import Path
from specify_core import api

analysis = api.analyse("plate_v1.step")
complete = api.export_answers("plate_v1.step", analysis, {"part.material": "6061-T6"})
Path("plate.answers.json").write_text(json.dumps(complete, indent=2) + "\n", encoding="utf-8")
api.write("plate_v1.step", api.apply(analysis, complete), "plate.pmi.step", answers=complete)
```

Keys are sorted recursively; arrays keep their order. The resulting map is accepted by
all answer-taking operations, including `api.diff`. See [answer files](answers.md).

## Limit dimensions and combined ISO 2768 classes

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

Analysis `existing` dimension records and intent size/location rows add optional
`limits: [lower, upper]` in mm (degrees for an imported angle). `upper` and `lower`
remain deviations from nominal for tolerance checks and existing consumers. The
`limits` field is absent for deviations and fits; no haecceity schema changes are needed.
A basic (theoretically exact) dimension's `existing` record adds `basic: true`; the field
is absent otherwise. A basic dimension has no `upper`/`lower` of its own. Scene metadata
(`specify[id]`) carries the same `basic: true` for a basic dimension, and diff words add
` (basic)`.
The Python `ExistingPmi.basic` field is `False` unless the dimension is basic;
`to_dict()` emits `basic` only when it is `True`.
The Python `ExistingPmi.limits` field is an optional `(lower, upper)` tuple;
`to_dict()` emits it as a two-element list only for limits. Stored answer strings resume
unchanged. Python and WASM expose the same answer grammar and combined class choices.

Specify began as a port of the Python [specify-core](https://github.com/pzfreo/specify-core), which
runs on OpenCascade and Python's quiddity. The two engines share the command line, the JSON formats
and the STEP files they read and write, so Draftwright Specify can drive either, one engine per part.
`diff`, the STEP diff action and the browser module are this engine's own.

## The Python package

`crates/specify-core-py` builds the Python package `specify_core` on this engine: specify-core's
API (`load`, `parts`, `analyse`, `questions`, `apply`, `preview`, `write`, `write_parts`,
`mesh`, with its types `Question`, `Intent`, `WriteReport`, `ExistingPmi` and its errors), plus
`problems`, `choices`, `serve.reply` and the `specify-core` command line, which runs this
engine's (`specify_core::cli`) in the Python process. It needs no Python dependency (no
quiddity, build123d or OpenCascade). A `LoadedPart` is a handle on the part held by the engine:
it has `path`, `binding`, `part_count` and `name`, but none of OpenCascade's objects. The
second program name `pmi-assist` is not kept.

It is a PyO3 extension built with maturin (abi3, one wheel per platform for CPython 3.12 and
later): `maturin build --release -m crates/specify-core-py/Cargo.toml`. `tests/python` is a
smoke suite of the package; `.github/workflows/python.yml` builds, tests and publishes the
wheels. The engine itself is tested in Rust.

To build and test it locally:

```bash
uv venv /tmp/specify-py && uv pip install --python /tmp/specify-py/bin/python maturin pytest
VIRTUAL_ENV=/tmp/specify-py /tmp/specify-py/bin/maturin develop --release \
  -m crates/specify-core-py/Cargo.toml
/tmp/specify-py/bin/python -m pytest tests/python
```

Its README (`crates/specify-core-py/README.md`) is the PyPI page, so it is written for users.

Parity was checked once, before the Python repository was archived, by running specify-core's
own test suite at cc341dd (the revision the oracle was captured from) on this package: 169 of
its 219 test functions apply to it, and of their 238 cases 210 passed, 24 failed by differences
recorded in the ledger (`tests/fixtures/known_differences/`), 3 by documented decisions with no
ledger entry (an assembly part written alone, D3; an assembly with PMI from elsewhere written in
add mode, Q6; a `'theoretical'` dimensional note per basic dimension where Python shares one),
and 1 was skipped for a NIST file not at hand. The other 50 test Python's own internals
(OpenCascade objects, private functions, modules the package does not expose) or a tool of the
Python repository.

## History

Every module of specify-core is ported (part loading, features, existing PMI, the rules,
mates, meshing, anchors, the writer with resume and assemblies, the API, the command line and
the JSON-lines service) and checked against outputs captured from the Python implementation.
Every difference from Python carries a verdict and its evidence, listed in
[docs/known-differences.md](known-differences.md) (generated from
`tests/fixtures/known_differences/`; its "Open work" section lists every difference that is a
known bug or unsettled). What is verified per module, the stand-ins used for what quiddity-rust
does not yet provide, and what remains are in [docs/design.md](design.md), "Status"; the
needs from quiddity-rust are in [docs/upstream-needs.md](upstream-needs.md) and the
decisions waiting for the maintainer in [docs/questions.md](questions.md).

## Files written by one, read by the other

OpenCascade (and so specify-core's Python) opens every Rust-written file and reads its datums,
sizes and tolerances, on one face or several (Q16, Q20; `tests/cross_engine.rs`); it reads a fit
without its limits, no thread and no surface finish, as specify-core's reader asks only for its
own forms of those. Named parts now store schema 3, which older engines skip;
see [authored face names and compatibility](face-names.md). Unnamed parts retain
schema 2. Answers the Rust engine stores in schema 2 (Q14) resume in
specify-core from its branch `fingerprint-v2` on, which writes schema 2 too; files the older
specify-core wrote (schema 1) do not resume in Rust.

## Where this engine differs

Each of these is recorded with its reason (docs/known-differences.md, docs/questions.md):

- **Assembly analysis includes a sourced `bom` row** for optional editor questions.
  This Rust-engine extension is also exposed by the current Python package; the archived
  Python oracle predates it. Exact per-part entries and evidence are in the
  [analysis ledger](../tests/fixtures/known_differences/analysis.json). BOM writing remains
  opt-in; see [the BOM contract](bom.md).
- **Datums are proposed as measurement origins in ± mode too** (maintainer decision,
  draftwright-rust #122; rules version 2). The same A/B/C defaults apply, including a turned
  part's axis A and shoulder B. They are written as AP242 datum features without adding
  geometric tolerances in ± mode. Selected datum planes also set the origins of explicit ±
  hole locations; changing or removing datums changes those origins. Verdict and exact Python
  before/after evidence: [rules differences](../tests/fixtures/known_differences/rules.json) and
  `tests/fixtures/rule_defaults_oracle.json.gz`; the captured Python results are retained.
- **Part notes and surface finish are written in AP242's standard forms**, not Python's text
  route: a finish (the default or on faces) as an ISO 10303-1110 surface texture, the coating,
  heat treatment and edge note as the PMI practice's 'semantic text' (Q3). specify-core reads
  the notes back but not the finish.
  Rust also supports ordered free-text manufacturing notes through `part.notes` (default
  `[]`), each a semantic `general note` with an explicit one-based `note index`. See the
  [STEP form and readback contract](part-notes.md). Drawing-only notes stay in the
  drawing Spec; no automatic migration is performed.
- **How a part is made** (`part.process`) is Rust-only, asked last of every made part; an
  unanswered part is machined and asked and written as before. See [process.md](process.md).
- **Knurls are written as AP242's `turned_knurl`**, straight or diamond, which Python writes
  as text: the number of teeth, tooth depth, root fillet and a diamond's helix angle are
  derived from DIN 82 and stated in the intent with their sources, filled in with a warning
  for an intent that lacks them; a pitch or pattern the standard does not give is listed in
  `not_written` (Q4, Q15).
- **An assembly that already has PMI from elsewhere is written** (by haecceity's add mode,
  keeping every original byte), where Python refuses it because of OpenCascade; pending the
  maintainer's confirmation (Q6).
- **Feature ids can differ from Python's** for the same feature (quiddity-rust reports a
  feature by its defining faces, Python's quiddity by its constituent faces, and some families
  differ), so question ids and answer keys do too. An analysis and its answers belong together;
  analyse and write with the same engine (Q1, upstream needs U2, U12).
- **A gzipped STEP file is read** (told by its magic bytes, decompressed in memory, bound by
  the decompressed bytes' hash), where specify-core refuses it; `write -o out.step.gz` writes
  it gzipped (maintainer decision 2026-10-15).
- A file that cannot be read, or an assembly analysed without `--part`, is an `error:` line on
  stderr (exit 1) where Python prints a traceback; `write`'s stdout is only the report, without
  OpenCascade's transfer statistics; meshes are triangulated by a local stand-in (U4) with about
  1.8 times Python's triangles (Q12); a product with surface models beside its solid (NIST
  CTC-02, 04, 05) is one part, not OpenCascade's several (Q7).

## Tests

Tests compare the port with fixtures captured from specify-core (`tools/capture.py`, run in
specify-core's environment); tests whose input is a quiddity corpus file skip without the
corpus. `docs/known-differences.md` is regenerated after an area file changes with
`UPDATE_KNOWN_DIFFERENCES=1 cargo test --release --test known_differences_doc`.

Optional live Python/OpenCascade tests use only an explicitly configured
`SPECIFY_TEST_PYTHON=/path/to/specify-core/.venv/bin/python`; they never discover a
checkout on the host. `SPECIFY_TEST_NIST_AP242=/path/to/plain/nist` additionally enables
live comparisons on the five `nist_ctc_0N_asme1_ap242.stp` inputs. The committed captures
remain the default regression record; these live checks do not recapture them.

## Resuming answers beside foreign PMI

Writes store supplied answers even when the part already contains foreign PMI. Reopening
returns them in `resume.answers`, while unmarked foreign PMI stays in `existing` and keeps
its questions answered. Writing edited answers replaces Specify's provenance-marked items
and preserves foreign entity bytes. Specify's characteristic numbers survive these edits.
Mixed parts use Add after selective removal of Specify's items; older Add-mode files with
no stored answers continue to open as existing PMI. See [writing modes](design.md).

Mixed-file rewrites require a writer implementing `pmi_scope`. Older writers, including
0.1.7 and the archived Python engine, can read the saved answers without honoring that
scope; their replacement rules do not guarantee preservation of foreign PMI. Use the
scoped writer for these rewrites. Existing 0.1.7 Add-mode files without saved answers keep
their existing-PMI opening behavior in this engine.
