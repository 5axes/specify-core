# Free-text part notes

Maintainer decision, draftwright-rust #123: notes about making a part belong in the model as
PMI. Drawing-only notes stay in the drawing Spec. No existing drawing or legacy note text is
automatically migrated.

Rules version **3** adds `part.notes`, an ordered JSON array of free-text strings, beside
`part.surface_finish`, `part.coating`, `part.heat_treatment` and `part.edges`. The question's
kind is `text_list`, with default `[]`. Non-array answers and non-string members are refused
by both the rules and the direct intent writer. An empty list writes nothing; every member
of a nonempty list is written literally, including empty strings, whitespace, duplicates and
words such as `None`. The intent carries the list as `part.notes` (key `notes` within `part`).

Each note is a part-owned `AttributeSet` named `semantic text`, following the existing part
notes and PMI practice §7.4. It holds:

- `general note`: the text, as a `DESCRIPTIVE_REPRESENTATION_ITEM`.
- `note index`: its **one-based** position, as an `INTEGER_REPRESENTATION_ITEM`.

There was no existing list-order convention for part notes. The explicit index makes order
independent of entity numbers and physical file order. The reader sorts positive indices
ascending, preserving read order for ties. Unindexed notes (including nonpositive or
noninteger indices) follow indexed notes in read order. Indices need not be contiguous.
The same form from another tool is read without any Specify-specific stored answers.
Only `semantic text` sets owned by the part qualify; similarly named items in other sets or
on features do not become part notes. The part's owner is resolved semantically, including
for assemblies.

Example for two notes (`#10` is the part's `PRODUCT_DEFINITION_SHAPE`, `#20` its
representation context; illustrative entity numbers):

```step
#100=DESCRIPTIVE_REPRESENTATION_ITEM('general note','Inspect bore after coating');
#101=INTEGER_REPRESENTATION_ITEM('note index',1);
#102=REPRESENTATION('',(#100,#101),#20);
#103=PROPERTY_DEFINITION('semantic text','user defined attribute',#10);
#104=PROPERTY_DEFINITION_REPRESENTATION(#103,#102);
#105=GENERAL_PROPERTY('','semantic text',$);
#106=GENERAL_PROPERTY_ASSOCIATION('',$,#105,#103);
#110=DESCRIPTIVE_REPRESENTATION_ITEM('general note','Pack dry');
#111=INTEGER_REPRESENTATION_ITEM('note index',2);
#112=REPRESENTATION('',(#110,#111),#20);
#113=PROPERTY_DEFINITION('semantic text','user defined attribute',#10);
#114=PROPERTY_DEFINITION_REPRESENTATION(#113,#112);
#115=GENERAL_PROPERTY('','semantic text',$);
#116=GENERAL_PROPERTY_ASSOCIATION('',$,#115,#113);
```

`existing::part_settings` reports these as `notes` in order. With stored answers, opening the
file restores `resume.answers["part.notes"]`, editable and removable like the other part
notes; stored-answer schema remains 2. Without stored answers, analysis reports `part.notes`
as an already-stated requirement, following the other imported part notes. Specify's
existing add policy retains foreign PMI; it does not take ownership of it for editing.
Reapplying an identical foreign note list writes no duplicates. Adding a different nonempty
list is refused, since replacing foreign notes is outside add mode and restarting their
indices would make order ambiguous.

An unchanged resumed rewrite retains the original file bytes, including entity numbers and
datum presentation. The writer first builds and verifies the replacement candidate, then
keeps the original only when every written part's complete semantic PMI, caller attributes
and exact stored-answer payload already match it. Edits and removals use normal replacement.

`tests/part_notes.rs` checks literal text and ordering, write/read answer equality,
byte-stable rewrite, edits and removal, foreign attribute sets without a resume payload,
assembly ownership, invalid inputs and a live OpenCascade import when the Python venv is
available. The complete Python/Rust rules departures are pinned in
`tests/fixtures/rule_defaults_oracle.json.gz`; CLI envelopes have a separate exact snapshot.

With quiddity-rust v0.2.6 (#137), written notes appear in the `General notes` callout.
Blank and whitespace-only rows display `(blank)` to avoid an empty tessellated curve;
their semantic strings, note indices and saved answers retain the exact original text.


Specify's live `pmiScene` composes imported part notes and accepted settings into one
GENERAL NOTES graphic in #163 order. It replaces their file graphic only in the scene;
lossless append keeps all original STEP records. A file-only scene retains its original
callout geometry and `step:` identity, reports `source: "file"` even when the producer
marked it `haecceity.generated/1`, and resolves its material DMIA through part provenance.
Material is a scalar in `haecceity.pmi/1`, so Specify adds `part.material` as a companion
alias in `presentation.items`, `presentation.semantic_ids` and `specify`, without adding
or changing a semantic PMI row. Feature-owned notes keep separate feature callouts;
unpresented feature notes get the upstream generated fallback and stack below the combined
block with a gap of two text heights. Imported manufacturing
requirements of kind `semantic text` are rendered as free part notes. Explicit part-level
phrases such as “break sharp edges” answer `part.edges` using the original text; feature
notes and unrelated manufacturing prose never supply that part default.
