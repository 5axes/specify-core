# NIST AP242 test parts

The AP242 versions (geometry with semantic PMI representation and/or presentation, ASME
variant 1) of five Combined Test Cases and one Fully-toleranced Test Case from the
[NIST MBE PMI Validation and Conformance Testing Project](https://www.nist.gov/el/systems-integration-division-73400/mbe-pmi-validation-and-conformance-testing-project).
They are works of the U.S. federal government and are not subject to copyright in the United
States (public domain). They are kept here gzipped (`gzip -n -9`), otherwise unmodified, so the
oracle and the tests need no network fetch.

Neither engine reads gzip: the oracle capture tool (`tools/capture.py`) and the Rust harness
(`tests/common`, `Case::input`) decompress each file to a temporary `.stp` first, and the
analysis binding's `source_sha256` is that of the decompressed bytes below.

| File | Copied from | Bytes | sha256 of the uncompressed file |
|---|---|---|---|
| `nist_ctc_01_asme1_ap242.stp.gz` | `dw-examples/steps/nist/` (dw-examples f1196bf) | 396445 | `85a5752da05f53c456ca3a9e038c90358e1d5a3141d1f0d6e5f0970f2356e821` |
| `nist_ctc_02_asme1_ap242.stp.gz` | `dw-examples/steps/nist/` (dw-examples f1196bf) | 1981342 | `99a0a2079ddeb64d05c2432cbe931fa17c89da60d8b0bcb0b11bd0ed94fa2e68` |
| `nist_ctc_03_asme1_ap242.stp.gz` | `dw-examples/steps/nist/` (dw-examples f1196bf) | 673846 | `196b665776e759282f80fc8fb27d7bceb995df77cf78b7ce48347535a4d6cb5f` |
| `nist_ctc_04_asme1_ap242.stp.gz` | `dw-examples/steps/nist/` (dw-examples f1196bf) | 1287291 | `20b43b54ce25d4ed17cff794084c406e831c687f5b62471d3371eef33669e355` |
| `nist_ctc_05_asme1_ap242.stp.gz` | `dw-examples/steps/nist/` (dw-examples f1196bf) | 878030 | `59bbc09a34621c03106e4c1b2a5bc909fdb67463117c16c4965ee7a6fe5c1521` |
| `nist_ftc_09_asme1_ap242-e1.stp.gz` | `draftwright/tests/fixtures/` (draftwright c76c0c52) | 6109836 | `f1215fe15a78085a9fa78dd81714caf774b59071be92580b04ebfdc19f52a1bd` |

`solid-edge-ctc01.step.gz` is the Solid Edge 2026 re-export of NIST CTC-01 from
quiddity-rust v0.4.5 (`845a8919`), `tests/fixtures/ap242/reexport/09-NIST-reexported-by-SE.stp.gz`.
It loses datum B's establishing feature. Reader /19 (as /17 and /15 did) reports B as Unresolved and
omits that datum, retaining A and C. This tests Specify's existing-PMI and CLI
finding output, rather than inventing a valid support for the lost datum.

## v0.4.6 Add upgrade audit

These exact AP203 source repairs now refuse a direct **kernel Add**. Each input
has no foreign PMI, so Specify selects Replace before writing and still succeeds;
there is no fallback after refusal and no Specify oracle expectation to change.
`tests/add_mode_upgrades.rs` checks both paths, exact ids, and the material readback.

| Corpus file under `nist/` | Required repair / original ids |
|---|---|
| `nist_ctc_01_asme1_rd.stp` | R1 approval assignment: 14, 15 |
| `nist_ctc_02_asme1_rc.stp` | R2 conversion-unit dimensions: 20750 |
| `nist_ctc_03_asme1_rc.stp` | R1 approval assignment: 14, 15 |
| `nist_ctc_05_asme1_rd.stp` | R2 conversion-unit dimensions: 6596, 6599, 6625, 6634, 6651, 6660, 6669 |
| `nist_ftc_06_asme1_rd.stp` | R1 approval assignment: 14, 15 |
| `nist_ftc_09_asme1_rd.stp` | R2 conversion-unit dimensions: 5778, 5781, 5805, 5814, 5827, 5836 |

The AP214 `PRODUCT_TYPE` case is also repaired by Replace when PMI-less. A
synthetic foreign-PMI AP214 regression in `src/writer/tests.rs` verifies that an
Add refusal naming its keyword rewrite reaches Specify without a Replace retry.

With v0.4.7 the same six inputs need exactly the same repairs; R3 (INTEGER items
written as integral REALs) is required by none of them, and the test's exact
repair lists are unchanged. Reader /17 reports CTC-04's projected zone #18897,
whose projection end #18894 has no geometry, as a `fact` finding (it was a
nonconformance); CTC-01's dimensions with no stated value (#24, #25, #126, #127)
no longer produce a nonconformance finding.

With v0.4.8 (reader /19) the six inputs still need exactly these repairs, and the
existing-PMI and CLI finding expectations for CTC-01 to CTC-05 and the Solid Edge
re-export are unchanged.
