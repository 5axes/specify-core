# How a part is made

Maintainer decision (5 November 2026, the "How is it made?" proposal, all eight decisions as
recommended): a per-part answer, `part.process`, sets Specify's question defaults. It sets
**defaults, not prohibitions**: an explicit answer always wins.

## The question

`part.process` is a `choice` question with the options `machined`, `3D printed: FDM`,
`3D printed: resin`, `3D printed: powder`, `3D printed: metal`, `sheet metal`, `moulded` and
`cast`. It is the **last** question of every part, after the hole, datum and full-thread
questions, so no existing question moves. It never blocks a write (it is not among the open
questions).

**No answer and no file value means machined.** Every question, default and write of such a
part is what it was before the process existed: the machined default is never added by
`--accept-defaults` / `with_all_defaults`, stored in answers or written.

**The guess.** A material from one of the material list's printing classes suggests the
matching process: filament → FDM, resin → resin, powder → powder, metal → metal. Any other
material, or none yet, suggests machined. The answered material is used, else the one the file
states. The suggestion is the question's default; a printed suggestion is marked attention
`check` with its reason (machined, what no answer means anyway, is `routine`). It is not
applied until it is answered or accepted. Accepting defaults takes a printed
suggestion first, on its own, then that process's defaults.

## What each process changes

| Process | General tolerance | Holes | Locating | Not asked |
|---|---|---|---|---|
| machined | ISO 2768-m | as before | position tolerances | — |
| 3D printed: FDM | `±0.3 mm or ±0.3 % (FDM)` | clearance by default; `heat-set insert`, `printed thread` offered | ± dimensions | surface finish, edges, coating |
| 3D printed: resin | `±0.15 mm or ±0.15 % (resin)` | as FDM | ± dimensions | as FDM |
| 3D printed: powder | `±0.3 mm or ±0.3 % (powder)` | as FDM | ± dimensions | as FDM |
| 3D printed: metal | `±0.1 mm or ±0.2 % (metal)` | as machined (tapped stays the default) | ± dimensions | — |
| sheet metal | ISO 2768-m | clearance by default; tapped offered | ± dimensions | surface finish (edges default to `Deburr only`) |
| moulded, cast | ISO 2768-m | as machined | position tolerances | surface finish |

- **General tolerance.** The ± figures are vendor figures, not a standard (no standard sets
  general tolerances for 3D printing), so the default is marked `check`. The ISO 2768 classes
  stay offered, and where a ± figure is offered a person's own is accepted as typed: `±0.2`,
  `+-0.2 mm` or `±0.2 mm or ±0.2 %` read as `±0.2 mm` / `±0.2 mm or ±0.2 %`. A ± figure is
  written as a **note**, because the kernel's general tolerance entity carries an ISO 2768
  class: a part-owned `semantic text` attribute set with one `general tolerance` text item.
  It reads back as the part's `general_tolerance`. General dimensions take the greater of the
  millimetres and the percentage of the nominal.
- **Moulded and cast** keep the ISO 2768 class options. ISO 20457 (moulded plastics) and
  ISO 8062-3 (castings) need a tolerance group or grade the rules cannot choose, so they are
  not offered as defaults, and a machined, sheet metal, moulded or cast part's general
  tolerance stays an ISO 2768 class (whose typed forms are read as before).
- **Holes.** Where a machined part's hole would be tapped, a printed plastic or sheet metal
  part's is a clearance hole: for the screw whose clearance hole it is, else for a screw not
  stated (`clearance`), never a smaller screw that also passes a tap-drill-sized hole. A
  printed plastic part's clearance hole has no fit: no H11 size is written, and the general
  tolerance covers it (sheet metal and metal printing keep H11). A `heat-set insert` or `printed thread` answer is written as a note on the
  hole (a `semantic text` set on the hole feature with a `hole note` item), read back as the
  hole's existing note, so the hole is not asked again; an Add-mode write does not repeat a
  note already stated. No fit is a default; fits stay offered.
- **Locating.** The default of `part.locating` is `± dimensions`, which removes the position,
  frame and datum-control questions; answering `position tolerances` brings them back.
