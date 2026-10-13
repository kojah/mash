"""Shared fixtures: a temporary state directory, a fake Mash and Git projects."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

HOOKS = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HOOKS))

NOW_MS = 1_800_000_000_000
MINUTE_MS = 60_000


def chat(
    chat_id: str,
    updated_at: int,
    project: str | None,
    repository: str | None = None,
    title: str | None = "A chat",
    last: str | None = "last",
    provider: str = "claude-code",
) -> dict[str, object]:
    row: dict[str, object] = {
        "chat_id": chat_id,
        "source": {"provider": provider},
        "title": title,
        "project": project,
        "repository": repository,
        "created_at": updated_at - MINUTE_MS,
        "updated_at": updated_at,
    }
    if last is not None:
        row["enrichment"] = {
            "timestamps": {
                "updated_at": {"kind": "message-boundary", "message_id": last}
            }
        }
    return row


class Environment(unittest.TestCase):
    """A private state directory and a fake `mash` reachable through MASH_BIN."""

    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.state = self.root / "state"
        self.fixture = self.root / "fixture.json"
        self.binary = self.root / "mash"
        fake = HOOKS / "tests" / "fake_mash.py"
        self.binary.write_text(f'#!/bin/sh\nexec {sys.executable} {fake} "$@"\n')
        self.binary.chmod(0o755)
        self.write_fixture(chats=[], messages=[])
        self.environment = {
            **os.environ,
            "MASH_BIN": str(self.binary),
            "MASH_RECALL_STATE_DIR": str(self.state),
            "FAKE_MASH_FIXTURE": str(self.fixture),
        }
        self.saved = dict(os.environ)
        os.environ.update(self.environment)

    def tearDown(self) -> None:
        os.environ.clear()
        os.environ.update(self.saved)
        self.directory.cleanup()

    def write_fixture(self, **values: object) -> None:
        data: dict[str, object] = {
            "chats": [],
            "messages": [],
            "revision": 0,
            "batches": [],
            "sleep": 0,
            **values,
        }
        self.fixture.write_text(json.dumps(data))

    def read_fixture(self) -> Any:
        """Decoded test JSON; assertions check its shape."""
        return json.loads(self.fixture.read_text())

    def git_project(self, origin: str | None = "https://example.test/repo.git") -> Path:
        project = self.root / "repo"
        (project / "sub").mkdir(parents=True)
        subprocess.run(["git", "init", "-q", str(project)], check=True)
        if origin:
            subprocess.run(
                ["git", "-C", str(project), "remote", "add", "origin", origin],
                check=True,
            )
        return project.resolve()

    def run_script(
        self, name: str, stdin: bytes, *arguments: str, timeout: float = 30
    ) -> subprocess.CompletedProcess[bytes]:
        return subprocess.run(
            [sys.executable, str(HOOKS / name), *arguments],
            input=stdin,
            capture_output=True,
            env=self.environment,
            timeout=timeout,
            check=False,
        )
