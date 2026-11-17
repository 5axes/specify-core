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
`three` 0.184 to 0.186 and `three-cad-viewer` ~5.1.3. `three-cad-viewer` 5.1.3 depends on `three` 0.184.0 exactly, so
install `three@0.184.0` to have one copy of three; with any other version the host bundles two (this package's
tests run on 0.184.0).

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

The default engine is still imported (the worker loads it when no `wasmUrl` is given), so the host's build still
emits its ~16 MB `.wasm`, although the page never fetches it. To leave it out, alias the package to a stub, in Vite:

```js
// vite.config.js
resolve: { alias: { '@draftwright/specify-core-wasm': fileURLToPath(new URL('./src/no-bundled-engine.js', import.meta.url)) } }
// src/no-bundled-engine.js
export default function init() { throw new Error('This host serves the Specify engine itself: pass wasmUrl.'); }
```

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
npm run test:browser   # builds test/harness and drives <StepDiff> in Chromium (Playwright), on the
                       # dependency's engine and again with wasmUrl
npm run test:package   # packs the tarball, builds test/package-host on it, and runs the browser test there
```

The browser test needs a Chromium for Playwright (`npx playwright install chromium`, or set `CHROMIUM_PATH`).

Until `@draftwright/specify-core-wasm` is on npm, `package-lock.json` resolves it from the checksum-pinned
specify-core release asset; refresh the lockfile from the registry once it is published (the release workflow
refuses to publish before then).

## Releasing

CI (`.github/workflows/ci.yml`) runs every suite on each pull request and push to main. Publishing a GitHub release
tagged `v<version>` runs `.github/workflows/release.yml`: it checks the tag, the engine pin and that the engine is on
npm, runs the suites, packs, and then waits for approval in the `npm` environment before publishing with npm trusted
publishing (OIDC, with provenance). The guard also refuses a release whose tag is not a `v*` tag on main, or whose
engine on npm is not the pinned release's. The `npm` environment must have a required reviewer and be restricted to
`v*` tags.

npm adds a trusted publisher only from an existing package's settings, so the very first publish
(v0.1.0) uses a short-lived token:

1. Create a short-lived granular npm token with publish rights to `@draftwright`. If the npm account requires two-factor authentication for writes, enable
   "Bypass two-factor authentication" on the token, otherwise the publish fails with EOTP.
2. Add it as secret `NPM_TOKEN` in the `npm` environment (maintainer as required reviewer, `v*` tags allowed).
3. Publish the GitHub release `v0.1.0` and approve the `npm` environment.
4. Delete the `NPM_TOKEN` secret and revoke the token.
5. Within 2 days before the next release (a new trusted publisher expires unless it publishes), add the trusted
   publisher on npmjs.com: organization `pzfreo`, repository `specify-ui`, workflow `release.yml`, environment `npm`,
   with `npm publish` ticked.
6. After it has published once, set "Require two-factor authentication and disallow tokens" on the package.

## Licence

Apache-2.0, Copyright (C) 2026 Betwixt Limited (trading as Draftwright). See `LICENSE` and `NOTICE`, which also
records third-party material (`third_party/`, `test/fixtures/README.md`).
