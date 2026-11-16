# Engine verification fixtures

`scattered_holes.step` is the synthetic crowded plate from
`pzfreo/draftwright`, revision `59180446bfe406863df4787a533cb5ebdab6b379`,
`tests/fixtures/parts/scattered_holes.step`, under AGPL-3.0-only. It exercises
restoring a computed callout that the automatic planner replaced with a hole table.

`assembly.step` and `placed.step` are synthetic loading fixtures from
`pzfreo/specify-core` at `42ff22c093f544aab934f30dbe625844fa5d55a3`, under
AGPL-3.0-only. They exercise assembly refusal and single-part placement restoration.

`plate.step` is Specify's synthetic mounting plate from `pzfreo/specify-core` at
`42ff22c093f544aab934f30dbe625844fa5d55a3` (`tests/fixtures/parts/plate.step`), under
AGPL-3.0-or-later. It was the editor's sample until text-to-cad's mounting plate replaced it;
`verify:engines`, `test:wasm-parity`, the bench edit sequences, the MCP evaluation tasks, the
native `dw-bench` tests, `scripts/mcp-card-browser.mjs` and smoke's STEP-reopening steps use it
for its holes (the ⌀8 bore `holes:9`, the 6× ⌀6.6 pattern), its M5 tapped holes and its datums.

`specify-sample-plate.step` is the plate draftwright.io's Specify offered as a sample, copied
unchanged from `pzfreo/draftwright-io`'s `public/samples` (MIT, `third_party/draftwright-io/LICENSE`).
It was the start screens' "Plate" sample until text-to-cad's mounting plate replaced it.
`scripts/agent-wasm.test.mjs`, `bench/agent-bench.mjs`, `scripts/webmcp-browser.mjs` and the
sheet-format steps of `scripts/sheets-browser.mjs` use it for its unclaimed face, its fix choices
at 5:1 and its sheet sizes.

`legacy_specify.step` is the mounting-plate sample saved by the pre-alpha browser
writer (Specify `42ff22c0`) with H7 and 6082-T6 answers, using stored-answer schema 1.
The geometry is `plate.step`, Specify's AGPL sample.
Since quiddity v0.1.1 its schema 1 fingerprint cannot be reproduced (some zero centroids
change sign). Specify alpha.8 resumes its answers by feature ID and marks them unverified;
the editor's report says so.

`alpha6_specify.step` is `plate.step` written by the Specify
v0.1.0-alpha.6 WASM (stored-answer schema 2, quiddity alpha.8) with 6082-T6, an H7 fit on
`holes:9` and an answer for `holes:99`, a feature the plate does not have. It verifies
that alpha.6 exports resume under the current kernel and that an answer that cannot be
matched is reported with its value.

`alpha1_bolt_thread_knurl.step.gz` is copied unchanged from
`pzfreo/draftwright` tag `v0.1.0-alpha.1`, commit
`d4ed8d128f0383ea504f8a0ce61502bd05bf33e1`, at
`tests/fixtures/pmi/step/bolt_thread_knurl.step.gz` (AGPL-3.0-only).
It checks native manufacturing thread/knurl callouts alongside the separate
semantic AP242 thread fallback exercised by the plate fixture.

`cast_bearing_unit_before.step.gz` and `cast_bearing_unit_after.step.gz` are
`cast_bearing_unit.step` from `pzfreo/step-diff-demo` (Apache-2.0) at
`8103d57768f9fa7ddda57c0b126cfd4d1ceefb88` and `9f1018816812bb0d305f7b7d06d4d5262afd60a2`,
gzipped: the demo pull request's two revisions. `scripts/diff-browser.mjs` compares them
on the /diff page.

`gramil_assembly_before.step.gz` and `gramil_assembly_after.step.gz` are gramil's assembly
(`pzfreo/gramil` at `9171ef6`, Open Community License v1), 12 distinct parts, the same bytes
as `tests/fixtures/revisions/gramil_assembly.old.step.gz` and `.new.step.gz` in
`pzfreo/specify-core` (branch `assembly-diff`), made there by `gramil_assembly.py`. Both
are the production build (`process.prototype=False`, as gramil's `scripts/build_production.py`).
After is gramil's defaults; before has `shaft.drive_plate_mount_depth` 4 (not 5) and
`drive_plate.mount_hole_diameter` 2.2 (not 2.4): GRM-02 shaft and GRM-04 drive plate change,
the other 10 parts do not. `scripts/diff-browser.mjs` compares them as an assembly.

`gramil_shaft_before.step.gz` and `gramil_shaft_after.step.gz` are gramil's production shaft
(`pzfreo/gramil` at `9171ef6`, Open Community License v1), the same bytes as
`tests/fixtures/revisions/gramil_shaft.old.step.gz` and `.new.step.gz` in `pzfreo/specify-core`
at `3bb019e`, made there by `gramil_shaft.py`: after adds a 1.5 mm end chamfer. With the answers
Specify defaults for before, the diff pairs location dimensions whose ids differ (`old_id`):
`scripts/diff-browser.mjs` uses them for a renumbered item's old callout. (Its no-callout note
comes from the step-diff-demo spindle pair: with Specify's defaults accepted on the old side,
specify-core 0.1.8 reports the defaulted M4 thread removed, and its manufacturing note with no
callout, only faces.)

