"""The cross-engine check's Python side: what specify-core (OpenCascade) reads back of the files
the Rust port wrote (``tests/fixtures/cross_engine/<case>/rust.step.gz``, written by
``cargo run --release --example write_fixtures``).

For each such file the read-back is exactly the oracle's (``cases.readback``: per part,
``read_existing``, ``part_settings`` and ``resume.answers_for`` on ``load(step, part=k)``, not
``api.analyse``), saved as ``python_readback.json.gz`` beside it, in the oracle's
``readback.json`` shape. Beside it, ``python_meta.json.gz`` says what was read (the
decompressed file's sha256, so the Rust test can tell a stale capture) and with which
specify-core.

OpenCascade can die reading a file (SIGSEGV in ``STEPCAFControl_Reader``: docs/upstream-needs.md
U15), so every read runs in a child process of its own: first the part count, then each part's
read-back, then (where wanted) each part's resume evidence, so a crash in one never takes the
value of another with it.
A child killed by a signal is recorded in place of the value it would have given as
``{"error": "crash", "message": "<signal> in <the call it was in>"}``, and the capture carries
on. A child that exits otherwise without a result is a failure of this tool, not an oracle
result, and stops the capture.

Also captured: where OpenCascade crashed, ``python_readback_without_symbols.json.gz``, a probe
reading the same file without the datum symbols' relationship (``without_symbol_relationship``)
to show what it reads apart from the crash; per part, the resume evidence (what Python's
pattern finds stored and the faces it fingerprints, ``_resume_evidence``) in
``python_meta.json.gz``; and the same evidence for every file Python wrote for the oracle
(``tests/fixtures/oracle/**/written.step.gz``) in ``<case>/python_written_evidence.json.gz``.
"""

from __future__ import annotations

import gzip
import hashlib
import json
import re
import shutil
import signal
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

from .common import ORACLE, provenance, scratch_dir, scrub, sha256, today, write_json_gz

#: The capture tool, run again as each child.
CAPTURE = Path(__file__).resolve().parents[1] / "capture.py"


def child(step: Path, which: str, out: Path) -> None:
    """In a child process: ``which`` is ``count`` (how many parts ``load_all`` finds), a part
    index (that part read back) or ``evidence-<k>`` (part ``k``'s resume evidence). The value
    goes to ``out`` as JSON; before each call its name is written on stderr as
    ``stage: <call>``, so the parent can say where a crash was."""
    from specify_core.load import load_all

    from .cases import _readback_part
    from .common import attempt, failed

    def stage(call: str) -> None:
        print(f"stage: {call}", file=sys.stderr, flush=True)

    if which == "count":
        stage("load.load_all")
        count = attempt(lambda: len(load_all(step)))
        value: Any = count if failed(count) else {"count": count}
    elif which.startswith("evidence-"):
        k = int(which.removeprefix("evidence-"))
        value = attempt(lambda: _resume_evidence(step, k, stage))
    else:
        value = _readback_part(step, int(which), stage)
    out.write_text(json.dumps(value))


def _resume_evidence(step: Path, k: int, stage: Callable[[str], None]) -> dict[str, Any]:
    """Why ``resume.answers_for`` resumes part ``k`` or not: the payload Python's pattern finds
    stored for it (``resume.saved``), the part's face count, and the faces Python fingerprints
    (``(kind, centroid rounded to 3)`` each, as ``resume.fingerprint`` hashes them) with that
    hash, so the Rust test can compare them with its own faces. Also ``fingerprint_v2``, the
    stored-answer schema 2 fingerprint of the same faces by the canonical form docs/design.md
    defines (every zero written unsigned, ``0.0``, after rounding), computed here from Python's
    own faces whichever schema this specify-core writes (``schema``), so the Rust test can check
    the two engines' schema 2 fingerprints are equal. ``fingerprint`` is specify-core's own
    for the schema it writes; ``fingerprint_v1`` is its own schema 1 fingerprint (the one a
    schema 1 payload, as in the files the oracle's specify-core wrote, resumes on)."""
    from specify_core import resume
    from specify_core.features import describe_faces
    from specify_core.load import load

    stage(f"resume evidence (part {k})")
    loaded = load(step, part=k)
    faces = [(f.kind, [round(c, 3) for c in f.centroid]) for f in describe_faces(loaded)]
    return {
        "stored": resume.saved(loaded.path, k),
        "face_count": loaded.binding.face_count,
        "faces": faces,
        "fingerprint": resume.fingerprint(loaded),
        "fingerprint_v1": (
            resume.fingerprint(loaded, 1) if resume.SCHEMA >= 2 else resume.fingerprint(loaded)
        ),
        "schema": resume.SCHEMA,
        "fingerprint_v2": _fingerprint_v2(faces),
    }


