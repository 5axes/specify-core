# cadgen PMI plate with semantic PMI (gist)

- Source: Amy's gist <https://gist.github.com/amywork777/dd147f8a3f0a25b66cdcee4d485e8ff5>,
  revision `0e32421fb33bc9b97cc32b6da1c0998b02fcf1c8` (2026-11-02 17:13 UTC), "cadgen sample
  plate with semantic PMI and an ISO 2768-mK general tolerance (AP242 STEP + source)".
- Files, copied byte for byte from that revision:
  - `pmi_plate.step`, stored gzipped (`gzip -9n`) as `pmi_plate.step.gz`; the decompressed
    SHA-256 is `16d35aa3a755e0ccfcbc657e8a694f146b7dd9e635909c3e8ec75dd77768e2ef`
    (36,712 bytes, 7 faces).
  - `pmi_plate.py`, the cadgen model source that exported it; SHA-256
    `2226c6c08af4e2c36c3a5c4764451c563116845b993f6b70c67fb42b66c3d046`. It is kept for
    reference only and is not run by the tests.
- Licence: the gist states none. Its author confirmed to Paul on 2026-11-02 that it is
  MIT. The MIT text is in `LICENSE` beside these files.

Unlike `../pmi_plate/` (rev1–rev3, geometry only, with Specify supplying the PMI),
this file carries cadgen's own AP242 semantic PMI:

- datums A (bottom), B and C;
- thickness 10 ±0.2 and ⌀10 +0.1/0;
- basic 30 and 50 locating the hole from B and C (`'dimensional note','theoretical'`);
- flatness 0.05, position ⌀0.1 A|B|C, profiles on B and C, and a general profile 0.5 A|B|C;
- material `Alu_G6061_T6` and general tolerance `ISO 2768-mK`.

The source also declares notes ("edges", and a flag note on the hole) and views A-A and
iso; those are not in the STEP. Each callout's presentation is one leader line with no text.

`tests/text_to_cad_plate.rs` reads it as foreign PMI. The general tolerance reads once as
ISO 2768-mK, the 30 and 50 read as basic, and the intent adds no Specify size or location
requirement beside the file's own dimensions.
