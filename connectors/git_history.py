"""Local git commit history as a Mash source.

A repository is a container; each commit is an entry with its author, message
and parent hashes. History is a DAG, so commits carry no linear sequence:
recency comes from created_at and the parent hashes stay in metadata. The
source key is repo-scoped and one node owns it (source-model D2), even though
the reader is local.
"""

import subprocess
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path

from mash_sdk import Author, AuthorKind, Object, Role

RECORD = "\x1e"
FIELD = "\x1f"
FORMAT = f"{RECORD}%H{FIELD}%an{FIELD}%ae{FIELD}%aI{FIELD}%P{FIELD}%B"
KIND_CHAT = "git:repository"
KIND_COMMIT = "git:commit"


@dataclass(frozen=True)
class Settings:
    path: Path
    account: str
    since_days: int = 90

    def __post_init__(self) -> None:
        if self.since_days <= 0:
            raise ValueError("since_days must be positive")

    @property
    def source_id(self) -> str:
        # Repo-scoped and machine-independent: two clones are one dataset.
        return f"git:{self.account}"

    def since(self, now: datetime | None = None) -> str:
        start = (now or datetime.now(UTC)) - timedelta(days=self.since_days)
        return start.isoformat()


def chat(settings: Settings) -> Object:
    return Object(
        role=Role.container,
        kind=KIND_CHAT,
        id=settings.account,
        title=settings.account,
    )


def commit(settings: Settings, record: str) -> Object:
    sha, name, email, authored, parents, body = record.split(FIELD, 5)
    return Object(
        role=Role.entry,
        kind=KIND_COMMIT,
        id=sha,
        parent=settings.account,
        content=body.strip(),
        author=Author(id=email.strip() or "unknown", name=name.strip() or None, kind=AuthorKind.human),
        created_at=_milliseconds(authored.strip()),
        metadata={"parents": [parent for parent in parents.split() if parent]},
    )


def _milliseconds(value: str) -> int | None:
    if not value:
        return None
    try:
        moment = datetime.fromisoformat(value)
    except ValueError:
        return None
    return int(moment.timestamp() * 1000)


def read(settings: Settings, now: datetime | None = None) -> list[Object]:
    """Container plus every commit in the window, newest first."""
    result = subprocess.run(
        [
            "git",
            "-C",
            str(settings.path),
            "log",
            f"--since={settings.since(now)}",
            f"--format={FORMAT}",
        ],
        capture_output=True,
        check=True,
        text=True,
    )
    objects = [chat(settings)]
    for record in result.stdout.split(RECORD):
        if record.strip():
            objects.append(commit(settings, record))
    return objects
