"""Walk a new user's first run on a fresh home directory, against a real binary.

    python .github/scripts/first-run-smoke.py path/to/mash

Seeds Claude Code and Codex histories where those agents keep them, then
follows what a new user does: runs `mash index` before setting anything up
(which must point at `mash init`), runs `mash init`, searches, and checks
`mash status`. Prints the time to the first successful search. Unix only:
Windows resolves profile folders through the shell rather than HOME, so the
Windows workflow runs init and search in its runner's real profile instead.
Standard library only.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

CLAUDE = "firstrunclaude"
CODEX = "firstruncodex"


def run(binary: Path, home: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    environment = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith(("XDG_", "MASH_", "CODEX_", "CLAUDE_"))
    }
    environment["HOME"] = str(home)
    return subprocess.run(
        [str(binary), *arguments],
        capture_output=True,
        text=True,
        encoding="utf-8",
        env=environment,
        timeout=300,
    )


def reply(result: subprocess.CompletedProcess[str]) -> dict:
    """The command's JSON reply: its last stdout line, or its stderr error."""
    output = result.stdout if result.returncode == 0 else result.stderr
    lines = [line for line in output.splitlines() if line.strip()]
    return json.loads(lines[-1])


def seed(home: Path) -> None:
    claude = home / ".claude" / "projects" / "-work-first"
    claude.mkdir(parents=True)
    (claude / "first-claude.jsonl").write_text(
        json.dumps(
            {
                "type": "user",
                "sessionId": "first-claude",
                "uuid": "c1",
                "cwd": "/work/first",
                "timestamp": "2026-10-28T09:00:00Z",
                "message": {
                    "role": "user",
                    "content": f"{CLAUDE} how do I rotate the logs",
                },
            }
        )
        + "\n",
        encoding="utf-8",
    )
    codex = home / ".codex" / "sessions" / "2026" / "10" / "28"
    codex.mkdir(parents=True)
    records = [
        {
            "timestamp": "2026-10-28T10:00:00Z",
            "type": "session_meta",
            "payload": {
                "id": "first-codex",
                "timestamp": "2026-10-28T10:00:00Z",
                "cwd": "/work/first",
            },
        },
        {
            "timestamp": "2026-10-28T10:00:01Z",
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": f"{CODEX} fix the flaky test"}
                ],
            },
        },
    ]
    (codex / "rollout-2026-10-28T10-00-00-first-codex.jsonl").write_text(
        "".join(json.dumps(record) + "\n" for record in records), encoding="utf-8"
    )


def main() -> int:
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory() as directory:
        home = Path(directory)
        seed(home)
        started = time.monotonic()
        # 1. Indexing before setup fails, and says how to set up.
        early = run(binary, home, "index")
        assert early.returncode != 0, early
        message = reply(early)["error"]["message"]
        assert "mash init" in message, message
        print(f"index before init points at setup: {message}")
        # 2. Setup finds both agents' histories without being told where.
        init = run(binary, home, "init", "--no-agents")
        assert init.returncode == 0, init
        providers = reply(init)["config"]["providers"]
        assert {"claude-code", "codex"} <= providers.keys(), providers
        # 3. The first search finds both, with no models installed.
        for marker in (CLAUDE, CODEX):
            search = run(binary, home, "search", marker, "--local")
            assert search.returncode == 0, search
            assert marker in search.stdout, search.stdout
        first_search = time.monotonic() - started
        # 4. Status agrees that search is ready.
        status = run(binary, home, "status")
        assert status.returncode == 0, status
        assert reply(status)["ready"] is True, status.stdout
        print(f"init found {sorted(providers)}; first search after {first_search:.1f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
