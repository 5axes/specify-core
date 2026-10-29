# Engine verification fixtures

`scattered_holes.step` is the synthetic crowded plate from
`pzfreo/draftwright-rust`, revision `59180446bfe406863df4787a533cb5ebdab6b379`,
`tests/fixtures/parts/scattered_holes.step`, under AGPL-3.0-only. It exercises
restoring a computed callout that the automatic planner replaced with a hole table.

`assembly.step` and `placed.step` are synthetic loading fixtures from
`pzfreo/specify-core-rust` at `42ff22c093f544aab934f30dbe625844fa5d55a3`, under
AGPL-3.0-only. They exercise assembly refusal and single-part placement restoration.

`legacy_specify.step` is the mounting-plate sample saved by the pre-alpha browser
writer (Specify `42ff22c0`) with H7 and 6082-T6 answers, using stored-answer schema 1.
The geometry is `public/samples/plate.step` from Specify's AGPL sample corpus.
Since quiddity v0.1.1 its schema 1 fingerprint cannot be reproduced (some zero centroids
change sign). Specify alpha.8 resumes its answers by feature ID and marks them unverified;
the editor's report says so.

`alpha6_specify.step` is `public/samples/plate.step` written by the Specify
v0.1.0-alpha.6 WASM (stored-answer schema 2, quiddity alpha.8) with 6082-T6, an H7 fit on
`holes:9` and an answer for `holes:99`, a feature the plate does not have. It verifies
that alpha.6 exports resume under the current kernel and that an answer that cannot be
matched is reported with its value.

`alpha1_bolt_thread_knurl.step.gz` is copied unchanged from
`pzfreo/draftwright-rust` tag `v0.1.0-alpha.1`, commit
`d4ed8d128f0383ea504f8a0ce61502bd05bf33e1`, at
`tests/fixtures/pmi/step/bolt_thread_knurl.step.gz` (AGPL-3.0-only).
It checks native manufacturing thread/knurl callouts alongside the separate
semantic AP242 thread fallback exercised by the plate fixture.

`cast_bearing_unit_before.step.gz` and `cast_bearing_unit_after.step.gz` are
`cast_bearing_unit.step` from `pzfreo/step-diff-demo` (Apache-2.0) at
`8103d57768f9fa7ddda57c0b126cfd4d1ceefb88` and `9f1018816812bb0d305f7b7d06d4d5262afd60a2`,
gzipped: the demo pull request's two revisions. `scripts/diff-browser.mjs` compares them
on the /diff page.
