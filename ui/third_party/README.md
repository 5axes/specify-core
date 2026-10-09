# Reused Specify editor components

`src/specify/` and `src/types/pmi.ts` are adapted from `pzfreo/draftwright-io`
at commit `61647f9` (current main when this prototype was scaffolded).
The original MIT licence is preserved in `draftwright-io/LICENSE`.

The reusable parts are the Three.js model picker, camera/navigation, annotation
renderer, characteristic formatting, and question review helpers. The original
application's authentication, HTTP transport, durable server sessions, analytics,
and drawing handoff are not included in this standalone browser prototype.

`public/samples/plate.step` comes from `specify-core-rust` at
`42ff22c093f544aab934f30dbe625844fa5d55a3`, under AGPL-3.0-or-later.
`plate.json` is captured by this repository's `dw-intent` example using both Rust
engines. Its provenance records the Specify revision and the input fixture.

`scripts/fixtures/assembly.step` and `placed.step` are the same upstream revision's
synthetic `assembly.step` and
`a_placed_part_is_meshed_in_the_frame_the_analysis_uses.step` fixtures, also under
AGPL-3.0-or-later. They check assembly refusal and placed-part loading.

`crates/intent/src/pmi_writer.rs` is adapted from that revision's `src/writer.rs`
under AGPL-3.0-or-later. It retains semantic authoring, AP242 verification and
merge/replacement logic, replacing filesystem staging with a single-part byte API.
It depends on the pinned upstream's public types and is intended to move upstream.
The upstream licence is preserved in `specify-core-rust/LICENSE`.
