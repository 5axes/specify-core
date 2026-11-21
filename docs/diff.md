# Comparing revisions: `specify-core diff`

```bash
specify-core diff old.step new.step [--title TITLE] [--part N] [--pmi-scenes] [--viewer URL] -o DIR
```

`diff` compares two revisions of a part, or of an assembly part by part (see
[Assemblies](#assemblies)). It reports what changed in words a person can check
against the model, draws the change, and lists the requirements added, removed and changed
between the two files, per feature, with their characteristic numbers. It is how a requirement
change is reviewed in a pull request: between the old and new annotated files.
The [STEP diff action](../step-diff/README.md) runs it on pull requests.

**It reads only the PMI in the two files** ([ADR P11](https://github.com/pzfreo/draftwright-project/blob/main/adr/P11-the-file-is-the-only-copy.md)):
each file's requirements are read as any file's are, whoever wrote them. Answers a file
stores are never read to compare, so the same requirements pair the same way with or without
them (only the items' labels, each file's `pmiScene` ids and statuses, follow the editor);
`diff` takes no answers and writes none. (Carrying requirements to a new revision is not `diff`'s.)

## What it writes

| File | What it holds |
| --- | --- |
| `report.md` | The review in Markdown, headed `TITLE` (by default the new file's name), written to be posted as a pull request comment. |
| `model.png` | The new part, drawn isometrically. |
| `report.json` | All of it as data: the verdict, summary, changed features, measures, PMI changes and requirements to check. |

It also prints `{"verdict", "summary"}` to stdout. The summary names each feature and its
requirement notation, for example "1 requirement needs checking: Ø6 × 10 through hole moved
X −15, so its ⌀6 requirement may no longer fit."

The STEP diff action includes the prose report, never a JSON payload.

## How the parts are matched

quiddity's correspondence matches the two parts face by face. It finds the motion that takes
the old part onto the new one, so a part moved or rotated as a whole still matches. Each feature of
the old part (a hole, a pattern, a boss, a slot, a fillet) is then:

- **unchanged:** the same feature, perhaps with renumbered faces;
- **adapted:** the same feature, resized or moved;
- **orphaned:** gone;
- **ambiguous:** after the motion, it is as near several features that look alike (no motion
  was found to tell them apart, or they changed together), or several features have become one;
- **new:** in the new part only.

**A symmetric part is matched in the file's frame.** When the old part fits the new one as well
with another rigid motion as with the one taken (a plate symmetric about its centre, which fits
turned 180° as well as it lies), the motion nearest the identity is taken: no motion at all
when the part fits as it stands. The file's frame is evidence, since CAD exports seldom turn a
part, so the tie is not asked; requirements then follow the faces in that frame. The report
says so, visibly: "Matched in the file's frame; a 180° turn about Z fits equally." (or "Matched
by the motion nearest the file's frame (turned 90° about Z); …" when the part moved too), and
`report.json` adds `frame_tie`: `{motion, alternative, note}`, the motion taken (`null` for
none), the other in words (`null` when no quarter or half turn about the part's centre names
it: "another rigid motion") and the sentence. The verdict is unchanged by it: a tie alone is
`ok`. A turned part's free turn about its own axis is no tie and is not reported. Features that
remain alike after the motion is chosen are still `ambiguous`, as above.

## What the report says

- **The verdict:**
  - ✅ `ok`: no requirement needs checking.
  - ⚠️ `decide`: a requirement the new file states is on a feature that moved or changed size
    (see "Requirements to check").
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
  A feature has moved only when its faces have: a hole cut into a wall, or a pattern's holes
  resized, shifts the centroid of the body or pattern, but its faces stay where they were, so
  it is not reported (the cut hole and the resized holes are). Where no alignment was found,
  an unchanged feature whose faces are displaced in the parts' own frames is reported moved, its offset
  including any rigid motion of the part, as for faces below.
- **Other faces** lists the changes to faces that no feature above explains: a block face or
  a sharp-cornered pocket wall pushed 5 mm, say, which no hole, fillet or other recognised
  feature holds. Each is a face the matching found changed (`adapted`), gone (`orphaned`),
  new, or can't place (`ambiguous`), with what is needed to see the change: a plane's normal
  and its signed move along it ("moved 5 mm outward"), a cylinder's radius and axis move, and
  the area before and after.
  - Where the matching found how the part moved between the revisions, positions are compared
    where the part is now, so a part that moved as a whole does not read as every face moved.
  - Where it found none (a part with too few features to anchor it), each face is compared in
    the parts' own frames and a displaced one is `moved`; its offset then includes any rigid
    motion of the part.
  - A face a feature above covers is not listed again. Nor is the context of one: a face whose
    area changed (and nothing else) beside a face a feature holds, or a new or removed face all
    of whose neighbours a feature holds (a counterbore's floor). `context_faces` counts them,
    so the listed faces, `unchanged_faces`, `context_faces` and the faces the features hold
    make up the part's faces.
  - Faces that moved or changed shape come first. In the picture they are orange (their old
    place red), as feature faces are.
  - In `report.json` these are `other_faces` (`class`, `old`, `new`, `kind`, `normal`, `offset`,
    `tilt`, `radius`, `axis_move`, `area`, `at`), `unchanged_faces` and `context_faces`.
- **Ambiguous features** say what happened:
  - "are now one": several features merged into one;
  - "is now split in N": one feature split into several;
  - "still there, but alike…": the features remain, but which is which can't be told.
- **Requirements to check** are the new file's requirements on a feature that moved or changed
  size: a fit or a clearance chosen for the old size may be wrong for the new one. One line
  names a feature's change once, with each of its requirements as the PMI list names it. A
  location is listed when its feature moved, not when it only changed size. `report.json`'s
  `check` gives each as `{number, id, requirement, feature, changes}`.
- **PMI** compares the requirements the two files state as `haecceity.pmi/1`: each old
  requirement is paired with a new one through the face matching and its kind, and listed
  `added`, `removed`, `changed` or `renumbered`, with its characteristic number. Each file is
  read alone and paired without reading stored answers; the items then take each file's
  `pmiScene` ids and statuses, as the editor shows them. Pairing by kind alone, the last
  resort for requirements whose faces cannot be followed, never pairs requirements whose faces
  or references are found elsewhere on the new part. A datum is its faces: one relettered on
  them is `changed`.
  - **A change of limits** on the same feature and kind (H7 → H8, ⏥ 0.05 → 0.02) is `changed`,
    with both values and the same number.
  - **A different measurement** (a hole located from another face) is the old requirement
    `removed` with its number and a new one `added` with a new number.

### PMI requirements

Characteristic numbers drive pairing when the new file continues the old file's number history:
the old file's numbered supports are read on the new part through the face matching, and the
new file's own history must continue that structural snapshot and high-water mark. Values,
thread classes and notation are outside that identity (For #96). Carried numbers match first,
then ids, corresponding faces and unique remaining kind; differently numbered items stay
distinct at the weaker stages. A removal is then `retired`: its number is never reused.

Two files numbered independently (each written afresh) reserve nothing: items match by id,
then corresponding faces with kind resolving multiple items on one support, then unique
remaining kind. A same-id match must not contradict known face correspondence. When some of
the old item's faces are gone, its surviving faces must map into the new item's, and none of
the new item's faces may be another old face's image: a removed hole's location does not pair
with the hole that took its face numbers. `renumbered` records the number difference on the
resulting pair. A split can map one old face to several new faces. Ambiguous item matches stay
added/removed rather than choosing arbitrarily.

After exact faces and before unique remaining kind, dimensions can match through support
growth. Every mapped old face must remain in the new support; the dimension type and stated
source measurement role must agree. Location targets and references are checked separately,
preserving pitch endpoints and measuring planes. The match must be unique in both directions
among unmatched items. This keeps a pattern's existing pitches, bore diameter and locations
when counterbores add supporting faces, without letting a new counterbore diameter consume the
bore requirement. These pairs report `changed` with reason `support changed`.

Comparison uses haecceity's full semantic notation and frame cells, including
material and datum modifiers, diameter zones, fits, qualifiers and note text.
Semantic fields are compared too, including the datum system, basic flag and fit
class even when equal stated limits hide the class in the notation. Topology and
file-local support/unit identities are compared through their correspondence or
resolved meaning rather than their JSON ids. A semantic-only change carries a
`reason` even when its displayed text stays equal. Notes use their own semantic
wording (`pmiScene.specify[id].notation`), not the shared GENERAL NOTES graphic;
changing one note does not change the other requirements in that block. Thread manufacturing
notes use the same designation, class and depth/THRU wording as their drawing callout
(e.g. `M6×1-6H THRU`) on both sides. Other semantic-only attribute notes, including knurl
manufacturing attributes, show their named values instead of the fallback `note`.
Semantic-only knurls show their pattern and pitch; surface textures show their stated
parameters. Existing drawing wording remains authoritative when a callout is present.

A thread's own `thread manufacturing` attribute is compared as part of the thread,
giving one item for an added, removed or changed thread. Folding requires the attribute's
owner id to be that thread's id, identical face anchors and identical drawing wording
within each revision. Its semantic fields remain part of the comparison: a depth or
other manufacturing edit still changes the thread item even when the thread record
itself is unchanged. The item's `old` / `new` use each revision's drawing wording.
Notes with another owner, different faces or different wording, and notes without a
matching thread, remain separate items. Optional scenes retain all semantic records.

`report.json` adds `pmi.items`, an ordered list of changes:

```json
{"class":"changed","number":7,"id":"size:holes:9","kind":"dimension",
 "faces":{"old":[{"face":9}],"new":[{"face":9}]},
 "status":"answered","on":"⌀6.6 holes","old":"⌀6.6 H7","new":"⌀6.6 H8"}
```

Each item has `class` (`added`, `removed`, `changed`, or `renumbered`), optional
`number`, `id`, `kind`, `status`, `on` (feature words), and optional `old`, `new`
and `reason`. A numbered removal from a carried number history has
`reason: "retired"`; those numbers are never reused. Fresh numbers (no carried history, or two
independent histories) reserve nothing: a removal there is `removed` with its old number and no
`retired` reason, since the new file may give that number to another item. Renumbering is reported if detected, though ordinary editing preserves
numbers. A renumbering alone is not a change to the requirement: the summary counts it apart ("2
characteristic numbers changed (requirements unchanged)"), and Markdown lists it after the changes as `#25 ⏥ 0.05: unchanged; characteristic number
#22 → #25`. A new file written afresh numbers afresh, and its renumberings show here. Value and material-modifier edits keep the number and produce one `changed`
item per edited requirement, including direct STEP edits (For #95). Changes sort by
characteristic number, then unnumbered id. Markdown prints one line per change, for example:

```text
#7 ⌀6.6 H7 → ⌀6.6 H8 (4× ⌀6.6 holes)
#12 added: ⌖ ⌀0.1 Ⓜ A B C (hole pattern)
#5 retired: 50 (hole moved)
#15 removed: ⌀8 H11 (Ø8 × 10 through hole)
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

- Rust: `revision::diff_with_options(old, new, DiffOptions { scenes: true })`
  (also `revision::assembly::diff_with_options`). `revision::diff` keeps its default.
- CLI: `specify-core diff old.step new.step -o result --pmi-scenes`.
- WASM: `diff(oldName, oldBytes, newName, newBytes, "{}", title, true)` (the fifth argument is
  not read).
- Python: `api.diff(old, new, scenes=True)`.

Both documents have the `pmiScene` shape (`pmi`, `presentation`, `specify`). They
are each file's `pmiScene` comparison scene (its PMI with the editor's ids and statuses), in
current semantic notation, not potentially stale saved As Drawn ink. No additional scene
derivation is performed. Assembly
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

The deprecated `pmi.stored`, `pmi.existing` and `pmi.existing_count` are gone (0.3.0): use
`items`.

WASM `diff` returns these items in `report.pmi.items` (and in each assembly part's
report), with ids and numbers suitable for highlighting `pmiScene` characteristics
in `/diff`. The Python package exposes `specify_core.api.diff(old, new,
*, title="Revision", part=None, scenes=False)`, returning `{report, markdown}` for the selected
part. It accepts paths or loaded part handles; paths accept a part index.

## The picture

`model.png` is the new part, seen isometrically with Z up. Changed faces are orange, new faces
green, faces that can't be told apart purple, and the rest grey. The old part's faces that are
gone or moved are drawn over it in translucent red, placed by the matching motion.
Changed PMI callouts and removed ghosts also appear, with the camera including
their presentation geometry; see [PMI requirements](#pmi-requirements).

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
- **Requirements** are each part's own, read from its PMI as a single part's are.
- **`report.json`** is `{verdict, summary, parts}`, each part `{name, status, old, new}` (which
  part of each file and how many times it is placed) and its `report` when it is in both.

`--part N` compares only part N of each file, as before: a single part's report. Both files must
have that part.
