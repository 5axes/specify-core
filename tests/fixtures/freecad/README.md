# FreeCAD AP214 PRODUCT_TYPE regression (#89)

`hex-standoff-m3.step.gz` is a reproducible gzip of the unmodified M3 × 12 mm
male/female hex standoff by **nicoguaro**, licensed under **CC-BY-3.0** (see
`LICENSE-CC-BY-3.0.txt`). No geometry or STEP records have been changed.

Source: [FreeCAD parts library, commit f5ca01c](https://github.com/FreeCAD/FreeCAD-library/blob/f5ca01cdcaa2f69b9295a6382719c2d90628780c/Mechanical%20Parts/Fasteners/Standoff/Hex-Standoff-M3-12mm-Male-Female.step),
originally added in `01b153366d` (2015-09-23). Copied from dw-examples'
`steps/freecad/hex-standoff-m3.step`, whose README records that provenance.
Uncompressed SHA-256: `2cab3c2ecf765ccbebd64363e76dd80420043d9887483e8db494c10ad1f731bd`.

This small real FreeCAD export contains `#769 = PRODUCT_TYPE('part',$,(#7));`.
The CLI regression checks analysis, accepted-default writing, AP242 conversion,
unchanged bytes of every original entity except the keyword, and PMI/answer readback.

`bearing-lm10uu.step.gz` is the unmodified LM10UU linear bearing by **Francisco**,
also CC-BY-3.0, from [the same library revision](https://github.com/FreeCAD/FreeCAD-library/blob/f5ca01cdcaa2f69b9295a6382719c2d90628780c/Mechanical%20Parts/Bearings/linear_bearings/LM10uu.step).
Added in `7f6c942480` (2015-03-19); copied from dw-examples `steps/freecad/`.
Uncompressed SHA-256: `e37e54ea1d82012caf60b181a140e8ae30be707473f438b937729d9edd2f536d`.
Its three separate Ø19 lands share datum A. The regression checks that the
0.4.2 datum-constituent reader does not merge their independent size supports.
