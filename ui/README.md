# specify-ui

Two React components for the browser, on [specify-core-wasm](https://github.com/pzfreo/specify-core):

- **Specify review** (`SpecifyReview`): the workspace for answering a part's Specify questions, with the part, its PMI
  callouts, the characteristics table and the AS9102 / CSV exports.
- **STEP diff** (`StepDiff`, `AssemblyDiffView`): compares two revisions of a STEP file (or assembly) with Specify's
  diff, lists what changed, draws the part and its PMI changes in 3D, and links the two ways.

There is no drawing engine here; drawings are Draftwright's.

## Status

Private and pre-release (version 0.0.0). The npm scope is undecided and nothing is published.

## Usage

```tsx
import { StepDiff } from 'specify-ui';
import 'specify-ui/style.css';

<StepDiff
  before={{ name: 'bracket.step', bytes: oldBytes }}
  after={{ name: 'bracket.step', bytes: newBytes }}
  wasmUrl="/wasm/intent/specify_core_wasm.js"
  onEvent={name => console.log(name)}   // 'diff_viewed' | 'diff_failed' | 'diff_change_picked'
/>
```

`createIntentEngine({ wasmUrl })` starts the Specify engine in a worker (a `WorkerClient`) for `SpecifyReview` and
your own calls. The helpers `characteristics`, `as9102`, `xlsx`, the diff and PMI-diff functions and the `types/pmi`
types are exported too; see `src/index.ts`.

## What the host serves

- **specify-core-wasm** at `wasmUrl` (default `/wasm/intent/specify_core_wasm.js`), with its `_bg.wasm` beside it.
  The WASM is not bundled. `engines.lock.json` pins the release the tests use; `npm run fetch:wasm` installs it
  (checksum-verified) into `public/wasm/intent/`.
- **The font**: `/fonts/IBMPlexMono-Regular.ttf` (IBM Plex Mono, SIL OFL), loaded by `src/specify/PmiLab.css`.

## Peer dependencies

`react` ^19, `react-dom` ^19 and `three` 0.186.x. `three-cad-viewer` 5.1.3 is a dependency. The built bundle
(`dist/specify-ui.js`, `specify-ui.css`, declarations, and the worker in `dist/assets/`) leaves all of these external.

## Development

Node 22.

```sh
npm install
npm run fetch:wasm     # specify-core-wasm into public/wasm/intent (tests, dev)
npm run build          # typecheck, vite library build, declarations
npm test               # unit tests (node --experimental-strip-types)
npm run test:browser   # builds test/harness and drives <StepDiff> in Chromium (Playwright)
```

The browser test needs a Chromium for Playwright (`npx playwright install chromium`, or set `CHROMIUM_PATH`).

## Licence

Apache-2.0, Copyright (C) 2026 Betwixt Limited (trading as Draftwright). See `LICENSE` and `NOTICE`, which also
records third-party material (`third_party/`, `test/fixtures/README.md`).
