"""Reconstruct exact captured source units; never emit their bodies in reports."""
from dataclasses import asdict, dataclass

from pair_review import Key
from preceding_context import Role
from reference_review import digest
from source_spans import Kind, structural_spans


@dataclass(frozen=True)
class Unit:
    key: Key
    role: Role
    kind: Kind
    start: int
    end: int
    source_id: str
    source_sha256: str
    text: str

    def metadata(self) -> dict:
        return {"key": asdict(self.key), "role": self.role, "kind": self.kind,
                "start": self.start, "end": self.end, "source_id": self.source_id,
                "source_sha256": self.source_sha256, "text_sha256": digest(self.text.encode())}


def reconstruct(documents: dict, report: dict) -> tuple[Unit, ...]:
    if (report["schema_version"] != 1
            or report["scope"] != "development-admitted-anchors-and-oracle-reference-evidence-spans"
            or report["offset_unit"] != "unicode-code-points"):
        raise ValueError("unsupported captured unit scope")
    rows = {(r["thread"], r["identity"]["ordinal"]): r for r in report["rows"]}
    if len(rows) != len(report["rows"]) or set(rows) != set(documents):
        raise ValueError("captured source scope changed")
    units = []
    for (thread, ordinal), (identity, text) in sorted(documents.items()):
        row = rows[(thread, ordinal)]
        if (row["identity"] != identity or row["source_chars"] != len(text)
                or row["source_bytes"] != len(text.encode())
                or identity["searchable_sha256"] != digest(text.encode())):
            raise ValueError("captured source identity or body changed")
        spans = structural_spans(text)
        rebuilt = [{**asdict(s), "text_sha256": digest(text[s.start:s.end].encode()),
                    "source_bytes": len(text[s.start:s.end].encode())} for s in spans]
        if rebuilt != row["spans"]:
            raise ValueError("captured source offsets or text changed")
        for i, span in enumerate(spans):
            units.append(Unit(Key(thread, ordinal, i), Role(identity["role"]), span.kind,
                              span.start, span.end, identity["id"], identity["searchable_sha256"],
                              text[span.start:span.end]))
    return tuple(units)
