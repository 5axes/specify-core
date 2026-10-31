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

`public/samples/plate.step` comes from `pzfreo/specify-core` at
`42ff22c093f544aab934f30dbe625844fa5d55a3`, under AGPL-3.0-or-later.
`plate.json` is captured by this repository's `dw-bench` capture example using both Rust
engines. Its provenance records the Specify revision and the input fixture.

`scripts/fixtures/assembly.step` and `placed.step` are the same upstream revision's
synthetic `assembly.step` and
`a_placed_part_is_meshed_in_the_frame_the_analysis_uses.step` fixtures, also under
AGPL-3.0-or-later. They check assembly refusal and placed-part loading.

The browser engines are published upstream WASM packages pinned in
`engines.lock.json`. They are downloaded with SHA-256 verification; the complete
package, including upstream licensing and font notices, is served beside its JS
and WASM. Local Rust engine wrappers and the adapted PMI writer have been removed.
The native integration harness uses the same upstream crates through Cargo.
The upstream Specify licence is preserved in `specify-core`'s `LICENSE`.
