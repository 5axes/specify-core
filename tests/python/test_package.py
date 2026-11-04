"""Smoke tests of the Python package: each part of its API reaches the engine and comes back
in specify-core's Python types. The engine itself is tested in Rust (tests/*.rs)."""

import json
import math
import os
import subprocess
import sys
from pathlib import Path

import pytest

from specify_core import api
from specify_core.choices import SpecError, interpret
from specify_core.existing import ExistingPmi, read_existing
from specify_core.load import Binding, LoadedPart, load
from specify_core.problems import problems
from specify_core.rules import IncompleteError, Intent, Question
from specify_core.writer import WriteReport

STEEL = {"part.material": "steel"}
BIN = Path(sys.executable).parent


def _script(name):
    return BIN / (name + (".exe" if os.name == "nt" else ""))


def test_the_package_needs_no_python_dependency():
    import specify_core._native as native

    assert native.ANALYSIS_SCHEMA == 1
    assert not any(m in sys.modules for m in ("quiddity", "build123d", "OCP"))


def test_load_gives_a_handle_that_analyse_takes_as_it_takes_a_path(plate):
    loaded = load(plate)
    assert isinstance(loaded, LoadedPart) and isinstance(loaded.binding, Binding)
    assert loaded.part_count == 1 and loaded.binding.face_count == 14
    assert api.analyse(loaded) == api.analyse(plate)
    with pytest.raises(TypeError):
        api.analyse(42)


def test_an_assembly_is_listed_and_a_part_must_be_chosen(assembly):
    assert [p["name"] for p in api.parts(assembly)] == ["plate", "pin"]
    with pytest.raises(ValueError, match="choose one"):
        load(assembly)
    assert load(assembly, part=1).binding.part == 1


def test_questions_apply_and_preview_return_specify_cores_types(plate):
    analysis = api.analyse(plate)
    qs = api.questions(analysis, {})
    assert qs and all(isinstance(q, Question) for q in qs)
    assert "part.material" in {q.id for q in qs}
    intent = api.apply(analysis, STEEL, accept_defaults=True)
    assert isinstance(intent, Intent) and intent.part["material"] == "steel"
    assert Intent.from_dict(intent.to_dict()) == intent
    assert isinstance(api.preview(analysis, STEEL), Intent)


def test_answers_left_open_are_an_incomplete_error(plate):
    with pytest.raises(IncompleteError) as raised:
        api.apply(api.analyse(plate), {})
    assert "part.material" in raised.value.missing
    assert isinstance(raised.value, ValueError)


def test_write_writes_a_step_that_reads_back(plate, tmp_path):
    loaded = load(plate)
    intent = api.apply(api.analyse(loaded), STEEL, accept_defaults=True)
    out = tmp_path / "plate-pmi.step"
    report = api.write(loaded, intent, out, answers=STEEL)
    assert isinstance(report, WriteReport) and report.output == out
    assert "material steel" in report.written
    existing = read_existing(load(out))
    assert existing and all(isinstance(e, ExistingPmi) for e in existing)
    assert api.analyse(out)["resume"] == {"answers": STEEL}


def test_write_parts_writes_both_parts_of_an_assembly(assembly, tmp_path):
    answers = {0: STEEL, 1: {"part.material": "brass"}}
    intents = [
        (api.apply(api.analyse(assembly, k), answers[k], accept_defaults=True), answers[k])
        for k in (0, 1)
    ]
    out = tmp_path / "assembly-pmi.step"
    report = api.write_parts(assembly, intents, out)
    assert any(w.startswith("plate: ") for w in report.written)
    assert any(w.startswith("pin: ") for w in report.written)
    assert [p["written"] for p in api.parts(out)] == [True, True]


def test_mesh_ranges_triangles_by_face(plate):
    mesh = api.mesh(load(plate))
    assert len(mesh["faces"]) == 14 and mesh["indices"]
    assert mesh == api.mesh(plate)


def test_problems_and_a_typed_spec(plate):
    analysis = api.analyse(plate)
    assert isinstance(problems(analysis, STEEL), list)
    hole = next(q for q in api.questions(analysis, {}) if q.id.startswith("hole.function:"))
    assert interpret(hole.id, "H7", hole.options, hole.diameter) == "fit:H7"
    with pytest.raises(SpecError):
        interpret(hole.id, "", hole.options, hole.diameter)


def test_problems_state_each_sealed_interior_void():
    step = Path(__file__).parent.parent / "fixtures/voids/sealed_cavities.step.gz"
    facts = [p for p in problems(api.analyse(step), {}) if p.get("code") == "interior_void"]
    assert sorted(p["faces"] for p in facts) == [[6, 7, 8], [9, 10, 11], [12, 13, 14], [15, 16, 17]]
    assert {(p["severity"], p["diameter"], p["length"]) for p in facts} == {("info", 2.5, 12.0)}
    assert all(abs(p["estimated_volume"] - math.pi * 1.25**2 * 12) < 1e-9 for p in facts)


