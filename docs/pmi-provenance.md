# PMI written by Specify

For #22, the exact, case-sensitive authorship token is `specify-core:written:v1`
(`writer::PMI_PROVENANCE`). It identifies Specify as the writer of a requirement, not its
original designer, the source of its nominal geometry, or a security/authenticity claim.
Unmarked PMI is of unknown authorship. Do not infer authorship from a shared face, datum,
part, file name or the presence of stored interview answers.

## STEP-only readback contract

The writer uses free standard AP242 fields and `DESCRIPTION_ATTRIBUTE(description,
described_item)`. In the table, **D(X)** means exactly
`DESCRIPTION_ATTRIBUTE('specify-core:written:v1', #X)`: find this inverse reference in the
same `p21::Document`, checking both fields, not an arbitrary string in the consumed graph.
Slots below are zero-based Part 21 attribute positions.

| PMI kind | Exact STEP marker location | quiddity alpha.8 access |
|---|---|---|
| Size / location dimension (including standalone size) | D(its `SHAPE_DIMENSION_REPRESENTATION`) | `provenance.parts[p].dimensions[i]` contains the representation; raw Document + inverse references |
| Geometric tolerance, including complex instances | `GEOMETRIC_TOLERANCE.description` = token | Semantic `parts[p].tolerances[i].description` |
| Datum | Inherited `SHAPE_ASPECT.description` = token (`DATUM` slot 1); datum identification is unchanged | `provenance.parts[p].datums[i]` + raw root slot |
| Thread | Inherited `CHARACTERIZED_OBJECT.description` = token (`THREAD` slot 1) | `provenance.parts[p].threads[i]` + raw root slot |
| Knurl | Inherited `CHARACTERIZED_OBJECT.name` = token (`TURNED_KNURL` slot 0); description retains the pattern | `provenance.parts[p].knurls[i]` + raw root slot |
| General tolerance | D(its `PROPERTY_DEFINITION_REPRESENTATION`, whose slot 0 references `PROPERTY_DEFINITION('default tolerances', '', ...)`) | `provenance.parts[p].general[i]` + raw Document / inverse references |
| Face / default surface finish (texture) | D(its root `PROPERTY_DEFINITION_REPRESENTATION`, whose slot 0 references `PROPERTY_DEFINITION('surface texture', '', ...)`) | `provenance.parts[p].surface_textures[i]` + raw Document / inverse references |
| Part notes, coating, heat treatment, edges; thread manufacturing; caller UDAs (including carried payloads and the `specify-core original` record); stored answers | D(each set's `PROPERTY_DEFINITION_REPRESENTATION`, whose slot 0 references that set's `PROPERTY_DEFINITION`) | `provenance.parts[p].attributes[i]` + raw Document / inverse references |
| Material | D(its `PROPERTY_DEFINITION_REPRESENTATION`, whose slot 0 references `PROPERTY_DEFINITION('material property', 'material name', ...)`) | `provenance.parts[p].material` (one ID list) + raw Document / inverse references |
| New ordinary feature / datum feature | Empty inherited `SHAPE_ASPECT.description` becomes the token (slot 1) | `provenance.parts[p].features[i]` + raw root slot |

For property-based requirements, check the root property definition belongs to that item's
ID list as well as the marked link. Never transfer authorship from a shared feature or
referenced datum. Thread / knurl parameter helpers `PROPERTY_DEFINITION('', '', #THREAD_OR_KNURL)`
and surface-condition parameter properties remain unmarked; their authorship follows
their marked thread / knurl / root texture, respectively. Links, parameter values, presentation and
geometry do not independently express manufacturing requirements.

All fixed CAx-IF / NIST / haecceity vocabulary is retained. In particular, every UDA keeps
`property_definition.description = 'user defined attribute'`; `surface_condition` keeps
its empty description. No item, note or attribute set is added for provenance, so the UDA
reader, Python readbacks and OpenCascade metadata see the same semantic values and counts.
Material's property description and representation name both stay `material name`.

