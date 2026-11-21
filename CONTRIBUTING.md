# Contributing to Specify

Contributions are welcome: bug reports, fixes, features and documentation.

Before review, run `tools/verify.sh`. It runs what CI runs on a pull request (the Rust suite, the WASM
checks, the UI and app jobs against this checkout's build, the wheel's pytest and the crate package) and
ends with `VERIFY: PASS` or `VERIFY: FAIL (<steps>)`; paste that output in the pull request. Commit first: its crate-package step refuses uncommitted changes, as CI's does.
`tools/verify.sh --dry-run` lists the steps. The corpus is
[pzfreo/quiddity-corpus](https://github.com/pzfreo/quiddity-corpus) at the revision `.github/workflows/ci.yml`
pins; point `SPECIFY_CORPUS` at its checkout (default `../quiddity/tests/corpus`).

A change to the WASM API also needs its `api.d.ts` snapshot and a CHANGELOG `### WASM API` entry
(`python3 tools/check_wasm_api.py --base-ref origin/main`). CI's `wasm` job runs that and the size and
parity checks; `.github/workflows/ci.yml` lists every step.

A change that alters a recorded expectation (fixtures, oracle departures, known differences)
says why for each change. Don't weaken a test to make it pass.

## Test files and data

Add only files you created yourself or whose licence allows redistribution, and record each
file's source and licence beside it (a fixture README or NOTICE entry). Never add customer,
confidential or NDA-covered files, or geometry extracted from them, to the repository, to
issues or to pull requests. For a bug seen on such a part, share a reduced synthetic part that
reproduces it instead.

## Developer Certificate of Origin

Contributions are accepted under the project's licence, Apache-2.0 (its section 5 covers
contributions). Each commit must be signed off to certify the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote the
change, or otherwise have the right to submit it under that licence. Sign off with
`git commit -s`, which adds a line with your real name and email:

```
Signed-off-by: Your Name <you@example.com>
```

A check on pull requests from forks confirms every commit is signed off. To sign off commits
already made, `git fetch upstream && git rebase --signoff upstream/main` (with `upstream` this repository) and force-push the branch.
