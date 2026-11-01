"""Capture specify-core's (Python's) outputs as the oracle the Rust port is checked against.

Run with specify-core's own environment (it imports specify_core, OCP and quiddity):

    PY=/Users/paul/repos/specify-core/.venv/bin/python
    $PY tools/capture.py all tests/fixtures/oracle [--jobs N] [--only parts,nist,corpus]
    $PY tools/capture.py case INPUT tests/fixtures/oracle
    $PY tools/capture.py readback WRITTEN.step OUT_DIR
    $PY tools/capture.py cross-engine tests/fixtures/cross_engine
    $PY tools/capture.py scenarios tests/fixtures/oracle [NAME ...]
    $PY tools/capture.py interpret tests/fixtures/oracle
    $PY tools/capture.py make-scenarios tests/fixtures/oracle

``all`` captures every input -- the synthetic parts (tests/fixtures/parts), the NIST files
(tests/fixtures/nist) and Quiddity's corpus (QUIDDITY_CORPUS, default
/Users/paul/repos/quiddity/tests/corpus; its inputs are not committed, its outputs are) -- then
the scenarios and the typed-spec cases. Each input file is captured in a fresh process.
``case`` captures one input (it must lie in one of those three roots). ``readback`` reads a
STEP back exactly as the oracle reads Python's written files (``readback.json.gz``: a list,
one entry per part), for checking a file the Rust port wrote; each read runs in a child
process, so an OpenCascade crash is recorded (``{"error": "crash"}``) rather than lost.
``cross-engine`` does that for every Rust-written ``rust.step.gz`` of the cross-engine check
(``python_readback.json.gz`` and ``python_meta.json.gz`` beside each; see
tools/capture/crossengine.py). ``make-scenarios`` rewrites the scenario inputs from specify-core's tests (see tools/capture/scenarios.py); ``all`` only runs
them. tests/fixtures/oracle/README.md describes every artifact.

A Python refusal or crash is an oracle result: it is recorded as ``{error, message}`` in place
of the value, and the run carries on. Output is reproducible: rerunning changes only the
``captured`` date in each meta.json.
"""

from __future__ import annotations

import argparse
import multiprocessing
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from capture import common  # noqa: E402
from capture.common import write_json_gz  # noqa: E402


def _quiet() -> None:
    """OCCT prints progress on stdout from C++: send file descriptor 1 to /dev/null (serve
    sends it to stderr), so this tool's log on stderr is only its own."""
    sys.stdout.flush()
    null = os.open(os.devnull, os.O_WRONLY)
    os.dup2(null, 1)
    os.close(null)


def _inputs(kinds: list[str]) -> list[tuple[str, str]]:
    from capture.cases import root_of

    out = []
    for kind in kinds:
        root = root_of(kind)
        if not root.is_dir():
            print(f"{kind}: {root} does not exist; skipped", file=sys.stderr)
            continue
        found = sorted(p for p in root.rglob("*") if common.is_input(p))
        out += [(kind, str(p.relative_to(root))) for p in found]
    return out


def _kind_of(path: Path) -> tuple[str, str]:
    from capture.cases import KINDS, root_of

    path = path.resolve()
    for kind in KINDS:
        root = root_of(kind).resolve()
        if path.is_relative_to(root):
            return kind, str(path.relative_to(root))
    raise SystemExit(
        f"{path} is in none of the capture's roots: {[str(root_of(k)) for k in KINDS]}"
    )


def _worker(job: tuple[str, str, str]) -> tuple[str, str, dict | str, float]:
    _quiet()
    from capture.cases import capture_file

    kind, rel, oracle = job
    started = time.monotonic()
    try:
        summary = capture_file(kind, rel, Path(oracle))
    except Exception as exc:  # noqa: BLE001 -- reported, the run carries on
        import traceback

        summary = f"capture failed: {exc!r}\n{traceback.format_exc()}"
    return kind, rel, summary, time.monotonic() - started


