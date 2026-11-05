# Answer files and changed geometry

Answers remain a JSON map from question ID to answer. Existing plain maps work in CLI
`questions`/`write`, serve, Python, WASM and the editor's calls through WASM:

```json
{"part.material":"6061-T6","datum.A":[4],"datum.B":[1],"datum.C":[0]}
```

A plain map has no record of the geometry on which its face indices were chosen. Specify
checks whether the supplied answers are valid on the current analysis, but cannot tell
whether an in-range face number now denotes a different face. In particular, the plain v1
plate map above still writes on v2 after a groove splits bottom face 4 into faces 4 and 8:
it selects only face 4. It could equally have been handwritten for v2. Unchanged-geometry
plain maps remain accepted; Specify does not guess provenance from face numbers. Use
`diff` from the actual old part when carrying these maps to edited geometry.

This also applies to **released v0.1.8 and earlier `changes/answers.json` files**: they have no
`_specify` metadata. The literal old `changes/answers.json` repro still exits 0 with
`--accept-defaults`, because nothing in that file says an answer was dropped. **Re-run
`diff` with the original old/new parts and old answers to regenerate these changes files**;
merely upgrading the writer cannot recover their lost carry decisions.

## Export a complete sidecar

Export after choosing required answers, before writing STEP:

```sh
specify-core analyse plate_v1.step -o plate.analysis.json
specify-core questions plate.analysis.json --answers chosen.json \
  --resolve-defaults --step plate_v1.step > plate.answers.json
specify-core write plate_v1.step plate.analysis.json plate.answers.json -o plate.pmi.step
# After editing geometry, carry the committed sidecar from its matching old part:
specify-core diff plate_v1.step plate_v2.step --answers plate.answers.json -o changes
```

`questions --resolve-defaults` is the single CLI export surface. It writes an **answer
map** to stdout, rather than a question list, and never writes STEP. `--step` is required:
an analysis alone cannot supply verified face evidence. Its binding selects the part of an
assembly and must match that STEP. `--open` cannot be combined with export.

The map contains every supplied answer (including any whose question is not asked; see
below) and each accepted PMI default, including defaults
of questions revealed by earlier answers, plus `_specify.evidence` for this part and
`_specify.unresolved: {}`. Optional BOM suggestions remain opt-in; supplied BOM answers
are preserved. Output recursively sorts object keys, uses two-space indentation and a
final newline, and preserves array order. Identical inputs export identical bytes,
independent of input key order. Re-exporting the complete map is idempotent.

An incomplete export **refuses** (CLI exit 2, no JSON on stdout), naming every missing
required question or blocked carry. It never accepts a default for an unresolved question.
Supply explicit answers to acknowledge blocks, then export again. A shell may create or
truncate a redirected file before the command runs; export to a temporary path and rename
on success when replacing an existing committed sidecar. Existing saved evidence is checked
before export, so changed geometry cannot be silently restamped with fresh evidence.

Like `write`, export reads answers only from `--answers`, not from answers already stored
in the STEP: exporting a PMI-written STEP with `{}` refuses its required questions. Pass
the part's committed sidecar (or the answers you want) explicitly.

The same non-writing operation is available in existing front-end shapes:

- Rust: `api::export_answers(&loaded, &analysis, &answers)` returns an answer map.
- Serve: `{"op":"export_answers","step":"plate_v1.step","analysis":<analysis>,"answers":<chosen>}`
  returns `result.answers` in the normal reply envelope.
- Python: `api.export_answers(step_or_loaded_part, analysis, answers)` returns a dict;
  serialize with `json.dumps(complete, indent=2) + "\n"` for the same stable file layout.
- WASM: `engine.exportAnswers(answersJson)` returns the answer map as a JSON string;
  use `JSON.stringify(JSON.parse(result), null, 2) + "\n"` for a formatted sidecar.

