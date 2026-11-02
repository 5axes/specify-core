# Shared text-to-cad PMI plate

- Source: <https://github.com/amywork777/text-to-cad-upstream>
- Commit: `d18aadfe97869ede0e7f48ee0830e040449cc80d` (`amy/pmi`,
  text-to-cad PR <https://github.com/earthtojake/text-to-cad/pull/692>).
- Model: `models/examples/src/pmi_plate.py`.
- Exporter: that checkout's `packages/cadgen`, **cadgen 0.8.3** (editable install),
  build123d **0.13.0**, cadquery-ocp-novtk **8.0.1.1.0**, Python **3.13.15**.
- Generated: **2026-11-02**. Millimetres throughout. The source MIT `LICENSE`
  is copied verbatim beside these files (Copyright Thompson Labs LLC).

## Construction and exact edits

Each revision is built cold from a script named `pmi_plate.py`, with the original
function `pmi_plate`, local variable `body`, and `body.label = "plate"`.
All `annotate(...)` PMI, face selections, notes, views and the visual material
assignment/imports are removed; `return body` replaces `return annotate(...)`.
Only `build123d as bd` and `cadgen.step` imports remain (plus future annotations).
The decorator's output changes from `../STEP/pmi_plate.step` to `pmi_plate.step`.
Specify, rather than cadgen, supplies the PMI and material in the test.

| Revision | Exact construction expression assigned to `body` |
| --- | --- |
| rev1 | `bd.Box(100, 60, 10) - bd.Cylinder(5, 10)` |
| rev2 | `bd.Box(100, 60, 12) - bd.Cylinder(6, 12)` |
| rev3 | `bd.Box(100, 60, 10) - bd.Cylinder(5, 10) - bd.Pos(30, 15, 0) * bd.Cylinder(3, 10)` |

Thus rev2 increases thickness 10 → 12 and centre bore Ø10 → Ø12. Rev3 is
**rev1 plus** a Ø6 through-hole centred at (30, 15), not rev2 plus a hole.
Box and cylinders use build123d's default centred alignment: rev1/rev3 span
z = −5…5, rev2 z = −6…6, x = −50…50 and y = −30…30.
The expressions are edited source, not a parameterised model or synthetic names.

## Export route and regeneration

`generate.py` is the short scratch generator retained for regeneration, **not a
test dependency**. From a checkout of the pinned text-to-cad commit:

```sh
uv venv --python 3.13 .venv
uv pip install --python .venv/bin/python --editable ./packages/cadgen
.venv/bin/python /path/to/generate.py /path/to/output-directory
```

This follows the checkout's editable-package setup, installing cadgen's declared
runtime dependencies without the unrelated repository test/browser dependencies.
The generator isolates state/cache, disables telemetry, and uses
`cadgen.engine.api.build_script(..., daemon=False)` to build each decorated model
in a fresh Python process. This is cadgen's own **`@step` export** through
`cadgen.engine.export.step.write` / `write_document`, `STEPCAFControl_Writer`,
`SetNameMode(True)` and **`write.stepcaf.subshapes.name = 1`**. No direct
build123d exporter, name assignment, STEP rewriting, or XBF fallback is used.
Cadgen's folded-name property tables remain in the STEP, spelling out hash atoms.
Only gzip compression (level 9, mtime 0) is applied afterwards.

| File | STEP bytes | Gzip bytes | Faces |
| --- | ---: | ---: | ---: |
| rev1.step.gz | 21,741 | 4,581 | 7 |
| rev2.step.gz | 21,741 | 4,582 | 7 |
| rev3.step.gz | 26,436 | 5,459 | 8 |

## Authored identity changes, rev2 → rev3

Names are identical between rev1 and rev2. Rev3 renames three existing physical
faces when the second boolean cut is added:

| Face | Old name | New name |
| --- | --- | --- |
| Top (+Z) | `f:pmi_plate.body_make_box1.6;M:pmi_plate.body` | `f:pmi_plate.body~3969c36f3543` |
| Bottom (−Z) | `f:pmi_plate.body_make_box1.5;M:pmi_plate.body` | `f:pmi_plate.body~620ac07d5aac` |
| Centre bore wall | `f:pmi_plate.body_make_cylinder1.1;M:pmi_plate.body` | `f:pmi_plate.body_cut1~7202e23a5382` |

The four side faces retain `f:pmi_plate.body_make_box1.1`, `.2`, `.3`, `.4`.
The added bore has `f:pmi_plate.body_make_cylinder2.1;M:pmi_plate.body`.
The Rust regression freezes these real names and refuses missing-name bindings;
its expected refusal list should change when cadgen preserves the names.
