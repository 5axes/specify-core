# Changelog

All notable changes to specify-core-rust. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- `parts` (CLI, serve, Python, WASM `IntentEngine.parts`) gives each part `same_as`: the index
  of the first part it is a copy of, or `null` (#150). A copy has the same geometry moved by a
  proper rigid motion, proven face by face (quiddity's `congruent`, the check draftwright's pack
  uses), and states the same BOM facts (part number other than a bare `PRODUCT.id`,
  description, revision, make/buy, material, optional, stock) and the same saved answers.
  Names and defaults never decide it. Every part keeps its own index (decision D1), so saved
  answers are unaffected; a picker can show `M3×8 hex bolt ×4` from the first and its copies.
  Only parts sharing a face count with another are built, so a file of distinct parts costs
  the same as before.

### Changed

- Move native, WASM and Python builds from quiddity/haecceity v0.4.8 to v0.4.9 (`17a883aa`),
  taking quiddity with `default-features = false, features = ["presentation"]` so its new
  `cli` feature (clap) stays out of every build. Recognition `/6`, fingerprints `/3`, the PMI
  reader string `/19` and both packaged schemas are unchanged. One corpus expectation changes:
  `mfcadpp_holdout/363` gets quiddity #215's circular blind step on faces 20 and 24 in place of
  `cylindrical_profiles:20`.

### WASM API

- A refusal to write a BOM into a file with no assembly structure (`writePmi` with `bom=true`,
  or a stale `bom.*` answer) now carries the stable code `bom_no_assembly:` (`bom::NO_ASSEMBLY`)
  before its message, so the editor can match the code instead of the prose. 0.1.13 changed
  the `bom=true` message, which broke the editor's text match (its fallback for loose parts).

## 0.1.13 (2026-11-07)

### Changed

- Move native, WASM and Python builds from quiddity/haecceity v0.4.7 to v0.4.8
  (`449ab51d`). Writes that carry general notes add a default AP242 Notes saved view
  facing their annotation plane (quiddity #198); Add keeps foreign views and reuses its
  own, edition-1 Add skips it, and Remove/Replace clean it up. New draughting models get
  unique names, materials without a separate name repeat their id as description, and
  presentation follows the CAx-IF PMI Recommended Practice 4.1 (#193). Recognition `/6`,
  fingerprints `/3` and both packaged schemas are unchanged.
- Adopt reader `haecceity-pmi-read/19` in place of /17.
- Add optional `saved_view_findings` (`{kind, part, ids, entity, detail}` kernel findings
  about the Notes saved view) to native/CLI, Python and WASM write reports.
- Written STEP bytes change for these 0.4.8 causes only: the Notes view, the material
  description, RP 4.1's FILE_DESCRIPTION reference and curve-set names, and `PMI callouts (2)`
  model names. As a result, all fourteen cross-engine byte goldens are regenerated and
  recaptured, and the BOM opt-in, legacy settings-only and unnamed-revision hashes are
  updated. An Add over Specify's own output now rewrites the kernel's own Notes view in place
  and retires its private camera; every other original entity keeps its bytes.
- Known differences: the frozen Python reader no longer reads a material written without a
  separate name (`material identification (no separate name)`) on single-part files; Rust
  still reads it.
- The carried `specify-core original` record also stores the original `FILE_DESCRIPTION`
  (`description`), because v0.4.8 writes add the PMI RP 4.1 reference to it (#193).
  `original()` (native and WASM) restores it, so it again gives back the source exactly.
  Without this, every new write returned `undefined`. A record without `description`, from an
  earlier write, gains one when that file is rewritten. Readers that restore only
  `FILE_SCHEMA` (the editor's `originalOf` fallback) now differ from the source in that
  statement.
- The BOM questions are `routine`, not `required` (`reviewed()` no longer overrides them), with
  short prompts (`Made or bought?`, `Description`, `Part number`, `Revision`, `Optional item?`),
  `why` "Shown in the drawing's parts list." and the make/buy proposal as the `reason`
  ("From the file", "Your answer", or "Proposed: made/bought (…)"). The one `check` is a
  standard-hardware name on a part recognised with pockets or keyways (proposed made; its reason asks which). Part number and revision
  carry no suggestion. `bom.stock` is no longer asked: it follows make/buy, is still proposed
  by `--bom` and an explicit answer is still written. Suggestions stay out of `default`, so
  `--accept-defaults` writes no BOM field.

### Fixed

- The `bom.*` questions are asked only of a file the BOM can be written into: an assembly
  product definition with a shape and an assembly occurrence of every part, the writer's own
  condition (#149). A file of several products with no `NEXT_ASSEMBLY_USAGE_OCCURRENCE` is no
  longer asked them, and writes without `--bom`; `--bom` on it is refused as on a single part
  (now saying the file has no assembly structure, so there is no BOM to write),
  and a stale `bom.*` answer is refused by the writer as before. `bom` still lists its rows.

### WASM API

- `WriteReport` adds optional `saved_view_findings: SavedViewFinding[]`, absent when empty;
  new `SavedViewFinding` (`kind`, optional `part`, `ids`, `entity`, `detail`).

## 0.1.12 (2026-11-06)

### Added

- How a part is made: `part.process` (`machined`, `3D printed: FDM`, `resin`, `powder`,
  `metal`, `sheet metal`, `moulded`, `cast`), asked last of every made part, after all
  existing questions, and never blocking a write. With no answer and no file value a part is
  machined and every question, default and write is unchanged; the machined default is never
  added by accepted defaults. A printing-class material suggests its printed process
  (attention `check`). The process sets defaults only: printed parts default to a ± general
  tolerance from vendor figures (written as a part note, as the kernel's general tolerance is an
  ISO 2768 class), clearance holes without an H11 fit, with `heat-set insert` and
  `printed thread` offered (written as hole notes) and ± locations, and are not asked roughness, edges or coating;
  sheet metal, moulded and cast parts are not asked roughness. An answer such a process leaves
  unasked, or only another process offers, is an `unasked` row (#126). Stored as the CAx-IF UDA set `specify process`
  (`manufacturing_process`) on each product definition, with the BOM's
  `default.manufacturing_process` marker for an accepted suggestion; read back as answered,
  file (another tool's) or default, and a stated value is not asked. Bought parts are asked
  no process. Shared by native/CLI, serve, Python and WASM questions, apply and write; an older
  Specify refuses an answer file holding `part.process`. See `docs/process.md`.
- `specify-core completions SHELL` prints a shell completion script (bash, zsh, fish,
  powershell or elvish) to stdout; the README gives the install one-liners.

### Changed

- The command line is parsed by clap instead of a hand-written argparse-style parser. Commands,
  arguments and defaults are unchanged, and so are exit codes for everything that worked before
  (usage errors still exit 2), but help and
  usage errors are clap's text rather than argparse's, and `-h` and `--help` both give a
  command's full help. The top-level help now lists `diff`, `open` and `bom`, which were left
  out to match Python's text, and a prefix of `--help` at the top level (`--he`, `--h`) prints
  the help (exit 0). Positionals after an option are taken: `write S A B -o OUT C D` writes both
  pairs and `write S A -o OUT B` the pair `A B`, where argparse left `C D` and `B`
  unrecognised. The WASM build leaves the command line out (the root crate's default `cli`
  feature, off for `specify-core-wasm`).
- `specify-core open`'s help and the README name the prompt Chrome shows the
  first time ("draftwright.io wants to access other apps and services on this device"): Allow
  lets the page fetch the file; if it did not open while the prompt was up, stop the command
  and run it again; a Block is undone in draftwright.io's site settings.

## 0.1.11 (2026-11-05)

### Fixed

- The BOM `stock` proposal follows make/buy: bought parts (such as the gramil grub screw,
  washers and pan-head screws) are proposed `from stock`, made parts `to be made`, and
  `not known` proposes nothing. Before, every part was proposed `to be made`, so accepted
  BOM suggestions marked bought hardware as made in-house. Applies to `bom` rows, the
  `bom.stock` suggestion and `accept_suggestions` (where an explicit `bom.make_buy` answer
  decides it); explicit and file stock values are unchanged. A stock suggestion accepted
  into a file before this fix is re-proposed from make/buy when the BOM is read.

## 0.1.10 (2026-11-04)

### Added

- Review `problems` state each sealed interior void (an `interior_voids` feature whose
  kernel record is a `closed_shell` measurement) as an `info` fact with
  `code: "interior_void"`, its `feature`, `faces`, the kernel's `estimated_volume` and,
  for a capped cylinder, `diameter` and `length`: "Closed internal cavity ⌀2.5 × 12
  long: no opening to the outside; can't be drilled, tapped or moulded as modelled."
  Grid-sampled chambers behind openings are not stated. Shared by native review, serve,
  Python `problems` and WASM `review`; no question is added or removed.
- `specify-core open STEP` opens a STEP file in draftwright.io/specify (`--editor`: /editor)
  with nothing to click and nothing uploaded: the file is served once on 127.0.0.1 at a
  random one-off address, to the editor's origin only (with Chrome's Private Network Access
  preflight), and the command exits once it is fetched or after 10 minutes. `--site ORIGIN`
  points it at another deployment; `--no-browser` prints the address instead of opening it.

### Changed

- Move native, WASM and Python builds from quiddity/haecceity v0.4.6 to v0.4.7
  (`dcba9d38`). Sealed interior voids are reported only as `interior_voids` (each
  sealed shell gets an exact `closed_shell` record), no longer also as holes.
  Dimensions with no stated value are written through unchanged; a projected zone
  with no end geometry is read as a `fact` finding. An Add needing the new R3
  integer-literal repair is refused with its ids, as R1/R2 are. Recognition `/6`, fingerprints
  `/3` and both packaged schemas are unchanged.
- Adopt reader `haecceity-pmi-read/17` in place of /15.
- Add optional `retained_shared` (ids of auxiliary support Replace kept for PMI it
  cannot read) to native/CLI, Python and WASM write reports.
- Known differences: NIST CTC-04's projected zone #18897 is a `fact` finding (formerly
  nonconformance); cgb217's void shell (faces 361-363, from `BREP_WITH_VOIDS`) is a
  Rust-only interior void, and its two former cylindrical-profile entries are kept as
  `resolved`. Cross-engine byte goldens, readbacks and no-BOM hashes are unchanged.

### WASM API

- `WriteReport` adds optional `retained_shared: number[]`, absent when empty.

## 0.1.9 (2026-11-04)

### Added

- Export the complete answer set before writing with `questions --resolve-defaults
  --step STEP`, serve `export_answers`, Python `api.export_answers` and WASM
  `engine.exportAnswers`. Accepted defaults are explicit, including dependent questions;
  sidecars carry verified `_specify.evidence`, sort keys recursively and refuse incomplete
  sets or unresolved carries. Existing partial-default operations keep their shape.
  WASM export and write check evidence before feature IDs, so a full old map with
  disappeared features reports stale geometry rather than an unknown feature.
- Add Amy's cadgen PMI plate (gist, MIT) as a fixture with cadgen's own AP242 semantic
  PMI. Tests check that it reads once as foreign PMI: ISO 2768-mK general tolerance, basic
  30/50, and no extra Specify size or location requirement beside its dimensions.

### Changed

- Move native, WASM and Python builds from quiddity/haecceity v0.4.4 to v0.4.6
  (`1ebf8cb6`).
  Add preserves original entity bytes in the kernel; a datum on a kept carrier
  gets a separate `.T.` datum feature. Add upgrades requiring source repairs
  refuse with entity ids and repair details, without a Replace retry. PMI-less
  inputs continue selecting Replace before writing.
- Share per-face geometry carriers and reference datum features directly (quiddity#181).
  Specify stamps new complex carriers and protects carriers and face support shared
  with foreign requirements during owned-item removal; characteristic numbers stay stable.
- Adopt reader `haecceity-pmi-read/15` in place of /14, including authored standalone
  members named `haecceity.feature/1`. A datum whose feature reaches no face is an
  `Unresolved` finding, surfaced as a new `PMI unresolved datum …` analysis/CLI warning.
- Refresh cross-engine STEP byte goldens and four no-BOM hashes for shared-carrier
  and direct-datum links and quiddity#184's notes placement clear of drawn ink. Per-case
  reasons are documented in the fixture README. All fourteen independent Python
  semantic readbacks remain identical; both part-index refusals stay fixed.
- Add optional `geometry_carrier_exemptions` (`{part, condition, aspect, feature}`),
  `preserved_normalizations` (`{id, normalization}`) and `presentation_warnings` to
  native/CLI, Python and WASM write reports. Update the packaged presentation schema
  for optional model-space `Graphic.leader`; the semantic schema stays unchanged.
- Source the v0.4.6 kernel from `pzfreo/quiddity`, renamed from `pzfreo/quiddity-rust`.
  The crates remain named `quiddity` and `haecceity`; GitHub redirects the former URL.

### Fixed

- An answer valid in form whose question is not asked given the other answers (e.g.
  `datum.A.flatness` or `hole.position:*` with `part.locating` `"± dimensions"`) is listed in
  every write report's `not_written` with the reason, instead of being dropped silently. It
  stays stored and exported; `apply` does not refuse it. Rust `api`, CLI, serve and Python now
  refuse an answer ID no question can have (`unknown answer ID`), as WASM already did.
  A carry block applies only while its question is asked; otherwise it is listed the same
  way, and it refuses again once later answers ask the question. A STEP keeps such a block
  in its stored-answers payload (`unresolved`, beside `answers`) and restores it on reopening;
  older builds ignore it. Unknown IDs stored by earlier versions are left out on reopening
  with a warning, not refused. `preview` no longer computes these rows; the editor's write
  adds them. For #126.
- Preserve dropped revision-stable datum/part/BOM answers as `_specify.unresolved`
  carry blocks: CLI, serve, Python and WASM bulk defaults cannot silently replace them.
  Failed feature carries remain report-only: old face-based IDs may name a different
  feature in the destination. Explicit answers clear stable question blocks, which
  `questions` and review now show as required, with no default and the diff reason.
  Diff answer maps also carry the in-STEP face evidence; write refuses
  stale evidence before output, naming CLI answer files and directing callers to diff.
  Plain maps remain compatible and cannot detect changed in-range face identities.
  **Re-run `diff` to regenerate changes files from released v0.1.8 and earlier**: their
  metadata-free `changes/answers.json` cannot reveal dropped answers, so the literal
  old-file `write --accept-defaults` repro still exits 0 on updated tools.
  Older Specify/editor WASM builds reject new `_specify` maps as `unknown answer ID`;
  older CLI and Python installs may accept and ignore `_specify`, without its evidence and
  carry-block checks. Old plain files remain compatible with new tools.
- For #123, #124, #125: diff maps an added requirement's (`extra.*`) `faces` and
  `reference` through the same face matching as datums, on unnamed parts as on named
  ones, instead of keeping old indices that could name another feature under an `ok`
  verdict. A face with no match leaves it `orphaned`, several `ambiguous`. A carried
  answer the new part refuses (a size whose two holes now differ) is listed as
  `unresolved` with the refusal instead of raising, so the rest of the report is
  returned. A refusal naming no answer is pinned only on one whose removal clears it;
  otherwise the answers stay carried and the report adds a `carry_refused` problem.
  Dropped `extra.*` answers are `_specify.unresolved` carry blocks: write
  and bulk defaults refuse until the requirement is answered again or its entry removed.
- Diff refuses stale answer evidence with exit 2 and asks for the matching old part,
  instead of suggesting `diff` again while already comparing revisions.
- Diff folds a thread's own `thread manufacturing` note into its thread item when
  owner id, faces and drawing wording agree. Added, removed and changed threads
  appear once; manufacturing-only edits still change the thread item with its
  drawing wording. Other manufacturing notes remain separate across native, CLI,
  WASM and Python diff.
- Analysis `existing` records (and Python `ExistingPmi`) mark a basic dimension with
  `basic: true`, as does the scene's `specify[id]` metadata, and diff words add ` (basic)`.
  Basic dimensions such as cadgen's "theoretical" 30 and 50 on the PMI plate were read by
  haecceity as basic but reported as plain untoleranced dimensions.

### WASM API

- Add `IntentEngine.exportAnswers(answers: string): string` for complete, deterministic
  answer sidecars with geometry evidence; incomplete sets and carry blocks throw.
- Add `AnswerFileMetadata` for optional `_specify.evidence` and `_specify.unresolved`
  in answer maps; default acceptance and writes enforce carry blocks and face evidence.
- `PmiSceneMetadata` adds optional `basic: true` for a basic (theoretically exact) dimension.
- Add `WriteReport`, `GeometryCarrierExemption` and `PreservedNormalization`
  interfaces, with optional `geometry_carrier_exemptions`, `presentation_warnings`
  and `preserved_normalizations` report keys. Expose optional `PmiSceneGraphic.leader`
  in millimetres.

## 0.1.8 (2026-11-02)

### Added

- Preserve answered and imported limit dimensions through STEP, resume, scene labels and
  diff reports; size and distance answers accept lower/upper slash bounds. Analysis and
  Python existing-PMI records expose optional absolute limits alongside derived deviations.
  Size write reports now show explicit ± and signed deviation bands as well as fits and limits.
- Offer ISO 2768 f/m/c/v × H/K/L combined general-tolerance classes and round-trip their
  geometric class, retaining the ISO 2768-m default and existing linear tables.
- Add the shared text-to-cad PMI plate from pinned cadgen 0.8.3 exports, with
  regeneration source and MIT attribution. Tests cover Specify-authored PMI,
  schema-3 face names, all 16 answers surviving thickness/bore edits, and the
  current added-hole result: 12 attached, four explicitly unresolved after
  cadgen renames supports. Verify written face names and AP242 PMI anchors.

### Changed

- The Python package's links and the step-diff README point at draftwright.io; the step-diff
  handoff accepts `/diff` from both draftwright.io and editor.draftwright.io (#108, #112).
- Store supplied answers in Add mode beside foreign PMI. Resume keeps unmarked PMI in
  `existing`; mixed rewrites replace only Specify's provenance-marked requirements and
  preserve their characteristic numbers while retaining foreign entity bytes. Scoped saves
  supersede stale legacy answer frames without removing their unmarked records.

### Fixed

- An intent no longer lists the file's own stated dimensions as requirements. The shared
  planner still uses them, so General guesses stay off those features, but the file's PMI
  stays the characteristic: on NIST CTC-01 the editor showed every toleranced hole twice,
  once with Specify's bare nominal. Written STEP bytes are unchanged.
- Answered diff scenes remove replaced file identities together with their PMI
  rows before merging retained CAD requirements, keeping semantic links and old
  callouts consistent with each scene's own document.
- Every diff pairing pass checks requirement kinds and known measurement roles,
  including number and id matches. Thread callouts cannot pair with location
  dimensions; location roles follow the part's rotation during comparison.

## 0.1.7 (2026-11-02)

- Diff retains uniquely matched planned dimensions when their target/reference
  supports grow, after exact-face matches and before kind fallback. Measurement
  roles distinguish existing bore/pitch/location requirements from new counterbores.

- Diff pairs fresh-numbered plans by requirement/geometry identity; only continued
  characteristic-number histories give numbers priority. Thread manufacturing notes
  show drawing wording, and semantic-only attribute notes, knurls and finishes show
  their values.
  WASM assembly diff accepts per-part `acceptDefaults` maps and names the part and
  question id when a nested answer is invalid.

- Prefer an exact tap-drill match over an ambiguous smaller clearance screw when suggesting a hole function; accepted tapped holes carry thread semantics without a tap-drill fit. Document and type additive haecceity polyline roles and text cell boxes (pass-through from v0.4.4).

### Fixed

- `presentation.semantic_ids` lists only the PMI document's own ids again. The
  `part.material` alias stays in `items` and `specify`, but no longer appears in
  `semantic_ids`, where consumers that cross-check it against the PMI refused the
  whole drawn PMI whenever a material was answered.

### Added

- Diff reports optionally include the exact compared `pmi.scenes` through Rust,
  CLI `--pmi-scenes`, WASM and Python. Requirement items add differing `old_id`
  and per-side mesh face anchors, including semantic-only requirements. Revision
  PNGs draw changed/added presentation callouts and translucent removed ghosts,
  framing their supplied geometry alongside existing geometry change colours.

- For #79: opt-in assembly BOM writes via explicit `bom.*` answers or `--bom` / API
  acceptance. Optional questions expose suggestions, never ordinary defaults. Assembly
  analysis adds a sourced `bom` row using its existing parse. Separate occurrence UDAs
  preserve older-engine resume; explicit answers override current export data, accepted
  suggestions do not. Geometric item history tolerates fingerprint drift, small parts
  and stable duplicate ordering; removed numbers are retired. Read through Rust, CLI,
  Python and WASM. See [BOM storage and numbering](docs/bom.md).

- Regression coverage for #89 using FreeCAD's attributed CC-BY-3.0 hex standoff:
  AP214 `PRODUCT_TYPE` upgrades to `PRODUCT_RELATED_PRODUCT_CATEGORY`, preserving
  every other original entity byte, and written PMI/answers read back. Add the
  attributed LM10UU fixture to cover independent size supports and unchanged resaves.

### Changed

- Diff reports describe requirements in Markdown and JSON summary text, naming the
  changed feature and its requirement notation when review is needed. STEP diff
  comments omit the JSON payload and flag differing committed sidecars in one
  line, with a 3D review link for public repositories. `answers.json`, JSON keys
  and verdict meanings remain unchanged.

- Refresh the numbered M2 presentation fixture and four distinct no-BOM write
  hashes for v0.4.4's added thread ink and neighbouring callout placement.
  Legacy semantic/part-setting comparisons and all six exact API/CLI byte
  comparisons remain enforced; neither the old semantic inputs nor recognition
  expectations change.

- Keep exact tap-drill defaults and their exact-value guards after the main merge:
  five frozen rule cases (CGB202, CGB217, CGB243, NIST CTC-04 part 0 and FTC-10)
  gain thread/depth answers and dependent ordering; thirteen CLI envelope entries
  carry the same reason without wildcard cases or paths. Explicit-clearance fit
  tests still choose clearance. Fresh-number order checks replace only the
  former tap-drill size with its thread; CGB243's obsolete General-size conflict
  disappears while its written count is unchanged.

- Move quiddity/haecceity (native, WASM and the Python binding) to v0.4.4
  (`53cf3604`), including thread callouts, polyline roles and text cell boxes. The pin checker requires the same release. Reader binding advances
  to `haecceity-pmi-read/14` for datum-constituent identity (#173); the packaged
  `haecceity.pmi/1` schema is byte-identical to upstream and remains unchanged.
  Upstream #171 improves callout/frame placement and label packing; #172 bounds
  generated STEP string literals and chunks long descriptive values. Recognition /6
  and fingerprints /3 are unchanged.
- Read haecceity #172's marked ordered text chunks when recovering saved answers
  and carried application payloads. Previously a long saved answer failed write
  verification after the kernel bounded its strings; legacy single strings remain
  readable. Regression coverage checks long-answer resume/resave and rejects
  unmarked, unordered or cyclic compounds.

- For #163: live PMI scenes combine accepted part settings, imported semantic notes and
  material in one GENERAL NOTES block, replacing the file block in the scene while
  lossless writes keep its records. Reopened appended files also draw one block.
  Part-level edge-break prose answers `part.edges`
  as written. Feature notes retain their owner; haecceity generates their fallback ink
  together with the combined block and owns all placement. Rotated-part regression
  checks use the notes plane. File callouts
  report file source and material DMIAs link through `part.material`.

- For #75, written hole sizes, pattern
  origins/pitches and prismatic plane levels/extents follow the shared planner.
  Inspection plans contain fewer rows: the default cross-engine plate changes from
  21 dimensions / 28 characteristics to 12 / 19. The raw all-General planner has
  11 quantities; Specify adds two offsets with distinct Basic/General contracts
  and omits the drill already carried by the thread.
  Existing numbers win; removed rows retire permanently and the write report
  lists their numbers. On the frozen pre-planner
  plate, numbers **6–11 and 21–26** retire. New requirements allocate above the
  previous high-water mark. Deferred feature owners retain their existing path.
  Locations require real face supports; thread drills and duplicate legacy spans
  are stated once. Answered fits and Basic locations retain their contracts, including the radial
  bolt-circle diameter belonging to a position frame.
  Determined enlargement depths are reference dimensions. Unknown-bottom depths
  and bounded shoulder stations that lack unambiguous semantic supports are
  reported instead of adding inspection constraints; physical-plane witnesses
  keep their legacy path.
  General-tolerance range selection uses the same six-decimal nominal as the
  written dimension, keeping boundary values stable under rigid moves. Source
  metadata for newly shared dimensions names their complete measured target.

- For #79: record the additive assembly `analysis.bom` field in the per-part oracle
  ledger, preserving all legacy layout/value assertions and opt-in write behavior.
  Review the three `--bom` CLI usage/help departures with exact text assertions;
  retain the datum-origin snapshot and revalidate ordinary write bytes against main.

- For #79: preserve opt-in BOM alongside shared planner dimensions; ordinary writes
  are checked byte for byte against main 8d5a040 through the CLI and API.

- Earlier v0.4.2 upgrade (`d7112fd`), retained as recognition evidence:
  Recognition /6 and fingerprints /3 stay unchanged; turned steps with spanner
  flats, grouped interrupted torus fillets and unproved corners follow upstream #167.
  PMI now repairs reversed bounds and datum-feature child aspects (#168), and
  duplicate source identifiers and incomplete non-runout zones (#170).
- Refresh the packaged PMI schema for optional per-item `identifier` values.
  The documented reader binding moves from `haecceity-pmi-read/11` to `/13`
  because #168/#170 change readings and findings; Specify always rereads STEP.
- Rejudge the frozen-Python recognition ledger for v0.4.2's grouped interrupted
  fillets and omitted unproved bevel corners. Retain 192 retired paths as resolved
  assertions and add 266 exact case/path judgments, with a native torus audit of
  all 44 changed groups (220 patches); do not regenerate the Python captures.
  Inventory tests count torus patches, retaining their original coverage thresholds.
- Revalidate v0.4.2 after merging #91's planner dimensions and #94's opt-in BOM.
  Regenerated cross-engine byte goldens and independently recaptured Python readings
  remain unchanged from main, including both refusals. The pin checker now requires
  v0.4.2 as well as matching native/WASM manifests.

### Fixed

- For #96: give characteristic-history schema 2 an explicit text vocabulary, independent
  of Rust Debug and generated planner IDs. Keep fit classes, modifiers and values out of
  identity; compare temporary matching hints as typed values. Pin readable keys and
  preserve all six numbers when upgrading a file written by the v0.1.6 engine.

- For #95: keep characteristic numbers through tolerance value and material-modifier edits,
  so diff reports one changed requirement without a spurious renumbering. History schema 2
  uses structural requirement identity; schema-1 files retain their numbers and high-water
  marks on migration. Removed numbers stay retired.

- The step-diff Action's "answers carried" block hides Specify's own `_specify*`
  records (such as `_specify_characteristic_numbers`) at any depth. They stay in the
  committed answers file.

- Remove the temporary LM10UU writer adjustment: haecceity #173 now preserves
  separately sized datum constituents itself. Replace the retirement guard with a
  direct upstream write/read regression for the three Ø19 lands and the whole
  datum A. A structural v0.4.2 workaround baseline checks all dimensions, tolerances,
  datums and the characteristic count, independently of STEP IDs and presentation.

### WASM API

- Add optional `polyline_roles` and text-placement `cell_box` declarations for host-font presentation rendering; pass through when supplied by haecceity.
- `diff` accepts an optional seventh `scenes` boolean. `DiffPmi.scenes` contains
  `{old: PmiScene, new: PmiScene}` when requested; `PmiItemChange` adds optional
  `old_id` and `faces: {old: {face: number}[], new: {face: number}[]}`.

- `pmiScene` exposes the scalar material through the `part.material` companion alias
  in `presentation.items`, `semantic_ids` and `specify`; the upstream semantic material
  field is unchanged. Reviewed scenes alias every part note to one shared graphic.

- For #75: live analysis snapshots carry optional shared-planner input and stored
  dimension contracts. Planned scene dimension IDs include stable face-derived
  roles; pitch callouts use interval counts and datum members leave group counts.

- For #79: add static `IntentEngine.bom(bytes)` returning sourced assembly BOM JSON;
  optional fourth `writePmi` argument explicitly accepts BOM suggestions. Omitted/false
  preserves the ordinary PMI write. BOM analysis questions are optional suggestions.

- Add optional `PmiSceneItem.identifier` for the source characteristic number;
  references continue to use the unique `id`, including when identifiers repeat.


## 0.1.6 (2026-11-01)

On quiddity/haecceity 0.4.1. Adds answers keyed by authored STEP face names (#87, cadgen's `f:` names), adopts 0.4.1's presentation (callouts by their features, one numbered GENERAL NOTES block, plane views, `stroked` text), adds `PmiScene` TypeScript types, and builds the Linux command line against glibc 2.28 so it runs on Amazon Linux 2023 and other older distributions. Feature-on scenes and written STEP layouts change with the new presentation. Named parts store answers schema 3; unnamed parts are unchanged.

### Changed

- Compare and label each part note by its semantic wording, so a change
  within the shared GENERAL NOTES graphic leaves other note requirements unchanged.
- Pin quiddity and haecceity to v0.4.1. Recognition `/6` and fingerprints `/3`
  stay unchanged. Adopt regenerated ladders/callouts, one numbered GENERAL NOTES
  block, generated plane views, AP214 PRODUCT_TYPE upgrades, panic isolation and
  graphical callout reading; feature-on scenes and written STEP layouts change.
- Build the Linux CLI against glibc 2.28 using pinned Zig/cargo-zigbuild tools;
  check its symbols and revision smoke on Amazon Linux 2023 and AlmaLinux 8.
  Release asset names and archive layout stay the same.
- Preserve optional text-placement `stroked` flags in PMI scenes; prune saved-view
  references when graphics are filtered or replaced, removing empty views.
- For #87: only unique names in the authored `f:` namespace become
  face identities. Shared names and generic/class labels (including singletons)
  retain geometric evidence and schema 2 when no usable identities remain. Omit
  unusable labels from analysis; retain the full oracle comparison unchanged.

### Added

- For #87: carry authored STEP face names in analysis and stored-answer schema 3.
  Resume and diff match names exactly before unnamed geometric supports; missing or
  duplicate names leave answers unresolved. Unnamed parts retain schema 2 and their
  existing bytes. See [face-name compatibility](docs/face-names.md).

### WASM API

- Add optional `PmiSceneMetadata.notation` for individual semantic note labels;
  shared presentation graphics retain their render-once identity and geometry.
- Add `PmiScene` JSON result interfaces for both `pmiScene` entry points, including
  semantic rows/ids, graphics, optional `stroked` text placements, planes, views and
  Specify statuses. Document parsing and rendering; the wire return remains a string.
- For #87: snapshot faces add optional `name`; evidence adds schema 3 `face_names`
  and optional mixed-part `geometry`. Add descriptive `AnalysisFace` and
  `AnswerEvidence` JSON types; existing methods and answer shapes are retained.

## 0.1.5 (2026-10-31)

On quiddity/haecceity 0.2.8. Adds `pmiScene` (one PMI scene for the editor's Specify, diff and viewer), rebuilds `diff`'s PMI section on haecceity PMI with characteristic numbers, records dimension origins (#77), and adds `pmi_scene` and `api.diff` to the `specify-core` Python package. `diff`'s `pmi.stored`/`pmi.existing` are deprecated in favour of `pmi.items`. The WASM ceiling is now 15,000,000 bytes.

### Changed

- Revision PMI reports compare haecceity semantic requirements and full notation,
  with `pmiScene` ids/statuses and inspection numbers, geometry-derived dimensions,
  foreign PMI in resumed files, and support matching across face splits. Add
  `pmi.items`; retain deprecated `stored`/`existing` for one release. Numbered
  removals read “retired”; Markdown lists changes in inspection order.
- The reviewed WASM size ceiling is raised from 12,980,000 to **15,000,000** bytes (maintainer, 2026-10-31), after `pmiScene` measured 12,903,620 bytes.

### Added

- For #142: native `pmi_scene::Scene` and raw-byte `pmi_scene`, Python `pmi_scene`
  and bundled `specify_core/schema/` JSON Schemas. Scenes contain haecceity's
  semantic/presentation documents plus Specify status, question, number, origin/source.
  Requirement-based Specify ids survive reviews; face anchors match mesh indices.
- The Python package `specify-core` on this engine (`crates/specify-core-py`): specify-core's
  Python API and the `specify-core` command line, with no Python dependency; abi3 wheels for
  CPython 3.12 and later, built and published by `.github/workflows/python.yml`. See
  [docs/python.md](docs/python.md#the-python-package).
- Its PyPI page (0.1.4.post1, packaging only): a README for users, with examples checked
  against the published package, plus project links, keywords and classifiers.
- For #77: each written size/location dimension records its origin and stable geometric
  source as a CAx-IF UDA on the dimension itself. Native PMI/analysis and WASM snapshots
  expose stored metadata; accepted defaults count as requirements and kept foreign PMI
  remains byte-identical. Edition-1 writes upgrade the header to AP242 edition 4.
  See [dimension origin and source](docs/pmi-origin.md).

### WASM API

- `diff` reports add `pmi.items` with ids/numbers matching `pmiScene`; export
  `PmiItemChange` and `DiffPmi` TypeScript interfaces. Python adds `api.diff`
  returning the same selected-part report plus Markdown.
- Add `IntentEngine.pmiScene(answers)` on the open part session (including `openPart`)
  and `pmiScene(stepBytes, part?)` for view-only As Drawn PMI without an interview.

### Fixed

- PMI scenes resolve part-setting questions from the rules table and writer's kind
  mapping, including `part.coating`. Feature-owned thread/knurl attributes inherit
  their requirement's question/status; supporting rows have no interview overlay.
  Pattern counts apply only to sizes, locations and tolerances, never datums.
  Foreign numeric ids become characteristic numbers only with Specify history.
  Python scene answers use the same input validation as WASM.
  General locations without a feature id use their face supports for stable identity.
  Locations shared by a datum member and its pattern keep their question identity;
  repeated measurements retain distinct characteristic numbers through STEP readback.
- Scene metadata/history ids follow the requirement owner and property name across
  answer changes. Foreign datum identities and file callouts are reused during a live
  review, including when adding a control; saved views and planes survive the merge.
  Part-default surface finishes retain their #65 characteristic number in live and stored scenes;
  their question and the edge-condition question use `part.surface_finish` and `part.edges`.

### Changed

- Upgrade quiddity/haecceity to **v0.2.8** (`840b2ad`). Scene export uses only the
  serialise-only `json` feature in native, WASM and Python; no JSON import API is exposed.
  Presentation retains text placement, plane/graphic ids, saved views and datum links.
  Package schemas match v0.2.8. Written mass units use conversion-based gram per
  CAx-IF Annex C.1. Existing Specify STEP goldens remain byte-identical: its material
  writer supplies no density and foreign density records are preserved. Recognition stays unchanged.
- Upgrade quiddity/haecceity to **v0.2.7** (`eaa7762`); recognition stays unchanged.
  Writer expectations change for slash-separated deviations and notes/frame placement.
  Remove the blank-note drawing workaround: literal blank rows now write valid STEP directly.

- Upgrade quiddity and haecceity to **v0.2.6** (`b9addd9`, quiddity-rust#137/#135),
  including native, WASM and Python. Written STEP now carries dimension and tolerance
  callouts, ISO 1302 face surface symbols and a `General notes` block for part settings.
  Multi-face aspects are composite, locations reference their own aspects, and a datum
  feature that also has a size uses separate datum-feature and dimensional-size entities.
  Presentation stays enabled; optional F3D import stays off. Recognition schema 6 and
  fingerprint format 3 are unchanged.

## 0.1.4 (2026-10-30)

### Added

- For #65: stable per-part characteristic numbers in standard AP242 `ID_ATTRIBUTE`s,
  with a persistent high-water mark that retires removed numbers. PMI/analysis and WASM
  snapshots expose file numbers; answer revision carry preserves them through quiddity's
  correspondence. General dimensions written as PMI are numbered; rows without an entity remain unnumbered.
  A part's first numbering follows inspection order: datum letters, then front/top/right view
  and row-major face position, with size/location/control ties; face finishes, threads and
  knurls share that walk and unpositioned defaults go last. Later additions append above the
  high-water mark. See
  [characteristic number storage and allocation](docs/characteristic-numbers.md).
- The STEP states every dimension a drawing would (#63): the writer writes the dimensions the
  general tolerance covers (plain sizes, plane levels and hole locations, `general.rs`) with
  their nominal alone, under the general tolerance written for the part. A part located by ±
  dimensions now carries its hole locations. They are counted in one report line,
  `N dimensions under the general tolerance`. A part with PMI from elsewhere gets none (its
  own dimensions stand), and a datum's own faces get no size (its datum stays a plain datum
  feature). Read back, Specify's own nominal-only dimensions settle no question.

### Fixed

- For #67, revision reports describe an added turned end chamfer with its size and
  boss context. Retained supporting surfaces and boundary edges distinguish trimmed
  flat centroids from real moving or removal. Raw recognition, correspondence and answer migration are unchanged.

### Changed

- Relicensed from AGPL-3.0-or-later to **Apache-2.0** (`LICENSE`, `NOTICE`, both crates'
  `license` fields), matching quiddity and haecceity.

- Upgrade quiddity and haecceity (native and WASM) to **v0.2.5** (`6110b4c`,
  quiddity-rust#134). Boss heights and free ends include their own convex end chamfers
  and rounds; edge-treated plates stay plates, and inside turned fillets are described
  as concave. Saved boss answers retain their face targets; revision migration still
  reviews changed dimensions. Recognition schema 6 and fingerprint format 3 stay unchanged;
  optional F3D import stays off. Plate centre coordinates use the same six-decimal mm
  publication as face centroids, keeping native and WASM snapshots consistent after
  projected edge strips are restored.

- Upgrade quiddity and haecceity (native and WASM) to **v0.2.4** (`2f6b3a6`): haecceity's Add
  shares the face usages a file's location dimensions hold, so PMI added beside them no longer
  breaks `item_identified_representation_usage.UR1` (quiddity-rust#132).

- Upgrade quiddity and haecceity (native and WASM) to **v0.2.3** (`ced75a5`).
  Analyses and revision reports describe turned bodies, annular face grooves (rim
  diameters, depth and wall angles), cylindrical profile surfaces and concave fillets,
  including spherical inside corner caps. These families follow the existing body,
  groove and surface rules without additional interview questions or duplicate PMI.
  Full-length turned ODs are no longer bosses; their external cylinders retain the
  existing OD datum defaults. Saved answers on a removed boss remain
  visibly unresolved for review and cannot migrate to another family. Recognition
  schema 6 and fingerprint format 3 are unchanged; optional F3D import stays off.

## 0.1.3 (2026-10-29)

### Added

- `specify-core diff` compares two revisions of an assembly with no `--part`: its parts paired
  by name, a parts table of those changed, added, removed, placed a different number of times or
  moved, each changed part's own report, and a picture of the whole assembly coloured by part.
  Answers are by part name; a single part's answers are taken as its part's when its file
  becomes an assembly. `--viewer URL` links each changed part to the /diff page with
  `&part=NAME`. A single part's report is unchanged (`docs/diff.md`, "Assemblies").
- The STEP diff action compares assemblies: one comment with the parts table, a section for each
  changed part, the assembly's picture and a /diff link for each changed part.

### WASM API

- `diff` compares two revisions of an assembly part by part: `{report, markdown, parts}`, each
  changed part with its own `markdown` and `view`. Its signature is unchanged.

## 0.1.2 (2026-10-29)

### Changed

- Upgrade quiddity and haecceity (native and WASM) to **v0.2.2** (`e58d174`). Deep
  hole-entry steps are counterbores; multi-step holes retain their outer counterbores and
  holes with different outer steps get separate questions. Tangent convex rounds are
  fillets at any wall angle (up to a 135° sweep), while bend skins and false rounds are
  no longer fillets. Failed recognition proofs stay refusals, never question targets.
  Recognition schema 6 and fingerprint format 3 are unchanged; optional F3D import stays off.

- The reviewed WASM size ceiling is raised by 10%, from 11,800,000 to 12,980,000 raw bytes,
  after the revision-report work measured 11,807,493 in CI.
- The revision report says complex changes plainly (#56):
  - a hole pattern laid out again (another count, circle or spacing) is one line, the old
    pattern become the new (`pattern of 4 × Ø6 … → pattern of 5 × Ø6 … on a Ø48 circle`),
    its holes' own pairings folded into it; same-place folding uses 10% of the smaller
    feature's radius (pattern circle radius or member spread), with a 0.1 mm floor, so a
    large part does not make distant small holes into replacements;
  - a hole that became another kind of hole in its place reads as that (`Ø6 × 10 through
    holes → Ø6 through holes, counterbored Ø12 × 3 deep`), not as removed and added;
    counterbores, countersinks, spotfaces and outer steps include their sizes in hole and
    pattern descriptions;
  - what a feature borders is not a change to it, and neighbourhood signatures are never
    shown; when unchanged bevels continue tangentially round new fillets, anchored to carried
    old chamfer faces, the report says `2× chamfers now run round the new fillets`, rather than
    counting the extra faces as chamfers added; separate or resized chamfers remain changes;
  - a feature recognised twice (a pocket by its depth and by its section) is one line, in plain
    sizes, not `section.sides.2`;
  - when the whole part moved, the report says so: `The whole part moved between the revisions
    (turned 90° about Z, moved X +8, Y +15, Z +3)`; half-turns recover their axis from
    the symmetric part of the rotation, where the skew part vanishes;
  - a slot is named by its size (`16 × 6 × 10 slot added`).
- A report feature's `class` can be `replaced`, with `became`: the new feature in words; or
  `continued`, with the chamfer's extension records in `candidates` and their faces in the view.

## 0.1.1 (2026-10-27)

### Added

- The STEP diff action's picture links, in a public repository, to draftwright.io/diff, which
  compares the two revisions live in the browser and shows the part in 3D.
- The WebAssembly `diff`'s `view` has `changes`: the report's "What changed" lines in order,
  each with its features and their faces on the new part and the old, so a viewer can link the
  lines and the faces.

## 0.1.0 (2026-10-27)

### Added

- The STEP diff action (`step-diff/`): one workflow step, `uses:
  pzfreo/specify-core-rust/step-diff@main`, that runs `specify-core diff` on each STEP file a pull
  request changes and keeps one comment up to date with the report, its picture and the answers
  carried.

### Changed

- The README describes Specify as it is now, with install steps for the release binaries;
  `docs/diff.md` explains how revisions are compared and answers carried, and
  `step-diff/README.md` is a guide to the action. The library API moved to `docs/library.md`
  and the Python engine's history and differences to `docs/python.md`.
- quiddity and haecceity are pinned to quiddity-rust's `v0.2.1` release (`37df6a0`), from
  `v0.2.1-alpha.1`. The release changes no library code, so recognition (schema 6), fingerprints
  and answer IDs are unchanged.

### WASM API

- `diff(oldName, oldBytes, newName, newBytes, answersJson, title)`: `specify-core diff` in the
  browser, returning the report, its Markdown and `view`, the meshes and face classes a viewer
  draws the change with (`revision::view`, `session::diff`). For draftwright.io's STEP diff page.

## 0.1.0-alpha.9 (2026-10-26)

### Added

- The `specify-core` command line for Linux (x86_64), macOS (Apple Silicon) and Windows
  (x86_64), attached to each release (`.github/workflows/cli.yml`), each built, tried on a
  revision and checksummed.
- `specify-core diff OLD NEW --answers A -o DIR` (`revision::diff`, `revision::picture`):
  answers carried to a new revision of the part through quiddity's correspondence, where
  `resume::rematch_answers` leaves every feature and datum answer unresolved once the face
  evidence differs. Each answer's faces are mapped and the answer moved to the feature of its
  family holding them; orphaned, ambiguous, unmatched and stale answers are left unresolved
  with their value and reason, never guessed. The report lists changed features, answers to
  check (carried to a feature quiddity reports changed), new questions and new review problems,
  as JSON and as Markdown for a pull request comment, and `model.png` draws the new part
  coloured by what changed. A Rust-only command, left out of the Python-parity top-level help.
- For #2, `api::parts_of(file)` lists a `part::File`'s parts as `api::parts` lists a path's
  (`api::parts` now goes through it), and `carried::part_payload(bytes, part, name)` reads a
  payload written on one part of an assembly.

### Changed

- CI: cache Rust builds and check CLI/library composition on named wrapper cases; use
  a shared named corpus subset (`SPECIFY_CORPUS_ALL=1` opts into the full sweep locally).
  macOS PRs build all tests and run floating-point checks;
  main, manual runs and `release*` PRs keep the full suite, as Linux does on every run.
- Upgrade quiddity and haecceity (native and WASM) to **v0.2.1-alpha.1** (`7291435`),
  retaining recognition schema 6 and fingerprint format 3. The feature picker gains
  annular grooves, semicircular blind slots, sharp/rounded V-channels (`trough_radius`)
  and rectangular through-notches. Drill-point holes include their tip cones, rectangular
  slots their end walls, pockets more of their proved boundaries, and polygonal bosses
  their free-end caps. Slot end planes use the actual claimed walls so questions, datum
  defaults and written PMI remain correct under rigid motion.
- Adopt quiddity #91's outline-based roll: `golden_oriented_slots` and its captures and the
  17-degree slotted assembly return to the plate's file axes and four-wall slots; cgb241
  returns to file axes with face 47 an R23 blend; cgb217 uses a 13.07-degree frame with
  all sixteen R1 rounds classified as blends. Saved answers on verified unchanged geometry
  follow unique same-family supports as cones/walls are added; ambiguous or missing supports
  stay unresolved with their values. Without face evidence, changed IDs remain unresolved,
  and automatic/manual provenance follows each answer. Malformed assembly cycles and
  nesting deeper than 64 are refused as a whole by haecceity.
- `revise` is now `specify-core diff` (`revision::diff`): it says what changed between two
  revisions as well as carrying the answers.
- `diff` compares PMI (`Revised.pmi`). Without `--answers` it reads the answers Specify wrote
  as PMI in the old file (`pmi.answers_from`: `given`, `stored` or `none`); where the new file
  holds Specify's PMI too, each answer it differs on from those carried is listed
  (`pmi.stored`); a new file without it is said to need writing. The files' own PMI (from CAD) is
  matched through the faces and each dimension, tolerance, datum or note added, removed or
  changed is listed by the feature it is on (`pmi.existing`: `Ø7 × 10 through hole: flatness
  0.2 → 0.1`). The summary counts the changes to the PMI.
- `diff`'s report on a real part: the summary names features first and counts the rounds and
  surfaces that follow them (`2 holes changed, 8 rounds and surfaces with them`); a freeform
  surface is `reshaped`, not its control points; a move is the component a person sees (a
  round riding a drafted face moves `Z +2`, not `X +0.1, Y +0.1, Z +2`); and an answer on a
  pattern is listed to check when its holes change.
- `diff`'s report reads as a person would describe the change. The summary leads with what
  changed (`2 holes removed.`). A table gives the part's volume, area, bounding box and face
  count before and after. Each feature is named as a drawing names it (`Ø5 × 30 through
  hole`) and placed by the side of the part it is on, the boss it sits in and its offset from
  an axis or corner (`FeatureChange.what` and `.place`); alike features changed alike read as
  one line. Answers are listed by their question, and an answer on a removed feature says
  which feature went (`Revised.measures`, `Unresolved.question`, `Check.question` are new).
- For #2, `writer::write_bytes` and `write_bytes_with` take a part of an assembly (from
  `part::File::load`), as `write_parts_bytes` does: the other parts, their placements, PMI and
  stored answers are kept. They no longer refuse a `LoadedPart` whose `part_count` is not 1.
- For #2, extra attributes (`write_bytes_with`, `write_with`, `write_parts_with`,
  `write_parts_bytes_with`) are written on one part of an assembly, on its product definition,
  and verified like the rest. Several parts written together with extras are refused
  (`extra attributes are written on one part: write one part of an assembly at a time`), no
  longer every assembly.

### WASM API

- For #2, add static `IntentEngine.parts(name: string, bytes: Uint8Array): string` and
  `IntentEngine.openPart(name: string, bytes: Uint8Array, part: number): IntentEngine` (Rust:
  `session::Session::{parts, open_part}`). An assembly's part, or a part placed more than
  once, opens; its `writePmi` writes that part into the original file, keeping every other part
  and placement byte for byte, and `payload` reads the payload on that part. The constructor
  is unchanged for a file of one part used once and still refuses other files, its messages
  now naming `openPart`. `snapshot().provenance.part` is the part opened (0, as before, from
  the constructor). Existing calls are unchanged.

## 0.1.0-alpha.8 (2026-10-25)

### Added

- For #3, retain the existing turned-diameter ISO 261 thread choices (coarse and fine)
  alongside fits and knurls, and include both 0.05 mm match boundaries despite floating
  point subtraction noise. Otherwise no thread choice is offered. Verify typed/selected
  STEP equivalence and native/WASM exposure.
- For #21, expose blind tapped holes' minimum full-thread depth as an editable text
  question in mm, with the computed default visible. An answered tap retains its inline
  or computed manufacturing depth; optional edits override it, remain unrounded, reach STEP PMI and
  resume from saved answers. Through holes and unknown drill depths have no depth question.
- For #35, `resume::rematch_answers(loaded, features, answers, evidence, automatic)` matches
  answers held outside the STEP (a saved project's) to the part as now recognised, with the
  same face-support rule as answers stored in a STEP. Callers keep `resume::evidence(loaded)`
  (`{schema: 2, face_count, faces}`) with their answers. Legacy projects without saved face
  evidence (and schema 1 exports whose fingerprint can't be reproduced) resume with their
  answers matched by feature ID (a datum kept when its faces exist), the rest `unresolved` with their values, and `unverified` says so. Evidence that
  contradicts the part still leaves every answer on a feature or a datum (including
  `datum.X.flatness`/`.control`) `unresolved` with its value; only `part.*` answers are kept
  then. `automatic` marks answers the default path gave and returns their IDs renamed.
  New public
  fields `UnresolvedAnswer.automatic`, `Resumed.unverified` and `Resumed.automatic` break Rust
  struct-literal callers. See `docs/design.md`, "Saved answers across kernel upgrades".
- For #35, `carried::attributes(source, name, value)` stores an application's named JSON
  payload in the STEP for `writer::write_bytes_with`, with a `specify-core original` record.
  `carried::payload(bytes, name)` reads it back, and `carried::original(bytes)` gives back the
  part first written from, verified by its sha256. The payload's encoding is the editor
  project's, so files written either way are read either way. Only an item in a user defined
  attribute is read as a payload. The core sets no payload size limit; callers set their own.

### Changed

- Pin quiddity and haecceity, including WASM, to tagged **v0.2.0-alpha.2**.
- Adopt recognition schema 6 (#92): publish file-coordinate records, accept measuring
  axes as letters or vectors, and derive slot/channel end planes from the record's own
  origin and distances. Merge oriented slots/chamfers/patterns into the ordinary families;
  vector axes and exact converted extents replace the old direction fields. Remove the
  typed view and `_local`/`_frame`; recognition provenance is never geometry.
  All three golden 30° slots and their six end planes retain questions, defaults and
  written PMI through a 30° Z rotation/translation and a generic rigid rotation; end-wall
  agreement is within 0.000003 mm. Golden's slot-pattern direction is exactly −X.
  The formerly oriented mfcadpp/10252 chamfer remains one feature on face 2, with its
  18.694-mm overlapping run, 0.54-mm legs and 45° angle.
- Kernel frame-vote corrections (#81) recover fillets: cgb217 **6 → 14**, CTC-02 AP242
  **3 → 20**. With the concavity correction (#89), cgb202 is **272 → 279** (16 recovered
  convex fillets, nine previously reported concave fillets removed). CTC-02 AP242 gains
  **4 pockets** and step levels **2 → 13**; its four former risers disappear. Ambiguous 49.5-mm hole rows
  are no longer patterns (#82). Symmetric countersink seats stay linked (#83), and plate
  boundaries retain their original planes (#84/#85). Fingerprints remain version 3.
- Preserve old-kernel saved-answer fixtures with **13 / 37 / 34 answers** for oriented
  slots, cgb217 and CTC-02 AP242. Resume checks require each value to reattach by geometric
  support or remain open with its original ID and reason, never disappear. Golden's 13
  answers are part/datum settings and resume unchanged; none is keyed to an oriented slot.

### Fixed

- For #35, schema 1 answers saved by the pre-alpha browser writer resume again under
  quiddity v0.1.1. That kernel gives some zero centroid components the other sign, so their
  schema 1 hash cannot be reproduced. Such payloads in this port's attribute form now resume
  without face evidence, with their answers matched by feature ID, an
  `analysis.resume.unverified` reason, a warning and a `saved_answers_unverified` review
  problem. Rewriting stores schema 2.
  Python-written schema 1 files still open as foreign files (Q14).

### WASM API

- For the kernel upgrade (#92), `snapshot().analysis.features[*].record` uses schema-6
  file-coordinate measurements. Axis fields accept letters or unit vectors. Converted
  records carry a shared `origin`, including when their axes mix letters and vectors;
  `_local` and `_frame` are removed. Oriented families merge into `slots`, `chamfers`
  and `slot_patterns`; formerly oriented IDs change to those families, and optional
  plain-record fields can be null. Method signatures do not change.

- For #3 and #21, editors use the existing question/choice/default structures and unchanged
  JS/TypeScript signatures. Render `hole.full_thread:<feature>` as a text question; its
  string default is drill depth minus three pitches, rounded down to 0.5 mm (at least one
  pitch), or the inline depth. Submit a positive mm value that leaves three pitches, or
  use `acceptDefaults`. The depth is an optional `refine` question, excluded from `open`:
  an answered tap keeps its inline or computed depth in preview and written PMI even
  without a depth answer. Merely showing the depth question adds no accepted answer.
- For #35, add `IntentEngine.rematchAnswers(answers: string, evidence?: string | null,
  automatic?: string | null): string` and `IntentEngine.evidence(): string`. Editors keep
  `evidence()` with saved answers and use `rematchAnswers` instead of setting aside answers
  by a list of answer kinds. Without evidence answers are matched by feature ID; with
  evidence that does not match every answer on a feature or a datum is returned in
  `unresolved` with its value, not in `answers`. See docs/wasm.md, "Answers
  saved outside the STEP".
- For #35, `IntentEngine.writePmi(answers, project?, payload?)` takes an optional third
  argument, `{name, value}` JSON, carried in the STEP as base64 JSON under `name`. Give the
  project or the payload, not both. Either now also records the original part. Add
  `IntentEngine.payload(name: string): string | undefined` and
  `IntentEngine.original(): Uint8Array | undefined`. Existing calls are unchanged; a
  `writePmi` without a project or payload writes none, dropping any the opened file carried,
  and no original record. Written bytes with a project gain the `specify-core original` attribute. Analysis can report
  `resume.unverified`; review can report `code: "saved_answers_unverified"`. The Rust
  `session::Session::write_pmi` gains the `payload` argument (pass `None`).

## 0.1.0-alpha.7 (2026-10-23)

### Added

- For #22, STEP-only provenance on every Specify-written semantic PMI requirement, using
  the stable `specify-core:written:v1` token in AP242 fields and representation descriptions.
  Imported PMI remains unmarked; existing-PMI results expose the token independently of
  stored answers. `existing::part_settings_provenance` also exposes settings authorship.
  The new public `ExistingPmi.provenance` field breaks Rust struct-literal callers; add
  `provenance: None` when constructing an unmarked row. See `docs/pmi-provenance.md`.

### Changed

- Pin quiddity/haecceity to **v0.1.1** (a27b7064), including the WASM crate. Specify's
  version is unchanged. Draftwright's released kernel pin temporarily differs; the
  advisory pin checker continues to warn.
- Adopt the kernel's file-grid/near-axis publication and support-plane plate boundaries
  (#79), and remove CTC-04's false quarter-cone countersink features (#80). Hidden-line
  drawing corrections are upstream; Specify does not consume that drawing API.
- Read framed recognition correctly: file-space vectors/positions stay in file space,
  local slot/channel end planes are projected through the inferred frame, and remaining
  local record coordinates carry explicit frame metadata. Equal-area datum proposals
  keep face order across rigid motion within max(1e-6 mm², 1e-6 times the larger area).
  Aligned and framed slot/channel end-plane checks use the same published precision.
- Migrate saved feature answers by unique same-family geometric face support after
  validating the legacy ordered face fingerprint. Unmatched, ambiguous or colliding
  answers retain their old ID/value/reason in `analysis.resume.unresolved` and appear in
  the existing review warnings; affected questions stay open. **Editor upgrading note:**
  review these warnings and answer the open questions before saving; recover old values
  from the resume report: unresolved old values are not persisted when saving again.
  Files whose geometry no longer matches open as foreign files, retaining existing PMI
  and part settings. Alpha.6 STEP files and unchanged schema-2 payload bytes remain
  supported. See `docs/design.md`, "Saved answers across kernel upgrades".
- **Fewer features on four parts.** Under quiddity v0.1.x, 44 features that alpha.6 recognised are not
  recognised and their faces are unexplained in quiddity's face accounting (no named refusal): 24 fillets
  on cgb202, 18 on the NIST CTC-02 AP242 file, one on cgb207 and an oriented chamfer on mfcadpp/10252.
  Their questions no longer appear. They are recorded as `undetermined` in the known differences
  pending a quiddity fix.
- Adapt PMI to the v0.1.x model and step-io 0.3 decoded strings; keep stored answers,
  literal note text and the STEP-only provenance token round-tripping.

- CI: run the full workspace tests on Ubuntu and macOS with Quiddity's pinned STEP corpus;
  require the corpus and its captured inputs instead of silently skipping them.
- **Native Rust helper:** `pyfmt::fmt_rules` now returns `Result<String, FormatError>`;
  callers must handle non-finite dimension refusals instead of relying on a panic (#26).

### Fixed

- Refuse invalid browser answers, state, non-finite dimensions and oversized inputs with
  thrown string errors; preserve the session after refusals. Audit the WASM-reachable panic
  and indexing sites and exercise hostile inputs in the WASM CI job (#26).
- Preserve Python-compatible text answers (including `null`, empty and non-string values)
  and saved state with stale or repeated automatic-answer IDs at the WASM boundary (#26).
  Reuse core choice validation and the settled review to avoid redundant rule passes.

### WASM API

- No JS/TypeScript signatures change for the kernel pin. Session analysis can additionally
  report `resume.migrated` and `resume.unresolved`; review problems expose unresolved
  saved-answer warnings using `code: "saved_answer_unresolved"`.

- No JavaScript/TypeScript signature changes (#26). Invalid answer IDs, contradictory choices,
  invalid choice/note types and malformed state now refuse explicitly. Inputs are
  bounded to 64 MiB STEP, 8 MiB per JSON/project argument and 64 KiB names/interpretation/mode.

## 0.1.0-alpha.6 (2026-10-20)

### WASM API

- Behaviour change for the editor (issue #20, #28): without a call to `acceptDefaults`, unanswered
  default datums, positions and the general tolerance are no longer drawn or written. Review
  intent and `writePmi` include only answered specifications and the file's own datums. The
  defaults stay on their questions as suggestions with their reasons, and their ids stay in
  `open`; an unanswered general tolerance is absent from `intent.part`. Intent items keep
  their `default` field, which is now always `false`. No signatures change.
- Add `IntentEngine.acceptDefaults(answers: string): string` and
  `acceptDefaults(analysis: string, answers: string): string` (#28). Both return ordinary
  editable answers with every unanswered default accepted to the same fixed point as CLI/serve
  `accept_defaults`. Supplied answers are preserved, questions without defaults stay open, and
  the session is not changed. The existing `review`, `settle` and `writePmi` take the returned
  answers. Existing signatures are unchanged.

### Added

- Free-text part notes as semantic PMI (#25). A new `part.notes` question (kind `text_list`,
  default `[]`) takes an ordered list of literal strings for notes about making the part;
  drawing-only notes stay in the drawing Spec and no existing text is migrated. Each note is
  written as a part-owned `semantic text` attribute set with a `general note` text item and a
  one-based integer `note index`. Answers that are not a list of strings are refused by the
  rules and by the writer. `existing::part_settings` reads the notes back as `notes` in index
  order, also from files without stored answers and for each part of an assembly. Writing an
  identical list over notes another tool wrote adds no duplicates; writing a different one is
  refused. Rules version 3. See `docs/part-notes.md`.

### Changed

- Datums A/B/C are now proposed as measurement origins on every part, not only when a hole
  position or runout needs a frame: also in ± locating mode, when every hole uses the general
  tolerance, and on parts without holes (#24; rules version 2). The same A/B/C defaults apply, including a
  turned part's axis A and shoulder B, and the ± datum prompts ask where the part is measured
  from. Explicit ± hole locations are dimensioned from the selected datum planes instead of
  the default edge faces, so changing or removing a datum changes or removes those
  dimensions. ± mode asks for and adds no position tolerances, datum controls or runout, and does not
  report unused datums. Accepted datums are written as AP242 datum features without
  geometric tolerances. Like other defaults, these datums are written only once answered or
  accepted (#28).
- Rewriting a resumed file whose PMI and stored answers are unchanged returns the original
  bytes, after the replacement has been written and verified (#25).
- Pin quiddity and haecceity (native and WASM) to quiddity-rust `v0.1.0-alpha.8`,
  matching draftwright-rust alpha.9 (#27). This brings derived PMI geometry, measured
  modelled threads and graphic-only PMI presentation accounting. Modelled thread
  faces are excluded from plain datum seats; existing thread interviews and verified
  writing remain available. New departures from the frozen Python captures are
  recorded with independent OpenCascade evidence in the known differences.

### Fixed

- Answering a question no longer accepts every unanswered default (issue #20, #28). Review,
  preview and WASM PMI writing include only answered specifications and existing datums;
  datum, hole-position and general-tolerance defaults remain open suggestions with their
  reasons. CLI/serve writing with explicit `accept_defaults` is unchanged. The editor's
  automatic ±/plain-hole path does not accept the remaining suggestions. Answered controls
  waiting for unaccepted datum frames now report `awaits_datum_frame` in review/serve
  problems instead of disappearing silently; strict apply still refuses an unavailable
  datum control.
- Turned parts with a common principal axis now default to A on the plain recognised cylinder
  with the greatest total cylindrical area (diameter breaks ties) (#23). Threads and knurls from
  answers, recognition or PMI are excluded. Interrupted single-diameter shafts recognised as
  dominant bosses also use their axis. B is a perpendicular locating shoulder (assembly
  contact first, otherwise largest area), excluding recess walls unless the only perpendicular
  face. Explicit datum answers and file datums retain precedence; prismatic parts keep plane
  datums. Coaxial holes use diametral position to A and are recognised as located by that axis;
  offset and cross holes still need location. Runout follows the primary axis. See
  `docs/turned-defaults.md` for detection, ISO 1101 zone interpretation and before/after
  corpus verification.
- Blind tapped-hole help now gives a valid full-thread-depth example (such as
  `M2 tapped, 2.5 mm full thread`) and explains the computed default (#23). Choice and
  typed-answer summaries show that depth, including native serve and WASM interpretation;
  canonical answers, explicit depths, through holes and unknown depths retain their behaviour.

## 0.1.0-alpha.5 (2026-10-18)

### WASM API

- `IntentEngine.settle(state)` returns `{review, state}`: the review settled to the default path
  with label anchors. `answerEdit`, `tolerancingEdit` and `openedTolerancing` change a project's
  `{answers, auto, gdt}` as the editor's tolerancing controls, All specs answers and project
  opening do.
- Add a reviewed TypeScript API snapshot and CI acknowledgement gate: declaration or snapshot
  changes require a new Unreleased WASM API bullet; refresh with `python3 tools/check_wasm_api.py --update`.

### Added

- `tolerancing`: a drawing by default, GD&T on request. The editor's rules for the default path
  (± locating, plain holes except proposed taps, plain diameters except threads and knurls,
  recorded in `auto`) and for GD&T mode changes, moved from draftwright-editor so the editor and
  the MCP server make the same changes.
- Enforce a reviewed optimized WASM size ceiling, with raw, gzip and Brotli measurements and
  headroom on every build. Compare both quiddity kernel pins with Draftwright's latest release
  on main and before uploading releases; mismatches warn and add a Kernel note to release notes.

## 0.1.0-alpha.4 (2026-10-16)

### Added

- Browser package `specify-core-wasm` in a Cargo workspace (issue #11, PR #13), preserving the
  editor's `IntentEngine`, JSON review/interpretation and verified PMI writing API, including
  face-order checks, geometry anchors and the optional `draftwright editor project` attribute
  via the upstream writer. Shared `api::review` composes presented questions, open ids, preview
  intent and problems for any front end. Filesystem APIs and the line service are cfg-gated
  out of WASM; native entry points and default Cargo commands retain their behaviour.
- `wasm-release` profile (LTO, one codegen unit, optimization level 3), wasm-pack builds and
  Node/native parity on PRs and main; published `v*` releases attach a web `pkg/` tarball and
  SHA-256 checksum after parity verification. Actions are SHA-pinned, release write access is
  job-scoped, and nothing is published to npm. See `docs/wasm.md` for use and editor migration.

- `writer::{write_bytes_with, write_parts_bytes_with}` accepts caller-provided
  `&[haecceity::pmi::AttributeSet]`, written and verified in the same edit as the PMI and stored
  answers (issue #10, PR #12). `writer::{write_with, write_parts_with}` also supports atomic file
  output, including `.gz` destinations. Existing APIs delegate with an empty slice and keep
  their signatures and bytes. Nonempty extras require a single-part file; both set and item
  names reserve `pmi-assist answers` to protect resume. Extras follow the existing replace/add
  behaviour: replace removes old attributes, so callers resupply those wanted; add preserves
  existing attributes. Tests mirror the editor's base64 `draftwright editor project`, including
  readback through haecceity, resumed answers, name clashes, empty-slice parity, rewrites,
  foreign PMI and gzip output.

## 0.1.0-alpha.3 (2026-10-16)

Pinned by rev to quiddity-rust's 0.1.0-alpha.4 release commit `f5ddb72` (from `v0.1.0-alpha.3`,
by way of rev `0d601d2`); no tag exists yet.

### Added

- Gzipped STEP input (maintainer decision 2026-10-15): every command, serve request and library
  load (`part::File::read`/`of_bytes`, `part::load_bytes`, `api`) reads a gzipped STEP file or
  bytes, told by its magic bytes, not its name, and decompressed in memory. The binding's
  `source_sha256` and the stored-answers fingerprint are of the decompressed bytes, so a `.step`
  and its `.step.gz` bind and resume identically. `write` to a destination named `.gz` writes it
  gzipped (a write in place to a `.gz` input stays gzipped); other destinations are STEP text. A
  gzip that does not decompress is refused, saying so. `part::gunzip`.

- A turned diameter's question ("a fit, a thread or knurled?") offers every ISO 261 metric
  thread at its size as a choice beside the fits and knurls: the coarse thread as before
  (`thread:M10`, called up M10×1.5-6g), then the fine pitches (`thread:M10x1.25`, code
  `M10×1.25 thread`, callout M10×1.25-6g), from M1.6 to M64 (`standards::ISO_261`,
  `external_threads`, `external_thread`). A size ISO 261 gives in fine pitches only, such as
  Ø15, offers those (M15×1.5, M15×1); a diameter at no ISO 261 size offers no thread. Typing
  the same thread in the free-text box reads as the same choice and writes the same AP242
  thread; a fine pitch typed for a turned diameter is no longer refused. Existing answers
  (`thread:M6`) are unchanged. The oracle sweeps set aside exactly these threads and still compare everything else with Python.
- `part::File`: one haecceity `StepFile::read` per file, from which every part, its
  definitions and the file's hash are taken (`read`, `of_bytes`, `of_shared`, `load`,
  `load_all`, `parts`, `definitions`, `sha256`, `path`, `bytes`).
- `api::write_bytes(bytes, jobs)`, `api::IN_MEMORY` and `writer::write_parts_bytes`: write
  several parts of a file held in memory; the other parts are loaded from the same bytes.
- `PartSummary.product_id`, listed after `name` in the library, `api::parts`, the CLI and serve
  (Python's listing has no such key: recorded as `rust-correct` in `docs/known-differences.md`).

### Changed

- quiddity-rust 0.1.0-alpha.4 takes Python's local-degradation retry, so on corpus parts 13975
  and 14052 the features match Python's: the four entries for features only Rust reported there
  (`holes:0` and `pockets:*` on 13975, `holes:8` and `plates:5.6` on 14052) are removed as stale,
  and the section-recess entry no longer cites 14052 (`docs/known-differences.md`). No question
  changes.

- A gzipped STEP file is no longer refused ("the file is gzip-compressed; decompress it first");
  see Added.

- Writing an assembly mixed in kind, one part resuming specify-core's stored answers and another
  carrying PMI from elsewhere, writes each part by its own mode: replace for the resuming part
  (its PMI is the intent's, once, and its answers are stored), add for the other (nothing it had
  is lost). It was one add for both, which wrote the resumed part's PMI a second time and was
  refused by haecceity (`item_identified_representation_usage.UR1`).
- The analysis of a part held in a `part::File` (`api::analyse_reporting_in`: `analyse --part
  N`, serve) takes the file's part definitions from it and parses the file as Part 21 once, for
  the stored answers and the PMI together, instead of reading the definitions again and parsing
  up to three more times. Output is unchanged; on `nist_ctc_01_asme1_rd.stp` the analysis runs
  in 6% fewer instructions (9.10e9 to 8.55e9), 7% on `parts/plate.step`. New:
  `existing::read_parsed`, `resume::stored_in`, `resume::answers_for_parsed`.
- `docs/upstream-needs.md`: U1 and U9 cite quiddity-rust PR #10 (rev 0d601d20).
- Every part is haecceity's own `Part` (`StepFile::part`); specify-core's stand-ins for part
  extraction, part names, the placed-back read and face matching are gone (upstream needs U1
  and U9 met). `LoadedPart` keeps all its fields; `rebuilt` is now always false.
- Analysis of an assembly part (`api::analyse_file`, CLI and serve `analyse`) loads the part
  and the other parts its mates need from one read; `api::analyse` loads them from
  `loaded.bytes`, never from the path.
- A part of an assembly can be written alone: the refusal when other parts had stored answers
  is gone, and the write mode is decided over the parts being written. An assembly with no
  part chosen still refuses, naming its parts.
- The plate read from the assembly lists 6 of its 26 mesh edge polylines in a different order
  (haecceity numbers edges its own way); the edge set is unchanged.

## 0.1.0-alpha.2 (2026-10-14)

Pinned to quiddity-rust `v0.1.0-alpha.3` (from `v0.1.0-alpha.1`).

### Added

- `part::load_bytes` and `writer::write_bytes`: one part loaded from bytes and written to
  verified AP242 bytes, with no filesystem (for the editor's browser build); byte for byte
  the file API's output (`tests/bytes_api.rs`).
- `Question::to_presented_dict()`: an opt-in presentation record with a feature row's `title`
  and a typed `characteristic` (flatness, perpendicularity, parallelism, position, runout) for
  datum, hole and turned questions; `to_dict()` is unchanged.

### Changed

- quiddity-rust `v0.1.0-alpha.3` publishes `section_recesses` in `Features`, so the analysis
  lists section recesses. Against Python, 154 of them differ only by Python's added plane faces
  (constituent, not defining: upstream need U2, as for holes and bosses), and the regions Python
  lists but quiddity-rust refuses or does not recover are still listed as missing
  (`docs/known-differences.md`). No question changes: the rules read only holes, hole patterns
  and turned steps.
- Writing PMI into the NIST AP203 files (CTC-01, CTC-02, CTC-03, CTC-05, FTC-06, FTC-09) now
  succeeds: haecceity's upgrade repairs their two classes of instance that are invalid in AP242
  (and in AP203) and reports each repair, where the write was refused before. These writes now
  match the Python oracle.

## 0.1.0-alpha.1 (2026-10-10)

The first alpha: a pure-Rust port of [specify-core](https://github.com/pzfreo/specify-core),
interview-driven AP242 PMI authoring for STEP parts, with no OpenCascade and no Python. Features
come from quiddity-rust's recognisers; STEP reading, meshing and PMI reading and writing from its
haecceity kernel (pinned to quiddity-rust `v0.1.0-alpha.1`).

### Added

- Every module of specify-core ported: part loading, features, existing PMI, the rules and
  their defaults with reasons, mates in assemblies, meshing for a face picker, anchors, the
  writer with resume and assemblies, the API, the command line (`analyse`, `mesh`, `questions`,
  `write`, `parts`, `serve`) and the JSON-lines service, with specify-core's JSON formats and
  exit codes, so Draftwright Specify can drive either engine.
- Every output checked against captures from the Python implementation; every difference from
  Python carries a verdict and its evidence (`docs/known-differences.md`).
- Written files open in OpenCascade, and so in specify-core's Python: datums, sizes and
  tolerances on one face or several (one usage per face), notes and surface texture in AP242's
  standard forms.
- Stored answers in schema 2 (fingerprints with unsigned zeros), which resume across engines
  with specify-core's `fingerprint-v2` branch.
- ISO 286 fits up to 3150 mm, agreeing with specify-core's `iso286-3150` branch on every class
  and size.
- Knurls written as AP242 turned knurls, their parameters derived from the knurling standard
  (whole tooth count at the nominal pitch; 30° helix and no hand for a diamond knurl).
- Built on quiddity-rust v0.1.0-alpha.1's kernel API (the maintainer's PR #1: AP242
  attribute sets' description and general property); risers and step levels from its
  recognition, with the differences from specify-core's quiddity 0.3.9 oracle verdicted.
- A write whose requirement names no faces writes the rest and lists that item as not written.
- About three times faster than the Python engine on the one part timed (cadgenbench
  flanged_spool_132: analyse 0.8 s against 2.4 s, write 0.5 s against 1.5 s).

### Known limitations

- Answers the older specify-core stored (schema 1) do not resume in Rust; specify-core's
  `fingerprint-v2` and `iso286-3150` branches are not yet merged into its main.
- specify-core's Python reads a Rust-written fit without its limits, and no thread or surface
  finish (it reads only its own forms of those).
- A knurl's tooth depth (half the pitch) is assumed, as no table available gives it
  (docs/questions.md).
- Open maintainer questions are in docs/questions.md; the needs from quiddity-rust in
  docs/upstream-needs.md.
