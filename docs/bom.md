# Assembly BOM (For #79)

The STEP file is the authority for the drawing pack, assembly parts list and CSV.
`specify-core bom assembly.step` returns a JSON array, one row per distinct geometric
part definition in the loader's flat assembly parts list. Instances of the same part
share a row. Nested assemblies are flattened, using the loader's placed leaf count.
Part indexes select the current file's parts for `analyse --part`; they are not identities.

## Fields and lossless storage decisions

The mapping was checked against the [PDM Schema Usage Guide 4.3](https://www.mbx-if.org/home/wp-content/uploads/2024/05/pdmug_release4_3.pdf)
and [CAx-IF UDA practice 1.8](https://www.mbx-if.org/home/wp-content/uploads/2024/05/rec_prac_user_def_attributes_v18.pdf).
The writer cannot change existing entity records to populate an explicit slot or
change an entity's type. Exported data remains available byte for byte; an explicit
answer takes precedence in Specify's BOM read.

| Field | Native home / read | Append decision |
| --- | --- | --- |
| Part number, name | `PRODUCT.id`, `.name` (PDM §1.1.1.1) | Explicit overrides use `specify bom` UDA items `part_number`, `name`. No duplicate product chain. |
| Description | `PRODUCT.description`, then `PRODUCT_DEFINITION.description` (PDM §1.1.1) | `description` UDA override; no record replacement. |
| Revision | `PRODUCT_DEFINITION_FORMATION.id` (PDM §1.1.1.2) | `revision` UDA override. |
| Make/buy | `PRODUCT_DEFINITION_FORMATION_WITH_SPECIFIED_SOURCE.make_or_buy` | `make_buy` UDA with `made`, `bought`, `not known`. PDM §1.1.1.3 itself cautions about supply-chain ambiguity; do not change the formation subtype. |
| Standard-part category | `PRODUCT_RELATED_PRODUCT_CATEGORY` (PDM §2.1.1) | Preserve categories. A bought answer does not assert that bespoke hardware belongs to an ISO class. |
| Material | Existing haecceity material reader/writer, `part.material` | Reuse `material property` / `material name` and density units. No second BOM material writer. |
| Quantity | Placed occurrences, including parent multiplicity | Do not add `QUANTIFIED_ASSEMBLY_COMPONENT_USAGE` alongside individually placed instances (PDM §4.1.2/§4.1.3). |
| Find number | Numeric NAUO name where unambiguous, or named UDA | Append integer `item_number` UDA. PDM §4.1.2 describes find numbers; §4.7.1 records the competing NAUO.name convention. `reference_designator` identifies occurrence locations and is not silently repurposed. |
| Optional, stock | No unambiguous native slot | Boolean `optional`, text `stock` (`from stock`, `to be made`) UDAs. Optional denotes quantity 0–1; writing it requires one placed occurrence. |

Material uses haecceity's existing Material Identification and Density R2.1
§4.1/§4.2 mapping. In UDA 1.8, Annex C.1 defines SI base units; density's derived
unit example is C.4.2. There is no new density inference.

`specify bom` is a property set attached to each immediate leaf NAUO using UDA
§5/§6.2: `PROPERTY_DEFINITION`, `GENERAL_PROPERTY_ASSOCIATION`,
`GENERAL_PROPERTY`, `PROPERTY_DEFINITION_REPRESENTATION`, `REPRESENTATION` and
text/integer/boolean representation items (§7.1/§7.2). Repeated occurrences receive the
same parts-list row. Wrappers are resolved through the loader's part definitions.
The representation has its own nongeometric context. On BOM-enabled writes, newly
emitted Specify saved-answer and characteristic-history properties are associated with
the part's definition shape, which the PMI reader treats as the same whole-part owner.
Input instances are never moved. This avoids OCCT `ReadMaterials` interpreting every
descriptive property on a product definition as a material (regardless of its name).
Assembly allocation history also uses the definition shape, avoiding a spurious material
on the assembly. The schema permits property definitions as property owners. The chunked
history avoids OpenCascade's scanner buffer limit on long STEP string tokens. These UDAs are readable without
Specify's saved interview payload. Other CAD programs must understand the documented
UDA names to interpret them; XCAF is only expected to expose the native material/name.

## JSON and APIs

Every BOM value is an object with `value` and `source`. `source` is `file` for
pre-existing export data, `answered` for explicit BOM answers or saved material
answers, and `default` for suggestions and automatic item allocation. Unknown values
are JSON null. Unnumbered files are not numbered merely by reading.

```json
[{
  "part": 0,
  "item_number": {"value": 1, "source": "default"},
  "part_number": {"value": "GRM-01", "source": "answered"},
  "name": {"value": "GRM-01 shank", "source": "file"},
  "description": {"value": "Brass shank", "source": "answered"},
  "revision": {"value": null, "source": "default"},
  "make_buy": {"value": "made", "source": "answered"},
  "material": {"value": "Brass", "source": "answered"},
  "quantity": {"value": 1, "source": "file"},
  "optional": {"value": false, "source": "answered"},
  "stock": {"value": "to be made", "source": "answered"}
}]
```

- Rust: `specify_core::bom::{read(&File), read_path(&Path), read_bytes(Vec<u8>)}`,
  returning `Vec<Item>`. `Item` and `Field` support serde.
- CLI: `specify-core bom assembly.step` (also accepts gzip through the normal loader).
- Python: `specify_core.bom(path)` or `specify_core.api.bom(path)`.
- WASM: `JSON.parse(IntentEngine.bom(stepBytes))`. The STEP size limit also applies.
- Assembly analysis adds `bom`, that part's row, even before writing is requested.
  This Rust-engine extension lets the editor show optional questions from current file data;
  reading it never accepts suggestions or allocates numbers. The archived Python oracle
  predates BOM support: exact per-part departures are recorded in
  [the analysis ledger](../tests/fixtures/known_differences/analysis.json). Existing `parts`, `analyse --part`
  and multi-part `write` workflows continue to select the parts being answered.
  Analysis derives only the selected row from the existing parsed document and PMI
  reading, rather than loading and reading the whole BOM again. A resumed part reads
  only its own PMI against that same parse; no geometry is loaded for BOM metadata.

## Opt-in questions and writes

Questions are `bom.make_buy`, `bom.description`, `bom.part_number`, `bom.revision`,
`bom.optional`, `bom.stock`. They have `optional: true`, a null `default`, and a
`suggestions` array with the proposed value and a `reason`. Existing part numbers and
revisions are not asked again. BOM questions never block a write and are excluded from
`--accept-defaults` / `with_all_defaults`. Material remains `part.material`.

A write opts in when its intent contains at least one explicit `bom.*` answer, or when
BOM suggestions are explicitly accepted (using a current assembly analysis):

- CLI: `specify-core write asm.step analysis.json answers.json -o out.step --bom`
  (one analysis/answers pair per part to include; `--accept-defaults` independently accepts PMI).
- Rust: apply/preview normally, then `bom::accept_suggestions(&analysis, &answers, &mut intent)`.
- Python: `api.apply(analysis, answers, accept_defaults=True, bom=True)`.
- WASM: `session.writePmi(answersJson, project, payload, true)`; omitted/false fourth argument
  keeps the existing behavior. Native session API: `write_pmi_with_bom(..., true)`.

Without opt-in, neither single parts nor assemblies receive new BOM records, history,
numbers or owner changes; the ordinary PMI write bytes remain unchanged. Reading never
allocates numbers. Once an assembly opts in, all its parts receive numbers; only the
selected parts' answered or explicitly accepted fields are stored.

Hardware is proposed bought by whole tokens and the last noun, ignoring size suffixes:
screws, washers, dowels, bearings, nuts, bolts, rivets, circlips, pins, studs, fasteners.
A project drawing number such as `GRM-03` or recognised pockets/keyways strongly suggests
made, including bespoke screws. `bearing housing`, `leadscrew`, `minute` suggest made.
Descriptions suggest the display name and optional suggests no. Stock follows make/buy: a
bought part is proposed from stock and a made part to be made; `not known` proposes no stock.
When suggestions are accepted, an explicit `bom.make_buy` answer decides the stock proposal.

Only explicit BOM answers have source `answered`. Accepted suggestions have source
`default`, marked by boolean `default.<field>` UDA items. Empty text is never stored.
Current file data always wins over suggestions, including after re-export; explicit
overrides alone win over native data. Foreign attribute keys are case-insensitive;
known enum values are normalised, invalid/blank values ignored. No invalid value becomes
a proposal. BOM answers live exclusively in the `specify bom` occurrence property sets,
**outside the ordinary saved-answers payload**, so older engines ignore them and resume
non-BOM answers. New engines merge explicit BOM answers into resume results.

## Item allocation and revisions

An assembly-level `specify bom item numbers` property set is attached to its
non-leaf product definition shape (a whole-assembly bookkeeping property). Ordered text
items `history.000001`, `history.000002`, … each hold up to 2,048 characters. Concatenate
them to obtain compact JSON:
`{"schema":1,"high_water":12,"entries":[{"number":1,"name":"…","geometry":{…}}]}`.
`geometry` is quiddity's versioned recognition fingerprint, including faces/features.
An entry also carries `fields`, a snapshot of the answered BOM UDA items, so a renamed
part can retain its BOM data through geometric matching. An optional `material` snapshot
holds an answered material for assembly revision carry; the revision API restores it as
`part.material` only when the new export lacks a material, using the existing writer.
Occurrence UDAs remain the
public read contract. Snapshots append; the last valid snapshot is current, and counters cannot decrease.
Corrupt histories refuse the write. Numbers start at 1; every new number is above the
high-water mark. Removed entries disappear from the current snapshot, retiring their
numbers permanently. Reintroducing a removed part receives a new number.

A current file's stored item number is matched first through quiddity correspondence,
with the same geometric tolerances used by revision matching. Exact geometry follows,
then tolerant correspondence (a majority of carried/adapted faces, including one- and
two-face parts). Identical duplicate parts retain stored identities first, then matching
names and stored/occurrence order; they do not acquire new numbers due to ambiguity.
STEP entity IDs never establish identity. A fresh CAD export with no transported history
has no prior allocation to preserve and starts at 1 (unless native find numbers exist).
`revision::diff` carries the history in opaque answer member `_specify_bom`; retain it
alongside `_specify_characteristic_numbers`. Destination history wins over transport.
For assembly changes, `revision::assembly::diff` exposes `bom_history` when present,
its optional per-part `bom_answers` supplies geometrically carried BOM answers, and
`answers()` (CLI `answers.json`) includes the transport on every new part,
even when all parts have been renamed and none pairs by name for PMI comparison.
Keep the returned transport in answers passed to `write`, including subset writes.
BOM matching uses geometry independently of the existing assembly PMI name-pairing rule.

BOM updates append their complete current overrides and leave earlier UDA records
intact. Omitting a BOM answer preserves its earlier explicit value; use `no` or `not known`
to change enum values. Empty text is ignored. Only explicit fields, never suggestions,
are carried in history to later revisions. A write without BOM intent adds no BOM/history records and takes
the previous PMI path. An identical resumed write is byte-identical. The BOM append
verifies every original candidate record and reads back every field and history before
the normal writer's final PMI verification and atomic publication.

## Export survey

The supplied snapshot contains 46 drawing-example files and 100 pinned-corpus files.
The build123d/gramil, FreeCAD and text-to-cad exports generally provide product names
and category `part`, with empty descriptions, formation revisions and reference
designators. OCCT linkrods has description `void`. NIST files carry meaningful product
descriptions and specified-source formations (`made`/`not known`), plus validation/PMI
properties. A whitespace-only definition description is treated as absent.
No supplied file in the initial survey had a populated reference designator or
`MATERIAL_DESIGNATION`; NIST material-named validation properties were not assumed to
be actual material values. Exporter labels are taken from headers, not folder names.
No Onshape or Fusion header was found in these supplied snapshots; no inference is made about those exporters.
The gramil export's default spacer thickness is zero: the optional spacer is absent
from that input and must be added as an assembly variant to demonstrate 0–1 quantity.

For an independent XCAF check, run `tools/check_bom_xcaf.py written.step bom.json`
using an OCP environment. It asserts that the standard material assignments equal the
BOM's material values; Specify-only UDAs are checked by the Rust/WASM/Python BOM APIs.
