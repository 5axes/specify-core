# Stable characteristic numbers (For #65)

The STEP file is the authority for inspection numbers. CSV and drawing consumers read
these numbers; they must not allocate their own or use recognition feature IDs as numbers.
There is no dependency on the editor or draftwright.

## AP242 storage

Specify uses the **standard derived `id` slot**, populated by
`ID_ATTRIBUTE('<positive decimal integer>',#characteristic)`. It does not put numbers in
`name` (which carries dimension type or other semantics) or in the authorship description.
The integer is canonical decimal, starts at 1 on an unnumbered part, and is unique within
each part. Imported numeric ids on consumed PMI (including shared feature shape aspects)
reserve their values too, to respect AP242's wider id uniqueness constraints.

This choice follows CAx-IF [PMI Recommended Practices v4.1, §5.1, Figure 4](https://www.mbx-if.org/home/wp-content/uploads/2024/06/rec_pracs_pmi_v41.pdf),
which recommends `id_attribute` and describes its uniqueness constraints. The pinned
haecceity AP242 EXPRESS table's `id_attribute_select` includes `dimensional_size`,
`geometric_tolerance`, `shape_aspect`, `shape_aspect_relationship` and `property_definition`.
`dimensional_location` inherits `shape_aspect_relationship`; `datum` inherits `shape_aspect`.
The implementation was checked against quiddity v0.2.3's `docs/step-ap242.md`, its PMI writer,
and its schema table, rather than assuming the visible names were free identification slots.

| Characteristic | Exact entity carrying `ID_ATTRIBUTE` |
| --- | --- |
| Size or location dimension | `DIMENSIONAL_SIZE` or `DIMENSIONAL_LOCATION` (and subtypes) |
| Geometric tolerance | The simple or complex `GEOMETRIC_TOLERANCE` root |
| Datum feature requirement | `DATUM`; follow `SHAPE_ASPECT_RELATIONSHIP` from its `DATUM_FEATURE` to this datum |
| Surface texture, including a part default | `PROPERTY_DEFINITION('surface texture',...)`, not a parameter property |
| Thread or knurl | Its exclusive `PRODUCT_DEFINITION_SHAPE` whose definition is `THREAD` or `TURNED_KNURL` |

Only one entity carries each number. In particular, putting the same number on both
`DATUM` and `DATUM_FEATURE` would violate AP242 shape-aspect id uniqueness. A datum feature
may also be shared by several datum labels or a dimension; using `DATUM` keeps those
requirements distinct. Threads and knurls are `characterized_object`s, outside
`id_attribute_select`; their definition shapes provide a standard property-definition id
slot without assigning a requirement's identity to shared geometry.

The part-level **user-defined property name is `specify characteristic numbers`**.
It uses the existing CAx-IF UDA `AttributeSet` route, on that part's product definition, with
one text item named **`history`**. Its compact JSON is:

```json
{"schema":2,"faces":"<resume schema-2 face fingerprint>","high_water":7,"entries":[{"key":{"role":"size","kind":"dimension","type":"Size_Diameter","faces":[9],"reference":[]},"number":3}]}
```

`high_water` is the greatest number ever allocated, including retired requirements. Entries
are the current requirements, not a list of reusable numbers. The key describes requirement
kind/type, target role and face support; readers use `ID_ATTRIBUTE` instead of interpreting it. Values, modifiers, zones and notation are not
identity. A replace carries the history even when all requirements are removed.
An additive write retains earlier snapshots; the last snapshot is current and its counter
must be at least every preceding snapshot's counter. The property name is reserved from
caller attributes. A corrupt history or exhausted counter refuses the write, without output.

### Schema-2 key grammar (For #96)

Keys are JSON objects with fields in the following writer order: `role`, `kind`, `type`,
`faces`, `reference`. Face lists contain sorted, unique, zero-based face indices in the
history's fingerprint namespace; an absent support is `[]`. Strings use ordinary JSON
escaping. These spellings are a storage contract, independent of Rust enum names,
Debug output and derived enum serialization.

| `kind` | `type` | Base `role` |
| --- | --- | --- |
| `datum` | The stated datum label, e.g. `A` | `datum` |
| `dimension` | Explicit size/location vocabulary below | `size` or `location` |
| `geometric_tolerance` | Explicit tolerance vocabulary below | `feature`, `dimension`, `relation`, `whole_part` |
| `surface_texture` | Stated ISO parameter names sorted and joined by `\|`, e.g. `Ra\|Rz` | `feature` or `whole_part` |
| `thread` | `Internal` or `External` (explicit constants retained from schema 1) | `feature` |
| `knurl` | `knurl` | `feature` |

Size types are `Size_` followed by `CurveLength`, `Diameter`, `SphericalDiameter`,
`Radius`, `SphericalRadius`, `ToroidalMinorDiameter`, `ToroidalMajorDiameter`,
`ToroidalMinorRadius`, `ToroidalMajorRadius`, `ToroidalHighMajorDiameter`,
`ToroidalLowMajorDiameter`, `ToroidalHighMajorRadius`, `ToroidalLowMajorRadius`,
or `Thickness`. An angular size is `Size_Angular`; an otherwise unnamed size with
a path is `Size_WithPath`, otherwise `Location_None` (the existing API convention).
Location types are `Location_Angular`, `Location_CurvedDistance`,
`Location_LinearDistance`, or `Location_LinearDistance_From<X>To<Y>` where X/Y are
`Center`, `Inner`, `Outer`, excluding Center-to-Center. Unmapped location names use
`Location_WithPath` when a path exists, otherwise `Location_None`.

Tolerance types are `Angularity`, `CircularRunout`, `Coaxiality`, `Concentricity`,
`Cylindricity`, `Flatness`, `ProfileOfLine`, `Parallelism`, `Perpendicularity`,
`Position`, `CircularityOrRoundness`, `Straightness`, `ProfileOfSurface`, `Symmetry`,
and `TotalRunout`. These are hand-written mappings in `existing.rs`, shared with
the read API; changing an upstream variant's Rust spelling cannot change a key.

A planned dimension appends `|planner:` to its base role, followed by
`measure=<stated measurement role>` for a size (e.g. `measure=bore.depth`), or
`axis=x`, `axis=y`, `axis=z` for a location. Measurement roles are literal planner
measurement names, not its generated requirement IDs. An optional suffix describes
the structural pattern role: `;pattern=first`, `;pattern=centre`, `;pattern=pitch`.
Neither pattern counts nor pitch interval counts enter identity. An out-of-domain
axis is spelled `unknown`; valid planner locations use axes 0–2.

For example, an H7 hole fit and an h6 shaft fit both use `Size_Diameter`; `6H`/`6g`
thread classes, `ISO 2768-m`, `qualifier=max`, `pattern=6x`, nominal/tolerance values,
knurl patterns and GD&T modifiers are **values or notation, not key components**.
They must not be added as ISO tokens to the key. The old
`specification:"10.000000000|fit:Iso286Class { ... }"` becomes `role:"size"`;
an old `specification:"0.200000000|[MaximumMaterialRequirement]|..."` becomes
`role:"feature"`. A unique requirement keeps its number across either edit.
Temporary matching hints compare typed values and are never serialized.

## Allocation and carry

The **first allocation is in inspection order**, consecutively 1…N without gaps on a
part with no reserved numeric ids. Later allocations append above `high_water`, even if
an added characteristic would sort first geometrically. Existing numbers always win,
including files numbered in category order by the 0.1.3-era writer. Reordering, adding or
removing another requirement does not compact the numbers. A removed number is retired
permanently; reintroducing that requirement later receives a new number.

The first-allocation rule is:

1. Datum features first, by datum letter (A, B, C…). Equal labels retain semantic model order.
2. Other characteristics are assigned to **front, top (Draftwright's `plan`), right
   (Draftwright's `side`)**, in that order, in the part's own stored coordinates. The
   projections are respectively (X,Z), (X,Y), (Y,Z), with the second coordinate upward.
   These are Draftwright's `model::VIEWS`. Its `analyse_step_input` keeps files carrying
   PMI in their stored frame; its optional squaring of slightly rotated numberless parts
   therefore does not apply to Specify's written PMI. Quiddity's inferred recognition
   frame is not a second rotation of those stored face coordinates.
3. Reuse Draftwright's simple geometric view rules (`pmi_lowering::face_on`, `edge_on`,
   `plane_to_bore`, and `dims::pmi_dimensions`): bore sizes and plane-to-bore
   locations (including bore-to-bore locations) use the cylinder's end-on view (X axis → right, Y → front, Z → top).
   Other linear dimensions use the reference planes' normal, then their target surfaces'
   common direction, then the vector between support box centres (a lone support uses
   its longest extent). X/Z measurements lie in front; Y measurements lie in right.
   Angular dimensions use the cross product of their first two surface directions to
   choose the end-on view of the angle; parallel or missing directions fall back to front.
   Geometric tolerances use the common cylinder axis, otherwise the support box's longest
   extent, end-on. Surface textures use their surface's common direction edge-on
   (X/Z → front, Y → right), otherwise the longest extent. External cylindrical sizes,
   threads and knurls use front, as in
   `dims::turned::diameter_views` (X/Z in profile, Y end-on). Internal threads use their
   cylinder axis end-on, like the hole callout carrying them; other manufacturing supports
   use the longest extent end-on.
   A common direction means every supporting surface selects the same dominant axis.
   For oblique directions choose the largest absolute component; component/extent ties
   choose X, then Y, then Z. This is a deterministic principal-view approximation:
   Specify does not reproduce Draftwright's trimmed-face witness proofs, feature-callout
   merging, omitted-view substitutions, sections, or user-selected sheet layout.
4. Position is the centre of the union of the characteristic's target face bounds.
   A location uses its target's position; its reference selects the measuring view,
   so size, location and control on one feature can share a site. Within each view use
   **row-major zones**, top to bottom, then left to right. Round projected coordinates
   to 0.001 mm (ties to even) so numerical noise does not split a drawing row. Rows are
   easier to walk across repeated holes than a polar/clockwise sort with an arbitrary centre.
5. At an equal projected position: size before location before geometric tolerance,
   then surface/manufacturing characteristics. Remaining ties retain the existing
   semantic model order (datums, dimensions, tolerances, textures, threads, knurls).
   Thus the order is total and deterministic, including coincident requirements.
6. Requirements without finite target face bounds go last in the same tie order.
   This includes a part default finish and whole-part controls. Face textures, threads
   and knurls participate by position; they are not collected at the end by category.

Only a first allocation with `high_water == 0` uses this sort. Subsequent writes match all
existing requirements first, then append unmatched ones in the existing semantic model
order above the counter. Read APIs retain their semantic array order for compatibility;
CSV/drawing consumers use the stored `number` to walk the inspection sequence. A different
sheet layout never changes those file numbers.

Numbers are read from the actual STEP entities before planning a rewrite. Identity uses
requirement kind/type and geometric face support (including the reference support of a
location), never recognition feature IDs or entity IDs. Unchanged coexisting requirements match first using an in-memory value comparison,
so an added control on the same face cannot steal an existing control's number. This is a
tie breaker, never persisted identity. A uniquely matched requirement can change its value,
material modifier, zone or notation and retain its number. File scenes validate each actual
`ID_ATTRIBUTE` against its structural history key, so even direct STEP edits keep numbers. Unique support growth follows the same conservative geometric-support rule as
answer resume: this accommodates quiddity adding defining faces after an upgrade. Ambiguous
or missing correspondence is not guessed; those numbers are retired, while the high-water
mark survives.

`revision::diff` uses its existing quiddity fingerprints/face correspondence to map the old
numbered supports to the revised part. Its returned `answers` (including CLI `answers.json`
and WASM `diff.report.answers`) contains an opaque `_specify_characteristic_numbers` member
when the old file has number history. **Keep that member with the returned answers** when
applying/writing the revision. The ordinary writer verifies its target geometry fingerprint,
uses the carried history and consumes the member; it does not store it among user answers.
The next resume reads the freshly written entity numbers and current high-water mark.
Once the destination has its own history, that history takes precedence over stale revision
transport, so reusing external answers cannot reset its counter or resurrect retired numbers.
No separate number-matching operation is needed in a drawing or CSV consumer.

An Add-mode write preserves imported PMI byte for byte. Existing numeric characteristic ids
reserve their numbers; newly written characteristics receive higher ones. A foreign datum
that haecceity reuses is not written or numbered again. Material, general-tolerance classes,
text notes and caller metadata are settings rather than individual inspection characteristics.

## Read APIs and compatibility

- `characteristics::read(&LoadedPart)` returns numbered requirements, each with `number`,
  `entity` (the exact numeric-id carrier), `kind`, `type`, `faces`, and optional `reference`.
  `characteristics::number(&Document, entity)` reads that entity's numeric id directly.
- `api::analyse` adds `characteristics` only when the file has numbered requirements. It is
  available even on a resumed file, where interview `existing` remains empty.
- `existing::read_existing` / `Reading::existing` add `ExistingPmi.number: Option<u64>`;
  JSON omits `number` when absent. Numbered thread/knurl/finish rows are kept distinct instead
  of aggregating different requirements of the same kind. A default part texture is in the
  authoritative `characteristics` list even though it has no face-note row.
- WASM `IntentEngine.snapshot().analysis` exposes the same list, and `writePmi` and `diff`
  use the same writer and carry path. JavaScript signatures are unchanged.

For #95, history schema 2 replaces the value-sensitive `specification` field with a
structural `role` (size/location, feature/dimension/relation/whole part, or datum). Schema 1
is still read: its obsolete specification is discarded, and its absent role permits the
existing kind/type/support match. Actual entity numbers and the high-water mark survive.
The next rewrite or revision carry emits schema 2; a byte-preserving no-op may retain schema 1.
Older engines that only accept schema 1 cannot rewrite schema-2 histories.

Planner dimensions retain only their measurement/axis and structural pattern role after
`|planner:`. Upstream generated IDs are not a storage format: stored IDs can include
nominals and Debug tolerance text. Earlier unreleased schema-2 roles containing those
IDs, and roles without a suffix, still use the conservative unique-support fallback;
linear locations also retain main's reversed-support matching during planner upgrades.
The `v0.1.6` fixture `characteristic-numbers-016.step.gz` contains authentic schema-1
Debug fit/qualifier/pattern signatures. Its upgrade test rewrites all six requirements,
retains every number and the high-water mark, and checks a second write is unchanged.

For #96, the LM10UU regression pins the three independently sized Ø19 datum constituents.
Their identities use leaf face supports `[0]`, `[4]`, `[8]`, `Size_Diameter` and role `size`;
no shape-aspect entity id or composition depth enters the key. Removing
`writer::datum_supports` with haecceity 0.4.3 therefore needs no history migration when its
native reader returns those same leaf supports. Remove the raw-reader guard with the
adjustment, and retain the number/resave/edit regression. If the native lowering changes
supports or measurement roles instead, preserve or explicitly migrate those keys before
removing the adjustment; never map an individual land to the whole datum.

Files without numeric characteristic ids have no extra analysis or PMI JSON fields. Their
legacy serialization and note aggregation/order are unchanged. No write adds a history
property to a part that has never allocated a characteristic number. No-op resumed writes
retain their original bytes once their dimensions carry the current origin attributes; lossless Add keeps every original entity record unchanged.

## General ISO 2768 rows

General size and location dimensions that Specify writes as nominal-only PMI (#63) **are
numbered**. They have individual STEP entities, and inspection consumers need the same
stable identity as for explicitly toleranced dimensions. Their inferred limits remain
governed by the part's general tolerance. They participate in the same allocation, carry,
and retirement rules; adding a general dimension never renumbers an existing one.

A general CSV row with **no individual STEP entity** remains unnumbered. Leave its number
cell empty or explicitly mark it unnumbered; the general class alone is a part setting and
gets no number. Specify does not create an entity solely to hold a number. The interview's
`existing` list still omits Specify's nominal-only dimensions (#63), while the authoritative
`characteristics` list exposes their numbers and supports to native and WASM consumers.

## Verification and upstream boundary

The writer verifies both semantic PMI and the numbers read back from their actual entities,
including number uniqueness and the history. Schema and EXPRESS-rule tests cover the standard
slots. First-allocation tests pin every number and geometric support on the prismatic plate
(14 characteristics), turned pin (12), and GRM-02 shaft (15), with each station explained
in the test. They also prove that a later top-left hole appends despite sorting first on
a fresh part. `characteristic-numbers-category.step.gz`, captured from unmodified
`68f375c` with the test's `intent()`, pins the old category order through the origin-metadata upgrade, a subsequent no-op,
and a changed-value rewrite with an addition. `tests/characteristic_numbers.rs` covers
all characteristic kinds, repeat writes,
additions, value edits, removal/re-addition/empty writes, foreign append, old-file compatibility,
an older-quiddity fixture and the `hole_removed` revision pair. Numberless analysis, PMI
serialization and settings-only writes are also checked against SHA-256 snapshots captured
from the unmodified main `a49dc3d` (quiddity 0.2.4 and #63) in a separate checkout and target directory.
These hashes run on macOS, where captured; portable absence assertions run everywhere.
For quiddity-rust#137 (v0.2.6), only settings-only writer hashes were regenerated: they gain
General notes presentation. Analysis and read hashes remain unchanged. The cross-engine
suite's v0.2.6 specimens and independent Python readbacks include characteristic numbers,
callouts, surface symbols, notes and new aspect shapes; the writer check requires exact bytes. The older-engine test adds
only `ID_ATTRIBUTE` records to the committed alpha.6 specimen; it never regenerates its old
PMI with the new writer. Requirements newer rules stop emitting are retired.

`tests/kernel_upgrade.rs` still compares every written PMI field under rigid motion.
Its two initially numberless shaft poses now pin their first-allocation numbers separately:
a rotated stored frame can change principal-view positions. Only those expected numbers
change; the semantic and source-face comparisons remain intact. Already numbered files
retain their numbers through the ordinary correspondence/carry path.

The WASM M2 tap byte pin is `tests/fixtures/thread-depth-numbered.step.gz`. The original
`thread-depth-alpha7.step.gz` remains unchanged. The native `thread_depth_help` test checks
every byte of the numbered pin against the writer. For quiddity-rust#137 this pin was
regenerated with presentation and new aspect shapes. Every legacy semantic readback field
and part setting still matches alpha.7 after projecting out characteristic numbers; the
original alpha.7 input is never regenerated. For #95, only the numbered pin's history
payload was migrated to schema 2; every other STEP byte was checked unchanged.

No haecceity API change is required: Specify uses its existing PMI/AttributeSet writer and
lossless `Document::apply`/`Edit` APIs. The v0.2.6 reader exposes identifiers alongside semantic roots (#135), including thread/knurl
definition shapes. Specify still reads numeric identifiers directly from the exact semantic
entity, preserving its strict positive-decimal and ambiguity rules. Tests cross-check the new
reader identifiers against Specify's numbers on the separate datum/size, composite and
location aspects introduced by quiddity-rust#137.

For #77, per-dimension records also expose optional stored `origin` and `source`.
See [dimension origin and source](pmi-origin.md) for the UDA and geometric source grammar.

Scene labels for a characteristic that appears in GENERAL NOTES use
`specify[id].notation`, the requirement's semantic wording. Presentation graphics
can be shared across requirements: their titles, text cells and graphic ids identify
the complete render-once block, not an individual characteristic. Number allocation
and characteristic identities continue to use semantic roots and supports.
