"""Write tests/fixtures/known_differences/existing.json:

    python3 tools/known_differences/existing.py tests/fixtures/known_differences/existing.json

then run the existing sweep (tests/existing.rs, `existing_and_part_settings_match_the_oracle`);
it must pass. Every entry is hand-judged against the file text and haecceity's findings (the
evidence names the entities); with SPECIFY_DUMP=DIR the sweep writes its differences to
DIR/existing.json in full, for judging a new one.
"""
import json, sys
E = []
def e(case, path, pattern, verdict, evidence):
    E.append({"case": case, "path": path, "pattern": pattern, "verdict": verdict, "evidence": evidence})
Q7 = "Question Q7: OpenCascade's XCAF makes a part of each shape representation of one product; haecceity has one part per product definition, so Python's parts 1 and 2 (one surface face each) do not exist in Rust."
for c in ["corpus/nist/nist_ctc_02_asme1_rc", "corpus/nist/nist_ctc_04_asme1_rd", "corpus/nist/nist_ctc_05_asme1_rd", "nist/nist_ctc_02_asme1_ap242", "nist/nist_ctc_04_asme1_ap242"]:
    e(c + "#*", "part", "Python's surface-model part does not exist in Rust", "not-applicable", Q7)
e("*", "existing[key=dimension|DimensionPresentation|*", "presentation read as a dimension", "rust-correct",
  "OpenCascade's reader turns a presentation-only callout (DRAUGHTING_CALLOUT / annotation occurrence with no semantic dimension; specify-core's own thread and knurl callouts, NIST's graphic PMI) into a 'DimensionPresentation' dimension of value 0.0. It states no size; haecceity reads semantic PMI only (every Python item of this type has value 0.0).")
e("nist/nist_ctc_01_asme1_ap242*", "existing[key=dimension|Location_None|[41]|[37]#1]", "a location dimension the file states without a value", "rust-correct",
  "#24=DIMENSIONAL_LOCATION('linear distance',$,#324,#325) has no dimensional_characteristic_representation (§5.2.1; haecceity reported it as a nonconformance until v0.4.7, which preserves it without a finding). Rust reports Location_LinearDistance, as named, with no value; OpenCascade falls back to its default type Location_None and invents 0.0.")
e("nist/nist_ctc_01_asme1_ap242*", "existing[key=dimension|Location_LinearDistance|[41]|[37]#1]", "the same dimension, as Rust reads it", "rust-correct", "See the Location_None entry for [41]/[37] (#24).")
e("nist/nist_ctc_01_asme1_ap242*", "existing[key=dimension|Location_None|[88]|[84]#1]", "a location dimension the file states without a value", "rust-correct",
  "#25=DIMENSIONAL_LOCATION('linear distance',$,#328,#329) has no dimensional_characteristic_representation: Rust reports Location_LinearDistance with no value; OpenCascade Location_None 0.0.")
e("nist/nist_ctc_01_asme1_ap242*", "existing[key=dimension|Location_LinearDistance|[88]|[84]#1]", "the same dimension, as Rust reads it", "rust-correct", "See the Location_None entry for [88]/[84] (#25).")
e("nist/nist_ctc_01_asme1_ap242*", "existing[key=dimension|Location_None|[*,*]|#1]", "a diameter the file states without a value", "rust-correct",
  "#126/#127=DIMENSIONAL_SIZE(..,'diameter') on datum features B and C have no dimensional_characteristic_representation: Rust reports a second Size_Diameter on [89,90] and [75,76] with no value; OpenCascade gives its default type Location_None and 0.0.")
e("nist/nist_ctc_01_asme1_ap242*", "existing[key=dimension|Size_Diameter|[*,*]|#2]", "the same diameters, as Rust reads them", "rust-correct", "See the Location_None entries for [89,90] and [75,76] (#126, #127).")
e("nist/nist_ctc_01_asme1_ap242*", "existing[key=dimension|Size_Diameter|[7*]|#1].*er", "limits read as deviations", "rust-correct",
  "The diameters on [71,72] and [73,74] (#84, #87) state limits 34.8/35.2 on 35: Rust reports them as their offsets from nominal, +0.2/-0.2; OpenCascade reads a range and specify-core reports no deviations.")
e("nist/nist_ctc_02_asme1_ap242#0", "existing[key=geometric_tolerance|*].datums[*]", "datum letters repeated by OpenCascade", "rust-correct",
  "Python lists [A, A, B, B, B, B, C]: OpenCascade builds a datum per datum target (A1-A3, B1-B4, C1) of each tolerance's datum system; the system is A|B|C, which Rust reports once each, primary first.")
e("nist/nist_ctc_02_asme1_ap242#0", "existing[key=datum|*|[]|#1]", "a datum established by targets alone, reported without faces", "rust-correct",
  "Datums A, B and C are established by point datum targets only (A1-A3, B1-B4, C1; no datum feature): haecceity reads them with no faces; OpenCascade's reader drops them although every tolerance cites them, so specify-core would offer A, B, C again.")
e("nist/nist_ctc_02_asme1_ap242#0", "existing[key=datum|D|[1]|#1]", "a datum no tolerance references", "rust-correct",
  "Datum D is in the file (DATUM 'D', datum feature on face 1); OpenCascade's reader builds datums only through the tolerances citing them, and none cites D.")
