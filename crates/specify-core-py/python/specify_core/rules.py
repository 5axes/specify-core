"""The questions a part needs and the intent its answers amount to: ``Question``, ``Intent``
and ``IncompleteError``, as specify-core's ``rules`` defines them.

The questions and the intent are the Rust engine's (``specify_core._native``); these are the
types they come back as, field for field, so a caller of specify-core's Python engine reads
them unchanged. ``questions`` is ``api.questions``, under the name specify-core's ``rules``
also gives it; ``with_all_defaults`` is the engine's.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from . import _native

#: The rules' version, stated in every intent (the engine's).
RULES_VERSION = _native.RULES_VERSION

#: How holes are located: by position tolerances from datums, or by ± dimensions
#: on the drawing under the general tolerance, which needs no datums.
LOCATING = ("position tolerances", "± dimensions")

#: How much each question needs a person, so a part can be reviewed rather
#: than interrogated: ``required`` has no default and must be answered;
#: ``check`` has a default the rules doubt, with a ``reason``; ``routine`` has
#: a default from a standard or the geometry, accepted unless changed;
#: ``refine`` is an optional tightening that defaults to none or general.
ATTENTION = ("required", "check", "routine", "refine")


@dataclass(frozen=True)
class Question:
    id: str
    kind: str  # choice | text | faces
    prompt: str
    faces: tuple[int, ...]  # what to highlight
    default: Any = None
    options: tuple[str, ...] = ()
    why: str = ""
    feature: str | None = None
    #: For a position: the datum frame it is measured from, e.g. ``A|B|C``.
    frame: str | None = None
    #: How much a person needs to look at it; see ``ATTENTION``.
    attention: str = "routine"
    #: Why a ``check`` question was flagged.
    reason: str = ""
    #: When identical features are asked about once: each one's id and faces.
    #: Each still gets its own requirements.
    parts: tuple[tuple[str, tuple[int, ...]], ...] = ()
    #: For a text question: common answers to offer as it is typed; any text is accepted.
    suggestions: tuple[Any, ...] = ()
    #: For a hole or turned diameter: each option's purpose, label, code and callout.
    choices: tuple[dict[str, Any], ...] = ()
    #: The feature's diameter, against which a typed spec is checked.
    diameter: float | None = None
    #: For a blind hole: its drill depth, against which a typed thread depth is checked.
    depth: float | None = None
    #: Why the default is what it is, in a sentence: shown as a choice made for
    #: the person, to keep or change.
    basis: str = ""
    #: When the default comes from how the part fits another in its assembly: why.
    mate: str = ""
    optional: bool = False

    def to_dict(self) -> dict[str, Any]:
        out = {
            "id": self.id,
            "kind": self.kind,
            "prompt": self.prompt,
            "faces": list(self.faces),
            "default": list(self.default) if isinstance(self.default, tuple) else self.default,
            "why": self.why,
        }
        if self.optional:
            out["optional"] = True
        if self.options:
            out["options"] = list(self.options)
        if self.feature:
            out["feature"] = self.feature
        if self.frame:
            out["frame"] = self.frame
        out["attention"] = self.attention
        if self.reason:
            out["reason"] = self.reason
        if self.parts:
            out["features"] = [feature for feature, _ in self.parts]
        if self.suggestions:
            out["suggestions"] = list(self.suggestions)
        if self.choices:
            out["choices"] = list(self.choices)
        if self.basis:
            out["basis"] = self.basis
        return out

    @classmethod
    def from_record(cls, record: dict[str, Any]) -> Question:
        """A question from the engine's record of every field (JSON: lists for tuples)."""
        default = record["default"]
        return cls(
            id=record["id"],
            kind=record["kind"],
            prompt=record["prompt"],
            faces=tuple(record["faces"]),
            # A datum's default is its faces, a tuple in Python's rules.
            default=tuple(default) if isinstance(default, list) else default,
            options=tuple(record["options"]),
            why=record["why"],
            feature=record["feature"],
            frame=record["frame"],
            attention=record["attention"],
            reason=record["reason"],
            parts=tuple((id_, tuple(faces)) for id_, faces in record["parts"]),
            suggestions=tuple(record["suggestions"]),
            choices=tuple(record["choices"]),
            diameter=record["diameter"],
            depth=record["depth"],
            basis=record["basis"],
            mate=record["mate"],
            optional=record.get("optional", False),
        )



@dataclass
class Intent:
    binding: dict[str, Any]
    part: dict[str, Any] = field(default_factory=dict)
    datums: list[dict[str, Any]] = field(default_factory=list)
    requirements: list[dict[str, Any]] = field(default_factory=list)
    schema: int = 1
    rules_version: int = RULES_VERSION
    #: The dimensions the general tolerance covers (``general.dimensions``):
    #: listed for inspection and shown, not written into the STEP.
    general: list[dict[str, Any]] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "schema": self.schema,
            "rules_version": self.rules_version,
            "binding": self.binding,
            "part": self.part,
            "datums": self.datums,
            "requirements": self.requirements,
            "general": self.general,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> Intent:
        if data.get("schema") != 1:
            raise ValueError(f"unsupported intent schema {data.get('schema')!r}")
        return cls(
            data["binding"],
            data.get("part", {}),
            data.get("datums", []),
            data.get("requirements", []),
            data["schema"],
            data.get("rules_version", 0),
            data.get("general", []),
        )


def questions(analysis: dict[str, Any], answers: dict[str, Any]) -> list[Question]:
    from .api import questions

    return questions(analysis, answers)


def with_all_defaults(analysis: dict[str, Any], answers: dict[str, Any]) -> dict[str, Any]:
    """``answers`` plus every default, to a fixed point: some questions only
    exist once an earlier default is taken."""
    import json

    return json.loads(_native.with_all_defaults(json.dumps(analysis), json.dumps(answers)))


class IncompleteError(ValueError):
    def __init__(
        self, missing: list[str], part: int | None = None,
        carried: dict[str, str] | None = None,
    ) -> None:
        # In an assembly, which part's questions they are.
        of = f"part {part}: " if part is not None else ""
        self.carried = carried or {}
        if self.carried:
            shown = [
                f"{key} (carried answer could not be applied: {self.carried[key]})"
                if key in self.carried else key for key in missing
            ]
            super().__init__(of + ", ".join(shown))
        else:
            super().__init__(f"{of}{len(missing)} question(s) unanswered: {', '.join(missing)}")
        self.missing = missing
        self.part = part
