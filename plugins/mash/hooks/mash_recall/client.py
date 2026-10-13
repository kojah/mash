"""Run the configured Mash executable and decode the read-only results we use."""

from __future__ import annotations

import json  # Standard library only: runs in the host's interpreter.
import os
import shutil
import subprocess
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

# An explicit executable wins. Otherwise a configured wrapper (which supplies
# --config/--db/--models-dir) is preferred over the bare binary, matching the
# skill's "configured mash executable" fallback.
from .json_value import at, int_at, items, text_at

BINARY_ENV = "MASH_BIN"
WRAPPER_NAMES = ("mash-dev", "mash")
MAX_OUTPUT_BYTES = 16 * 1024 * 1024
CONFLICT_EXIT = 12
MAX_ERROR_BYTES = 64 * 1024


class MashError(Exception):
    """Mash was missing, timed out, failed or returned something unexpected."""

    def __init__(self, message: str, exit_code: int | None = None) -> None:
        super().__init__(message)
        self.exit_code = exit_code


def resolve_binary(environ: Mapping[str, str] | None = None) -> str | None:
    """The Mash executable to run, or None when none is installed."""
    env = os.environ if environ is None else environ
    explicit = env.get(BINARY_ENV, "").strip()
    if explicit:
        return explicit if os.access(explicit, os.X_OK) else None
    search = env.get("PATH", os.defpath)
    local_bin = str(Path(env.get("HOME", str(Path.home()))) / ".local" / "bin")
    for name in WRAPPER_NAMES:
        found = shutil.which(name, path=search) or shutil.which(name, path=local_bin)
        if found:
            return found
    return None


@dataclass(frozen=True)
class Chat:
    """One native chat from `list-chats`, with the anchor of its newest message."""

    chat_id: str
    provider: str
    title: str | None
    project: str | None
    repository: str | None
    updated_at: int
    last_message_id: str | None


@dataclass(frozen=True)
class Message:
    """One message returned by `view`; content is Mash's redacted text."""

    id: str
    content: str
    created_at: int | None


@dataclass(frozen=True)
class Receipt:
    source_revision: int


class Mash:
    """A bounded runner for one Mash executable."""

    def __init__(self, binary: str) -> None:
        self.binary = binary

    def run(self, arguments: list[str], timeout: float) -> object:
        if timeout <= 0:
            raise MashError("deadline exceeded")
        try:
            result = subprocess.run(
                [self.binary, *arguments],
                stdin=subprocess.DEVNULL,
                capture_output=True,
                timeout=timeout,
                check=False,
            )
        except subprocess.TimeoutExpired:
            raise MashError("mash timed out") from None
        except OSError:
            raise MashError("mash is unavailable") from None
        if len(result.stdout) > MAX_OUTPUT_BYTES:
            raise MashError("mash output is too large")
        value = decode(result.stdout)
        if result.returncode != 0:
            stderr = decode(result.stderr[:MAX_ERROR_BYTES])
            message = text_at(value, "error", "message") or text_at(
                stderr, "error", "message"
            )
            raise MashError(message or "mash failed", result.returncode)
        if not isinstance(value, dict):
            raise MashError("mash returned unexpected output")
        return value

    def list_chats(
        self, offset: int, limit: int, timeout: float
    ) -> tuple[list[Chat], int | None]:
        """One local page, newest activity first."""
        value = self.run(
            ["list-chats", "--local", "--limit", str(limit), "--offset", str(offset)],
            timeout,
        )
        rows = at(value, "chats")
        chats = [chat for raw in items(rows) if (chat := decode_chat(raw))]
        return chats, int_at(value, "next_offset")

    def view(self, message_id: str, context: int, timeout: float) -> list[Message]:
        value = self.run(["view", message_id, "--context", str(context)], timeout)
        messages = []
        for raw in items(at(value, "messages")):
            identity = text_at(raw, "id")
            if identity is not None:
                messages.append(
                    Message(
                        id=identity,
                        content=text_at(raw, "content") or "",
                        created_at=int_at(raw, "created_at"),
                    )
                )
        return messages

    def deliver(self, batch_path: Path, timeout: float) -> Receipt:
        """Commit one connector batch to local silver (`index --connector-batch`)."""
        value = self.run(["index", "--connector-batch", str(batch_path)], timeout)
        revision = int_at(value, "source_revision")
        if revision is None:
            raise MashError("mash returned no connector receipt")
        return Receipt(source_revision=revision)


def decode(output: bytes) -> object:
    try:
        return json.loads(output or b"null")
    except ValueError:
        return None


def decode_chat(raw: object) -> Chat | None:
    """A native chat, or None for connector containers and malformed rows."""
    chat_id = text_at(raw, "chat_id")
    provider = text_at(raw, "source", "provider")
    updated = int_at(raw, "updated_at")
    if chat_id is None or provider is None or updated is None:
        return None
    return Chat(
        chat_id=chat_id,
        provider=provider,
        title=text_at(raw, "title"),
        project=text_at(raw, "project"),
        repository=text_at(raw, "repository"),
        updated_at=updated,
        last_message_id=text_at(
            raw, "enrichment", "timestamps", "updated_at", "message_id"
        ),
    )
