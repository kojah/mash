"""Carry an index from one operating system to another through export/import.

    python .github/scripts/portability-smoke.py export path/to/mash DIRECTORY
    python .github/scripts/portability-smoke.py import path/to/mash DIRECTORY

`export` (run on Windows in CI) indexes a small Claude Code and OpenCode
history recorded in a Windows working directory, with CRLF line endings and
non-ASCII text, then exports it to DIRECTORY/export along with what a reader
should find. `import` (run on Linux) imports that export into a new database,
rebuilds its views offline and checks search, the exact project filter,
`list-chats` and `view` against those expectations. Standard library only.
"""

from __future__ import annotations

import json
import sqlite3
import subprocess
import sys
from pathlib import Path

PROJECT = "C:\\work\\portable café"
MARKERS = {
    "claude-code": "portableclaude",
    "opencode": "portableopencode",
}
TEXT = "café ✓ naïve"

# Lexical search needs no models, as `mash init` configures it without them.
VIEWS = '[views.build]\nrepresentations = "lexical-only"\n[views.query]\nintent = "exact-mention"\n'

# Required columns from upstream OpenCode core/session/sql.ts.
OPENCODE = """
PRAGMA journal_mode=WAL;
CREATE TABLE project(id TEXT PRIMARY KEY,worktree TEXT,vcs TEXT);
CREATE TABLE session(id TEXT PRIMARY KEY,project_id TEXT,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);
CREATE TABLE message(id TEXT PRIMARY KEY,session_id TEXT,time_created INTEGER,data TEXT);
CREATE TABLE part(id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,data TEXT);
CREATE TABLE session_message(id TEXT PRIMARY KEY,session_id TEXT,type TEXT,seq INTEGER,time_created INTEGER,data TEXT);
"""


def mash(binary: Path, root: Path, database: Path, *arguments: str) -> dict:
    result = subprocess.run(
        [
            str(binary),
            "--config",
            str(root / "config.toml"),
            "--db",
            str(database),
            "--models-dir",
            str(root / "models"),
            *arguments,
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=300,
    )
    if result.returncode != 0:
        raise SystemExit(
            f"mash {' '.join(arguments)} failed ({result.returncode}):\n{result.stdout}\n{result.stderr}"
        )
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    return json.loads(lines[-1])


def histories(root: Path) -> str:
    """Write the fixture histories and return their configuration."""
    claude = root / "claude" / "projects" / "portable"
    claude.mkdir(parents=True)
    record = {
        "type": "user",
        "sessionId": "portable-claude",
        "uuid": "c1",
        "cwd": PROJECT,
        "timestamp": "2026-10-28T09:00:00Z",
        "message": {"role": "user", "content": f"{MARKERS['claude-code']} {TEXT}"},
    }
    # Windows tools may end lines with CRLF.
    with open(
        claude / "portable-claude.jsonl", "w", encoding="utf-8", newline=""
    ) as file:
        file.write(json.dumps(record, ensure_ascii=False) + "\r\n")
    database = root / "opencode" / "opencode.db"
    database.parent.mkdir()
    connection = sqlite3.connect(database)
    connection.executescript(OPENCODE)
    connection.execute("INSERT INTO project VALUES('project',?,'git')", (PROJECT,))
    connection.execute(
        "INSERT INTO session VALUES('portable-opencode','project','Portable',?,1790726400000,1790726400000)",
        (PROJECT,),
    )
    connection.execute(
        "INSERT INTO session_message VALUES('o1','portable-opencode','user',1,1790726400000,?)",
        (json.dumps({"text": f"{MARKERS['opencode']} {TEXT}"}, ensure_ascii=False),),
    )
    connection.commit()
    connection.close()
    return (
        "version = 1\n"
        f"[providers.claude-code]\nroots = [{json.dumps(str(claude.parent))}]\n"
        f"[providers.opencode]\nroots = [{json.dumps(str(database))}]\n" + VIEWS
    )


def export(binary: Path, directory: Path) -> None:
    root = directory / "source"
    root.mkdir(parents=True)
    (root / "config.toml").write_text(histories(root), encoding="utf-8")
    database = root / "index.sqlite3"
    mash(binary, root, database, "index")
    expected = {}
    for provider, marker in MARKERS.items():
        reply = mash(
            binary, root, database, "search", marker, "--local", "--unit", "messages"
        )
        [hit] = reply["results"]
        expected[provider] = {
            "chat_id": hit["chat_id"],
            "project": hit["metadata"]["project"],
        }
        assert hit["metadata"]["project"] == PROJECT, hit["metadata"]
    mash(binary, root, database, "export", str(directory / "export"))
    (directory / "expected.json").write_text(
        json.dumps(expected, ensure_ascii=False), encoding="utf-8"
    )
    print(f"exported {len(expected)} chats recorded in {PROJECT!r}")


def import_(binary: Path, directory: Path) -> None:
    expected = json.loads((directory / "expected.json").read_text(encoding="utf-8"))
    root = directory / "reader"
    root.mkdir()
    (root / "config.toml").write_text(
        "version = 1\nproviders = {}\n" + VIEWS, encoding="utf-8"
    )
    database = root / "index.sqlite3"
    mash(
        binary,
        root,
        database,
        "import",
        "--into",
        str(database),
        str(directory / "export"),
    )
    mash(binary, root, database, "index", "--rebuild-local-views")
    for provider, marker in MARKERS.items():
        want = expected[provider]
        reply = mash(
            binary, root, database, "search", marker, "--local", "--unit", "messages"
        )
        [hit] = reply["results"]
        assert hit["chat_id"] == want["chat_id"], (hit["chat_id"], want)
        assert hit["metadata"]["project"] == PROJECT, hit["metadata"]
        # The exact project filter matches the Windows spelling here too.
        filtered = mash(
            binary,
            root,
            database,
            "search",
            marker,
            "--local",
            "--unit",
            "messages",
            "--project",
            PROJECT,
        )
        assert [result["chat_id"] for result in filtered["results"]] == [
            want["chat_id"]
        ], filtered
        message = mash(binary, root, database, "view", hit["lookup"]["view"]["id"])
        assert TEXT in json.dumps(message, ensure_ascii=False), message
    chats = mash(binary, root, database, "list-chats")
    listed = {chat["chat_id"] for chat in chats["chats"]}
    assert {want["chat_id"] for want in expected.values()} <= listed, listed
    print(f"imported and found {len(expected)} chats from {PROJECT!r}")


def main() -> int:
    mode, binary, directory = (
        sys.argv[1],
        Path(sys.argv[2]).resolve(),
        Path(sys.argv[3]).resolve(),
    )
    if mode == "export":
        export(binary, directory)
    elif mode == "import":
        import_(binary, directory)
    else:
        raise SystemExit(f"unknown mode {mode!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
