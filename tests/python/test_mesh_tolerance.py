from pathlib import Path
import pytest
from specify_core import api
from specify_core.load import load

PARTS = Path(__file__).resolve().parents[1] / "fixtures" / "parts"


def test_mesh_tolerance_path_and_loaded():
    path = PARTS / "mesh_cylinder.step"
    for step in [path, load(path)]:
        assert api.mesh(step) == api.mesh(step, max_deviation_mm=None)
        for limit in [0.1, 0.01, 1.0]:
            mesh = api.mesh(step, max_deviation_mm=limit)
            assert mesh["max_deviation_mm"] == limit
            assert 0 < mesh["achieved_deviation_mm"] <= limit
        for invalid in [0, -1, float("nan"), float("inf")]:
            with pytest.raises(ValueError, match="positive finite"):
                api.mesh(step, max_deviation_mm=invalid)


def test_sphere_dimensioning_reason():
    section = api.analyse(PARTS / "mesh_sphere.step")["dimensioning"]
    assert section["applicable"] is False
    assert section["reason"]


def test_torus_meets_fine_bound():
    for limit in [0.1, 0.01]:
        mesh = api.mesh(PARTS / "mesh_torus.step", max_deviation_mm=limit)
        assert mesh["missing"] == []
        assert 0 < mesh["achieved_deviation_mm"] <= limit
