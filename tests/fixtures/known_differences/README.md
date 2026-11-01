# Known differences from the Python oracle

The Rust port is checked against specify-core's own outputs (`tests/fixtures/oracle`, captured
by `tools/capture.py`). Python is the oracle, not the specification: where Rust differs, the
difference is recorded here with a verdict and the evidence for it, and a difference with no
entry fails the test that found it.

## Files

One file per area, `<area>.json`, a JSON array of entries:

```json
[
  {
    "case": "corpus/*",
    "path": "features[*].faces",
    "pattern": "a hole's faces are its defining faces, without its floor",
    "verdict": "rust-correct",
    "evidence": "what was checked: the geometry, the standard, the file text, a test"
  }
]
```

- `case` and `path` are globs over the difference's case name (`parts/plate`,
  `parts/assembly#1`, `corpus/mfcadpp/1000`) and path (`faces[3].area`, `binding.part`,
  `features[id=holes:3].faces[0]` when an array is aligned by a key; the root is the empty
  path). `*` matches any run of characters, dots and brackets included; every other character
  matches itself.
- `pattern` names what the difference is, in a few words.
- `verdict` is one of `rust-correct`, `rust-wrong`, `equivalent`, `undetermined`,
  `not-applicable`, `resolved`. A `rust-wrong` entry is a bug kept visible, not a pass.
- `evidence` says what was checked to reach the verdict.

Every field is a non-empty string and no other field is allowed (`tests/common`'s `load_known`
checks it).

## The rule

- **Each area's full oracle sweep is one test function that calls `check_known(area, &diffs)`
  once, for its own area.** It fails listing every difference no entry matches, and every entry
  that matched no difference (stale: the difference was fixed, or the entry is too narrow), and
  otherwise prints a tally of the differences by verdict. A difference is matched by the first
  entry whose `case` and `path` globs both match it.
- **Other areas' files are consulted only through `explained(&[areas], &diffs)`**, which returns
  the differences none of their entries match and asserts nothing (no stale check, no tally).
  A sweep that crosses areas (the CLI's, which runs features, rules and the writer together)
  filters through the earlier areas' files with it, because their `check_known` there would
  report as stale every entry that sweep does not meet.
- **Each area file is owned by one stream** (`analysis`, `features`, `existing`, `rules`,
  `mates`, `mesh`, `anchors`, `writer`, `cli`; see `docs/design.md`, "Comparison harness and known
  differences"): only that stream edits it.
- An entry is as narrow as the evidence: a pattern seen on every corpus case may be `corpus/*`;
  one checked on one file names that file.

The `analysis` area is owned by `tests/api.rs::analyses_have_python_layout`. It records
exact per-part additive fields; other API tests consult it through `explained`. Legacy
key order and values remain exact assertions, and missing additions make entries stale.

## Corrected historical differences

Keep an entry that a kernel correction removes with verdict `resolved`, the original
case/path and historical evidence, followed by the correcting quiddity PR and evidence.
This is an absence assertion: the area's sweep fails if **any** difference matches that
path, even if another active entry would cover it. Resolved entries never explain
current departures and are excluded from the stale-entry rule. `tests/oracle_harness.rs`
checks both properties. This retains history without exempting current mismatches or
deleting fixed entries to make the sweep pass.