- **Surface finish.** A finish written for a printed plastic, moulded or cast part (a face
  finish added as an extra requirement) has material removal **not allowed**, whether the
  process is answered or stated by the file.

## Answers the process leaves unasked

A part answer whose question the process does not ask (`part.surface_finish`, `part.edges`,
`part.coating`) is kept, but contributes no PMI: it becomes an `unasked` row with the reason
"part.surface_finish is not asked when part.process is '3D printed: FDM', so its answer was not
written", exactly as a dependent answer does when `part.locating` changes (#126). The answer is
stored and carried, and written again if the process changes back. The other `part.*`
answers, which the intent reads whether or not they are asked, stay exempt.

An answer only another process offers lapses the same way rather than being refused: a
`heat-set insert` or `printed thread` hole on a part no longer printed in plastic, or a ± general
tolerance on a part whose process offers none ("part.general_tolerance '±0.2 mm' is not offered
when part.process is 'machined', so its answer was not written"). The question shows its
default meanwhile.

## Assemblies

The process belongs to the **product**: a product placed several times has one process, on its
one product definition. Specify asks per part (`analyse --part N`). A part whose make/buy is
`bought`, from a BOM answer or from the file, is asked no process; an answer given for it is an
`unasked` row ("part.process is not asked of a bought part"). A bought **guess** does not
suppress the question, as a suggestion is never applied. The assembly product has no process.

`same_as` groups congruent copies only when their effective stated processes agree: the saved
`part.process` answer wins over the file’s stated process. A guess or accepted suggestion
contributes no stated process, so two defaults group even if their values differ, while a
stated process and no stated process split, as in draftwright’s pack. Bought parts have no
process for this comparison. The other stated BOM facts and saved `part.*`/`bom.*` answers
still have to agree.

## STEP form

A CAx-IF user defined attribute (UDA practice 1.8) property set `specify process` on the
part's `PRODUCT_DEFINITION`, with one text item `manufacturing_process` holding one of the
options. An accepted suggestion also carries `default.manufacturing_process`, the BOM's
marker, as the text item `'true'` (haecceity's writer writes no boolean items, as its reader
cannot read them back; the BOM's boolean `.T.` form is read too); an answer has no marker. It is written only when the
process is answered or a suggestion is accepted, in the same edit as the part's PMI, and
replaced with it on a rewrite. A part only ever has one: where Specify's set is in a file
written without stored answers (so Add mode keeps it), the same process is not written again
and a different one is refused.

```step
#100=DESCRIPTIVE_REPRESENTATION_ITEM('manufacturing_process','3D printed: FDM');
#101=REPRESENTATION('',(#100),#20);
#102=PROPERTY_DEFINITION('specify process','user defined attribute',#5);
#103=PROPERTY_DEFINITION_REPRESENTATION(#102,#101);
#104=GENERAL_PROPERTY('','specify process',$);
#105=GENERAL_PROPERTY_ASSOCIATION('',$,#104,#102);
```

**Reading.** Any part-level attribute set with a `manufacturing_process` item (matched ignoring
case, spaces and hyphens) whose value is one of the options (ignoring case) is read; other
values are ignored. Precedence is answer > file > guess, with the source reported as the BOM's:
`answered` (an answer, or Specify's unmarked set), `file` (another tool's set), `default`
(Specify's marked set, else the guess). Analysis reports the file's value as
`analysis.process = {value, source}`, and a stated value (Specify's unmarked set or another
tool's) as `analysis.part.process`: the question is then not asked, as other `part.*` values the
file states are not. A resumed Specify file's answer comes back with its other answers. An
accepted suggestion stored in the file is the question's default only while the current
material still suggests it; it is not an answer, so `apply` without accept-defaults on the
resumed part leaves it machined until the suggestion is accepted again. A `general tolerance`
note is read as the part's general tolerance only when Specify wrote it.
`specify_core::process::of(analysis, answers)` gives the current value and source (none for a
bought part); `process::read(&Document, product_definition)` reads a file's.

## Compatibility

An answer file with `part.process` is refused by an older Specify, as an unknown answer ID. An
older Specify reading a new STEP ignores the `specify process` set.
