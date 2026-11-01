#!/usr/bin/env python3
"""Independently compare standard material assignments with Specify's BOM JSON (For #79).

Run using an OCP environment: python check_bom_xcaf.py written.step bom.json.
UDAs need Specify's documented BOM reader; this checks the standard material route.
"""
import argparse
import json
from pathlib import Path

from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPCAFControl import STEPCAFControl_Reader
from OCP.TCollection import TCollection_ExtendedString
from OCP.TDataStd import TDataStd_Name
from OCP.TDocStd import TDocStd_Document
from OCP.XCAFDoc import XCAFDoc_DocumentTool

try:
    from OCP.collections import Sequence_TDF_Label as LabelSequence
except ImportError:
    from OCP.TDF import TDF_LabelSequence as LabelSequence


def check(step, bom):
    reader = STEPCAFControl_Reader()
    reader.SetMatMode(True)
    reader.SetNameMode(True)
    assert reader.ReadFile(str(step)) == IFSelect_RetDone, "XCAF STEP read failed"
    doc = TDocStd_Document(TCollection_ExtendedString("bom"))
    assert reader.Transfer(doc), "XCAF transfer failed"
    shapes = XCAFDoc_DocumentTool.ShapeTool_s(doc.Main())
    materials = XCAFDoc_DocumentTool.MaterialTool_s(doc.Main())
    labels = LabelSequence()
    shapes.GetShapes(labels)
    material_labels = LabelSequence()
    materials.GetMaterialLabels(material_labels)
    names = []
    for index in range(1, material_labels.Length() + 1):
        name = TDataStd_Name()
        assert material_labels.Value(index).FindAttribute(TDataStd_Name.GetID_s(), name)
        names.append(name.Get().ToExtString())
    expected = [row["material"]["value"] for row in json.loads(bom.read_text())]
    assert sorted(names) == sorted(expected), (names, expected)
    print(json.dumps({"file": str(step), "shape_labels": labels.Length(),
                      "material_assignments": len(names), "materials": names}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("step", type=Path)
    parser.add_argument("bom", type=Path)
    args = parser.parse_args()
    check(args.step, args.bom)