def capture_all(oracle: Path, kinds: list[str], jobs: int) -> int:
    inputs = _inputs(kinds)
    print(f"{len(inputs)} inputs, {jobs} at a time", file=sys.stderr)
    broken = 0
    context = multiprocessing.get_context("spawn")
    with context.Pool(jobs, maxtasksperchild=1) as pool:
        for n, (kind, rel, summary, seconds) in enumerate(
            pool.imap_unordered(_worker, [(k, r, str(oracle)) for k, r in inputs]), 1
        ):
            if isinstance(summary, str):
                broken += 1
                print(f"[{n}/{len(inputs)}] {kind}/{rel}: {summary}", file=sys.stderr)
                continue
            failures = "; ".join(summary["failures"]) or "no Python failures"
            cases = ", ".join(summary["cases"])
            print(
                f"[{n}/{len(inputs)}] {kind}/{rel} ({seconds:.0f} s): {cases}: {failures}",
                file=sys.stderr,
            )
    return broken


def _scenarios(oracle: Path, names: list[str] | None) -> int:
    from capture import scenarios

    skipped = scenarios.run(oracle, set(names) if names else None)
    for line in skipped:
        print(f"scenario not run: {line}", file=sys.stderr)
    return len(skipped)


def _interpret(oracle: Path) -> None:
    from capture import interpret

    write_json_gz(oracle / "interpret.json.gz", interpret.capture())


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="capture.py", description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("all")
    p.add_argument("oracle", type=Path)
    p.add_argument("--jobs", type=int, default=max(1, min(4, (os.cpu_count() or 2) // 2)))
    p.add_argument("--only", default="parts,nist,corpus", help="kinds to capture, comma separated")
    p = sub.add_parser("case")
    p.add_argument("input", type=Path)
    p.add_argument("oracle", type=Path)
    p = sub.add_parser("readback")
    p.add_argument("step", type=Path)
    p.add_argument("out", type=Path)
    p = sub.add_parser("cross-engine")
    p.add_argument("root", type=Path)
    p.add_argument("--rust-only", action="store_true", help="recapture Rust outputs; retain unchanged Python-written evidence")
    # Internal: one isolated read of ``cross-engine`` and ``readback`` (crossengine.child).
    p = sub.add_parser("readback-child")
    p.add_argument("step", type=Path)
    p.add_argument("which")
    p.add_argument("out", type=Path)
    p = sub.add_parser("scenarios")
    p.add_argument("oracle", type=Path)
    p.add_argument("names", nargs="*")
    p = sub.add_parser("interpret")
    p.add_argument("oracle", type=Path)
    p = sub.add_parser("make-scenarios")
    p.add_argument("oracle", type=Path)
    args = parser.parse_args(argv)

    if args.command == "all":
        kinds = [k for k in args.only.split(",") if k]
        broken = capture_all(args.oracle, kinds, args.jobs)
        _quiet()
        skipped = _scenarios(args.oracle, None)
        _interpret(args.oracle)
        return 1 if broken or skipped else 0
    _quiet()
    if args.command == "case":
        kind, rel = _kind_of(args.input)
        _, _, summary, seconds = _worker((kind, rel, str(args.oracle)))
        print(
            summary if isinstance(summary, str) else f"{summary} ({seconds:.0f} s)", file=sys.stderr
        )
        return 1 if isinstance(summary, str) else 0
    if args.command == "readback":
        from capture.crossengine import readback

        step = args.step.resolve()
        write_json_gz(
            args.out / "readback.json.gz", common.scrub(readback(step), {str(step): "{input}"})
        )
        return 0
    if args.command == "cross-engine":
        from capture import crossengine

        for line in crossengine.capture(args.root, python_written=not args.rust_only):
            print(line, file=sys.stderr)
        return 0
    if args.command == "readback-child":
        from capture import crossengine

        crossengine.child(args.step, args.which, args.out)
        return 0
    if args.command == "scenarios":
        return 1 if _scenarios(args.oracle, args.names) else 0
    if args.command == "interpret":
        _interpret(args.oracle)
        return 0
    from capture import scenarios

    names = scenarios.make(args.oracle)
    print(f"{len(names)} scenarios written", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
