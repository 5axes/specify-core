# Reused Specify editor components

`src/specify/` and `src/types/pmi.ts` are adapted from `pzfreo/draftwright-io`
at commit `61647f9` (current main when this prototype was scaffolded).
The original MIT licence is preserved in `draftwright-io/LICENSE`.

The Specify review workspace, answer inputs, suggestion cards, requirement and
datum editors, characteristics table, styles, Three.js picker, camera/navigation,
and annotation renderer are ported from that editor. `SpecifyReview.tsx` retains
its review interactions; `SpecifyEditor.tsx` adapts the engine contract to local
WASM and project history. Host changes cover tab layout, external undo/redo,
local STEP downloads and drawing handoff, and a local unavailable-QIF notice. The original
application's authentication, HTTP transport, durable server sessions, analytics,
and drawing handoff are not included in this standalone browser prototype.

`public/samples/mounting-plate.step`, the editor's sample part (also offered on the start
screens as "Mounting plate"), is text-to-cad's mounting plate: the model in
[`models/examples/src/mounting_plate.py`](https://github.com/amywork777/text-to-cad-upstream/blob/85d8fdbfb467ebd7e22994fe8bcd86afa6626ed3/models/examples/src/mounting_plate.py)
at `amywork777/text-to-cad-upstream` commit `85d8fdbfb467ebd7e22994fe8bcd86afa6626ed3`
(the branch of earthtojake/text-to-cad#674), exported to STEP with build123d 0.13.0's
`export_step`. text-to-cad publishes its models as build123d Python, so the STEP is an export,
not an original file. The file is copied unchanged from `pzfreo/dw-examples`
(`steps/text-to-cad/mounting-plate.step`, commit `c4c72a71c6440f0c824678660b59d0474f105e71`),
whose README records the export; its SHA-256 is
`1e71caba689593eb357ca6ac0108130c2655adfda280909db88f49a8baf3c750`.
text-to-cad is Copyright (c) 2026 Thompson Labs LLC, under the MIT License, reproduced in
`text-to-cad/LICENSE`.
`mounting-plate.json` is captured from it by this repository's `dw-bench` capture example using
both Rust engines. Its provenance records the Specify, Draftwright and kernel revisions and the
input part.

`public/samples/nist_ctc_01_asme1_ap242-e1.stp`, the "NIST CTC-01 with PMI" sample, is NIST's
Combined Test Case 1 (CTC-01, ASME tolerancing) in its AP242 edition 1, from the NIST MBE PMI
Validation and Conformance Testing Project, as the file's own header says. It is unmodified: the same bytes as `tests/fixtures/ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz` in
`pzfreo/quiddity` (then quiddity-rust) at `v0.4.1` (`a2195eed18057ec33389cd7d59d47dfeb2c20870`), decompressed;
SHA-256 `036683e8358ac27d51a92c35362e175c91bc44d3b600a5c605fb496865edcd37`. NIST's test files are
works of the US Government and not subject to copyright in the United States (public domain).
`scripts/fixtures/ctc01-text-filled.step.gz` is the same part with one authored text literal added
(see `scripts/fixtures/README.md`); the sample is the original.

The previous sample, Specify's synthetic mounting plate, is now the engine verification fixture
`scripts/fixtures/plate.step` (see `scripts/fixtures/README.md`). It comes from
`pzfreo/specify-core` at `42ff22c093f544aab934f30dbe625844fa5d55a3`, when specify-core was
AGPL-3.0-or-later. specify-core was relicensed Apache-2.0 on 2026-10-30 (`83c49c3`), and
`plate.step` is still in its `tests/fixtures/parts` at HEAD, so it is available under Apache-2.0.

`scripts/fixtures/assembly.step` and `placed.step` are the same upstream revision's
synthetic `assembly.step` and
`a_placed_part_is_meshed_in_the_frame_the_analysis_uses.step` fixtures, also taken when specify-core
was AGPL-3.0-or-later; since the Apache-2.0 relicence of 2026-10-30 (`83c49c3`) they are available
under Apache-2.0 (both are still in its `tests/fixtures/parts` at HEAD). They check
assembly refusal and placed-part loading.

The browser engines are published upstream WASM packages pinned in
`engines.lock.json`. They are downloaded with SHA-256 verification; the complete
package, including upstream licensing and font notices, is served beside its JS
and WASM. Local Rust engine wrappers and the adapted PMI writer have been removed.
The native integration harness uses the same upstream crates through Cargo.
The upstream Specify licence is preserved in `specify-core`'s `LICENSE`.
