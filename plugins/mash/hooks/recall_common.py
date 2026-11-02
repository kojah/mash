"""What the SessionStart hook and the record helper share: the Mash executable,
the session's project, its watermark file and the record text format. Standard
library only, because both scripts run in the agent host's own `python3`."""

from __future__ import annotations

import calendar
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

SOURCE_ID = "mash-recall"
ENTRY_PREFIX = f"connector#entry#{SOURCE_ID}#"
# `view` returns a record's text but not its connector kind or metadata, so the
# text carries supersession: a replaced record's first two lines are
# "Superseded <heading>" and "Superseded by: <entry id> (<its heading>)", and
# its replacement lists "Supersedes: <entry id> (<heading>)" before "Evidence:".
SUPERSEDED, SUPERSEDED_BY, SUPERSEDES = "Superseded ", "Superseded by: ", "Supersedes: "
# The UTC days a record was recorded or re-recorded, oldest first, listed before
# "Evidence:". A record written before this line existed has only its time.
CONFIRMED = "Confirmed: "
CONFIRMATIONS = 10  # Older days add little under power-law decay.


def superseded_by(content: str) -> str | None:
    """The record ID that replaced this record's text, or None while current."""
    heading, _, rest = content.partition("\n")
    marker = rest.partition("\n")[0]
    if not heading.startswith(SUPERSEDED) or not marker.startswith(SUPERSEDED_BY):
        return None
    return short_id(marker[len(SUPERSEDED_BY) :].partition(" ")[0]) or None


def confirmations(content: str, created_at: int) -> list[str]:
    """The days a record was confirmed, oldest first."""
    header = content.partition("\nEvidence:")[0].split("\n")
    for line in header:
        if line.startswith(CONFIRMED):
            return [day.strip() for day in line[len(CONFIRMED) :].split(",")]
    return [day(created_at)]


def day(milliseconds: int) -> str:
    return time.strftime("%Y-%m-%d", time.gmtime(milliseconds // 1000))


def strength(days: list[str], now_ms: int) -> float:
    """ACT-R base-level activation: each confirmation decays as age^-0.5 in days,
    so a statement confirmed repeatedly outranks one recorded once, recently."""
    total = 0.0
    for value in days:
        try:
            then = calendar.timegm(time.strptime(value, "%Y-%m-%d"))
        except ValueError:
            continue
        total += max(1.0, (now_ms / 1000 - then) // 86400) ** -0.5
    return math.log1p(total)


def project_records(
    binary: str, latest: str | None, window: int, timeout: float
) -> list[Any]:
    """Current records of a project as `view` rows, newest first, read from
    silver around its latest record. Superseded records keep their place and
    time, so the window is wider than the number used."""
    if not latest:
        return []
    view = run_mash(binary, ["view", latest, "--context", str(window)], timeout)
    rows = sorted(view["messages"], key=lambda row: -(row.get("created_at") or 0))
    return [
        row
        for row in rows
        if row["id"].startswith(ENTRY_PREFIX)
        and superseded_by(row.get("content") or "") is None
    ]


def short_id(value: str) -> str:
    """A record ID without the connector entry prefix that `view` needs."""
    value = value.strip()
    return value[len(ENTRY_PREFIX) :] if value.startswith(ENTRY_PREFIX) else value


def resolve_binary() -> str | None:
    """`$MASH_BIN`, else a configured `mash-dev` wrapper, else `mash`."""
    explicit = os.environ.get("MASH_BIN", "").strip()
    if explicit:
        return explicit if os.access(explicit, os.X_OK) else None
    local_bin = str(Path.home() / ".local" / "bin")
    for name in ("mash-dev", "mash"):
        found = shutil.which(name) or shutil.which(name, path=local_bin)
        if found:
            return found
    return None


class MashError(Exception):
    def __init__(self, message: str, code: int) -> None:
        super().__init__(message)
        self.code = code


def run(command: list[str], timeout: float) -> subprocess.CompletedProcess[bytes]:
    if timeout <= 0:
        raise TimeoutError("deadline exceeded")
    return subprocess.run(
        command, stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout
    )


def run_mash(binary: str, arguments: list[str], timeout: float) -> dict[str, Any]:
    """Mash's JSON output; raises on failure, timeout or a non-object result."""
    result = run([binary, *arguments], timeout)
    try:
        value = json.loads(result.stdout or b"null")
    except ValueError:
        value = None
    if result.returncode == 0 and isinstance(value, dict):
        return value
    error = value.get("error") if isinstance(value, dict) else None
    detail = error.get("message") if isinstance(error, dict) else None
    raise MashError(detail or "mash failed", result.returncode)


def git(directory: str, arguments: list[str], timeout: float) -> str | None:
    result = run(["git", "-C", directory, *arguments], timeout)
    text = result.stdout.decode(errors="replace").strip()
    return text if result.returncode == 0 and text else None


def resolve_project(cwd: str, timeout: float) -> tuple[str, str | None]:
    """Git toplevel and origin URL of `cwd`; a non-Git directory is its own."""
    if not Path(cwd).is_absolute() or not Path(cwd).is_dir():
        raise ValueError(f"not a directory: {cwd}")
    toplevel = git(cwd, ["rev-parse", "--show-toplevel"], timeout / 2) or cwd
    origin = git(toplevel, ["config", "--get", "remote.origin.url"], timeout / 2)
    return toplevel, origin


def path_key(path: str) -> str:
    """A path for comparison: forward slashes, no trailing slash, and for
    Windows drive paths (case-insensitive there) lowercase. Git reports
    "C:/Users/x" where transcripts may record "C:\\Users\\x"."""
    key = path.replace("\\", "/").rstrip("/")
    return key.lower() if len(key) > 1 and key[1] == ":" else key


def project_key(toplevel: str, repository: str | None) -> str:
    """The origin URL shared by every clone and worktree, else the checkout path."""
    identity = f"repository\0{repository}" if repository else f"path\0{toplevel}"
    return hashlib.sha256(identity.encode()).hexdigest()[:24]


def state_dir() -> Path:
    """`<Mash data dir>/recall`, matching Mash's `ProjectDirs` location."""
    if explicit := os.environ.get("MASH_RECALL_STATE_DIR", "").strip():
        return Path(explicit)
    if sys.platform == "darwin":
        return Path.home() / "Library/Application Support/dev.jamesqo.mash/recall"
    data = os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local/share")
    return Path(data) / "mash" / "recall"


def read_state(path: Path) -> dict[str, Any]:
    """A versioned state file, or {} when missing or unreadable."""
    try:
        value = json.loads(path.read_bytes())
    except (OSError, ValueError):
        return {}
    return value if isinstance(value, dict) and value.get("version") == 1 else {}


def write_state(path: Path, value: dict[str, Any]) -> None:
    """Atomically replace `path`. Writers take no lock, so two helpers finishing
    at once can lose one's update: a lost watermark only lists a chat again, and
    `record.deliver` recovers a revision that fell one behind."""
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    body = json.dumps({**value, "version": 1}, sort_keys=True).encode()
    descriptor, temporary = tempfile.mkstemp(prefix=".state-", dir=path.parent)
    with os.fdopen(descriptor, "wb") as output:
        output.write(body)
    os.replace(temporary, path)