def test_the_console_script_lists_the_parts(plate):
    done = subprocess.run(
        [_script("specify-core"), "parts", str(plate)], capture_output=True, text=True
    )
    assert done.returncode == 0, done.stderr
    assert json.loads(done.stdout)[0]["faces"] == 14


def test_serve_answers_over_the_pipe():
    done = subprocess.run(
        [_script("specify-core"), "serve"],
        input='{"id": 1, "op": "hello"}\n[]\n',
        capture_output=True,
        text=True,
        timeout=60,
    )
    first, second = (json.loads(line) for line in done.stdout.splitlines())
    assert first == {"id": 1, "ok": True, "result": {"protocol": 1}}
    assert second["ok"] is False and second["error"]["code"] == "invalid"
    assert done.returncode == 0


def test_serve_reply_answers_in_process():
    from specify_core.serve import reply

    assert reply({"id": 2, "op": "hello"})["result"] == {"protocol": 1}


def test_cli_main_returns_the_exit_code(capsys):
    from specify_core.cli import main

    assert main(["parts", "/no/such/file.step"]) == 1
    assert "error:" in capsys.readouterr().err


def test_pmi_scene_and_packaged_schemas(plate, tmp_path):
    import importlib.resources
    import json
    import re
    import specify_core
    from specify_core import api

    scene = specify_core.pmi_scene(plate, {
        "part.general_tolerance": "ISO 2768-m",
        "hole.function:holes:9": "fit:H7",
        "part.surface_finish": "Ra 3.2",
    })
    assert scene["pmi"]["schema"] == "haecceity.pmi/1"
    assert scene["presentation"]["schema"] == "haecceity.presentation/1"
    generated = [g for rows in scene["presentation"]["items"].values() for g in rows
                 if g["source"] == "generated"]
    assert generated
    for graphic in generated:
        assert len(graphic["polyline_roles"]) == len(graphic["polylines"])
        assert all("cell_box" in run for run in graphic.get("text_placements", []))
    assert scene["specify"]["planner:specified:dim:hole:9/bore.diameter"]["status"] == "answered"
    assert scene["specify"]["planner:specified:dim:hole:9/bore.diameter"]["question"] == "hole.function:holes:9"
    assert scene["specify"]["part.surface_finish"]["number"] > 0
    empty = api.pmi_scene(plate.read_bytes())
    assert empty["pmi"]["pmi"]["dimensions"] == []
    answers = {"part.material": "6061-T6"}
    written = tmp_path / "numbered.step"
    api.write(plate, api.apply(api.analyse(plate), answers, accept_defaults=True),
              written, answers=answers)
    readback = api.pmi_scene(written.read_bytes())
    dimensions = readback["pmi"]["pmi"]["dimensions"]
    assert dimensions and all(isinstance(row["identifier"], str) for row in dimensions)
    readbacks = [readback]
    for identifier in ("repeated", ""):
        data, changed = re.subn(rb"ID_ATTRIBUTE\('[^']*',",
                                f"ID_ATTRIBUTE('{identifier}',".encode(), written.read_bytes())
        assert changed > 1
        recovered = api.pmi_scene(data)
        dimensions = recovered["pmi"]["pmi"]["dimensions"]
        assert dimensions and all(row["identifier"] == identifier for row in dimensions)
        assert len({row["id"] for row in dimensions}) == len(dimensions)
        readbacks.append(recovered)
    root = importlib.resources.files("specify_core").joinpath("schema")
    for name in ("pmi", "presentation"):
        schema = json.loads(root.joinpath(f"haecceity.{name}-1.json").read_text())
        assert schema["$schema"] == "https://json-schema.org/draft/2020-12/schema"
        from jsonschema import Draft202012Validator
        Draft202012Validator.check_schema(schema)
        Draft202012Validator(schema).validate(scene[name])
        Draft202012Validator(schema).validate(empty[name])
        for readback in readbacks:
            Draft202012Validator(schema).validate(readback[name])