e("nist/nist_ctc_02_asme1_ap242#0", "existing[key=datum|K|[635]|#1]", "a datum on a surface model's face", "rust-correct",
  "Datum K is established by a circular datum target on face 635, a face of the surface model beside the solid (Q7), and no tolerance cites it; Python's part 0 has neither the face nor the datum.")
e("nist/nist_ctc_03_asme1_ap242*", "existing[key=datum|*|[]|#1]", "a datum OpenCascade gives no faces", "rust-correct",
  "Datums D and E: their datum_feature identifies no face and the datum's usage does (haecceity's nonconformance finding reads it as the datum feature's faces, §6.5.1); OpenCascade gives no faces and specify-core's text fallback finds none.")
e("nist/nist_ctc_03_asme1_ap242*", "existing[key=datum|D|[68]|#1]", "datum D with its faces", "rust-correct", "See the entry for datums without faces: D's usage names face 68.")
e("nist/nist_ctc_03_asme1_ap242*", "existing[key=datum|E|[110]|#1]", "datum E with its faces", "rust-correct", "See the entry for datums without faces: E's usage names face 110.")
e("nist/nist_ctc_03_asme1_ap242*", "existing[key=datum|F|[47,48]|#1]", "a datum no tolerance references", "rust-correct", "DATUM 'F' is in the file on faces 47 and 48; no tolerance cites it, so OpenCascade's reader builds no datum for it.")
e("nist/nist_ctc_03_asme1_ap242*", "existing[key=dimension|*].value", "inch values reported in millimetres", "rust-correct",
  "The file's length unit is the inch (CONVERSION_BASED_UNIT 'INCH'): Python reports the nominal as stated (0.75), Rust in millimetres (19.05), the unit specify-core works in; the deviations were already millimetres in both.")
e("nist/nist_ctc_03_asme1_ap242*", "existing[key=dimension|Location_None|[40,41]|#1]", "a diameter the file states without a value", "rust-correct", "A second 'diameter' dimensional_size on [40,41] with no dimensional_characteristic_representation: Rust Size_Diameter without a value; OpenCascade Location_None 0.0.")
e("nist/nist_ctc_03_asme1_ap242*", "existing[key=dimension|Size_Diameter|[40,41]|#2]", "the same diameter, as Rust reads it", "rust-correct", "See the Location_None entry for [40,41].")
e("nist/nist_ctc_03_asme1_ap242*", "existing[key=geometric_tolerance|Flatness|[110]|#1]", "a flatness with a unit basis OpenCascade drops", "rust-correct",
  "#37 is a complex FLATNESS_TOLERANCE + GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT (0.127 per rectangular unit area): haecceity reads it with its unit basis; OpenCascade's reader loses the whole tolerance.")
e("nist/nist_ctc_04_asme1_ap242#0", "existing[key=*|[]|#*]", "PMI on a feature naming the whole solid", "rust-correct",
  "Two diameters (12, 20±0.3) and a position (0.75 |A|B|C) apply to a shape aspect whose item is the MANIFOLD_SOLID_BREP (haecceity: unresolved-reference finding): Rust reports them with no faces; Python, whose XCAF split makes this part one of several, drops PMI with no faces as another part's (existing._elsewhere).")
e("nist/nist_ctc_04_asme1_ap242#0", "existing[key=geometric_tolerance|ProfileOfSurface|[483]|#1]", "a tolerance on a surface model's face", "not-applicable", Q7 + " Face 483 is the surface model's face, which Python's part 0 lacks.")
e("nist/nist_ctc_05_asme1_ap242", "existing[key=dimension|*].value", "inch values reported in millimetres", "rust-correct",
  "The file's length unit is the inch: Python reports 5.0, Rust 127.0 mm (5 x 25.4).")
e("parts/*@written*", "findings.not-modelled", "specify-core's stored answers are not PMI", "not-applicable",
  "haecceity reports Python's PROPERTY_DEFINITION('pmi-assist answers') as a property it does not model; it is specify-core's stored answers, read by resume (resume.rs), not existing PMI.")
for kind, why in [
    ("nonconformance", "haecceity reads through constructs that break the PMI practice and says so (e.g. a datum feature that identifies no face, a shape aspect naming no face); OpenCascade's reader is silent. Reporting is the honest form (R10)."),
    ("fact", "haecceity states a fact about a construct it reads and preserves (v0.4.7, quiddity #188: CTC-04's projected zone #18897 whose projection end #18894 has no geometry, formerly a nonconformance); OpenCascade is silent."),
    ("not-modelled", "haecceity reports what it consumes without holding (CATIA geometric sets, 'designed' qualifiers, unknown properties); OpenCascade is silent."),
    ("unconsumed", "haecceity lists semantic-PMI instances no part's PMI reaches (CTC-04's usage of an aspect naming the solid, CTC-05's unused qualifiers); OpenCascade is silent."),
    ("unresolved-reference", "haecceity reports a feature it cannot anchor (CTC-04: an aspect naming the MANIFOLD_SOLID_BREP); OpenCascade is silent."),
]:
    e("*nist*", f"findings.{kind}", f"haecceity reports {kind} findings", "rust-correct", why)
with open(sys.argv[1], 'w') as fh:
    fh.write("[\n" + ",\n".join(json.dumps(x, ensure_ascii=False) for x in E) + "\n]\n")
print(len(E))
