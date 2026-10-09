# Engine verification fixtures

`scattered_holes.step` is the synthetic crowded plate from
`pzfreo/draftwright-rust`, revision `59180446bfe406863df4787a533cb5ebdab6b379`,
`tests/fixtures/parts/scattered_holes.step`, under AGPL-3.0-only. It exercises
restoring a computed callout that the automatic planner replaced with a hole table.

`assembly.step` and `placed.step` are synthetic loading fixtures from
`pzfreo/specify-core-rust` at `42ff22c093f544aab934f30dbe625844fa5d55a3`, under
AGPL-3.0-only. They exercise assembly refusal and single-part placement restoration.