These exports accept defaults explicitly and require completeness. Existing
`with_all_defaults` / `acceptDefaults` operations still support partial review and retain
their existing return shapes. Exported maps can be passed directly to `write`, `diff`,
questions/review, serve, Python and WASM. Writing the unchanged part from the export gives
the same semantic PMI as accepting the original defaults. Writing changed geometry is
refused as stale; carry with `diff` instead, then explicitly resolve its blocks.

## Answers to questions not asked

An answer can be valid in form while its question is not asked, given the other answers.
Dependent questions come and go: with `part.locating` `"± dimensions"` there is no
`datum.A.flatness`, `datum.B.control`/`datum.C.control`, `hole.position:*`, `hole.frame:*` or
`diameter.runout:*`, and with `"position tolerances"` no `hole.location:*`; a full-thread
depth is asked only of a blind tapped hole; a datum the file already states is not asked.
`part.process` leaves some part questions unasked (a printed part's `part.surface_finish`,
`part.edges`, `part.coating`; see [process.md](process.md)), and a bought part has no
`part.process`: these part answers lapse in the same way. Other `part.*` answers are read
whether or not they are asked. Which IDs are asked is the rules' question list, the same as `questions` returns.

Such an answer contributes no PMI, and is **never dropped silently**. `apply` adds a
faceless intent row `{"kind":"unasked","answer":ID,"faces":[],"not_written_reason":…}`, and
every write report (CLI, serve, Python, WASM) lists it in `not_written`. `preview` leaves
these rows out, so reviews do not pay for them; the editor's WASM write, which writes its
preview, adds them with Rust `rules::unasked_rows`:

```text
datum.A.flatness is not asked when part.locating is '± dimensions', so its answer was not written
```

