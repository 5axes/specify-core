# specify-core

**Add manufacturing intent to a STEP part, and write it as AP242 semantic PMI.**

specify-core reads a STEP part, recognises its features (holes with their counterbores and
threads, hole patterns, slots, bosses, fillets and turned bodies) and works out what a
manufacturer still needs to know: the material, datums, fits, thread classes, hole positions, surface
finish and the general tolerance. It asks a question for each gap, with a suggested default
and the reason for it. Your answers are written back into the STEP as AP242 semantic PMI
(GD&T that CAD and inspection tools can read), and the file is read back to verify it.

It is deterministic: rules and standards tables, no AI. The answers are stored in the file
too, so a later revision of the part can be opened and carried on with.

- No dependencies: no OpenCascade, build123d or quiddity. The engine is Rust, in one wheel.
- Wheels for Linux (x86_64, aarch64), macOS (Apple silicon, Intel) and Windows, for
  CPython 3.12 and later.
- Apache-2.0.

It is the engine behind [Draftwright Specify](https://draftwright.io/specify), which adds
a 3D review of the same questions in the browser.

## Install

```bash
pip install specify-core
```

## Python

```python
from specify_core import api

analysis = api.analyse("plate.step")         # features, faces and any existing PMI

for q in api.questions(analysis, {}):         # what is still unknown
    print(q.id, "|", q.prompt, "| default:", q.default)

answers = {"part.material": "6082-T6", "hole.function:holes:9": "fit:H7"}
intent = api.apply(analysis, answers, accept_defaults=True)   # suggested defaults for the rest
report = api.write("plate.step", intent, "plate-pmi.step", answers=answers)

print(report.written)       # ['datum A', 'Ø8 H7 on holes:9', 'position 0.05 on holes:9', ...]
print(report.not_written)   # anything that could not be written, with the reason,
                            # including an answer whose question is not asked
```

Write reports optionally include `geometry_carrier_exemptions` (`{part, condition,
aspect, feature}`), `preserved_normalizations` (`{id, normalization}`),
`presentation_warnings` (strings) and `retained_shared` (ids of support Replace kept
for PMI it cannot read). `to_dict()` omits empty lists. The v0.4.7 kernel
preserves original entity text during Add, including when adding a separate datum
feature on a kept carrier. Add upgrades requiring source repairs raise a clear
`RuntimeError` naming the entities and repair; Specify never retries with Replace.

Opening the written file again picks up where you left off:
`api.analyse("plate-pmi.step")["resume"]["answers"]` returns the answers stored in it.

The main functions, all in `specify_core.api`:

| Function | What it does |
|---|---|
| `parts(step)` | Lists an assembly's parts. |
| `load(path, part=None)` | Reads a file once; pass the returned `LoadedPart` to the others instead of a path. |
| `analyse(step, part=None)` | Recognises features and reads existing PMI. Returns the analysis (a dict). |
| `questions(analysis, answers)` | The questions, given the answers so far (`Question` objects). |
| `apply(analysis, answers, accept_defaults=False)` | Turns answers into the intent to write. Raises `IncompleteError` naming what is still missing, unless defaults are accepted. |
| `preview(analysis, answers)` | The intent the answers so far would give, without requiring every answer. |
| `write(step, intent, output, answers=None)` | Writes the PMI (and the answers) and verifies the file. Returns a `WriteReport`. |
| `write_parts(step, intents, output)` | The same for several parts of an assembly, in one file. |
| `mesh(step, part=None)` | Triangles grouped by face index, for showing the part and picking faces. |

Answers are a dict of question id to value, e.g. `{"part.material": "6082-T6"}`. Requirements
no question asked for (a flatness, a fit, a finish on faces you choose) are added as `extra.*`
answers.

## Command line

```bash
specify-core analyse plate.step -o plate.analysis.json
specify-core questions plate.analysis.json --open                # unanswered questions, as JSON
echo '{"part.material": "6082-T6"}' > answers.json
specify-core write plate.step plate.analysis.json answers.json --accept-defaults -o plate-pmi.step
```

`specify-core parts` lists an assembly's parts and `--part` picks one; `write` takes an
analysis and answers pair for each part to write together. `specify-core mesh` writes the
face-indexed triangles.

## For a host program

`specify-core serve` answers JSON requests, one per line, on stdin and stdout. Each request
carries everything it needs, so a restarted process loses nothing. This is how an application
drives it as a child process:

```json
{"id": 1, "op": "analyse", "step": "/path/part.step"}
{"id": 2, "op": "questions", "analysis": {...}, "answers": {...}}
{"id": 3, "op": "write", "step": "/path/part.step", "output": "/path/out.step", "analysis": {...}, "answers": {...}, "accept_defaults": true}
```

Each reply is `{"id", "ok": true, "result"}` or `{"id", "ok": false, "error": {"code", "message"}}`,
where the code is `incomplete` (with what is `missing`), `invalid` or `failed`. `analyse` replies
with the analysis and the mesh; `write` with the report and the intent.
`specify_core.serve.reply(request)` answers the same requests in-process.

## What it writes

AP242 semantic PMI: datums and datum systems, size dimensions with ± and fit tolerances,
location dimensions, position, flatness, perpendicularity, parallelism, profile and runout
tolerances, threads, knurls, surface texture, material, notes and the general tolerance. Every
file is read back and checked before it is kept.

## Links

- Source and issues: [github.com/pzfreo/specify-core](https://github.com/pzfreo/specify-core)
- Changelog: [CHANGELOG.md](https://github.com/pzfreo/specify-core/blob/main/CHANGELOG.md)
- Building and testing this package: [docs/python.md](https://github.com/pzfreo/specify-core/blob/main/docs/python.md#the-python-package)
- Try it in the browser: [draftwright.io/specify](https://draftwright.io/specify)

Status: early (0.x). The JSON formats may change between minor versions; changes are listed in
the changelog. Draftwright® is a registered trade mark of Betwixt Ltd.

Complete answer sidecars (defaults explicit, with geometry evidence) can be exported
before writing: `api.export_answers(step, analysis, answers)` returns a dict.
Incomplete sets and unresolved carries are refused. See
[answer files](../../docs/answers.md#export-a-complete-sidecar).
