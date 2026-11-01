# Shared dimension requirements (#75)

Specify uses `quiddity::dimensioning` from quiddity/haecceity v0.4.1. The dependency's
`docs/dimensioning.md` defines the selection rules and slice boundary. Specify does
not select an independent set of holes, pitches or plane intervals.

A live analysis adds `dimensioning`, the shared `Input::from_recognition` result.
Rules supply the chosen datum face sets and answered/stored dimension contracts to
that input. Hole-position answers supply Basic contracts to the pattern's own
origin, pitches and bolt-circle diameter, rather than preserving the generated per-member witnesses.
A position frame makes the bolt-circle diameter Basic even when an axial datum
supplies no Cartesian plane-location witnesses.
Hole-only contract passes use the shared planner before the final consolidation;
this prevents a General neighbour swallowing a Basic pattern origin. No additional
basic dimensions are kept beyond the final returned set. Explicit face-picked and
stored dimensions enter `specified`; specified-wins replaces their generated
counterparts. Stored foreign PMI is already present and is not appended twice.
Coincident source authorities are recovered through the planner's own conveyances
before specified-wins runs. Their final shared dimension lowers to a real
`MultipleElements` aspect containing every covered origin, including the pattern
aspect itself. This also handles coincident basics on unpatterned holes; `covers`
is not merely stored as metadata. No extra Basic row is introduced.

Older JSON analyses have no typed geometry input. Their rule results remain
provisional, and the writer upgrades rule-generated intent using the loaded part.
Unmarked, hand-authored writer rows remain authoritative. Rule-produced General rows
carry a feature field (empty for planes), so general-only saved rule results also upgrade.
Invalid planner input is a write
failure, not a silently selected alternative planner.

Each independently written `Required` has one row with `planner` lowering information.
Requirements already carried by threads do not need separate dimensions; quantities
without physical supports or a reconciled contract are omitted or reported below. Sizes and
locations lower through the shared `dimension_kind` helper with a real feature
arena. Actual patterns use `PatternOfFeatures`, counted unpatterned groups use
`MultipleElements`. Locations require nonempty face supports at both ends.
The turned-envelope deferrals survive the hole-only passes; faceless box offsets
are not written. The writer also rejects faceless planned locations defensively. AP242 Table
4's `curve length` names axial length quantities (the helper's generic `length`
name is rejected by the kernel's own reader); angular quantities use degrees.
The exact role, count, pitch intervals, covered IDs, supports, frame and optional
reference proof round-trip in a dimension-owned `specify dimension plan` UDA.
Persistent planner metadata canonicalizes frame noise to nine decimal places;
generated PMI measurements retain Specify's six decimal places in millimetres.
References use the standard `DimensionModifier::Reference` (auxiliary dimension).
Dimension-owned metadata inherits #77's AP242 ed1-to-ed4 header upgrade when needed;
all original entity records remain byte-identical.
Scene IDs and characteristic matching retain the planner's face-derived role IDs,
so different quantities on one support remain distinct. Old numbers are matched
before new numbers are allocated, including reversed undirected linear spans.

The original path remains for slots, recesses/pockets, bosses/pads, turned-step
sizes and turned ladders, round/turned/whole-stock Cartesian envelopes, independent
through-step sections, plate thickness owners, chamfers, fillets and other
explicitly deferred or non-step planes. There is no new dimension-scheme handling.
Every empty-`by` conveyance is a later-slice ownership claim; it is not evidence
that the legacy dimension was conveyed by another dimension. Add-mode writing
retains foreign PMI once and adds the planner's missing slice-one quantities.
Later-slice General rows keep their existing Add-mode omission.
Hole-location contracts whose supports cannot be represented by slice one's principal-plane
grammar retain their existing writer path (the frozen cgb202 locations); faceless rows
remain Q17 diagnostics.

The default plate's raw all-General planner has 11 quantities. Specify writes 12
when position frames are accepted: two General centre-hole offsets (50 and 30)
cannot merge with Basic pattern origins, and the M5 thread already carries the
4.2 tapping drill. A thread's stated tapping drill diameter/depth never gets a
second inspection row. A generated bore depth with an unknown bottom is reported
as unresolved instead of being treated as an established blind drilling depth;
explicit stored depths remain authoritative. Answered bores whose legacy radius comparison left no
faces are reconciled by exact stable recognition identity, preserving H11 and
Basic contracts instead of generating General substitutes.

Bounded shoulder point stations require point/edge-specific semantic aspects that
Specify's writer does not yet encode. They are reported as unresolved; their
legacy physical-plane witnesses (within the planner's 0.001 mm station rounding)
keep the existing path. Writing two station values to the same face pair would
be ambiguous, so no arbitrary station is chosen.

Deferred legacy spans are dropped when the planner states the same physical
plane interval in either direction. Hole floors are excluded from part lengths,
including floors absent from legacy defining-face lists. Hole depths are published
to 0.01 mm upstream; floor ownership and reference proofs account for that rounding
while keeping the planner's nominal unchanged. When two retained
turned baselines determine a generated hole enlargement's depth, that depth
is retained as a reference with both baseline witnesses. This avoids introducing
an independent closed chain across the slice boundary. The same check makes a
generated plane span a reference when retained baselines already determine it.
A step proposal without a matching physical principal plane (including tilted
faces and inconsistent clustered stations) is reported and deferred; measured
legacy plane dimensions remain. Nonprincipal plane dimensions and oblique hole
locations retain the existing path.
Projected grid pitches resolve to aligned existing member-hole witnesses, so
orthogonal pitches never become two different values on one diagonal face pair.
Their shared requirement IDs, nominal values and interval counts are unchanged;
optional `witnesses` metadata records the physical PMI endpoints.
`tests/planner_dimensions.rs` checks complete STEP readback, reference modifiers,
stored-PMI authority, a pre-planner numbered plate resumed twice, and physical
supports, unique face pairs and independent plane chains across the test corpus.

The exact deferred-family predicate is every accepted defining family except
`holes`, `countersinks`, `step_levels`, `risers`, `through_steps` and `plates`, plus
the explicit `through_step section`, `plate thickness`, `turned profile`,
`turned envelope`, `polygonal_stock` and `non-step plane` ownership claims.
Derived pattern families have no defining claims of their own. This also retains
flats, grooves/annular and face grooves, blends, cylindrical profiles, freeform
surfaces, modelled threads, angled/paired/oblique/circular blind steps, oriented
chamfers, ribs, thin walls, interior voids, all slot/pocket/channel/notch/recess
variants, sheet-metal bodies, polygonal bosses/stock and raised pads. Schemes
continue on the existing path; Specify introduces no scheme interpretation here.