`19-spindle-semantic-links-only.step.gz` is `19-spindle-semantic-links-only.step` from the
2026-10-30 PMI comparison (#128), gzipped: a turned spindle Specify wrote, whose graphical PMI
(the 37.6 / 33.6 / 27.2 / 5 / 1.5 ladder, ⌖⌀0.2 A and ⌖⌀0.3Ⓜ A, ⌀3.3 H11, datums A and B) is
linked to its semantic items. `19-spindle-semantic-links-only.presentation.json.gz` and
`.pmi.json.gz` are its `haecceity.presentation/1` and `haecceity.pmi/1`, written by
`quiddity pmi json presentation` and `quiddity pmi json export` (quiddity branch `cx/q142`
at `3698257`, json feature), gzipped. `scripts/graphical-pmi.test.mjs` reads them;
`scripts/pmi-scene-wasm.test.mjs` reads the spindle's scene from Specify's pmiScene, and
`scripts/specify-only-browser.mjs` opens the spindle in /specify and draws it in Graphical (View PMI) from it.

`ctc01-text-filled.step.gz` is haecceity's
`crates/haecceity/tests/fixtures/presentation/ctc01-text-filled.step.gz` (quiddity `v0.2.8`):
NIST CTC-01 with its tessellated (filled) graphical PMI, plus one authored text literal,
`CTC-01 editable test text` at `[10, 20, 30]`, 3 mm high, that haecceity's `build_fixture.py`
adds (that directory's README records how). It is the foreign-PMI part: its callouts are the
file's (`source: file`, status `existing`). `scripts/pmi-scene-wasm.test.mjs` and
`scripts/specify-only-browser.mjs` use it.

`spindle_pmi_before.step.gz` and `spindle_pmi_after.step.gz` are the spindle above written by
Specify v0.1.6's WASM (`IntentEngine.writePmi`, defaults accepted, as the editor writes) with two
sets of answers: before, `part.material` "Aluminium 6082-T6" and the defaults; after, the same
with `hole.position:holes:14` 0.2 (was 0.3), `hole.position:holes:15` none (was 0.2),
`part.edges` "Break sharp edges 0.5 max" and `datum.B.control` 0.05. Specify's diff of them
reports six PMI changes: #12 ⌖ ⌀0.2 A removed, #13 ⌖ ⌀0.3Ⓜ A → ⌖ ⌀0.2Ⓜ A, #16 and #17 (the
tapped hole's location dimensions) and #18 ⟂ 0.05 A added, and the edges note reworded.
`scripts/diff-browser.mjs` compares them on /diff with their PMI drawn.

`spindle-specify-0.1.15.project.json` is the editor's stored-project payload for
`19-spindle-semantic-links-only.step.gz`, generated with Specify v0.1.15
(`7769ddad1fb1fb5723db3eeac145c258b3a1fbd4`). The released
`specify-core-wasm-0.1.15.tgz` was verified against SHA-256
`6d9362b767e1634642354fb541ba75ef97ae3b4f9f4830cda69317394686fa1c`.
Its answers came from that release's `IntentEngine.acceptDefaults` with explicit material
`6082-T6`, hole function `tapped:M2`, position `0.17`, and full-thread depth `1.4`, all
under the old `holes:15` keys. `answerEvidence` is that release's `evidence()` output;
`auto` lists the remaining defaulted answers and `accepted` the four explicit questions.
The source bytes stay in the existing gzip fixture, identified by the payload's SHA-256.
`pmi-scene-wasm.test.mjs` checks rematching and project persistence on the new kernel;
`save-browser.mjs` imports the old project through the editor, checks saved answers,
acceptances and automatic statuses, and checks the existing notice for a deliberately
inapplicable `holes:99` answer. See `docs/SPECIFY-0.1.16.md` for the kernel comparison.
