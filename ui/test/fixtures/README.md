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
It verifies that the shared alpha kernel resumes existing browser exports and
that rewriting them migrates the payload to schema 2 without losing answers.
