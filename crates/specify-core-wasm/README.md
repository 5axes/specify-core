# specify-core-wasm

Browser bindings for specify-core. See [the WASM guide](https://github.com/pzfreo/specify-core/blob/main/docs/wasm.md)
for the JSON API, build instructions and release installation.

Initialize the web module, then use `IntentEngine`, `review` and `interpret`.
Errors are thrown as strings. STEP input must be decompressed. The constructor opens a file
of one distinct part with one placement; for an assembly or a part placed more than once, list
the parts with `IntentEngine.parts(name, bytes)` and open one with
`IntentEngine.openPart(name, bytes, part)`, whose `writePmi` writes that part into the
original file. `writePmi` uses the upstream verified writer and optionally stores the
base64 `draftwright editor project` attribute on the part.

Licensed under AGPL-3.0-or-later; bundled haecceity/quiddity are AGPL-3.0-only.

For #87, snapshot faces include usable unique authored STEP `f:` names.
Shared/generic labels retain geometric matching and are omitted from analysis. Keep the entire
`evidence()` object with external answers: named parts use schema 3; missing or
duplicate names are unresolved without geometric fallback. See
[face-name matching and compatibility](../../docs/face-names.md).

Complete answer sidecars (defaults explicit, with geometry evidence) can be exported
before writing: `engine.exportAnswers(answersJson)` returns an answer map as JSON.
Incomplete sets and unresolved carries are refused. See
[answer files](../../docs/answers.md#export-a-complete-sidecar).
