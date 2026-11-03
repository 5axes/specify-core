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

## Specify metadata

Specify's diff answer maps (`answers.json`, `report.answers`, and each assembly part's
map) add a reserved `_specify` member. Preserve it when editing or storing answers:

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
write requires the saved count and ordered face fingerprint to match, without remapping
by name. This deliberately refuses changed numbering even if names could be rematched.
Malformed evidence is an error. A new diff checks supplied evidence against its old part
before carrying, so it cannot silently reinterpret stale indices either.

The shared writer checks evidence against the loaded part before writing bytes. CLI
also checks before saving `--intent`; stale evidence exits 2, names the answers path,
and directs the caller to `specify-core diff OLD NEW --answers ANSWERS -o CHANGES`.
An unchanged part's evidence remains valid even if its file bytes change without changing
faces. Existing in-STEP answer payloads retain their format and evidence; transport
metadata is consumed before answers are embedded.

`unresolved` maps each dropped question ID to diff's exact reason, including orphaned,
ambiguous and unmatched answers. A dropped answer contributes no PMI and cannot be
replaced by bulk default acceptance. CLI fails with
`incomplete: datum.A (carried answer could not be applied: <reason>)`, writes no STEP or
intent output, and preserves any existing output. Serve `accept_defaults`, Python
`apply(..., accept_defaults=True)` and WASM `acceptDefaults` use the same block. Review
can still show partial answers; the editor's automatic defaults skip blocked IDs.
The writer also refuses unresolved metadata if called directly with a preview intent.

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
