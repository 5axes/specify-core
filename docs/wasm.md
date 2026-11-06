# Browser WASM package

Answer maps accept optional `_specify` geometry evidence and unresolved carry blocks.
Bulk defaults cannot replace dropped answers; write rejects stale evidence. Plain maps
remain compatible but cannot detect stale in-range face indices. See
[answer files and changed geometry](answers.md).

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

`specify-core-wasm` is a workspace member in `crates/specify-core-wasm`. The root remains
`specify-core`, with its existing CLI and tests; the default workspace member is the root,
so ordinary `cargo build` and `cargo test` retain their scope. The binding is a `cdylib`
(and an `rlib` for native verification). It calls the same core rules, geometry and verified
PMI writer as native Specify; there is no copied writer.

## Install a release

Each published GitHub release whose tag starts with `v` gets
`specify-core-wasm-<version>.tgz` and `specify-core-wasm-<version>.tgz.sha256`. The archive
contains `pkg/`: the web ES module, `.wasm`, TypeScript declarations, package metadata and
README. Nothing is published to npm. Pin an exact release and verify the archive before
extracting it:

```sh
version=0.1.11 # choose a release that includes the WASM assets
archive="specify-core-wasm-${version}.tgz"
gh release download "v${version}" --repo pzfreo/specify-core \
  --pattern "$archive" --pattern "$archive.sha256"
sha256sum -c "$archive.sha256" # macOS: shasum -a 256 -c "$archive.sha256"
tar -xzf "$archive"
```

Serve the JS and WASM together (prefer `application/wasm` for the latter), retaining their
filenames. Initialization fetches the `.wasm` beside the JS unless given an explicit URL:

```js
import init, { IntentEngine, review, interpret } from './pkg/specify_core_wasm.js';
await init();
const engine = new IntentEngine('plate.step', new Uint8Array(await file.arrayBuffer()));
try {
  const { analysis, mesh, provenance } = JSON.parse(engine.snapshot());
  const answersJson = JSON.stringify({ 'part.material': 'steel' });
  const state = JSON.parse(engine.review(answersJson));
  const result = JSON.parse(engine.writePmi(answersJson, projectBase64));
  const stepBytes = Uint8Array.from(result.bytes);
  // Download stepBytes; present result.report and state.problems to the user.
} finally {
  engine.free();
}
```

## JavaScript API

All structured inputs and outputs are **JSON strings**, except the constructor's byte input
(`Uint8Array`). Failures throw **strings**. `free()` releases a session's WASM memory.

