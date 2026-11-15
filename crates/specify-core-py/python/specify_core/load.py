"""A STEP file's part, loaded once: ``load``, ``LoadedPart`` and ``Binding``, as specify-core's
``load`` defines them.

The part is loaded by the Rust engine and stays there: a ``LoadedPart`` is a handle on it,
with what identifies it (``path``, ``binding``, ``part_count``, ``name``), and ``analyse``
takes it in place of a path, so a part used more than once is read once. specify-core's
``LoadedPart`` also carries OpenCascade's objects (its document, shape and faces, ``face``,
``index_of``); this one has none.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import _native

#: Bumped whenever a change could renumber faces for the same bytes (the engine's).
LOADER_VERSION = _native.LOADER_VERSION


@dataclass(frozen=True)
class Binding:
    """What face indices are only valid against."""

    source_sha256: str
    face_count: int
    loader_version: int = LOADER_VERSION
    #: Which of the file's parts (see ``parts``); 0 for a file of one part.
    part: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {
            "source_sha256": self.source_sha256,
            "face_count": self.face_count,
            "loader_version": self.loader_version,
            "part": self.part,
        }


class LoadedPart:
    """One part of a STEP file, loaded once."""

    def __init__(self, handle: _native.LoadedPart) -> None:
        self._handle = handle
        self.path = Path(handle.path)
        self.binding = Binding(**json.loads(handle.binding))
        #: How many parts the file holds; this is part ``binding.part`` of them.
        self.part_count: int = handle.part_count
        #: The part's name in the file, if it has one.
        self.name: str = handle.name

    def __repr__(self) -> str:
        return f"LoadedPart({str(self.path)!r}, part={self.binding.part})"


def load(
    step: str | Path | bytes,
    *,
    part: int | None = None,
    name: str | None = None,
) -> LoadedPart:
    """Read ``step`` -- a path, or the STEP file's ``bytes`` -- and take its ``part``-th part;
    a file of several parts must say which (``part`` takes a part of an assembly from bytes too).

    ``name`` stands for the file in messages and is the part's ``path`` when ``step`` is bytes
    (default ``memory.step``); it is only for bytes. The same file loaded from bytes and from a
    path is the same part: the same binding, face numbering and analysis.
    """
    if isinstance(step, (bytes, bytearray, memoryview)):
        return LoadedPart(_native.load(bytes(step), part, name))
    if name is not None:
        raise TypeError("name is for bytes; a path names itself")
    return LoadedPart(_native.load(os.fspath(step), part))