def _fingerprint_v2(faces: list[tuple[str, list[float]]]) -> str:
    """Schema 2's fingerprint of ``faces`` (kind, centroid rounded to 3): sha256 of
    ``json.dumps`` of them with every zero written unsigned (docs/design.md, "The fingerprint")."""
    unsigned = [(kind, [0.0 if c == 0 else c for c in centroid]) for kind, centroid in faces]
    return hashlib.sha256(json.dumps(unsigned).encode()).hexdigest()


def _run(step: Path, which: str, scratch: Path) -> Any:
    """``child(step, which)`` in a process of its own: its value, or the crash as a failure."""
    out = scratch / f"{which}.json"
    done = subprocess.run(
        [sys.executable, str(CAPTURE), "readback-child", str(step), which, str(out)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if done.returncode == 0 and out.is_file():
        return json.loads(out.read_text())
    stages = [
        line[len("stage: ") :] for line in done.stderr.splitlines() if line.startswith("stage: ")
    ]
    if done.returncode < 0:
        # A crash is an oracle result, recorded where the value would have been.
        name = signal.Signals(-done.returncode).name
        where = stages[-1] if stages else "start-up"
        return {"error": "crash", "message": f"specify-core's process died with {name} in {where}"}
    raise RuntimeError(
        f"readback child for {step} part {which} exited {done.returncode}:\n{done.stderr}"
    )


def readback(step: Path) -> Any:
    """``cases.readback(step)``, each call isolated in a child process (above)."""
    return _readback_with_evidence(step, evidence=False)[0]


def _readback_with_evidence(step: Path, evidence: bool = True) -> tuple[Any, Any]:
    """``readback(step)``, and (when ``evidence``) per part its resume evidence
    (``_resume_evidence``, or the failure or crash in its place, from a child of its own; the
    read-back's own failure for the file as a whole), else None."""
    scratch = scratch_dir()
    try:
        count = _run(step, "count", scratch)
        if "error" in count:
            return count, count
        parts = range(count["count"])
        back = [_run(step, str(k), scratch) for k in parts]
        found = [_run(step, f"evidence-{k}", scratch) for k in parts] if evidence else None
        return back, found
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def capture(root: Path, *, python_written: bool = True) -> list[str]:
    """Every ``rust.step.gz`` under ``root`` read back by Python, and the resume evidence of
    every file Python wrote for the oracle (``written.step.gz``); returns a line per case."""
    lines = []
    for gz in sorted(root.rglob("rust.step.gz")):
        case = str(gz.parent.relative_to(root))
        data = gzip.decompress(gz.read_bytes())
        value, evidence = _read(data)
        write_json_gz(gz.parent / "python_readback.json.gz", value)
        meta = {
            "case": case,
            "rust_step_sha256": sha256(data),
            **provenance(),
            "command": f"tools/capture.py cross-engine {_display(root)}" + (" --rust-only" if not python_written else ""),
            "captured": today(),
            "read": "per part: existing.read_existing, existing.part_settings and "
            "resume.answers_for on load.load(step, part=k), each part in a child process",
            "resume_evidence": evidence,
        }
        probe = gz.parent / "python_readback_without_symbols.json.gz"
        if isinstance(value, dict) and value.get("error") == "crash":
            # What OpenCascade would read but for the crash (U15): the same file without the
            # relationship of the datum symbols' draughting model. A probe, never a fixture
            # either engine writes.
            stripped, removed = without_symbol_relationship(data)
            if removed:
                probe_value, meta["probe_resume_evidence"] = _read(stripped)
            else:
                probe_value = {"error": "probe", "message": "no datum symbol relationship"}
            write_json_gz(probe, probe_value)
            meta["probe"] = (
                f"python_readback_without_symbols.json.gz: the read-back of this file with its "
                f"{removed} MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP instance(s) relating "
                "the 'datum feature symbols' DRAUGHTING_MODEL removed (docs/upstream-needs.md "
                "U15), to show what OpenCascade reads of the semantic PMI apart from the crash"
            )
            lines.append(f"{case} without symbols: {_summary(probe_value)}")
        elif probe.exists():
            probe.unlink()
        write_json_gz(gz.parent / "python_meta.json.gz", meta)
        lines.append(f"{case}: {_summary(value)}")
    # The files Python wrote: the oracle has their read-back; what Python fingerprints in them
    # is captured here, so the Rust engine's failure to resume them can be explained.
    for gz in (sorted(ORACLE.rglob("written.step.gz")) if python_written else []):
        case = str(gz.parent.relative_to(ORACLE))
        data = gzip.decompress(gz.read_bytes())
        _, evidence = _read(data)
        out = {
            "case": case,
            "written_step_sha256": sha256(data),
            **provenance(),
            "command": f"tools/capture.py cross-engine {_display(root)}" + (" --rust-only" if not python_written else ""),
            "captured": today(),
            "resume_evidence": evidence,
        }
        write_json_gz(root / case / "python_written_evidence.json.gz", out)
        lines.append(f"{case}: Python-written file's resume evidence")
    return lines


def without_symbol_relationship(data: bytes) -> tuple[bytes, int]:
    """``data`` without each ``MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP`` that names a
    ``DRAUGHTING_MODEL('datum feature symbols', ...)`` (haecceity writes one per part with
    datum symbols, each instance on its own line), and how many were removed."""
    models = re.findall(rb"^#(\d+)=DRAUGHTING_MODEL\('datum feature symbols',", data, re.MULTILINE)
    removed = 0
    for model in models:
        pattern = (
            rb"^#\d+=MECHANICAL_DESIGN_AND_DRAUGHTING_RELATIONSHIP\([^;]*#"
            + model
            + rb"\b[^;]*\);\n"
        )
        data, n = re.subn(pattern, b"", data, flags=re.MULTILINE)
        removed += n
    return data, removed


def _read(data: bytes) -> tuple[Any, Any]:
    """``_readback_with_evidence`` of the STEP ``data``, from a temporary file whose path is
    written ``{input}`` in what is returned."""
    scratch = scratch_dir()
    try:
        step = scratch / "step.step"
        step.write_bytes(data)
        subs = {str(step): "{input}", str(scratch): "{output}"}
        value, evidence = _readback_with_evidence(step)
        return scrub(value, subs), scrub(evidence, subs)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


def _summary(value: Any) -> str:
    if isinstance(value, dict):
        return f"{value['error']}: {value['message']}"
    parts = []
    for k, part in enumerate(value):
        bad = [key for key, v in part.items() if isinstance(v, dict) and "error" in v]
        n = len(part["existing"]) if isinstance(part["existing"], list) else "-"
        parts.append(f"part {k}: {n} existing" + (f", failed {bad}" if bad else ""))
    return "; ".join(parts)


def _display(root: Path) -> str:
    try:
        return str(root.resolve().relative_to(CAPTURE.parents[1]))
    except ValueError:
        return str(root)