def test_foreign_pmi_scene_schema_and_as_drawn_metadata():
    import gzip
    import importlib.resources
    from jsonschema import Draft202012Validator
    from specify_core import api

    path = Path(__file__).parent.parent / "fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz"
    scene = api.pmi_scene(gzip.decompress(path.read_bytes()), part=0)
    analysis = api.analyse(path, part=0)
    face = next(face["id"] for face in analysis["faces"] if face["kind"] == "plane")
    merged = api.pmi_scene(path, {"extra.1": {"kind": "flatness", "faces": [face], "value": "0.05"}}, part=0)
    root = importlib.resources.files("specify_core").joinpath("schema")
    for name in ("pmi", "presentation"):
        schema = json.loads(root.joinpath(f"haecceity.{name}-1.json").read_text())
        Draft202012Validator(schema).validate(scene[name])
        Draft202012Validator(schema).validate(merged[name])
    assert scene["pmi"]["pmi"]["datums"] == merged["pmi"]["pmi"]["datums"]
    # v0.4.1 adds a generated plane view without changing any saved file view.
    views = merged["presentation"]["views"]
    assert all(view in views for view in scene["presentation"]["views"])
    generated = [view for view in views if view["id"].startswith("generated:")]
    assert len(generated) == 1
    assert generated[0]["graphics"] == ["generated:tolerances:flatness:extra.1"]
    presentation = scene["presentation"]
    assert presentation["planes"]
    assert presentation["views"]
    assert any(presentation["items"].get(datum["id"]) for datum in scene["pmi"]["pmi"]["datums"])
    graphics = [g for rows in presentation["items"].values() for g in rows]
    assert all(g["source"] == "file" for g in graphics)
    assert any(g.get("plane_id") for g in graphics)


@pytest.mark.parametrize("answers", [
    {"part.surface_finish": []},
    {"diameter.fit:missing-feature": "fit:h6"},
    {"datum.A": [999999]},
])
def test_pmi_scene_rejects_malformed_answers(plate, answers):
    from specify_core import api

    with pytest.raises(ValueError, match="Invalid answers"):
        api.pmi_scene(plate, answers)
    with pytest.raises(ValueError, match="Invalid answers"):
        api.pmi_scene(plate, {"answers": answers, "auto": []})


def test_diff_exposes_numbered_pmi_with_scene_identities(plate, tmp_path):
    from specify_core import api

    old = tmp_path / "old.step"
    new = tmp_path / "new.step"
    before = {"hole.function:holes:9": "fit:H7"}
    after = {"hole.function:holes:9": "fit:H8"}
    analysis = api.analyse(plate)
    api.write(plate, api.preview(analysis, before), old, answers=before)
    api.write(old, api.preview(api.analyse(old), after), new, answers=after)
    result = api.diff(api.load(old), new)
    scene = api.pmi_scene(new)
    changed = [c for c in result["report"]["pmi"]["items"] if c["class"] == "changed"]
    assert changed
    for item in changed:
        assert scene["specify"][item["id"]]["number"] == item["number"]
    assert "→" in result["markdown"]


def test_bom_read_api_and_cli_agree_on_file_sources(assembly):
    from specify_core import bom
    from specify_core import _native
    import json

    rows = bom(assembly)
    assert len(rows) == 2
    assert rows[1]["quantity"] == {"value": 2, "source": "file"}
    assert rows[0]["item_number"] == {"value": None, "source": "default"}
    assert json.loads(_native.bom(assembly)) == rows


def test_bom_write_requires_explicit_acceptance(assembly, tmp_path):
    from specify_core import bom
    analysis = api.analyse(assembly, part=0)
    questions = [q for q in api.questions(analysis, {}) if q.id.startswith("bom.")]
    assert questions and all(q.optional and q.default is None and q.suggestions for q in questions)
    assert "bom" not in api.apply(analysis, STEEL, accept_defaults=True).part
    intent = api.apply(analysis, STEEL, accept_defaults=True, bom=True)
    out = tmp_path / "bom.step"
    api.write(assembly, intent, out, answers=STEEL)
    rows = bom(out)
    assert rows[0]["item_number"]["value"] == 1
    assert rows[0]["description"]["source"] == "default"
    assert api.analyse(out, part=0)["resume"]["answers"] == STEEL


def test_diff_scenes_are_optional_and_match_pmi_scene(plate):
    given = {"part.notes": ["Keep dry"]}
    default = api.diff(plate, plate, given)
    included = api.diff(plate, plate, given, scenes=True)
    scenes = included["report"]["pmi"].pop("scenes")
    assert included == default
    assert scenes["old"] == api.pmi_scene(plate, given)
    assert scenes["new"] == api.pmi_scene(plate, included["report"]["answers"])


def test_limits_and_combined_iso_class_round_trip(plate, tmp_path):
    given = {
        "part.general_tolerance": "ISO 2768-mK",
        "extra.size": {"kind": "size", "faces": [9], "value": "7.8/8.2"},
        "extra.distance": {"kind": "distance", "faces": [2], "reference": [4], "value": "9.8/10.2"},
    }
    analysis = api.analyse(plate)
    question = next(q for q in api.questions(analysis, {}) if q.id == "part.general_tolerance")
    assert question.default == "ISO 2768-m"
    assert len(question.options) == 16 and "ISO 2768-mK" in question.options
    out = tmp_path / "limits.step"
    api.write(plate, api.preview(analysis, given), out, answers=given)
    assert api.analyse(out)["resume"]["answers"] == given
    limits = [d for d in read_existing(out) if d.limits is not None]
    assert {d.limits for d in limits} == {(7.8, 8.2), (9.8, 10.2)}
    assert all(d.to_dict()["limits"] == list(d.limits) for d in limits)
    assert all(abs(d.upper - .2) < 1e-9 and abs(d.lower + .2) < 1e-9 for d in limits)
    scene = api.pmi_scene(out)
    labels = "; ".join(m.get("notation", "") for m in scene["specify"].values())
    assert "9.8 - 10.2" in labels and "⌀7.8 - ⌀8.2" in labels
    assert "GENERAL TOLERANCES ISO 2768-mK" in labels


