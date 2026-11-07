"""The steps of specify-core's API, on the Rust engine: ``parts``, ``analyse``, ``questions``,
``apply``, ``preview``, ``write``, ``write_parts`` and ``mesh``.

The names, signatures and return shapes are specify-core's own ``api.py``; the work is done
by ``specify_core._native``. A file of several parts is specified a part at a time: ``parts``
lists them, ``analyse`` and ``mesh`` take the one to work on, by path or loaded once (``load``),
and ``write_parts`` writes any number of them into the assembly together.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from . import _native, rules  # noqa: F401  (specify-core's api has rules too)
from .load import LoadedPart, load
from .rules import Intent, Question
from .writer import WriteReport

__all__ = [
    "LoadedPart",
    "load",
    "parts",
    "bom",
    "analyse",
    "questions",
    "apply",
    "export_answers",
    "preview",
    "write",
    "write_parts",
    "mesh",
    "pmi_scene",
    "diff",
]

ANALYSIS_SCHEMA = _native.ANALYSIS_SCHEMA


def parts(step: str | Path) -> list[dict[str, Any]]:
    """The distinct parts of ``step``: name, face count, instances and their
    placements, whether specify-core has written the part (answers stored), and
    ``same_as``: the index of the first part this one is a copy of (the same shape
    moved, stating the same BOM facts and answers), or ``None``."""
    return json.loads(_native.parts(_path(step)))


def bom(step: str | Path) -> list[dict[str, Any]]:
    """Assembly BOM fields and their file/answered/default sources; see docs/bom.md."""
    return json.loads(_native.bom(_path(step)))


def analyse(step: str | Path | LoadedPart, part: int | None = None) -> dict[str, Any]:
    """Features, faces and existing PMI of ``step`` -- of its ``part``-th part, in
    an assembly -- as a JSON-safe snapshot.

    The snapshot is what gets stored: it is never re-derived for the same file,
    so its feature ids do not need to survive an engine upgrade.
    """
    if isinstance(step, LoadedPart):
        return json.loads(_native.analyse(step._handle))
    return json.loads(_native.analyse(_path(step), part))


def questions(analysis: dict[str, Any], answers: dict[str, Any]) -> list[Question]:
    records = json.loads(_native.questions(json.dumps(analysis), json.dumps(answers)))
    return [Question.from_record(r) for r in records]


def apply(
    analysis: dict[str, Any], answers: dict[str, Any], *, accept_defaults: bool = False, bom: bool = False
) -> Intent:
    """Intent from ``answers``; with ``accept_defaults``, unanswered defaults are taken.

    Defaults are resolved to a fixed point, because some questions only exist
    once an earlier default is taken.
    """
    text = _native.apply(json.dumps(analysis), json.dumps(answers), accept_defaults, bom)
    return Intent.from_dict(json.loads(text))


def export_answers(
    step: str | Path | LoadedPart, analysis: dict[str, Any], answers: dict[str, Any]
) -> dict[str, Any]:
    """Complete answer map with defaults explicit and ``_specify.evidence`` for this part.

    Does not write STEP. Raises IncompleteError for unanswered required questions
    or carry blocks, and ValueError for stale evidence or a mismatched analysis.
    Serialize with ``json.dumps(result, indent=2) + "\\n"`` for a stable sidecar.
    """
    return json.loads(_native.export_answers(
        _step(step), json.dumps(analysis), json.dumps(answers)
    ))


def preview(analysis: dict[str, Any], answers: dict[str, Any]) -> Intent:
    """What the answers so far amount to; see the engine's ``rules::preview``."""
    return Intent.from_dict(json.loads(_native.preview(json.dumps(analysis), json.dumps(answers))))


def write(
    step: str | Path | LoadedPart,
    intent: Intent,
    output: str | Path,
    *,
    answers: dict[str, Any] | None = None,
) -> WriteReport:
    """Write ``intent`` into ``step``, on the part it was made for. Given the
    ``answers`` it came from, they are stored in the file too, so that opening it
    again resumes them (``resume``)."""
    text = _native.write(
        _step(step),
        json.dumps(intent.to_dict()),
        os.fspath(output),
        None if answers is None else json.dumps(answers),
    )
    return WriteReport.from_dict(json.loads(text))


def write_parts(
    step: str | Path | LoadedPart,
    intents: list[tuple[Intent, dict[str, Any] | None]],
    output: str | Path,
) -> WriteReport:
    """Write several parts of the assembly ``step`` together: each intent on the
    part it was made for, with the answers it came from (or None)."""
    jobs = [(json.dumps(i.to_dict()), None if a is None else json.dumps(a)) for i, a in intents]
    return WriteReport.from_dict(
        json.loads(_native.write_parts(_step(step), jobs, os.fspath(output)))
    )


def mesh(step: str | Path | LoadedPart, part: int | None = None) -> dict[str, Any]:
    """Triangles of ``step`` (its ``part``-th part) ranged by the face indices its
    analysis uses, in the part's own frame."""
    if isinstance(step, LoadedPart):
        return json.loads(_native.mesh(step._handle))
    return json.loads(_native.mesh(_path(step), part))


def _step(step: Any) -> Any:
    return step._handle if isinstance(step, LoadedPart) else _path(step)


def _path(step: Any) -> str:
    if not isinstance(step, str | os.PathLike):
        raise TypeError(f"expected a path or a LoadedPart, not {type(step).__name__}")
    return os.fspath(step)


def pmi_scene(
    step: str | Path | LoadedPart | bytes,
    answers: dict[str, Any] | None = None,
    part: int | None = None,
) -> dict[str, Any]:
    """Semantic PMI, presentation and Specify review metadata. Bytes are view-only.

    Face anchors use the same indices as mesh(); assemblies take a part index.
    Omit answers to display the file's own PMI, including As Drawn graphics.
    """
    source = step if isinstance(step, bytes) else _step(step)
    return json.loads(_native.pmi_scene(
        source, None if answers is None else json.dumps(answers), part
    ))


def diff(
    old: str | Path | LoadedPart,
    new: str | Path | LoadedPart,
    answers: dict[str, Any] | None = None,
    *,
    title: str = "Revision",
    part: int | None = None,
    scenes: bool = False,
) -> dict[str, Any]:
    """Compare revisions of a selected part: ``{report, markdown}``.

    ``report["pmi"]["items"]`` uses pmi_scene identities, numbers and statuses.
    Omitted answers carry the old file's saved answers. Paths accept a part index;
    loaded handles already select their part. ``scenes=True`` includes the exact
    compared pmi_scene documents in ``report["pmi"]["scenes"]``.
    """
    return json.loads(_native.diff(
        _step(old), _step(new), json.dumps(answers or {}), title, part, scenes
    ))
