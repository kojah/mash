"""Desired Apple Notes adapter: a consistent read-only SQLite snapshot feeds Documents."""

from dataclasses import dataclass
from math import isfinite
from pathlib import Path

from ..scope import Scope


@dataclass(frozen=True)
class Settings:
    database_path: Path
    account_scope: str
    scope: Scope
    poll_seconds: float = 10

    def __post_init__(self) -> None:
        if not isfinite(self.poll_seconds) or self.poll_seconds <= 0:
            raise ValueError("poll_seconds must be finite and positive")


# The source adapter implements notes.Notes:
# - Scope IDs are stable notebook/folder IDs, not display names.
# - Open the original DB read-only and use SQLite backup for a consistent snapshot,
#   including WAL data. Never update Apple's tables or copy only the live main DB.
# - Inspect/version the native schema; decode native note text into a typed Note.
#   Evaluate existing parsers before writing a new compressed-Protobuf decoder:
#   https://github.com/RhetTbull/apple-notes-parser
#   https://github.com/threeplanetssoftware/apple_cloud_notes_parser
#   A schema error, inaccessible/locked note, or missing permission is not a deletion.
# - Use source UUIDs where available, not row numbers alone. Poll and compare
#   canonical fingerprints; unrelated table churn must not republish every note.
# - Extract metadata and attachment references separately; the DB alone may not
#   contain attachment files. Local attachment transport still needs implementation.
# - Feed notes.run(source, source_id=f"apple-notes:{settings.account_scope}", ...).
