"""What a write reports: ``WriteReport`` and ``VerificationError``, as specify-core's ``writer``
defines them. The writing is the Rust engine's (``api.write``, ``api.write_parts``).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class WriteReport:
    output: Path
    written: list[str] = field(default_factory=list)
    #: Intent that has no place in the file yet; it must reach the drawing another way.
    not_written: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    geometry_carrier_exemptions: list[dict[str, Any]] = field(default_factory=list)
    presentation_warnings: list[str] = field(default_factory=list)
    preserved_normalizations: list[dict[str, Any]] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        result = {
            "output": str(self.output),
            "written": self.written,
            "not_written": self.not_written,
            "warnings": self.warnings,
        }
        if self.geometry_carrier_exemptions:
            result["geometry_carrier_exemptions"] = self.geometry_carrier_exemptions
        if self.presentation_warnings:
            result["presentation_warnings"] = self.presentation_warnings
        if self.preserved_normalizations:
            result["preserved_normalizations"] = self.preserved_normalizations
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> WriteReport:
        return cls(
            Path(data["output"]),
            data["written"],
            data["not_written"],
            data["warnings"],
            data.get("geometry_carrier_exemptions", []),
            data.get("presentation_warnings", []),
            data.get("preserved_normalizations", []),
        )


class VerificationError(RuntimeError):
    pass