def test_spindle_diff_scenes_obey_editor_links():
    fixtures = Path(__file__).resolve().parents[1] / "fixtures" / "revisions"
    old = fixtures / "spindle_pmi.old.step.gz"
    new = fixtures / "spindle_pmi.new.step.gz"
    analysis = api.analyse(old)
    answers = dict(STEEL)
    while True:
        defaults = {
            q.id: q.default for q in api.questions(analysis, answers)
            if q.id not in answers and q.default is not None
        }
        if not defaults:
            break
        answers.update(defaults)
    report = api.diff(old, new, answers, scenes=True)["report"]
    thread = [item for item in report["pmi"]["items"] if "thread:holes:14" in item["id"]]
    assert len(thread) == 1
    assert thread[0]["id"] == "thread:holes:14"
    assert thread[0]["class"] == "removed"
    assert thread[0]["old"] == "M4×0.7-6H THRU"
    for side in ("old", "new"):
        scene = report["pmi"]["scenes"][side]
        ids = {
            row["id"] for section in ("pmi", "units")
            for rows in scene["pmi"][section].values() if isinstance(rows, list)
            for row in rows if isinstance(row.get("id"), str)
        }
        assert ids == set(scene["presentation"]["semantic_ids"]), side
    assert not any(
        item.get("old") == "M4×0.7-6H THRU" and item.get("new") == "1.5"
        for item in report["pmi"]["items"]
    )


def test_write_report_optional_geometry_carrier_exemptions():
    original = {"output": "out.step", "written": [], "not_written": [], "warnings": []}
    assert WriteReport.from_dict(original).to_dict() == original
    additions = [{"part": 2, "condition": "shared-carrier-controlled-pair", "aspect": 123, "feature": 45}]
    extended = {
        **original,
        "geometry_carrier_exemptions": additions,
        "presentation_warnings": ["feature note glyphs could not be drawn"],
        "preserved_normalizations": [{"id": 77, "normalization": "legacy-endpoint-retirement"}],
        "retained_shared": [301],
    }
    assert WriteReport.from_dict(extended).to_dict() == extended


def test_basic_dimensions_read_as_basic():
    ctc04 = Path(__file__).resolve().parents[1] / "fixtures" / "nist" / "nist_ctc_04_asme1_ap242.stp.gz"
    found = read_existing(ctc04)
    basic = [d for d in found if d.basic]
    assert len(basic) == 2
    assert all(d.kind == "dimension" and d.upper is None and d.lower is None for d in basic)
    assert all(d.to_dict()["basic"] is True for d in basic)
    assert all("basic" not in d.to_dict() for d in found if not d.basic)


def test_dropped_carry_cannot_default_and_saved_evidence_rejects_changed_geometry(tmp_path):
    revisions = Path(__file__).resolve().parents[1] / "fixtures" / "revisions"
    old = revisions / "plate_split.old.step.gz"
    new = revisions / "plate_split.new.step.gz"
    given = {"part.material": "6061-T6", "datum.A": [4]}
    carried = api.diff(old, new, given)["report"]["answers"]
    analysis = api.analyse(new)
    blocked = next(q for q in api.questions(analysis, carried) if q.id == "datum.A")
    assert blocked.default is None and blocked.attention == "required"
    assert blocked.reason == "carried answer could not be applied: " + carried["_specify"]["unresolved"]["datum.A"]
    with pytest.raises(IncompleteError, match="datum.A.*carried answer could not be applied"):
        api.apply(analysis, carried, accept_defaults=True)
    carried["datum.A"] = [4, 8]
    intent = api.apply(analysis, carried, accept_defaults=True)
    api.write(new, intent, tmp_path / "resolved.step", answers=carried)
    saved = api.diff(old, old, given)["report"]["answers"]
    api.write(old, api.apply(api.analyse(old), saved, accept_defaults=True),
              tmp_path / "unchanged.step", answers=saved)
    with pytest.raises(ValueError, match="stale answers.*diff"):
        api.write(new, intent, tmp_path / "stale.step", answers=saved)
    assert not (tmp_path / "stale.step").exists()
