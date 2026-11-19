"""For pzfreo/quiddity#303: a face whose area cannot be computed is left out and reported;
the rest of the part is specified and written."""
from pathlib import Path

from specify_core import api

FIXTURES = Path(__file__).resolve().parents[1] / "fixtures"


def test_unmeasured_face_is_reported_and_the_part_is_written(tmp_path):
    step = FIXTURES / "unmeasured_face_closed.step"
    analysis = api.analyse(step)
    (finding,) = analysis["unmeasured_faces"]
    face = finding["face"]
    assert finding["reason"] == "area_and_centroid_not_computable"
    assert str(face) in finding["detail"]
    assert [f["id"] for f in analysis["faces"]] == list(range(len(analysis["faces"])))
    wall = analysis["faces"][face]
    assert wall["unmeasured"] is True and "area" not in wall and "centroid" not in wall
    assert all("area" in f for f in analysis["faces"] if f["id"] != face)
    assert all(face not in f["faces"] for f in analysis["features"])
    assert any(f["family"] == "holes" for f in analysis["features"])
    assert all(face not in q.faces for q in api.questions(analysis, {}))

    answers = {"part.material": "steel"}
    out = tmp_path / "out.step"
    report = api.write(step, api.preview(analysis, answers), out, answers=answers)
    assert report.written
    again = api.analyse(out)
    assert again["unmeasured_faces"][0]["face"] == face
    assert all(face not in e["faces"] for e in again["existing"])
