# Comparing revisions: `specify-core diff`

```bash
specify-core diff old.step new.step [--answers old.answers.json] [--title TITLE] [--part N] [--pmi-scenes]
                  [--viewer URL] -o DIR
```

`diff` compares two revisions of a part, or of an assembly part by part (see
[Assemblies](#assemblies)). It reports what changed in words a person can check
against the model, draws the change, and carries the part's answers to the new revision.
The [STEP diff action](../step-diff/README.md) runs it on pull requests.

## What it writes

| File | What it holds |
| --- | --- |
| `report.md` | The review in Markdown, headed `TITLE` (by default the new file's name), written to be posted as a pull request comment. |
| `model.png` | The new part, drawn isometrically. |
| `answers.json` | The answers carried to the new revision, to commit as its answers. |
| `report.json` | All of it as data: the verdict, summary, carried answers, changed features, measures, PMI changes, unresolved answers, answers to check and new questions. |

It also prints `{"verdict", "summary"}` to stdout.

## How the parts are matched

quiddity's correspondence matches the two parts face by face. It finds the motion that takes
the old part onto the new one, so a part moved or rotated as a whole still matches. Each feature of
the old part (a hole, a pattern, a boss, a slot, a fillet) is then:

- **unchanged:** the same feature, perhaps with renumbered faces;
- **adapted:** the same feature, resized or moved;
- **orphaned:** gone;
- **ambiguous:** it matches several features that look alike, such as a symmetric
  repeat, or several features have become one;
- **new:** in the new part only.

## What the report says

- **The verdict:**
  - ✅ `ok`: every answer carried and the new part asks nothing new.
  - ⚠️ `decide`: an answer couldn't be carried, or the new part asks a question the old one
    didn't.
  - ❌ `problems`: Specify's review finds problems in the new part that it didn't find in the
    old one.
- **The summary** counts the changes by family ("2 holes changed, 8 rounds and surfaces with
  them"). Fillets, blends, chamfers and freeform surfaces that change along with another
  feature are counted after it.
- **The measures** are the volume, surface area, bounding box and face count, before and after.
- **What changed** groups the features that changed in the same way. Each one is named as a
  drawing names it, with its size ("2× Ø5 × 30 through holes", "R1 fillet", "10 thick plate").
  Then it's placed so a person can find it:
  - on a side of the part ("on the −Y side"), or through it along an axis;
  - in a host feature ("in the Ø70 boss around the Ø45 hole"), with offsets from that
    feature's axis;
  - or, otherwise, from a nearby larger hole's axis, or from a corner of the part ("from the −X,
    −Y corner").

  Then it says what changed: a size ("depth 11 → 13; diameter 10 → 9"), a move along the
  part's axes ("moved Z +2"), "reshaped" for a freeform surface, or "removed". A move along a
  feature's own axis isn't reported when its depth or height changed, since that move is the
  change in length. A move component of a tenth of the largest one or less is left out.
- **Ambiguous features** say what happened:
  - "are now one": several features merged into one;
  - "is now split in N": one feature split into several;
  - "still there, but alike…": the features remain, but which is which can't be told.
- **Answers not carried** keep their value and give the reason:
  - **orphaned:** the feature was removed;
  - **ambiguous:** it now matches several features;
  - **unmatched:** its faces carried, but no single feature of its family holds them;
  - **stale:** the old part had no such feature either.
- **Answers to check** were carried to a feature that changed. They're applied, but a fit or a
  clearance chosen for the old size may be wrong for the new one.
- **New questions** are the questions the new part asks that the old one didn't.
- **PMI** compares requirements from both revisions as `haecceity.pmi/1`, using
  the same ids, characteristic numbers and `answered` / `default` / `general` /
  `existing` statuses as `pmiScene`. This includes CAD PMI in Specify-written files,
  general-tolerance dimensions and geometry-derived basic locations.
  A new file without saved answers shows what writing the carried answers would
  produce alongside its own PMI; a new file with saved answers supplies its actual
  requirements. The two carried-answer plans are also compared for geometry-derived
  requirements; actual file edits take precedence, and planned changes absent from
  that comparison carry a reason. Explicit `--answers` override the old interview.

### PMI requirements

Items match by characteristic number, then stable requirement id (or haecceity id
for foreign PMI), then corresponding faces, with kind resolving multiple items on
one support, then a unique remaining item of the same kind. Numbered items with
different numbers are kept distinct at the weaker matching stages. A split can map
one old face to several new faces. Ambiguous item matches stay added/removed rather than choosing arbitrarily.

Comparison uses haecceity's full semantic notation and frame cells, including
material and datum modifiers, diameter zones, fits, qualifiers and note text.
Semantic fields are compared too, including the datum system, basic flag and fit
class even when equal stated limits hide the class in the notation. Topology and
file-local support/unit identities are compared through their correspondence or
resolved meaning rather than their JSON ids. A semantic-only change carries a
`reason` even when its displayed text stays equal. Notes use their own semantic
wording (`pmiScene.specify[id].notation`), not the shared GENERAL NOTES graphic;
changing one note does not change the other requirements in that block.

`report.json` adds `pmi.items`, an ordered list of changes:

```json
{"class":"changed","number":7,"id":"size:holes:9","kind":"dimension",
 "faces":{"old":[{"face":9}],"new":[{"face":9}]},
 "status":"answered","on":"⌀6.6 holes","old":"⌀6.6 H7","new":"⌀6.6 H8"}
```

Each item has `class` (`added`, `removed`, `changed`, or `renumbered`), optional
`number`, `id`, `kind`, `status`, `on` (feature words), and optional `old`, `new`
and `reason`. A numbered removal has `reason: "retired"`; numbers are never
reused. Renumbering is reported if detected, though ordinary editing preserves
numbers. Value and material-modifier edits keep the number and produce one `changed`
item per edited requirement, including direct STEP edits (For #95). Changes sort by
characteristic number, then unnumbered id. Markdown prints one line per change, for example:

```text
#7 ⌀6.6 H7 → ⌀6.6 H8 (4× ⌀6.6 holes)
#12 added: ⌖ ⌀0.1 Ⓜ A B C (hole pattern)
#5 retired: 50 (hole moved)
```

Paired items (`changed` and `renumbered`) include `old_id` **only when it differs
from `id`**. Use `old_id ?? id` for the old callout; removed items already use the
old `id`, and added items have no old identity. Every item includes
`faces: {"old": [{"face": meshIndex}], "new": [{"face": meshIndex}]}`. An absent
side or a part-level requirement has an empty array. These are sorted, unique
mesh face indices in each side's local frame, the same anchors as scene links;
they include semantic support even when no presentation graphic exists. Keeping
anchors on the item avoids requiring large scenes or making focus depend on ink.
Datum-system references do not add the datum's faces to the controlled feature.

Opt in to `report.pmi.scenes: {"old": PmiScene, "new": PmiScene}` using:

- Rust: `revision::diff_with_options(old, new, answers, DiffOptions { scenes: true })`
  (also `revision::assembly::diff_with_options`). `revision::diff` keeps its default.
- CLI: `specify-core diff old.step new.step -o result --pmi-scenes`.
- WASM: `diff(oldName, oldBytes, newName, newBytes, answersJson, title, true)`.
- Python: `api.diff(old, new, answers, scenes=True)`.

Both documents have the `pmiScene` shape (`pmi`, `presentation`, `specify`). They
are the exact primary scenes already built by the comparison: old with the given
or stored answers, new with carried answers unless the new file has Specify PMI,
when its actual semantic requirements win. Comparison presentations use current
semantic notation, not potentially stale saved As Drawn ink. Supplemental
carried-plan differences retain their existing `reason`; the new scene remains
the actual destination. No additional scene derivation is performed. Assembly
reports include the pair in each matched part's `report.pmi.scenes`.

Scenes are omitted by default because they can dominate report size. Opting in
only adds `pmi.scenes`; removing that property gives byte-identical serialized
report JSON to the default call. The other additive item fields above are always
available. The renderer retains only the changed presentation geometry from the
same scene pair even when JSON scenes are omitted.

`model.png` draws changed/renumbered callouts orange, added callouts green and
removed old callouts as translucent red ghosts (45% opacity), transformed by the
same alignment as old geometry. Unchanged callouts are omitted. Haecceity's
polylines and triangles are used as supplied, including leaders and stroked text;
there is no text layout or replacement of missing glyph geometry. Shared graphic
aliases (such as GENERAL NOTES) draw once per side; an added alias makes the
shared block green. Geometry keeps its existing change colours; a PMI-only part
is grey. The camera bounds include every selected stroke and triangle, on both
sides. Callouts overlay the part so their leaders remain visible.

`pmi.stored`, `pmi.existing` and `pmi.existing_count` retain their legacy meanings
for one release; **`stored` and `existing` are deprecated**. They omit requirements
and notation details covered by `items`; new clients should use `items`.

WASM `diff` returns these items in `report.pmi.items` (and in each assembly part's
report), with ids and numbers suitable for highlighting `pmiScene` characteristics
in `/diff`. The Python package exposes `specify_core.api.diff(old, new, answers=None,
*, title="Revision", part=None, scenes=False)`, returning `{report, markdown}` for the selected
part. It accepts paths or loaded part handles; paths accept a part index.

## How answers carry

Each answer's faces are mapped to the new part, and the answer moves to the new feature of the
same family that holds them. Its id is renamed when the faces renumbered. An answer is never
guessed onto a feature: when there's no single place for it, it's left unresolved with its
value and reason, for a person to place.

The answers carried are, in order of preference:

1. those given with `--answers`;
2. without `--answers`, or with an empty file, those Specify stored in the old STEP file when it
   wrote its PMI;
3. none, in which case the report still says what changed.

## The picture

`model.png` is the new part, seen isometrically with Z up. Changed faces are orange, new faces
green, faces that can't be told apart purple, and the rest grey. The old part's faces that are
gone or moved are drawn over it in translucent red, placed by the matching motion.

## Assemblies

A file of several parts, or of a part placed more than once, is compared as an assembly:

- **Parts are paired by name**, the product name in the STEP file (its product id when it has
  no name, else `part N`). Parts of the same name pair in file order, the second shown as
  `name (2)`. A renamed part is one removed and another added.
- **Each part in both files is compared on its own**, as a single part is, with the rest of the
  file consulted for its mates. Its status is `changed` when its comparison finds anything,
  else `quantity` when it is placed a different number of times, `moved` when an old placement
  has no new one at it, else `unchanged`. A part whose geometry turned or moved in its own
  frame is placed by its placement and that move together, so a plate turned end for end is
  `moved`; a move that lays the part on itself (a screw turned about its axis) is not counted.
  Parts in one file only are `added` or `removed`.
- **The verdict** is the worst of the changed parts'. The summary counts each status and names
  the parts when there are 5 or fewer: `2 parts changed (GRM-02 shaft, GRM-04 drive plate), 10
  unchanged.`
- **`report.md`** has the summary, the picture, a table of the parts that differ, and a section
  `#### name` for each changed part: that part's own report. With `--viewer URL` (the
  [/diff page](https://draftwright.io/diff)'s link for the two revisions), each section links to
  `URL&part=name`.
- **`model.png`** is the whole assembly, every part where it is placed: changed, re-counted and
  moved parts orange, added parts green, the rest grey, and in translucent red, where no part
  now lies over them, the removed parts and the placements that went (a moved part's old place,
  a re-counted part's missing copy).
- **Answers are by part name**: `--answers` takes `{part name: {question id: value}}`, and
  `answers.json` holds the answers carried, by part. A part with no entry carries the answers
  Specify stored in it. When the old file is a single part, `{question id: value}` is taken as
  that part's answers.
- **`report.json`** is `{verdict, summary, parts}`, each part `{name, status, old, new}` (which
  part of each file and how many times it is placed) and its `report` when it is in both.

`--part N` compares only part N of each file, as before: a single part's report. Both files must
have that part.