| Export | Behaviour |
| --- | --- |
| `new IntentEngine(name, bytes)` | Opens one STEP part. Refuses gzip, any count other than one distinct part, any count other than one placement, and a mismatch between haecceity's ordered face sources and the STEP definition. `name` labels the input; it is never read as a path. An assembly, or a part placed more than once, is opened with `IntentEngine.openPart`. |
| `IntentEngine.parts(name, bytes)` | The file's distinct parts, as `specify-core parts` lists them: `[{part, name, product_id, faces, instances, placements, written}]`, `written` saying whether Specify has stored answers for the part. Refuses gzip, as the constructor does. |
| `IntentEngine.openPart(name, bytes, part)` | Opens part `part` (an index from `parts`) of any file: an assembly's part, or a part placed more than once. See "Assemblies" below. Part 0 of a file of one part used once opens exactly as the constructor opens it. |
| `IntentEngine.bom(bytes)` | The file's BOM rows as JSON, one per distinct part, as `specify-core bom` gives them: each field `{value, source}`. Reading never accepts suggestions or allocates numbers. See [BOM](bom.md). |
| `engine.snapshot()` | `{analysis, mesh, provenance: {source_sha256, face_entities, part}}`; `part` is the part opened (0 from the constructor). |
| `engine.review(answersJson)` | `{questions, open, intent, problems}`; adds geometry anchors to `intent.anchors`. |
| `engine.exportAnswers(answersJson)` | Complete answer map as a JSON string: every accepted PMI default explicit, plus `_specify.evidence` and empty `_specify.unresolved`. Refuses unanswered required questions, carry blocks and stale evidence. Recursively sorts object keys; preserves array order. Does not write or mutate the session. Save with `JSON.stringify(JSON.parse(result), null, 2) + "\n"`. Accepted directly by review, writePmi and diff. See [answer files](answers.md). |
| `engine.acceptDefaults(answersJson)` | Answers object with every unanswered default filled to a fixed point, using the same rules as CLI/serve `accept_defaults`. Preserves supplied answers; questions without defaults stay unanswered. |
| `engine.writePmi(answersJson, projectBase64?, payloadJson?, bom?)` | `{bytes, source_sha256, report}`; `bytes` is an array of byte values. Writes and verifies answered PMI and stored answers in memory. Unanswered defaults remain suggestions. `report` is the upstream writer's report; its `output` is the constructor's name. `payloadJson` is `{name, value}`: any JSON value carried in the STEP under `name` (give the project or a payload, not both; see below). |
| `engine.payload(name)` | The JSON text of the payload `name` the opened STEP carries, or `undefined`. |
| `engine.original()` | `Uint8Array` of the part the opened STEP was first written from (without what the writer added, its `FILE_SCHEMA` restored), or `undefined` when it records none or the bytes do not hash to the recorded sha256. |
| `engine.evidence()` | `{schema, face_count, faces}`: the face evidence to keep with answers saved outside the STEP. |
| `engine.rematchAnswers(answersJson, evidenceJson?, automaticJson?)` | Saved answers matched to this part as now recognised: `{answers, unresolved?, migrated?, unverified?, automatic?}`. Does not change the session. See "Answers saved outside the STEP". |
| `review(analysisJson, answersJson)` | Same review composition without a session or geometry anchors. Calls `specify_core::api::review`. |
| `acceptDefaults(analysisJson, answersJson)` | Same accepted answers as the session method, using the supplied analysis. |
| `interpret(analysisJson, answersJson, questionId, text)` | `{value, code, callout?}` for a choice question; includes a diameter callout for `hole.function:` and `diameter.fit:`. |
| `diff(oldName, oldBytes, newName, newBytes, answersJson, title, scenes?)` | Two revisions of a part compared, as `specify-core diff` compares two files, carrying `answersJson` (`{}` carries the answers Specify stored in the old file): `{report, markdown, view}`. `report` is what `report.json` holds and `markdown` is the report headed `title` with no picture. `view` is what a viewer needs to draw the change as `model.png` does: `new` and `old` meshes (as `snapshot().mesh`), `faces` (each new face's class, `changed`, `new` or `ambiguous`, keyed by face number; unchanged faces left out), `gone` (the old faces gone or moved), `motion` (`{rotation, translation}` taking the old part's frame to the new one's, or `null`) and `changes`, the report's "What changed" lines in order, each `{line, features, new, old}`: its text, its features (indices in `report.features`) and their faces on the new part and the old, for linking the lines and the faces. Two revisions of an assembly are compared part by part, `answersJson` by part name: `{report, markdown, parts}`, the assembly's report and Markdown, and each part's `{name, status, old, new}`, with its `report` when it is in both files and, when it changed, its `markdown` (headed `#### name`) and `view`, shaped as a part's. Refuses gzip. |
| `start()` | Installs `console_error_panic_hook`; initialization also calls it automatically. |

Questions use `to_presented_dict()` with `diameter` and `depth` added when present. `open`
is the list of ids returned by `rules::open_questions`. `intent` is `api::preview(...).to_dict()`;
`problems` comes from `problems::problems`. Native front ends can use `api::review` directly.

Defaults and their reasons remain in `questions`, and unanswered ids remain in `open`.
They contribute no PMI to review or `writePmi` until their values are supplied in answers;
in particular, an unanswered general tolerance is absent from `intent.part`, so a drawing
can continue reporting `general_tolerance_missing`. Existing file datums remain available.
The shared #16 settling path still records its automatic ±/plain answers in `state.auto`;
it does not accept datum or general-tolerance suggestions. Diagnostic `problems` continues
to inspect the proposed scheme. These are behaviour changes with no signature changes.

For an explicit bulk acceptance, store `JSON.parse(engine.acceptDefaults(answersJson))`
as the editor's ordinary answers (or use the free function). The call returns answers only;
it does not mutate the session or write PMI. Defaults that reveal further questions are
accepted until no more defaults can be added. Repeating the call is idempotent. Feed the
result to `review`, `settle` (as `state.answers`, with `state.auto = []` to mark the bulk
result as manual) and `writePmi`: accepted items are answered,
editable, absent from `open`, and drawn/written through the existing paths. Accepting
`part.general_tolerance` puts it in `intent.part.general_tolerance`, clearing the condition
for `general_tolerance_missing`. A required question without a default, such as material,
still needs an answer. These two exports are additive; existing signatures are unchanged.

Answered items that await an unanswered datum frame remain omitted from PMI. Review's
`problems` (also returned by serve) includes an additive warning with
`code: "awaits_datum_frame"`, `about` naming the answered question, `datum_frame` listing
the complete required frame, and `awaiting` listing the missing datum letters. Its message
names both the answer and frame. This covers hole positions, explicit ± hole locations
(including partially available measurement planes), runout, flatness and datum controls.
Accepting or answering the missing datums removes the warning and enables the control.
Preview items retain their `default` field for API stability; it is always `false`.

`problems` also states each sealed interior void (an analysis `interior_voids` feature
whose record has `volume_method: "closed_shell"`) as a fact: `severity: "info"`,
`code: "interior_void"`, `feature` (its feature ID), `faces` (its faces), `about: ""` and
`estimated_volume` (the kernel's shell volume, mm³), with the message "Closed internal
cavity ⌀d × l long: no opening to the outside; can't be drilled, tapped or moulded as
modelled." A cylinder closed by caps square to its axis adds `diameter` and `length` (mm);
any other void quotes the volume in the message instead. A grid-sampled chamber behind
openings (`six_axis_grid`) is not sealed and is not stated. No answer changes it and no
question is asked about a void. The same list reaches serve (`questions`), Python
`problems` and both WASM `review`s; revision diffs leave `info` problems out, as before.

An optional project is written through `writer::write_bytes_with` as an `AttributeSet` with
`name: "draftwright editor project"`, `on: None`, one text item with that same name and the
supplied base64 text, and `description`/`general_property` both `None`. The editor's check is
preserved exactly: nonempty ASCII alphanumerics or `+`, `/`, `=` only (no decoding or JSON
validation). Omit the argument for no project attribute. Rewriting a resumed part replaces
previous extras, so resupply the project on every write that should retain it.

A payload (`{name, value}`) is stored the same way under its own `name`, as base64 of the
value's compact UTF-8 JSON; for `name: "draftwright editor project"` that is exactly the
item the editor's `readProject` and draftwright-mcp's `project::read` match, so files written
either way read either way. A name must not be empty, `pmi-assist answers` or
`specify-core original`. Either argument also stores a `specify-core original` attribute,
`{schema: 1, sha256, header, from}`, from which `original()` gives back the part: it takes out
every instance numbered `from` or more, restores the `FILE_SCHEMA` statement, and returns the
bytes only if they hash to `sha256`. Rewriting a file that records its original carries the
record forward, so `original()` remains the part first opened. `writePmi` without a project or
payload drops what the opened file carried: the written file has no project, payload or
original record, so resupply the project or payload on every write that should keep them.
The core sets no payload size limit (`carried` leaves limits to callers); `writePmi` refuses
payload JSON over 8 MiB, its JSON input limit. A part that came with PMI or presentation the
writer replaced, or a file written without a project or payload, gives `undefined`. Core
callers use `specify_core::carried::{attributes, payload, original}` with
`writer::write_bytes_with`.

## Assemblies

A file of several distinct parts, or of one part placed more than once, is refused by the
constructor (its message says to use `openPart`). List its parts, let the person choose one,
and open that one:

```js
const parts = JSON.parse(IntentEngine.parts('assembly.step', bytes));
// [{part: 0, name: 'plate', instances: 1, ...}, {part: 1, name: 'pin', instances: 2, ...}]
const engine = IntentEngine.openPart('assembly.step', bytes, parts[1].part);
```

The part is analysed as `specify-core analyse --part K` analyses it: in its own frame, with
its mates against the file's other parts (read from the same bytes), and its snapshot, review,
`acceptDefaults`, `settle`, `evidence` and `rematchAnswers` are that part's. `writePmi` writes
into the original file as `specify-core write` does for an analysis of that part: the part's
PMI, stored answers and any project or payload go on that part's product definition (once,
however often it is placed); every other part, every placement and what earlier writes put on
other parts are kept byte for byte, so parts are written one at a time, each from a session on
the file the previous write returned. `parts()` then shows each written part as `written`.
`payload(name)` reads the payload written on the opened part (another part's payload of the
same name is not this part's); `original()` gives back the whole file as first opened.

**BOM and process.** An assembly part's questions include the optional `bom.*` questions
(make/buy, part number, description, revision, optional, stock; [BOM](bom.md)), which never
block a write and are not accepted by `acceptDefaults`, and the part's snapshot carries its BOM
row as `analysis.bom`. Explicit `bom.*` answers are written with the part; `writePmi`'s fourth
argument `true` also stores the accepted BOM suggestions and numbers every part of the file
(omitted or `false`, a write without `bom.*` answers adds no BOM data). `part.process`, how the
part is made, is asked of every made part, last ([process](process.md)); a part whose make/buy
is `bought` (answered or in the file, not guessed) is asked no process, and the
assembly product none.

## Answers saved outside the STEP

A project that keeps its answers itself (not only in the STEP it exports) should also keep
`engine.evidence()` from when they were saved, and on opening call
`engine.rematchAnswers(answersJson, evidenceJson, JSON.stringify(state.auto))` instead of
setting aside answers by a list of answer kinds. The result is `resume::Resumed`, as
`analysis.resume` reports answers stored in a STEP:

- With evidence whose face count and fingerprint match the part, feature answers move to
  a unique same-family feature containing their saved faces (`migrated` lists old/new IDs),
  exactly as on reopening a STEP (docs/design.md, "Saved answers across kernel upgrades").
- Without evidence (a project saved before it kept evidence), the answers are trusted by
  ID: a feature answer is kept when the part has a feature with that ID, a datum when its
  faces exist on the part, and the rest are in `unresolved`. `unverified` says the match was
  by ID; surface it to the person, but the kept answers are in `answers`.
- With evidence that does not match (another part's, or the part before an edit), no face
  correspondence is inferred. A feature ID or face index that still exists is no evidence of
  the same feature: IDs renumber between kernels. So every answer on a feature (`hole.*`,
  `diameter.*`) or a datum (`datum.X` and its settings such as `datum.A.flatness`,
  `datum.B.control`) is left in `unresolved`, each with a reason, and `unverified` says why.
  Only the part's own answers (`part.*`) are kept.
- Everything not kept is in `unresolved` (`id`, `feature`, `answer`, `reason`), never dropped
  or applied. Show these to the person to confirm on the feature's current question; nothing
  in `unresolved` is settled.
  Those the default path gave (`automatic`) are marked `automatic: true`: the default path
  answers them again on the current ID, so they need not be reported to the person.
  `automatic` is the given list with migrated IDs renamed and unresolved ones left out; store
  it as `state.auto`.

Use `answers` for review, settle and write. A project saved before it kept evidence opens
without it, so its answers are matched by feature ID.

## Errors

Refusals throw **strings**, preserving the editor's existing error transport: malformed JSON
starts with `Invalid answers:`, `Invalid analysis:` or `Invalid state:`; rule and STEP errors
name what could not be read or specified. Unknown answer/feature/face IDs, invalid choice or
note types, and contradictory choices refuse explicitly. Text answers (`part.material`,
`part.coating`, `part.heat_treatment`) preserve the core rules' Python-compatible truthiness
and JSON values: `null` or `""` clears an answer, and non-string values remain accepted.
Saved state may retain stale or repeated `auto` IDs, as the core state transforms allow.
Partial answers remain valid: unanswered questions stay open. Dormant answers for existing features may remain in a
project when a mode hides their questions. The editor-only state transforms have no analysis,
so they check ID syntax and value shapes; session calls also resolve IDs against the part.

Limits are **64 MiB STEP**, **8 MiB per JSON, project, payload, evidence or automatic-list
argument**, and **64 KiB per input name, payload name, question ID, interpretation text or mode**, measured as UTF-8 bytes. Oversized arguments return
`<field> exceeds the <limit> byte limit` before Rust parses or clones them. wasm-bindgen still
has to transfer the argument into WASM memory before that check. These limits bound individual
inputs, not total browser memory or kernel computation.

Every session method borrows `&self`; analysis and loaded geometry are immutable. Review,
default acceptance, settling and editor edits compute new values, and PMI writing edits a
separate in-memory document and verifies it before returning bytes. A refusal cannot leave a
session half-mutated. Reuse the same engine after a string refusal; dispose it with `free()`.
No JavaScript/TypeScript signatures changed.

The pinned Rust `wasm32-unknown-unknown` target reports `panic="abort"`; `wasm-release`
inherits release and does not override that strategy. `std::panic::catch_unwind` therefore
cannot catch a panic. The target's prebuilt standard library and current build do not provide
an unwinding boundary; adding `catch_unwind` would give false protection. The panic hook only
logs. Unexpected kernel panics remain the separate quiddity-rust#52 scope; allocation failure
and arbitrary traps are not converted to refusals here.

`tools/probe_wasm_panic.py` makes a disposable source copy, injects a panic into
`Session::review`, builds with the same profile and optimizer, and runs
`scripts/wasm-panic.mjs` in a separate Node process. It introduces no production export or
panic feature. On Rust 1.99 / Node 26.9.0, the panic throws `WebAssembly.RuntimeError:
unreachable`. A subsequent shared review succeeds, but `free()` fails with `attempted to take
ownership of Rust value while it was borrowed`: wasm-bindgen's borrow bookkeeping never
unwound. An unexpected trap requires discarding the module, not treating it as a string refusal.

`scripts/wasm-hostile.mjs` exercises malformed STEP/JSON, contradictory answers, unknown IDs,
wrong types, non-finite feature records, invalid editor state and oversized inputs against the
release web package. After every refusal it checks an identical valid review and snapshot on
the same engine, then exercises default acceptance, verified writing and disposal. It also checks legacy text
values and stale automatic-answer state through review, writing and editor transforms. Native
compatibility tests compare these results with direct core calls. Both the
CI `wasm` job and release workflow run it beside parity and the existing safeguards.

## Build and verify

Rust comes from `rust-toolchain.toml`. The checks use Python 3.11+ and Node (CI uses Node 24).
Install wasm-pack 0.15.0 and the WASM target:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --version 0.15.0 --locked
wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked
python3 tools/check_wasm_api.py
python3 tools/wasm_size.py
python3 tools/check_quiddity_pin.py
python3 -m unittest discover -s tools -p 'test_*.py'
cargo build --locked --release -p specify-core-wasm --example parity_reference
node scripts/wasm-parity.mjs
node scripts/wasm-hostile.mjs
node scripts/wasm-limits.mjs
```

The root `[profile.wasm-release]` inherits release with `lto = true`, `codegen-units = 1`
and `opt-level = 3`. Both workflows also run wasm-pack's additional wasm-opt pass. Do not
use `--no-opt` for the size check or release package. wasm-pack downloads its optimizer
when needed. If its default cache is unwritable, set `WASM_PACK_CACHE` to a writable directory.
Generated `pkg/` is ignored by Git; its normalized TypeScript API is reviewed separately.

Measured with Rust 1.99, wasm-pack 0.15.0, wasm-opt 117, Node 26.9.0 and the committed lockfile
on 2026-10-16:

| WASM binary | Bytes | MiB |
| --- | ---: | ---: |
| Raw, optimized | 10,197,823 | 9.73 |
| gzip, level 9 | 3,157,943 | 3.01 |
| Brotli, quality 11 | 2,148,100 | 2.05 |
| Raw ceiling | 15,000,000 | 14.31 |

These measure `specify_core_wasm_bg.wasm`, excluding the small JS/TypeScript/metadata files.
CI and releases print current measurements and the remaining size headroom; sizes can change
with compiler or dependency updates. The committed baseline and ceiling are in
`tools/wasm-size-budget.json` (see safeguards below).

File loaders, disk writers (including staging names, process ids and clocks), and the JSON-line
service are cfg-gated out on `wasm32`. Memory loading, analysis, reviews, meshing, anchors and
verified writing remain available. Native file-loading APIs are unchanged. The native numeric
helper `pyfmt::fmt_rules` now returns `Result<String, FormatError>` so callers can propagate
non-finite dimension refusals; valid CLI/rule output is unchanged. At the pinned
kernel revision, `cargo tree --target wasm32-unknown-unknown` contains neither `uuid` nor
`getrandom`, so no JS randomness feature or backend override is needed. If a future kernel
introduces them, enable the appropriate browser feature only in WASM target dependencies.

The Node check loads the **web release package** directly, then runs a native reference on a
plate, a turned pin, a placed part and the GRM-02 shaft. With `QUIDDITY_CORPUS` or
`SPECIFY_CORPUS` configured it also checks `gramel/GRM-03_thumbwheel_drive_screw.step`.
It compares snapshots (analysis, mesh, provenance), session/free reviews and `acceptDefaults`,
idempotence, preserved answers, questions without defaults, settle, choice interpretation,
bare/project STEP bytes and reports, and resumed answers. Accepted-default PMI bytes and
reports also match native `apply(..., true)`. STEP bytes, reports, ids and strings compare exactly; geometric floating values allow
relative/absolute error of `1e-10` for native/WASM math differences. Mesh checks compare
metadata, per-face vertex sets, area and oriented area: circular planar caps can choose
different Delaunay diagonals on native and WASM while describing the same surface. It also checks thrown-string
errors, invalid project input, gzip/assembly/multiple-placement refusals, and reports raw,
gzip (level 9) and Brotli (quality 11) WASM sizes.

CI builds and runs this check on pull requests and pushes to `main`. The release workflow
checks that both crate versions equal the `v` tag, builds with the same profile, reruns parity,
archives `pkg/`, computes SHA-256, and uploads both files with `gh` and `GITHUB_TOKEN`. Only
that release job has `contents: write`; other jobs retain read-only contents access. Release
preparation must update both package versions and the binding's exact core dependency together.

## Release safeguards

Blind tapped holes expose `hole.full_thread:<feature>` using the existing `text` question
kind, string `default` and answer map. The default is drill depth minus three pitches,
rounded down to 0.5 mm (at least one pitch), or an explicit inline tapped depth. Submit a
positive number in mm (optionally suffixed `mm`) that leaves three pitches; edits are
unrounded and override inline depth. The depth question is an optional `refine`, excluded
from `open`; it adds no review or save obligation to older files. An answered tap retains
its inline depth, or its computed manufacturing depth, in preview and written PMI without
a separate depth answer. Showing the question does not insert an accepted answer (#28);
`acceptDefaults` explicitly records it like other defaults. Saved depth edits resume under
the same feature-migration rules as hole
function answers. Through holes and holes without known depth have no depth question.

Turned diameter choices include matching ISO 261 coarse and fine threads within 0.05 mm
of the nominal major diameter, inclusive; outside that range no thread choice is offered.
Choice values and interpreted text use the same canonical thread answer and STEP PMI.

### Reviewed WASM API

`crates/specify-core-wasm/api.d.ts` is the reviewed snapshot of wasm-pack's
`pkg/specify_core_wasm.d.ts`. `tools/check_wasm_api.py` removes generated comments (including
version noise), normalizes whitespace and sorts declarations and class/interface members.
It retains the public signatures and initialization types. After an intentional API change:

```sh
wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --locked
python3 tools/check_wasm_api.py --update
```

Review and commit the snapshot diff, and add a **new bullet** under `### WASM API` inside
`## Unreleased` in `CHANGELOG.md`. An existing note or a note in a released section does not
count. CI compares against the PR base or the commit before a main push; changing the snapshot
also triggers this requirement, even when it already matches the generated declarations.
A generated/snapshot mismatch fails unless the same change adds that changelog bullet.
The check prints the diff and snapshot update command. Locally `--base-ref <commit>` selects
the comparison commit (default `HEAD`).

### WASM size ceiling

`python3 tools/wasm_size.py` measures the optimized `specify_core_wasm_bg.wasm`: raw bytes,
gzip at level 9, and Brotli at quality 11. It uses Node's built-in zlib, already needed for
parity, with no extra Python packages. Both workflows fail if **raw optimized bytes** exceed
`ceiling_bytes` in `tools/wasm-size-budget.json`; equality passes. Every run prints all three
sizes and headroom in bytes and as a percentage of the ceiling. Compressed figures are
recorded for transfer-size visibility and do not have separate limits.

The initial ceiling is approximately 15% above the measured baseline, rounded upward to
100,000 bytes: 11,800,000 bytes, leaving 1,602,177 bytes of initial headroom. On 2026-10-29 the
maintainer raised it by 10% to 12,980,000 bytes, after PR #57's revision-report work measured
11,807,493 bytes in CI (7,493 over). On 2026-10-31 the maintainer raised it to 15,000,000 bytes,
when `pmiScene` (PR #83) measured 12,903,620 bytes, 76,380 below the old ceiling.
Raising it requires a deliberate, reviewed JSON edit; the size checker never
updates the budget automatically. When rebaselining, record the three measurements and
tool versions together and explain the increase in the changelog.

### Matching editor kernels

The editor passes STEP files between Specify's browser package and Draftwright's drawing
package. Different quiddity kernels can number the same faces differently.
`python3 tools/check_quiddity_pin.py` reads **both** `haecceity` and `quiddity` pins from the
root `Cargo.toml` and compares them with `pzfreo/draftwright-rust`'s latest published release.
Prereleases count; drafts do not. Tags and abbreviated revisions are resolved to commits,
so a full rev and a tag at that same commit match. Internal disagreement between the two
kernel crates also warns. The check prints each package's pins and the matching commit.

Public GitHub requests work without authentication; `GITHUB_TOKEN` (or `GH_TOKEN`) is used
when set. CI runs this on main pushes. Before a release upload, the release workflow also
passes `--release "$TAG"`: a mismatch emits a GitHub Actions `::warning::` annotation and
uses `gh release edit` to append a `Kernel note:` line listing each package's pins. Reruns
replace that line without duplicating it or changing the other release notes. A mismatch
never fails a release, because one repository necessarily releases first after a kernel bump.
Network or release-note update failures also warn without blocking the upload. The command
without `--release` only reads GitHub and never edits a release.

## Switching the editor

Keep the editor's local build until a tagged release actually contains these assets. Download
and checksum a pinned archive in the editor's dependency/update step, copy `pkg/` into its
served WASM directory, and change imports from `dw_intent.js` to `specify_core_wasm.js` (and
any explicit WASM URL from `dw_intent_bg.wasm` to `specify_core_wasm_bg.wasm`). Keep the same
initialization, `IntentEngine`, JSON calls and optional project argument. Then remove
`crates/intent`, its copied writer, workspace membership and its WASM build from editor CI.
The editor's separate drawing engine and its WASM build remain independently managed.

For #65, `snapshot().analysis.characteristics` exposes the file's stable inspection numbers,
including default part textures and resumed PMI. `diff` carries number history in its returned
`report.answers`; preserve the opaque `_specify_characteristic_numbers` member when passing
those answers to the revised part's `writePmi`. It is geometry-checked and consumed on write.
Numberless files omit the list. See [characteristic numbers](characteristic-numbers.md).

`diff` also returns `report.pmi.items`, with `pmiScene` ids, numbers and statuses for
highlighting changed requirements. The exported `PmiItemChange` and `DiffPmi`
TypeScript interfaces describe this JSON. Items include
`faces: {old: {face: meshIndex}[], new: {face: meshIndex}[]}`, including support for
semantic-only requirements with no graphic. Missing sides and part-level support
use empty arrays. `old_id` appears on paired changed/renumbered items only when
it differs from `id`; use `old_id ?? id` to ghost the old callout. Renumbering
semantics and characteristic-number matching are unchanged.

A thread's own `thread manufacturing` note folds into its thread item when owner
id, face anchors and drawing wording match. Manufacturing-only edits still change
that item, with drawing wording in `old` / `new`; other notes remain separate.
The optional scenes retain both semantic records. See the [shared fold rule](diff.md#pmi-requirements).

Pass an optional seventh argument `true` to `diff` to include
`report.pmi.scenes: {old: PmiScene, new: PmiScene}` (also in each matched assembly
part's report). Omitted/false keeps scene documents out of the report; removing
`scenes` from an opted-in report gives the exact default JSON. The scenes are
already built by the comparison: old uses explicit given answers, or actual file
PMI with saved interview metadata when none are given; new uses carried answers
unless it has Specify PMI, when actual semantic requirements win. They
have the same shape as `pmiScene`, with fresh semantic notation rather than saved
As Drawn ink. `view` continues to describe geometry; use the scene pair and
`report.pmi.items` for the PMI overlays shown in the CLI PNG. Use this pair directly
in `/diff`, avoiding both TypeScript scene selection logic and two more scene derivations. See [native/CLI/Python options and
PNG behaviour](diff.md#pmi-requirements).

The old `stored` / `existing` PMI lists
are deprecated for one release. See [PMI requirement comparison](diff.md#pmi-requirements).

For #77, dimension entries in `snapshot().analysis.characteristics` also expose optional
stored `origin` and `source`, including nominal-only general dimensions. `writePmi` writes
the same per-dimension UDAs as native. See [dimension origin and source](pmi-origin.md).

## PMI scenes

`JSON.parse(engine.pmiScene(JSON.stringify(answers)))` returns the PMI for the current
answers on the already-open part. `openPart` uses the same API, in the part's own frame.
The session caches the source STEP and semantic PMI; answering does not reload geometry.
Pass the editor's full `{answers, auto?, gdt?}` review state instead of just answers when
available: `auto` identifies the questions whose values are unreviewed automatic defaults.
An explicitly supplied value is `answered`, even when it equals a question's default.
Unanswered suggestions produce no PMI, just as in `review` and `writePmi`.

Both calls return a JSON string. The generated declarations export `PmiScene`,
`PmiSceneItem`, `PmiSceneItems`, `PmiSceneGraphic`, `PmiSceneTextPlacement`,
`PmiScenePlane`, `PmiSceneView`, `PmiSceneMetadata`, `PmiSceneStatus` and `PmiSceneId`.
For example, `const scene: PmiScene = JSON.parse(engine.pmiScene(JSON.stringify(answers)))`.
Semantic values retain the packaged haecceity schema (`unknown` in the row interface);
scene structure and presentation metadata are typed. The result is:

```ts
{
  pmi: /* haecceity.pmi/1 document */,
  presentation: /* haecceity.presentation/1 document */,
  specify: {
    [itemId: string]: {
      status: 'answered' | 'default' | 'general' | 'existing';
      question?: string; // extra.N for a face-picked extra
      number?: number;   // characteristic number, as the STEP writer allocates it
      notation?: string; // this requirement's own semantic note wording
      origin?: 'requirement' | 'general_tolerance' | 'imported';
      source?: {schema: number; faces: string; support: number[]; parameter: string};
    }
  }
}
```

The two haecceity documents retain their schemas: units are explicit and semantic decimals
are strings. `pmi.pmi` holds collections of `{id, value, identifier?}`; references use `id`.
The optional `identifier` preserves the source characteristic number, including duplicates
(haecceity v0.4.3); it is not a reference key.
`presentation.items[id]` holds graphics with `name` (full notation), `text` (cells),
`polylines`, `triangles`, and `source: 'generated' | 'file'`. With haecceity v0.4.3,
optional `id`, `plane_id` and `text_placements` retain model-space text origins, axes,
heights/alignment when stated. Each placement can carry `stroked?: boolean`: when
true, its glyphs are already present in that graphic's polylines, so skip rendering
that text run again. Absent or false permits rendering the text literal.
The scene prunes view membership after filtering or replacing graphics, and removes
views with no remaining graphics; document `planes` and `views` preserve annotation planes
and saved-view membership. Generated presentations include plane views, ladders/callouts and one numbered
GENERAL NOTES block for part settings. v0.4.3 places size callouts above feature
control frames and reserves label ink during saved-view packing.
Feature-on scene and written STEP presentation output therefore changes. Datum-feature
graphics also appear under their datum ids.
These fields survive a live review that adds PMI beside a file's existing callouts.
Pattern sizes, locations and tolerances include `6×` in the name and first cell;
datum labels carry only their letter. `presentation.unlinked` retains graphics with no semantic
item. Render text as DOM text. Face anchors are `{face: meshIndex}`, including named STEP
faces and assembly parts; indices address `snapshot().mesh.faces` and the analysis face list.

Specify ids come from the requirement, such as `size:holes:9`, `datum:A`, or an extra's
question, with reference supports distinguishing location axes and the question
distinguishing a member's measurement from its pattern's. General locations without a
feature id use their face supports. Changing a value leaves
the id intact. Foreign items retain haecceity's `ID_ATTRIBUTE` or content-derived id.
The same ids key semantic references, presentation and the requirement overlay `specify`.
Material is the scalar exception: Specify exposes `part.material` as a companion alias
for `pmi.pmi.material` in `items` and `specify`. No semantic row is added, and
`semantic_ids` lists only the PMI document's own ids, so it never contains `part.material`. The live scene replaces part-note graphics from the file with one combined
block, retaining imported text and putting material last. A file-only scene with one block
retains the file callout and its STEP identity; its source is `file`, including graphics previously
written by haecceity. Material links follow the file's DMIA and part provenance. Reopening
an appended file with several part-note blocks composes one scene block too. Feature
notes keep their feature and use a generated fallback when the file has no graphic.
For note and characteristic labels, use `specify[id].notation` when present: it is
that requirement's semantic wording (for example `Keep dry` or
`GENERAL TOLERANCES ISO 2768-m`). Several requirements can alias one GENERAL NOTES
graphic, whose `name`, text cells, strokes and id describe the whole block. Those
shared fields must not be used as an individual requirement's label or identity.
Supporting features, geometry and units remain in the haecceity documents but are omitted
from `specify`, as are saved-answer and number-history attributes. Their graphics may
therefore have no overlay entry. Thread/knurl manufacturing attributes inherit the owning
requirement's question and status. Part settings use the rules' question ids, including
`part.coating`; unreviewed values marked by `auto` are `default`, and absent or empty
settings produce no PMI, as in the writer. Foreign numeric `ID_ATTRIBUTE` values stay ids;
`number` is exposed only when the item also matches Specify's characteristic-number history.
Stored #77 origin and
source are read from the file; `general_tolerance` maps to `general`, `requirement` to
`answered` (or `default` when the review state marks it automatic), and foreign PMI to
`existing`. Legacy files without metadata do not gain inferred origin/source.

For view-only use, import the free `pmiScene` function and call
`JSON.parse(pmiScene(stepBytes, partIndex))`; omit the index for a single-part file.
No interview is required. File-owned As Drawn callouts retain `source: 'file'`, including
NIST CTC-01's graphics. For `/diff`, request `report.pmi.scenes` from `diff` instead;
changes link to those exact scenes by `id`, optional `old_id` and characteristic
`number`, with per-side face anchors for focus.

Native Rust exposes `specify_core::pmi_scene::{Scene, pmi_scene}`. Construct `Scene::new`
once with a `LoadedPart`, then call `scene` with cached analysis and answers, or
`scene_with_state` with `tolerancing::State`. Omit answers for the file's stored PMI.
Python exposes `specify_core.pmi_scene` and `specify_core.api.pmi_scene`: paths and
`LoadedPart` handles accept answers or review state; raw `bytes` are view-only. Its
packaged schemas are `specify_core/schema/haecceity.pmi-1.json` and
`specify_core/schema/haecceity.presentation-1.json`.

Scene export uses haecceity's serialise-only `json` feature in native, WASM and Python.
`json-import` is not enabled. The Python package ships the v0.4.7 schemas as data; its
smoke tests validate exports with Python's `jsonschema`, keeping schemas and import
validation code out of the WASM binary.

Haecceity v0.4.4 adds optional `polyline_roles`, parallel to
`polylines`: `text`, `frame`, `leader`, `extension` (witnesses), `symbol`, or
`dimension` (dimension lines and arrows). Each generated text placement also has
`cell_box: [[xmin,ymin],[xmax,ymax]]` in plane millimetres relative to its origin,
with x along `x_axis` and y along `normal × x_axis`. Hosts can fit and centre their
own font in that box. File graphics omit roles/boxes unless known. These additive
fields keep schema `/1`; strict `deny_unknown_fields` consumers must update their
types. Specify passes both through unchanged. Native, WASM and Python builds pin the released v0.4.7 tag; no local path
patch is needed.

In the role array, `text` includes explicit ISO glyph strokes already represented
in cell strings (⌖, ⌀, Ⓜ). Remove those when replacing whole cells with host-font
text. Standalone finish marks and leader scope rings remain `symbol` geometry.

Assembly `diff` answers use `{partName: answerMap}`. `acceptDefaults` returns the answer map as JSON; in JavaScript use
`JSON.stringify({[partName]: JSON.parse(engine.acceptDefaults("{}"))})` (and add the other
parts' maps). No `answers` or `parts` envelope is accepted. Validation follows assembly
routing; an invalid nested answer reports both part name and question id. Single-part
answer validation and JSON size limits remain in force. See [diff](diff.md#assemblies).

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

With v0.4.7, native/CLI, Python and WASM write reports expose these optional keys,
omitted when empty:

- `geometry_carrier_exemptions`: `{part, condition, aspect, feature}` records, with
  STEP entity ids and conditions `shared-carrier-controlled-pair`,
  `shared-carrier-datum-target-support`, `kept-carrier-datum-member`,
  `kept-carrier-independent-feature`, `kept-carrier-datum-feature` or
  `kept-carrier-datum-composite`.
- `preserved_normalizations`: `{id, normalization}` records for original entities
  Add retained instead of normalizing. Values are `legacy-text-rechunking`,
  `kept-usage-factoring`, `functional-sar-isolation`, `functional-sar-unwrapping`
  and `legacy-endpoint-retirement`.
- `presentation_warnings`: strings naming callout placement or glyph diagnostics.
- `retained_shared`: ids of auxiliary support Replace kept because PMI it cannot
  read still uses it (v0.4.7); datum links are rebound only to equal replacements.

`PmiSceneGraphic.leader` is an optional model-space leader tip in millimetres.
The packaged v0.4.7 presentation schema includes it; neither packaged schema changed from v0.4.6.
Specify passes the field through but does not yet accept a source-presentation companion.
Add preserves original entities in the kernel. It can add a separate datum feature
on a kept carrier; an upgrade requiring original-entity repairs instead fails with
ids and repair details. Specify never automatically retries a refused Add with Replace.
