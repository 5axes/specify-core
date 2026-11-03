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

## Specify metadata

Specify's diff answer maps (`answers.json`, `report.answers`, and each assembly part's
map) add a reserved `_specify` member. Preserve it when editing or storing answers.
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

`unresolved` contains blocks only for revision-stable question IDs: `datum.*`, `part.*`
(including general tolerance) and `bom.*`. Their entries preserve diff's exact refusal.
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
IDs. Added requirements with failed supports likewise remain report-only. Never use an
old feature ID as a clearing answer for a different hole. Review the reported removal or
unmatched requirement, and answer the new part's questions using its current geometry.
Earlier metadata that contains feature-scoped blocks is ignored for blocking, so it cannot
shadow a reused question ID or require an answer to a feature that no longer exists.

`questions` and review responses mark a blocked stable question as `attention: "required"`,
remove its acceptable suggestion (`default: null`), and put
`carried answer could not be applied: <diff reason>` in the existing `reason` field. This
presentation is shared by CLI, serve, Python and WASM. Review can still show partial
answers; the editor's automatic defaults skip blocked IDs.

Supply an explicit valid answer for that ID to acknowledge its refusal (for the split
bottom, `"datum.A":[4,8]`). This overrides its unresolved entry; future diff output removes
the resolved entry. Unresolved entries survive repeated diffs until acknowledged. If a
question has disappeared entirely, review its removal and explicitly remove its
`unresolved` entry. Removing all metadata discards these protections.

WASM `rematchAnswers` also reads embedded evidence when no separate evidence argument is
given. For maps with `_specify`, it returns fresh evidence and carry blocks for its
unresolved results, so saving the returned answers preserves the protection.

Both metadata fields are optional so callers may keep a carry block without evidence,
or attach WASM `evidence()` to a manually managed map. Do not fabricate evidence or copy
it from a different part. Assembly maps keep metadata inside each part's answer map.