Dimensions have no suitable free description. A description on their representation is
standard (`description_attribute_select` includes `representation`) and leaves identifiers,
kind, nominal, tolerance and tolerance principle untouched. The pinned schema table lists
`dimensional_size` UR1, WR1 and WR2 in both editions: ISO 10303-47 UR1 is uniqueness of
`(id, applies_to)`, and WR2 allows at most one `id_attribute`. The alpha.8 `express_rules`
checker does not implement these dimension rules. Provenance uses no identifier slot.
For #65, each size now has a distinct numeric `ID_ATTRIBUTE`; two sizes sharing an aspect
retain separate numbers and description-based provenance. The regression explicitly checks
that the two numeric identifiers differ. See [characteristic numbers](characteristic-numbers.md). `description_attribute_select` also explicitly
includes `property_definition_representation`, permitting UDA annotation without changing
its reserved description or visible items.

Only newly emitted records are annotated. Reused input features/datums and all imported
requirements remain unchanged, also in assemblies or on a shared face. Each new annotation
is a separate standard description, never an appended suffix or a duplicate on a retained
record.

## Pinned reader limitations for draftwright-rust#102

Checked locally against quiddity `v0.1.0-alpha.8`, commit
`c62081c6d999683a12bbe7091adae413a61a126c`, in
`crates/haecceity/src/{express_table.rs,express_rules.rs,pmi/model.rs,pmi/mod.rs,pmi/read.rs,pmi/write.rs}`.
Quiddity is unchanged. **Only geometric tolerances expose the marker in the semantic model.**
Dimensions, datums, threads, knurls, general tolerances, textures, UDAs (including stored
answers and part notes), material and features require the index-aligned `PmiRead.provenance`
and raw `Document` access listed above. Alpha.8 does **not** consume `DESCRIPTION_ATTRIBUTE`
into the per-item ID lists or expose it as semantic metadata: the consumer must find the
inverse descriptions of the representation/link IDs. An integration restricted to
`PartPmi` alone cannot attribute those kinds. This is an explicit limitation for the paired
draftwright work, not a dependency on changing quiddity.

`tests/pmi_provenance.rs` independently follows these paths through the pinned reader,
checks unchanged UDA read results / findings, fixed helper vocabulary, EXPRESS validity,
repeat writes, and byte preservation of imported AP242 PMI. GRM-02 uses its committed shaft
fixture if absent from the corpus; its defaults do not exercise explicit thread answers.
GRM-03 exercises explicit M2/M3 threads and a straight knurl when a corpus is available;
an exported corpus missing that file is a test failure.

## Specify's existing-PMI path and rewriting

Previously `existing::read_existing` reported Specify-written PMI like imported PMI;
stored matching answers were the writer's only ownership signal. Older unmarked output
still follows that legacy behaviour. Now marked `ExistingPmi` rows additionally serialize
`"provenance":"specify-core:written:v1"`; unmarked rows omit it. Notes aggregated by kind
and faces retain separate groups for marked and unmarked requirements.

Material, general tolerance and part settings retain their existing value shapes.
`existing::part_settings_provenance` (or `Reading::settings_provenance`) reports authorship
separately: scalar keys `material`, `general_tolerance`, `surface_finish`, `coating`,
`heat_treatment`, `edges` map to the token only when the selected value is marked. `notes`
is an array aligned with the ordered settings notes, with tokens for marked notes and null
for unmarked notes; the key is omitted if none is marked. Selection order and assembly
settings restrictions match `part_settings`. Face textures are attributed in existing note
rows. Stored answers remain available as UDAs through the raw readback contract above.
The interview's resume path still uses answers and returns its usual empty `existing` list.

Writing modes retain their contract: matching stored answers permit replacement; otherwise
PMI is added alongside input PMI. The marker alone grants no deletion permission. Before a
replace, Specify removes only its own description annotations of the selected parts'
consumed records, because alpha.8's removal planner does not recognise those annotations.
Foreign annotations retain the planner's normal refusal behaviour. All other parts remain
unchanged. Unchanged resumed output retains its bytes only after its semantic requirements
carry the markers; legacy unmarked output gains provenance on its next write, and the next
unchanged write is byte-identical. Empty additive edits retain every original byte.

For the shared dimension planner (#75), `COMPOSITE_SHAPE_ASPECT.description`
uses the same marker as an ordinary feature. A `COMPOSITE_GROUP_SHAPE_ASPECT`
keeps its semantic description (`pattern of features` or `multiple elements`);
its otherwise empty **name** carries `specify-core:written:v1`. This lets the
no-op resume check recognize the writer's group without changing its meaning.
Planner-created supplemental `CARTESIAN_POINT`s carry the marker in their own
name. The no-op check requires that marker on every supplemental point; unrelated
unmarked geometry does not acquire authorship from a linked dimension.
