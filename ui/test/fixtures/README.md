# Test fixtures

The fixtures here are used by this package's unit tests (`test/*.test.mjs`) and its component
browser test (`test/step-diff-browser.mjs`). Tests of the editor's other behaviour, which
reference further fixtures, stay in `pzfreo/draftwright-editor`.

The package's licence is Apache-2.0 (`LICENSE`); the fixtures keep the licences named below.

`cast_bearing_unit_before.step.gz` and `cast_bearing_unit_after.step.gz` are
`cast_bearing_unit.step` from `pzfreo/step-diff-demo` (Apache-2.0) at
`8103d57768f9fa7ddda57c0b126cfd4d1ceefb88` and `9f1018816812bb0d305f7b7d06d4d5262afd60a2`,
gzipped: the demo pull request's two revisions. `test/step-diff-browser.mjs` compares them.

`gramil_assembly_before.step.gz` and `gramil_assembly_after.step.gz` are gramil's assembly
(`pzfreo/gramil` at `9171ef6`, Open Community License v1), 12 distinct parts, the same bytes
as `tests/fixtures/revisions/gramil_assembly.old.step.gz` and `.new.step.gz` in
`pzfreo/specify-core` (branch `assembly-diff`), made there by `gramil_assembly.py`. Both
are the production build (`process.prototype=False`, as gramil's `scripts/build_production.py`).
After is gramil's defaults; before has `shaft.drive_plate_mount_depth` 4 (not 5) and
`drive_plate.mount_hole_diameter` 2.2 (not 2.4): GRM-02 shaft and GRM-04 drive plate change,
the other 10 parts do not. `test/step-diff-browser.mjs` compares them as an assembly.

`gramil_shaft_before.step.gz` and `gramil_shaft_after.step.gz` are gramil's production shaft
(`pzfreo/gramil` at `9171ef6`, Open Community License v1), the same bytes as
`tests/fixtures/revisions/gramil_shaft.old.step.gz` and `.new.step.gz` in `pzfreo/specify-core`
at `3bb019e`, made there by `gramil_shaft.py`: after adds a 1.5 mm end chamfer. With the answers
Specify defaults for before, the diff pairs location dimensions whose ids differ (`old_id`):
`test/step-diff-browser.mjs` uses them for a renumbered item's old callout. (Its no-callout note
comes from the step-diff-demo spindle pair: with Specify's defaults accepted on the old side,
specify-core 0.1.8 reports the defaulted M4 thread removed, and its manufacturing note with no
callout, only faces.)

`19-spindle-semantic-links-only.step.gz` is `19-spindle-semantic-links-only.step` from the
2026-10-30 PMI comparison (#128), gzipped: a turned spindle Specify wrote, whose graphical PMI
(the 37.6 / 33.6 / 27.2 / 5 / 1.5 ladder, ⌖⌀0.2 A and ⌖⌀0.3Ⓜ A, ⌀3.3 H11, datums A and B) is
linked to its semantic items. `19-spindle-semantic-links-only.presentation.json.gz` and
`.pmi.json.gz` are its `haecceity.presentation/1` and `haecceity.pmi/1`, written by
`quiddity pmi json presentation` and `quiddity pmi json export` (quiddity branch `cx/q142`
at `3698257`, json feature), gzipped. `test/graphical-pmi.test.mjs` reads them. (The editor's `pmi-scene-wasm.test.mjs` and
`specify-only-browser.mjs` also use them and stay in draftwright-editor.)

`spindle_pmi_before.step.gz` and `spindle_pmi_after.step.gz` are the spindle above written by
Specify v0.1.6's WASM (`IntentEngine.writePmi`, defaults accepted, as the editor writes) with two
sets of answers: before, `part.material` "Aluminium 6082-T6" and the defaults; after, the same
with `hole.position:holes:14` 0.2 (was 0.3), `hole.position:holes:15` none (was 0.2),
`part.edges` "Break sharp edges 0.5 max" and `datum.B.control` 0.05. Specify's diff of them
reports six PMI changes: #12 ⌖ ⌀0.2 A removed, #13 ⌖ ⌀0.3Ⓜ A → ⌖ ⌀0.2Ⓜ A, #16 and #17 (the
tapped hole's location dimensions) and #18 ⟂ 0.05 A added, and the edges note reworded.
`test/step-diff-browser.mjs` compares them with their PMI drawn.

`enclosure_base_pmi_before.step.gz` and `enclosure_base_pmi_after.step.gz` are two revisions of
an electronics enclosure base, each with PMI Specify wrote, gzipped (the 2026-11-18 diff demo's
`electronics-enclosure-base-pmi.step` and `electronics-enclosure-base-rev2-pmi.step`). Only the
base part, before's geometry, is text-to-cad's (its examples; MIT, Copyright (c) 2026 Thompson
Labs LLC; the licence is in `LICENSE-text-to-cad`). The second revision and the PMI on both are
ours (this repository's licence): after changes the four Ø3 × 8 blind holes to Ø3.4 and adds an
Ø8 × 3 through hole on the +X side. `specify-core diff` of them leaves one new
requirement to specify and two carried ones to check, and carries the rest of the answers:
`test/app-browser.mjs` and `test/app-agent.mjs` use them for diff mode.
