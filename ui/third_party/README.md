# Reused Specify editor components

`src/specify/` and `src/types/pmi.ts` are adapted from `pzfreo/draftwright-io`
at commit `61647f9` (current main when the editor prototype was scaffolded).
The original MIT licence is preserved in `draftwright-io/LICENSE`.

The Specify review workspace, answer inputs, suggestion cards, requirement and
datum editors, characteristics table, styles, Three.js picker, camera/navigation,
and annotation renderer are ported from that editor. `SpecifyReview.tsx` retains
its review interactions. The original application's authentication, HTTP
transport, durable server sessions, analytics, and drawing handoff are not
included. The editor's own host code (`SpecifyEditor.tsx`, project history,
downloads) stays in `pzfreo/draftwright-editor`, which also keeps the tests
that exercise it.

Parts of `src/specify/annotations.ts` are ported from pmi-assist-viewer, which
vendors step-pmi-viewer's label markup (Apache-2.0); see `NOTICE`.

The test fixtures and their provenance are in `test/fixtures/README.md`.
