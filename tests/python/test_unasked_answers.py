"""#126: an answer whose question is not asked is listed in not_written; unknown IDs are refused."""
import pytest
from specify_core import api

UNKNOWN = "Invalid answers: bogus.question: unknown answer ID"


def answers(locating):
    return {
        "part.material": "6082-T6",
        "part.locating": locating,
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.05",
        "hole.position:holes:12": "0.1",
        "hole.location:holes:12": "±0.1",
    }


def written(plate, tmp_path, given):
    analysis = api.analyse(plate)
    intent = api.apply(analysis, given, accept_defaults=True)
    return api.write(plate, intent, tmp_path / "out.step", answers=given)


def test_flatness_under_both_ways_of_locating(plate, tmp_path):
    report = written(plate, tmp_path, answers("position tolerances"))
    assert "flatness 0.05 on [2]" in report.written
    assert report.not_written == [
        "hole.location:holes:12 is not asked when part.locating is 'position tolerances', "
        "so its answer was not written"
    ]
    report = written(plate, tmp_path, answers("± dimensions"))
    assert not [w for w in report.written if "flatness" in w]
    assert report.not_written == [
        f"{qid} is not asked when part.locating is '± dimensions', so its answer was not written"
        for qid in ("datum.A.flatness", "hole.position:holes:12")
    ]


def test_an_unknown_id_is_refused(plate, tmp_path):
    analysis = api.analyse(plate)
    given = {**answers("position tolerances"), "bogus.question": "x"}
    for call in (
        lambda: api.questions(analysis, given),
        lambda: api.apply(analysis, given, accept_defaults=True),
        lambda: api.preview(analysis, given),
        lambda: api.export_answers(plate, analysis, given),
    ):
        with pytest.raises(ValueError, match=UNKNOWN):
            call()
