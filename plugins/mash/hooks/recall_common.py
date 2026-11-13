"""Shared executable, project lookup, and bounded local hook-state utilities."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Any


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


def saving(
    binary: str, session: Any, toplevel: str, repository: str | None
) -> str | None:
    """How an agent saves memory about its own session: the project's record
    scope and a citation by session ID, which needs no index. None when record
    capture is off, the session ID is not a plain token, or the installed
    binary predates session citations (a plugin can be newer than its binary)."""
    if os.environ.get("MASH_RECORD_CAPTURE", "").strip().lower() in (
        "0",
        "off",
        "false",
    ):
        return None
    if (
        not isinstance(session, str)
        or not 0 < len(session) <= 128
        or not all(c.isascii() and (c.isalnum() or c in "-_") for c in session)
    ):
        return None
    try:
        citations = run_mash(binary, ["--version-info"], 1.0).get("record_citations")
    except (OSError, subprocess.SubprocessError, TimeoutError):
        return None
    except MashError:
        # Releases before --version-info printed this from --version.
        try:
            citations = run_mash(binary, ["--version"], 1.0).get("record_citations")
        except (MashError, OSError, subprocess.SubprocessError, TimeoutError):
            return None
    if not isinstance(citations, list) or "session_id" not in citations:
        return None
    scope = (
        {"kind": "repository", "identity": repository}
        if repository
        else {"kind": "local_project", "path": toplevel}
    )
    citation = {"session_id": session, "quote": "THEIR EXACT WORDS"}
    return (
        "use the Mash MCP record tool with scope "
        + json.dumps(scope, separators=(",", ":"))
        + " and evidence "
        + json.dumps([{**citation, "purpose": "endorsement"}], separators=(",", ":"))
        + ". The quote is checked against this session's transcript, so no"
        " indexing is needed. Mark purpose endorsement only for the person's own"
        " words; save an uncertain interpretation with review candidate. A lasting"
        " choice you make yourself with a rationale, such as how to structure the"
        " code, is a decision with details.decided_by agent: cite your own message"
        " making it with purpose report and review established"
    )


def path_key(path: str) -> str:
    """A path for comparison: forward slashes, no trailing slash, and for
    Windows drive paths (case-insensitive there) lowercase. Git reports
    "C:/Users/x" where transcripts may record "C:\\Users\\x"."""
    key = path.replace("\\", "/").rstrip("/")
    return key.lower() if len(key) > 1 and key[1] == ":" else key


def state_dir() -> Path:
    """`~/.mash/recall`, inside Mash's own directory on every platform."""
    if explicit := os.environ.get("MASH_RECALL_STATE_DIR", "").strip():
        return Path(explicit)
    return Path.home() / ".mash" / "recall"


def read_state(path: Path) -> dict[str, Any]:
    """A versioned state file, or {} when missing or unreadable."""
    try:
        value = json.loads(path.read_bytes())
    except (OSError, ValueError):
        return {}
    return value if isinstance(value, dict) and value.get("version") == 1 else {}


def write_state(path: Path, value: dict[str, Any]) -> None:
    """Atomically replace hook state; concurrent writers may lose an update.

    State only throttles indexing and repeated hints, so a lost update can cause
    redundant work but cannot alter canonical history.
    """
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    body = json.dumps({**value, "version": 1}, sort_keys=True).encode()
    descriptor, temporary = tempfile.mkstemp(prefix=".state-", dir=path.parent)
    with os.fdopen(descriptor, "wb") as output:
        output.write(body)
    os.replace(temporary, path)
