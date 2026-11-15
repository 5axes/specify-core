"""For #180: `load` takes bytes, and `diff` can return its picture."""

import struct
from pathlib import Path

import pytest

from specify_core import api
from specify_core.load import load

REVISIONS = Path(__file__).resolve().parents[1] / "fixtures" / "revisions"


def test_a_part_loaded_from_bytes_is_the_part_loaded_from_its_path(plate):
    from_path = load(plate)
    from_bytes = load(plate.read_bytes())
    assert from_bytes.binding == from_path.binding
    left, right = api.analyse(from_bytes), api.analyse(from_path)
    assert left["features"] == right["features"] and left["faces"] == right["faces"]
    assert from_bytes.path == Path("memory.step")
    assert load(plate.read_bytes(), name="edited.step").path == Path("edited.step")


def test_bytes_take_a_part_of_an_assembly_and_say_which_is_needed(assembly):
    data = assembly.read_bytes()
    with pytest.raises(ValueError, match="choose one"):
        load(data)
    pin = load(data, part=1)
    assert pin.binding.part == 1
    assert pin.binding == load(assembly, part=1).binding
    assert api.analyse(pin) == api.analyse(load(assembly, part=1))


def test_a_name_is_for_bytes_only(plate):
    with pytest.raises(TypeError, match="name is for bytes"):
        load(plate, name="x.step")


def test_the_diff_picture_is_a_png_of_the_width_asked(tmp_path):
    old = load(REVISIONS / "hole_moved.old.step.gz")
    new = load(REVISIONS / "hole_moved.new.step.gz")
    assert "image" not in api.diff(old, new)
    result = api.diff(old, new, image_width=400)
    png = result["image"]
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    width, height = struct.unpack(">II", png[16:24])
    assert width == 400 and height > 0
    # The same comparison as without the picture.
    assert result["report"] == api.diff(old, new)["report"]


def test_the_diff_report_lists_faces_no_feature_explains():
    old = load(REVISIONS / "block_face_pushed.old.step.gz")
    new = load(REVISIONS / "block_face_pushed.new.step.gz")
    report = api.diff(old, new)["report"]
    assert report["features"] == []
    assert report["unchanged_faces"] > 0
    assert [f["offset"] for f in report["other_faces"] if f.get("offset")] == [5.0]
