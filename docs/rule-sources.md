# Where Specify's rules come from

Specify proposes defaults (general tolerance, finish, datums, what a hole is for, position
tolerances, process settings) and writes the answers as AP242 PMI. This page is the index of
every rule that supplies a value, the source that rule rests on, and how firm that source is.
Each question already carries a free-text `basis`; this page says, for each rule, whether the
value comes from a published table or is Specify's own choice.

Each rule has one **kind**:

| Kind | Meaning |
| --- | --- |
| `standard` | The value is read from a published standard's table or formula. |
| `convention` | Specify's own choice: common practice, not required by any standard. |
| `vendor` | A figure published by manufacturing services, where no standard exists. |
| `assumption` | A value no source gives, stated as assumed. |

An edition is given only where the code or these docs record it; otherwise "edition not
recorded". Code locations name the file and the function or constant, which outlast line
numbers.

## Published standards

| Standard | Edition | Supplies | Kind | Code |
| --- | --- | --- | --- | --- |
| ISO 2768-1 | not recorded | General tolerances for linear dimensions, classes f/m/c/v (±mm by nominal range, 0.5 to 4000 mm). The default class is `ISO 2768-m` (a convention, below). Only the linear table is held: no angular, radius or chamfer table. | `standard` | `src/general.rs` `ISO_2768`, `BOUNDS`, `deviation`; offered as `src/standards.rs` `GENERAL_TOLERANCES` |
| ISO 2768-2 | not recorded | Classes H/K/L are accepted and written in combined designations (`ISO 2768-mK`), but **no ISO 2768-2 table is held**: the geometric class adds no default. The linear deviation comes from ISO 2768-1 for the first letter. | `standard` (designation only) | `src/general.rs` `deviation` (comment), `src/standards.rs` `GENERAL_TOLERANCES` |
| ISO 286-1 | 2010 | Standard tolerance grades IT5 to IT11 (Table 1) and shaft fundamental deviations f, g, h (Table 2), k, m, n, p (Table 3), for nominal sizes up to 3150 mm; hole H is 0 at every size. | `standard` | `src/standards.rs` `RANGES`, `IT`, `SHAFT`, `fit_limits` |
| ISO 286-2 | 2010 | Cross-check of the 500 to 3150 mm values (Table 17: h6, k6, m6, g6, f7). | `standard` | `src/standards.rs` `IT` (comment) |
| ISO 273 | not recorded | Clearance holes for bolts M1.6 to M24, fine, medium and coarse series. A hole between a bolt's fine and coarse size is a plausible clearance hole for it; medium is the mating hole in the tapped-hole position formula. | `standard` | `src/standards.rs` `CLEARANCE_FINE`, `CLEARANCE`, `CLEARANCE_COARSE`, `clearance_bolts`, `largest_bolt_below`; used by `src/rules/holes.rs` |
| ISO 261 | 1998 (Table 1, recalled; not checked against a copy: `docs/next-session.md`) | Metric thread sizes M1.6 to M64 with coarse and fine pitches (all three choice columns; bracketed pitches and M14×1.25 left out), offered on turned diameters; coarse pitches M1.6 to M24 for holes and mates. | `standard` | `src/standards.rs` `ISO_261`, `PITCH`, `external_threads` |
| (tap drills) | — | Tap-drill diameters M1.6 to M24, from which a hole is recognised as tapped. The code labels this table "ISO 261 coarse tap drills"; ISO 261 sets sizes and pitches, so the table's actual source needs checking (see Open review). | `standard` (source to confirm) | `src/standards.rs` `TAP_DRILL` |
| ISO 965 | not recorded | Thread tolerance classes: 6H for internal (tapped holes), 6g for external (turned threads). | `standard` | `src/standards.rs` `INTERNAL_THREAD_CLASS`, `EXTERNAL_THREAD_CLASS`; used in `src/rules/holes.rs` `hole_requirements`, `src/rules/turned.rs` |
| ISO 1101 | not recorded | The position characteristic and its zone: a coaxial hole on a turned part gets diametral position to axis A alone. Interpretation taken from the [ZEISS ISO 1101 GPS poster](https://qualityforum.zeiss.com/migration/images/2500_56edcb4b5a2d29f667a6473ad461aeb8.pdf). | `standard` | `src/rules/mod.rs` `all_questions` (`located`); `docs/turned-defaults.md` |
| ISO 1302 | not recorded | How a surface finish is written: 'any process allowed', the basic symbol an unqualified `Ra 3.2` means (surface texture in ISO 10303-1110's form). It supplies no Ra value. | `standard` (notation) | `src/writer.rs` (module doc, surface texture) |
| DIN 82 | not recorded (text not available) | Knurl types, 90° profile, pitch series 0.5 to 1.6 mm, nominal diameter d1. Checked through IS 3403:1981 *Dimensions for knurls* (§3.1 to §3.4) and IS 6776:1980 *Knurling wheels* (§4.3 root radius 0.1 mm for P ≤ 1.0 mm, 0.15 mm above; type GE helix 30° each way). Neither DIN 82 itself nor ISO 13444 (indication of knurls) was available. | `standard` | `src/standards.rs` `KNURL_STANDARD`, `KNURL_PITCHES`, `KNURL_HELIX_ANGLE`, `knurl_parameters` |
| ISO 10303-242 (AP242), ISO 10303-1110 | follows the input file's schema; edition 1 files are upgraded in the header when Specify's metadata needs it | How answers are written as semantic PMI, and surface texture. Practice: CAx-IF PMI Recommended Practices 4.1 and the CAx-IF UDA practice for Specify's own attribute sets. | `standard` | `src/writer.rs` |

ISO 20457 (moulded plastics) and ISO 8062-3 (castings) are **not** used: they need a
tolerance group or grade the rules cannot choose, so moulded and cast parts keep the ISO 2768
classes (`docs/process.md`). ISO 262 is not used either; turned threads use ISO 261.

## Specify's rules: question defaults

Each row is a default a question proposes. Where an answer to an assembly mate exists, the
mate's suggestion replaces the rule's default (last section).

### Part settings (`src/rules/mod.rs` `machined_part_questions`, `part_questions_for`)

| Question | Default | Source | Kind |
| --- | --- | --- | --- |
| `part.general_tolerance` (machined, sheet metal, moulded, cast) | `ISO 2768-m` | "the usual general tolerance for machined parts"; the deviations are ISO 2768-1's | `convention` (class choice) on a `standard` table |
| `part.general_tolerance` (3D printed) | FDM `±0.3 mm or ±0.3 %`, resin `±0.15 mm or ±0.15 %`, powder `±0.3 mm or ±0.3 %`, metal `±0.1 mm or ±0.2 %`, whichever is greater; marked `check` | common vendor figures; no standard sets general tolerances for 3D printing. Vendors and dates are not recorded (`src/process.rs` `general_tolerance`) | `vendor` |
| `part.surface_finish` | `Ra 3.2` | "what ordinary machining gives without extra passes"; offered Ra 0.4 to 12.5 (`FINISHES`) | `convention` |
| `part.edges` (machined) | `Break sharp edges 0.2 max` | "the common standard note"; no standard is cited in code | `convention` |
| `part.edges` (sheet metal) | `Deburr only` | "the usual note for sheet metal" | `convention` |
| `part.coating` | `None (as machined)`; `None` for another process | nothing stated means no coating | `assumption` |
| `part.heat_treatment` | `None` | nothing stated means no heat treatment | `assumption` |
| `part.notes` | empty list | no free-text notes unless given | `assumption` |
| `part.locating` | `position tolerances`; `± dimensions` for 3D printed and sheet metal parts | "holes usually take parts that must line up"; printed and sheet metal holes are usually located on the drawing (maintainer decision 2026-11-05, `docs/process.md`) | `convention` |
| `part.process` | from the material's class in the material list (filament → FDM, resin, powder, metal); otherwise machined; a suggestion accepted earlier stands while the material still suggests it | `src/process.rs` `guess`, `question`; the material list is suggestions as CNC and printing services offer them (`src/materials.rs`) | `convention` |

Which questions each process asks is the maintainer's decision of 2026-11-05
(`docs/process.md`, `src/process.rs` `asks`, `clearance_holes`, `locates_by_dimensions`).

### Datums (`src/rules/datums.rs` `datum_questions`, `default_datums`; `docs/turned-defaults.md`)

| Question | Default | Kind |
| --- | --- | --- |
| `datum.A`, prismatic part | the largest flat face: usually what the part sits on or is clamped by | `convention` |
| `datum.A`, prismatic part in an assembly | the face where the part meets another part (contact) | `convention` |
| `datum.B`, prismatic | the largest flat face square to A | `convention` |
| `datum.C`, prismatic | the largest flat face square to A and B | `convention` |
| `datum.A`, turned part | the axis of the plain turned seat with the greatest total cylindrical area (diameter breaks ties; threads and knurls excluded) | `convention` |
| `datum.B`, turned part | the face perpendicular to A: assembly contact first, else the largest, recess walls excluded unless the only one | `convention` |
| `datum.C`, turned part; datums D to H | none proposed (left empty) | `convention` |
| `datum.A.flatness`, `datum.B.control`, `datum.C.control` | none (an optional tightening; `datum_control_questions`) | `convention` |

A published basis for the A/B/C choice (ISO 5459, ASME Y14.5) has not been sought yet (Open
review).

### Holes (`src/rules/holes.rs` `hole_questions`, `function_basis`, `position_basis`, `default_position`)

| Question | Default | Source | Kind |
| --- | --- | --- | --- |
| `hole.function` | `clearance:<bolt>` when the diameter is an ISO 273 clearance hole (nearest medium size first) | ISO 273 tables | `standard` |
| `hole.function` | `tapped:<size>` when the diameter is exactly a tap drill (wins over a smaller bolt that also clears); marked `check` | tap-drill table | `standard` (table source to confirm) |
| `hole.function` | `general` when the diameter matches neither | nothing to propose | `convention` |
| `hole.function` (3D printed plastic, sheet metal) | the clearance hole for the screw whose clearance hole it is, else `clearance` (screw not stated) | process decision 2026-11-05 | `convention` |
| `hole.frame` | the part's whole frame, A then B then C | | `convention` |
| `hole.position`, fitted hole | Ø0.05 | "a fitted hole is located closely" | `convention` |
| `hole.position`, tapped hole | Ø(H − F)/2 for a fixed fastener: H the ISO 273 **medium** clearance hole of the mating part, F the screw; M2 → (2.4 − 2)/2 = Ø0.2 | fastener formula (no standard cited in code) on ISO 273 medium | `convention` |
| `hole.position`, clearance hole | ØH − F for a floating fastener: H this hole, F the stated bolt, else the largest bolt this hole could clear (`largest_bolt_below`: one it passes with no more than twice its coarse-series play) | fastener formula on ISO 273 | `convention` |
| `hole.position`, no bolt known | Ø0.1 | "a common value" | `convention` |
| `hole.location` (± mode) | `general` (the general tolerance) | | `convention` |
| `hole.full_thread` | drill depth less 3 pitches (tap lead and drill point), rounded down to 0.5 mm, at least one pitch (`src/standards.rs` `full_thread`); or the depth given in the tapped answer | ISO 261 coarse pitch | `convention` |

### Turned diameters (`src/rules/turned.rs`)

| Question | Default | Kind |
| --- | --- | --- |
| `diameter.fit` | `general` (fits, ISO 261 threads and knurls offered) | `convention` |
| `diameter.runout` | none (an optional tightening) | `convention` |

### Assembly mates (`src/mates.rs` `suggest`; read by `src/rules/mates.rs`)

When a hole or diameter lines up with one in another part, the mate's suggestion becomes the
default, marked `check`:

| Mate | Suggestion | Kind |
| --- | --- | --- |
| tap drill under another part's clearance hole for the same screw | `tapped:<size>` | `convention` (ISO 273, tap-drill tables) |
| two clearance holes for the same bolt, or a hole over a tap drill or a thread-sized shaft | `clearance:<bolt>` | `convention` (ISO 273) |
| a shaft in a bore of its own size | `fit:H7` in the bore, `h6` on the shaft ("H7 assumed") | `assumption` (ISO 286 classes) |
| a thread-sized shaft in a tap drill | `thread:<size>` on the shaft, `tapped:<size>` in the hole | `convention` |

### BOM (`src/bom.rs` `questions`, `guessed`)

The optional `bom.*` questions have no default, only suggestions. `bom.make_buy` proposes
`bought` for a name whose last noun is a standard part (screw, washer, dowel, bearing, nut,
fastener, bolt, rivet, circlip, pin, stud), `made` for a name carrying a part number or
anything else, and `made` with a `check` when a "standard" part has machined pockets. Kind:
`convention`.

## Specify's rules: values in what is written

These are not question defaults but values the rules or the writer derive from an answer.

| Rule | Value | Source | Kind | Code |
| --- | --- | --- | --- | --- |
| Clearance hole size | H11 on the hole as drawn (not for a printed plastic part, whose general tolerance covers it) | Specify's choice of class; limits from ISO 286 | `convention` | `src/rules/holes.rs` `hole_requirements` |
| Tapped hole | designation, ISO 261 coarse pitch, class 6H, right hand, full thread (rule above) | ISO 261, ISO 965 | `standard` | `src/rules/holes.rs` `hole_requirements` |
| Turned thread | class 6g, right hand | ISO 965 | `standard` | `src/rules/turned.rs` |
| Knurl pitch | 1.0 mm | "a common pitch", in DIN 82's series | `convention` | `src/standards.rs` `KNURL_PITCH` |
| Knurl teeth | π·d/P rounded | DIN 82 d1 and pitch | `standard` | `src/standards.rs` `knurl_parameters` |
| Knurl tooth depth | P/2, the depth of the sharp 90° profile; crest and root rounding not counted. No table gives it. | — | `assumption` | `src/standards.rs` `knurl_parameters` |
| Knurl root fillet | 0.1 mm for P ≤ 1.0 mm, 0.15 mm above | IS 6776:1980 §4.3 | `standard` | `src/standards.rs` `knurl_parameters` |
| Knurl helix | 30° each way, no hand (diamond) | DIN 82 type RGE via IS 6776:1980 | `standard` | `src/standards.rs` `KNURL_HELIX_ANGLE` |
| General dimensions | every undimensioned length and diameter, baseline from the datum planes, toleranced by the ISO 2768-1 linear table | ISO 2768-1 table; the scheme is Specify's | `convention` | `src/general.rs`, `src/dimensioning.rs` |
| Printed ± general tolerance | the greater of the millimetres and the percentage of the nominal; written as a note | vendor figures | `vendor` | `src/general.rs` `plus_minus`, `deviation`; `src/writer.rs` |

## Rule ids

Every default Specify proposes carries a structured `source` beside its free-text `basis`:

```json
"source": {"rule": "holes.position.tapped", "kind": "convention",
           "refs": [{"standard": "ISO 273", "clause": "medium series"}]}
```

`rule` is a stable id from the table below; `refs` may be empty; `edition` and `clause` appear
only where recorded. The ids are defined once, in `src/rule_sources.rs`; a test
(`tests/rule_sources.rs`) checks that this table and the code list the same ids, and that every
default on the corpus and fixture analyses has one. Where it appears:

- **Questions:** `source` on each question with a default (and on a BOM `make_buy` suggestion),
  in `api::review` (both WASM `review`s), the MCP `questions` tool, the Python
  `Question.source` and `Question.to_dict()`, and, when asked for, CLI
  `questions --sources` and serve `{"op": "questions", "sources": true}`. Without the option
  the CLI and serve keep the Python implementation's document (`Question::to_dict`, which the
  oracle compares); `to_traced_dict` and `to_presented_dict` carry the source.
- **Preview intent:** `sources` beside `intent` in `api::review` (WASM `review`) and the MCP
  `characteristics` tool (`api::preview_sources`): question ID to source, for each answer that equals its question's
  default (an accepted default; an answer typed equal to the default is traced too).
- **Answers export:** `_specify.sources` in `questions --resolve-defaults`, serve
  `export_answers`, `api::export_answers`, WASM `exportAnswers`, Python and the MCP
  `export_answers` tool, on the same terms. It is output only: ignored when the map is read
  back and recomputed on each export. `diff` does not carry it.
- **Not in the STEP.** The source is not written into the file.

| Rule id | Kind | Refs | Default |
| --- | --- | --- | --- |
| `part.general_tolerance.machined` | convention | ISO 2768-1 | `ISO 2768-m` |
| `part.general_tolerance.printed` | vendor | — | the process's ± figure |
| `part.surface_finish.machined` | convention | — | `Ra 3.2` |
| `part.edges.machined` | convention | — | `Break sharp edges 0.2 max` |
| `part.edges.sheet_metal` | convention | — | `Deburr only` |
| `part.coating.none` | assumption | — | no coating |
| `part.heat_treatment.none` | assumption | — | no heat treatment |
| `part.notes.none` | assumption | — | no notes |
| `part.locating.position` | convention | — | `position tolerances` |
| `part.locating.dimensions` | convention | — | `± dimensions` (printed, sheet metal) |
| `process.material_class` | convention | — | the process the material's class suggests, else machined |
| `process.accepted` | convention | — | the suggestion accepted when the file was last written |
| `datums.prismatic.a` | convention | — | largest flat face |
| `datums.prismatic.a.contact` | convention | — | the face where the part meets another |
| `datums.prismatic.b` | convention | — | largest flat face square to A |
| `datums.prismatic.c` | convention | — | largest flat face square to A and B |
| `datums.turned.a` | convention | — | axis of the largest plain turned seat |
| `datums.turned.b` | convention | — | locating face perpendicular to A |
| `datums.none` | convention | — | no face proposed (empty) |
| `datums.control.none` | convention | — | no flatness or datum control |
| `holes.function.clearance` | standard | ISO 273 | `clearance:<bolt>` |
| `holes.function.tapped` | standard | ISO 261:1998 (coarse pitch) | `tapped:<size>` |
| `holes.function.general` | convention | — | `general` |
| `holes.function.printed` | convention | ISO 273 | clearance hole on a printed or sheet metal part |
| `holes.frame.primary` | convention | — | the whole frame, A then B then C |
| `holes.position.fitted` | convention | — | Ø0.05 |
| `holes.position.tapped` | convention | ISO 273 (medium series) | Ø(H − F)/2 |
| `holes.position.floating` | convention | ISO 273 | ØH − F |
| `holes.position.no_bolt` | convention | — | Ø0.1 |
| `holes.location.general` | convention | — | `general` |
| `holes.full_thread` | convention | ISO 261:1998 (coarse pitch) | drill depth − 3P, down to 0.5 mm, ≥ P |
| `holes.full_thread.given` | convention | — | the depth the tapped answer states |
| `turned.fit.general` | convention | — | `general` |
| `turned.runout.none` | convention | — | no runout |
| `mates.tapped` | convention | ISO 273 | `tapped:<size>` from a mate |
| `mates.clearance` | convention | ISO 273 | `clearance:<bolt>` from a mate |
| `mates.fit` | assumption | ISO 286-1:2010 | `fit:H7` / `h6` from a mate |
| `mates.thread` | convention | ISO 261:1998 | `thread:<size>` from a mate |
| `bom.make_buy.guess` | convention | — | made or bought, from the name |
| `bom.make_buy.recorded` | convention | — | the answer or file value, proposed back |

## Open review

Not done yet; each item is to be resolved with its source, and any change to a default made
separately with that source:

- **Texts not yet checked:** DIN 82 and ISO 13444 (knurls; values checked only through IS 3403
  and IS 6776), ISO 2768-2 (no tables held), ISO 261 (recalled, not checked against a copy).
- **Editions:** record the edition and exact table or clause for ISO 2768-1, ISO 273, ISO 965,
  ISO 1101 and ISO 1302 (ISO 1302 may have been replaced by ISO 21920-1; to confirm).
- **Tap drills:** the code calls `TAP_DRILL` "ISO 261 coarse tap drills"; find the standard that
  actually tabulates them and cite it.
- **Clearance hole class:** H11 is Specify's choice; check what ISO 273 itself recommends for
  each series.
- **Conventions:** find a published basis, or say plainly it is Specify's choice and why, for the
  datum A/B/C rules (ISO 5459, ASME Y14.5), the fastener formulas and the fitted Ø0.05 and
  no-bolt Ø0.1 positions (ISO 2692, ASME Y14.5 appendix), the `Ra 3.2` finish, and the
  0.2 mm edge break (ISO 13715).
- **Vendor figures:** name the vendors behind the 3D-printing ± figures and date them.
- **Assumptions:** knurl tooth depth P/2, and H7/h6 for a shaft in a bore of its own size.
