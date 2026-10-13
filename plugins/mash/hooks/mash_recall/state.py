"""Distillation watermarks and connector delivery state in Mash's data directory.

Layout under `<mash data dir>/recall/`:

- `projects/<project key>.json`: per-chat watermarks and the newest record anchor.
- `source.json`: the connector source revision and any unacknowledged batch.
- `lock`: serializes writers; readers never take it.

Records themselves live only in Mash silver; this state holds identifiers.
"""

from __future__ import annotations

import fcntl
import json  # Standard library only: runs in the host's interpreter.
import os
import sys
import tempfile
from collections.abc import Generator, Mapping
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path

from .json_value import int_at, mapping, text_at

STATE_ENV = "MASH_RECALL_STATE_DIR"
STATE_VERSION = 1


def mash_data_dir(environ: Mapping[str, str] | None = None) -> Path:
    """Mash's own data directory (`ProjectDirs::from("dev", "jamesqo", "mash")`)."""
    env = os.environ if environ is None else environ
    home = Path(env.get("HOME") or Path.home())
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "dev.jamesqo.mash"
    data_home = env.get("XDG_DATA_HOME", "")
    base = (
        Path(data_home)
        if data_home and Path(data_home).is_absolute()
        else home / ".local" / "share"
    )
    return base / "mash"


def state_dir(environ: Mapping[str, str] | None = None) -> Path:
    env = os.environ if environ is None else environ
    explicit = env.get(STATE_ENV, "").strip()
    return Path(explicit) if explicit else mash_data_dir(env) / "recall"


@dataclass(frozen=True)
class Watermark:
    """The newest message already distilled for one chat."""

    updated_at: int
    message_id: str


@dataclass
class ProjectState:
    project: str
    repository: str | None
    watermarks: dict[str, Watermark] = field(default_factory=dict)
    latest_record: str | None = None

    def advance(self, chat_id: str, mark: Watermark) -> bool:
        current = self.watermarks.get(chat_id)
        if current is not None and current.updated_at >= mark.updated_at:
            return False
        self.watermarks[chat_id] = mark
        return True


@dataclass
class SourceState:
    revision: int = 0
    pending: dict[str, object] | None = None


def read_json(path: Path) -> dict[str, object] | None:
    try:
        value = json.loads(path.read_bytes())
    except (OSError, ValueError):
        return None
    members = mapping(value)
    if members is None or members.get("version") != STATE_VERSION:
        return None
    return members


def write_json(path: Path, value: dict[str, object]) -> None:
    """Atomically replace `path` with a private file."""
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    payload = json.dumps({"version": STATE_VERSION, **value}, sort_keys=True).encode()
    descriptor, temporary = tempfile.mkstemp(prefix=".state-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(payload)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    except BaseException:
        Path(temporary).unlink(missing_ok=True)
        raise


def project_path(root: Path, key: str) -> Path:
    return root / "projects" / f"{key}.json"


def load_project(
    root: Path, key: str, project: str, repository: str | None
) -> ProjectState:
    state = ProjectState(project=project, repository=repository)
    value = read_json(project_path(root, key))
    if value is None:
        return state
    for chat_id, mark in (mapping(value.get("watermarks")) or {}).items():
        updated = int_at(mark, "updated_at")
        message_id = text_at(mark, "message_id")
        if updated is not None and message_id is not None:
            state.watermarks[chat_id] = Watermark(updated, message_id)
    state.latest_record = text_at(value, "latest_record")
    return state


def save_project(root: Path, key: str, state: ProjectState) -> None:
    write_json(
        project_path(root, key),
        {
            "project": state.project,
            "repository": state.repository,
            "latest_record": state.latest_record,
            "watermarks": {
                chat_id: {"updated_at": mark.updated_at, "message_id": mark.message_id}
                for chat_id, mark in sorted(state.watermarks.items())
            },
        },
    )


def load_source(root: Path) -> SourceState:
    value = read_json(root / "source.json")
    if value is None:
        return SourceState()
    revision = int_at(value, "revision")
    return SourceState(
        revision=revision if revision is not None and revision >= 0 else 0,
        pending=mapping(value.get("pending")),
    )


def save_source(root: Path, state: SourceState) -> None:
    write_json(
        root / "source.json", {"revision": state.revision, "pending": state.pending}
    )


@contextmanager
def locked(root: Path) -> Generator[None, None, None]:
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    with open(root / "lock", "a") as handle:
        fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
