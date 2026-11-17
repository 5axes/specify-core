# @draftwright/specify-ui

Two React components for the browser, on [specify-core-wasm](https://github.com/pzfreo/specify-core):

- **Specify review** (`SpecifyReview`): the workspace for answering a part's Specify questions, with the part, its PMI
  callouts, the characteristics table and the AS9102 / CSV exports.
- **STEP diff** (`StepDiff`, `AssemblyDiffView`): compares two revisions of a STEP file (or assembly) with Specify's
  diff, lists what changed, draws the part and its PMI changes in 3D, and links the two ways.

There is no drawing engine here; drawings are Draftwright's.

## Install

```sh
npm install @draftwright/specify-ui
```

Peer dependencies, which the host provides (the package bundles none of them): `react` and `react-dom` ^19,
`three` 0.186.x and `three-cad-viewer` ~5.1.3.

The Specify engine comes in as a dependency, `@draftwright/specify-core-wasm` (pinned to the release in
`engines.lock.json`). It runs in a worker; the host's bundler builds that worker and copies the engine's `.wasm`
beside it, so a host passes nothing. Two things the host's build must allow:

- **Vite**: in development, keep the package out of dependency pre-bundling, or the worker's URL breaks:
  `optimizeDeps: { exclude: ['@draftwright/specify-ui'] }`. Production builds need nothing.
- **Content Security Policy**: the engine needs `'wasm-unsafe-eval'` in `script-src`, and `worker-src 'self'`.

The pages also load **the font** `/fonts/IBMPlexMono-Regular.ttf` (IBM Plex Mono, SIL OFL; `src/specify/PmiLab.css`),
which the host serves at that path.

### Serving the engine yourself (`wasmUrl`)

A host that serves the engine itself (from a CDN, or a build it pins) passes `wasmUrl`: the URL of the
`specify_core_wasm.js` module (not a folder), resolved against the page, with `specify_core_wasm_bg.wasm` beside it.
Both files are in the `@draftwright/specify-core-wasm` package, or in the `specify-core-wasm-<version>.tgz` asset of a
[specify-core release](https://github.com/pzfreo/specify-core/releases). For example, Draftwright's editor has
served them as `/wasm/intent/specify_core_wasm.js` and `/wasm/intent/specify_core_wasm_bg.wasm` from its own
`public/`, installed by a checksum-pinned script like this repo's `scripts/fetch-wasm.mjs`, and passed
`wasmUrl="/wasm/intent/specify_core_wasm.js"`. A cross-origin `wasmUrl` needs the CSP's `script-src` (and
`connect-src`, for the `.wasm`) to allow it. Use the engine version the package pins; the components are tested
with that one.

## Usage

`StepDiff` compares two revisions of a STEP file:

```tsx
import { StepDiff } from '@draftwright/specify-ui';
import '@draftwright/specify-ui/style.css';

<StepDiff
  before={{ name: 'bracket.step', bytes: oldBytes }}   // Uint8Array; stepBytes(file) reads a File, .step.gz too
  after={{ name: 'bracket.step', bytes: newBytes }}
  onEvent={name => console.log(name)}                   // 'diff_viewed' | 'diff_failed' | 'diff_change_picked'
  // wasmUrl="/wasm/intent/specify_core_wasm.js"        // only when serving the engine yourself
/>
```

`SpecifyReview` is the review workspace. It renders into elements the host lays out (`slots`), and talks to the
engine through a `SpecifyEngine` (`questions`, `interpret`, `preview`) that the host implements, usually over
`createIntentEngine()`, the engine in a worker (a `WorkerClient`, with `request(operation, args)`). Its `session` is the
part opened: its analysis, mesh, questions and answers.

```tsx
import { SpecifyReview, type SpecifyEngine, type SpecifySlots } from '@draftwright/specify-ui';
import '@draftwright/specify-ui/style.css';

const slots: SpecifySlots = { view: viewElement, panel: panelElement, exports: exportsElement };

<SpecifyReview
  engine={engine}                    // your SpecifyEngine
  session={session}                  // the open part (PmiLabSession)
  slots={slots}
  externalAnswers={answers}          // the answers so far
  externalReview={review}            // Specify's review of them
  disabled={false}
  onExport={action => action()}      // run an export (gate it on sign-in, say)
/>
```

The helpers `characteristics`, `as9102`, `xlsx`, the diff and PMI-diff functions and the `types/pmi` types are
exported too; see `src/index.ts`.

## Development

Node 22.

```sh
npm ci
npm run fetch:wasm     # specify-core-wasm into public/wasm/intent (the browser test's own checks)
npm run build          # typecheck, vite library build, declarations
npm test               # unit tests (node --experimental-strip-types)
npm run test:browser   # builds test/harness and drives <StepDiff> in Chromium (Playwright)
```

The browser test needs a Chromium for Playwright (`npx playwright install chromium`, or set `CHROMIUM_PATH`).

Until `@draftwright/specify-core-wasm` is on npm, `package-lock.json` resolves it from the checksum-pinned
specify-core release asset; refresh the lockfile from the registry once it is published (the release workflow
refuses to publish before then).

## Releasing

CI (`.github/workflows/ci.yml`) runs every suite on each pull request and push to main. Publishing a GitHub release
tagged `v<version>` runs `.github/workflows/release.yml`: it checks the tag, the engine pin and that the engine is on
npm, runs the suites, packs, and then waits for approval in the `npm` environment before publishing with npm trusted
publishing (OIDC, with provenance).

## Licence

Apache-2.0, Copyright (C) 2026 Betwixt Limited (trading as Draftwright). See `LICENSE` and `NOTICE`, which also
records third-party material (`third_party/`, `test/fixtures/README.md`).
