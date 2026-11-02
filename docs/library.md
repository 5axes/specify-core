# The Rust library

Specify is also a Rust crate, `specify_core`. The command line, the service and the WebAssembly
module are built on it.

- `part::File::{read, of_bytes, load, load_all}` reads a file once and loads its parts;
  `part::load_bytes` loads a single-part STEP held in memory.
- `api::{analyse, preview, review}` analyses a loaded part, turns answers into its bound intent, and composes a front end review.
- `writer::{write, write_parts}` writes verified PMI and stored answers to a destination;
  `writer::{write_bytes, write_parts_bytes}` returns the same verified STEP bytes in memory.
- `writer::{write_bytes_with, write_parts_bytes_with}` adds `extra: &[haecceity::pmi::AttributeSet]`
  after the existing arguments. Extras are written through haecceity and verified in the same
  edit as the PMI and stored answers. `writer::{write_with, write_parts_with}` accepts the same
  extras for atomic file output, including `.gz` destinations. Existing APIs are unchanged and
  delegate with an empty slice, producing identical bytes.

For example, the editor can store its base64 project beside the answers:

```rust
use haecceity::pmi::{AttributeSet, AttributeValue};
use specify_core::writer;

let name = "draftwright editor project";
let extra = AttributeSet {
    name: name.into(),
    on: None, // the part
    items: vec![(name.into(), AttributeValue::Text(base64_project))],
    description: None,
    general_property: None,
};
let (bytes, report) = writer::write_bytes_with(&loaded, &intent, Some(&answers), &[extra])?;
```

Nonempty extras are written on the one part written: a file of one part, or one part of an
assembly (on that part's product definition, its other parts kept byte for byte); several parts
of an assembly written together with extras are refused. A set or item named `pmi-assist answers` is refused because that name
is reserved for resume. Names, owners and values pass through to haecceity; its read-only
`description` and `general_property` metadata follow its normal writer rules. Extras follow
the usual write mode: replacing a resumed part replaces its previous extras along with its PMI
and answers, so resupply every extra wanted in the new write. Adding to foreign PMI leaves
existing foreign attributes alone. Scoped Add-mode rewrites replace Specify-authored
extras together with its answers, so resupply those extras too. There is no separate merge or update by attribute name.

Characteristic numbers are read with `characteristics::read` and appear in analysis's optional
`characteristics` list and `ExistingPmi.number`. Writers allocate them from the file's history;
when writing a revision, retain the opaque number-history member in `revision::diff`'s returned
answers. See [the STEP storage and carry contract](characteristic-numbers.md).

Authored STEP names persist across code-first CAD rebuilds through the existing
resume and revision APIs; see [face names and schema compatibility](face-names.md).
