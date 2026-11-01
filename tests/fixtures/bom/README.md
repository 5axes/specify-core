# BOM acceptance inputs

`gramil.step.gz` is a deterministic gzip copy of the supplied
`dw-examples/steps/gramil/gramil-assembly.step` (2026-10-31 snapshot). The geometry
is by pzfreo/gramil, Open Community License v1; see the gramil source repository's
LICENSE. Its twelve distinct parts include four retainers and two blades. There
is no channel spacer: the source defaults to zero spacer thickness.

`gramil-spacer.step.gz` was generated in the supplied build123d/OCP scratch venv:

```python
from build123d import import_step, export_step, Compound, Box, Pos
original = import_step('gramil-assembly.step')
spacer = Pos(90, 0, 0) * Box(1., 3.6, 23.)
spacer.label = 'channel spacer'
variant = Compound(children=[original, spacer])
variant.label = 'gramil spacer variant'
export_step(variant, 'gramil-spacer.step')
```

The shim uses gramil's `build_channel_spacer` construction and measured blade
width/length; its displaced placement makes this an inspection fixture, not an
updated manufacturing assembly. Both files are compressed with `gzip.compress`
using `mtime=0`. Tests write fresh BOM data, never saved output snapshots.