Where switching `part.locating` would ask it, the reason says so; otherwise it says the
question is not asked of this part given the other answers, or that the file already states
the datum. `apply` does not refuse these answers: callers revising answers across a change
of `part.locating` keep both modes' answers, and the question takes its answer again when it
returns. The answer is still stored in the STEP with the others, exported unchanged by
`questions --resolve-defaults` (dropping it would lose it, and refusing would break the same
callers), and carried by `diff` like any other answer. A carry block applies only while
its question is asked: a block on `datum.A.flatness` under `"± dimensions"` does not stop
`write`, export or bulk defaults, and is listed in `not_written` ("datum.A.flatness was
carried (carried answer could not be applied: …) but is not asked when part.locating is
'± dimensions', so nothing was written for it"). It stays in `_specify.unresolved`, so once
later answers ask the question again it shows as required and refuses again. Write derives
which blocks apply from its own answers on the part, not from the intent's rows.

Through a STEP, the write stores such blocks in the stored-answers payload's own
`unresolved` member (`{ID: reason}`), never inside its `answers`, which older builds would
refuse. Reopening restores them as the resumed answers' `_specify.unresolved`, so the block
applies again when its question returns. Older Specify and editor builds ignore the extra
payload member: they reopen and write the STEP normally, but without the block, so on them
an accepted default can replace it.

An unasked answer's value is checked against the question's options only while the question
is asked. `part.*`, `bom.*` and `extra.*` answers
are read whether or not asked, so they never appear here.

An ID that **no** question can have, such as `bogus.question`, is refused with
`Invalid answers: bogus.question: unknown answer ID` by every entry point: Rust `api`,
CLI, serve, Python and WASM. `extra.*` requirements and the reserved `_specify*` members
remain accepted. Answers stored in a STEP by earlier versions, which kept such IDs verbatim,
are not refused on reopening: the unknown ID is left out of the resumed answers, listed in
`resume.unresolved` with its value, and named in an analysis warning
(`saved answer bogus.question: unknown answer ID: …`), so `questions` and `diff` still work.

## Specify metadata

Specify's complete exports and diff answer maps (`answers.json`, `report.answers`, and
each assembly part's map) add a reserved `_specify` member. Preserve it when editing or storing answers.
Old plain files remain accepted by the new tools. New maps require updated Specify/editor
WASM builds: older builds reject `_specify` as an `unknown answer ID`. Use an updated build
to open and write these maps.

Example:

```json
{
  "part.material":"6061-T6",
  "_specify": {
    "evidence":{"schema":2,"face_count":12,"faces":"<saved face fingerprint>"},
    "unresolved":{"datum.A":"face 4 has no match in the new part"}
  }
}
```

`evidence` is the same `{schema, face_count, faces, face_names?, geometry?}` that Specify
stores alongside answers inside STEP and returns from WASM `evidence()`. It belongs to
the **destination** part's face numbering. Schema 3 may add authored names and geometry;
write requires the saved count, ordered face fingerprint and indexed authored names
to match, without remapping by name. This deliberately refuses changed numbering even if names could be rematched.
Malformed evidence is an error. A new diff checks supplied evidence against its old part
before carrying, so it cannot silently reinterpret stale indices either. A mismatch exits
2: the answers file belongs to another revision, so pass its matching old part as `OLD`.

The shared writer checks evidence against the loaded part before writing bytes. CLI
also checks before saving `--intent`; stale evidence exits 2, names the answers path,
and directs the caller to `specify-core diff OLD NEW --answers ANSWERS -o CHANGES`.
An unchanged part's evidence remains valid even if its file bytes change without changing
faces. Existing in-STEP answer payloads retain their format and evidence; transport
metadata is consumed before answers are embedded.

`unresolved` contains blocks only for revision-stable IDs: `datum.*`, `part.*`
(including general tolerance), `bom.*` and added requirements' `extra.*`. Their entries
preserve diff's exact refusal. An `extra.<name>` key is the person's own name for a
requirement they added, with the same meaning in both revisions; dropping it on write
because its faces did not carry would silently lose a requirement no question asks for
again. Answer it again on the new part's faces, or remove its entry to drop it.
A dropped answer for one of these questions contributes no PMI and cannot be replaced by
bulk default acceptance. CLI fails with
`incomplete: datum.A (carried answer could not be applied: <reason>)`, writes no STEP or
intent output, and preserves any existing output. Serve `accept_defaults`, Python
`apply(..., accept_defaults=True)` and WASM `acceptDefaults` use the same block.
The shared writer also refuses these blocks when called directly with a preview intent.

Feature-scoped IDs such as `hole.function:holes:7` encode old face numbers: after removing
a hole, `holes:7` may already refer to a different new hole. A failed carry has no unique,
verified destination question. These answers remain in **the diff report's `unresolved`
list**, with their original value and reason, but do not create sidecar blocks under old
IDs. Never use an
old feature ID as a clearing answer for a different hole. Review the reported removal or
unmatched requirement, and answer the new part's questions using its current geometry.
Earlier metadata that contains feature-scoped blocks is ignored for blocking, so it cannot
shadow a reused question ID or require an answer to a feature that no longer exists.

`questions` and review responses mark a blocked stable question as `attention: "required"`,
remove its acceptable suggestion (`default: null`), and put
`carried answer could not be applied: <diff reason>` in the existing `reason` field. This
presentation is shared by CLI, serve, Python and WASM. Review can still show partial
answers; the editor's automatic defaults skip blocked IDs.

A block whose question is not asked under the current answers does not refuse; see
[Answers to questions not asked](#answers-to-questions-not-asked).

Supply an explicit valid answer for that ID to acknowledge its refusal (for the split
bottom, `"datum.A":[4,8]`). This overrides its unresolved entry; future diff output removes
the resolved entry. Stable carry blocks survive repeated diffs until acknowledged. If a
question has disappeared entirely, review its removal and explicitly remove its
`unresolved` entry. Removing all metadata discards these protections.

WASM `rematchAnswers` also reads embedded evidence when no separate evidence argument is
given. For maps with `_specify`, it returns fresh evidence and carry blocks for its
unresolved results, so saving the returned answers preserves the protection.

Both metadata fields are optional so callers may keep a carry block without evidence,
or attach WASM `evidence()` to a manually managed map. Do not fabricate evidence or copy
it from a different part. Assembly maps keep metadata inside each part's answer map.
