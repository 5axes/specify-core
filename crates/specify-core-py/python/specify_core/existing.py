"""The PMI a file already states: ``ExistingPmi``, ``read_existing`` and ``part_settings``, as
specify-core's ``existing`` defines them. The reading is the Rust engine's.

The engine's records also carry a requirement's authorship and inspection number; specify-core
has neither, so they are left out here.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import _native
from .load import LoadedPart


@dataclass(frozen=True)
class ExistingPmi:
    kind: str  # dimension | geometric_tolerance | datum | note
    type: str  # OCCT type name without prefix, the datum letter, or the note's kind
    faces: tuple[int, ...]
    #: Second reference set, for two-sided dimensions.
    faces2: tuple[int, ...] = ()
    value: float | None = None
    upper: float | None = None
    lower: float | None = None
    datums: tuple[str, ...] = ()
    #: Absolute lower/upper bounds; upper/lower above remain deviations.
    limits: tuple[float, float] | None = None
    #: A basic (theoretically exact) dimension, with no tolerance of its own.
    basic: bool = False

    def to_dict(self) -> dict[str, Any]:
        out = {k: v for k, v in self.__dict__.items() if v not in (None, ())}
        if not self.basic:
            out.pop("basic")
        # A datum the file attaches to no face still has faces: none.
        out.setdefault("faces", ())
        for key in ("faces", "faces2", "datums", "limits"):
            if key in out:
                out[key] = list(out[key])
        return out


def read_existing(loaded: LoadedPart | str | Path) -> list[ExistingPmi]:
    records = json.loads(_native.read_existing(_step(loaded)))
    return [
        ExistingPmi(
            r["kind"],
            r["type"],
            tuple(r["faces"]),
            tuple(r["faces2"]),
            r["value"],
            r["upper"],
            r["lower"],
            tuple(r["datums"]),
            tuple(r["limits"]) if r["limits"] is not None else None,
            r["basic"],
        )
        for r in records
    ]


def part_settings(loaded: LoadedPart | str | Path) -> dict[str, Any]:
    """The material, general tolerance and notes the file states, as the writer writes them."""
    return json.loads(_native.part_settings(_step(loaded)))


def _step(loaded: LoadedPart | str | Path) -> Any:
    from .api import _path

    return loaded._handle if isinstance(loaded, LoadedPart) else _path(loaded)
