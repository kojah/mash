"""Index histories that their application is still writing, against a real binary.

    python .github/scripts/live-writer-smoke.py path/to/mash[.exe]

Agents and editors keep their history files open while Mash reads them. A
writer process here holds an OpenCode WAL database and a Claude Code JSONL
file open the way they do, committing new records throughout, and once holds
a write transaction open across a whole `mash index`. Each index must succeed,
find every record committed before it started, and leave the upstream
database's contents untouched. This matters most on Windows, where file
sharing and SQLite locking differ from Unix. Standard library only.
"""

from __future__ import annotations

import json
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# Required columns from upstream OpenCode core/session/sql.ts, as in the
# adapter's own tests.
SCHEMA = """
PRAGMA journal_mode=WAL;
CREATE TABLE project(id TEXT PRIMARY KEY,worktree TEXT,vcs TEXT);
CREATE TABLE session(id TEXT PRIMARY KEY,project_id TEXT,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);
CREATE TABLE message(id TEXT PRIMARY KEY,session_id TEXT,time_created INTEGER,data TEXT);
CREATE TABLE part(id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,data TEXT);
CREATE TABLE session_message(id TEXT PRIMARY KEY,session_id TEXT,type TEXT,seq INTEGER,time_created INTEGER,data TEXT);
INSERT INTO project VALUES('project','/work/live','git');
INSERT INTO session VALUES('live','project','Live session','/work/live',1790726400000,1790726400000);
"""

# The writer runs as its own process so that its file handles and SQLite locks
# belong to another process, as an agent's would. It commits one OpenCode row
# and appends one Claude line per step, keeping both open throughout. Writing
# a step number to `hold` makes it take the write lock at that step and keep
# it until the file `release` appears.
WRITER = r"""
import json, sqlite3, sys, time
from pathlib import Path
database, jsonl, control = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
connection = sqlite3.connect(database, isolation_level=None)
connection.execute("PRAGMA busy_timeout=10000")
history = open(jsonl, "a", encoding="utf-8")
step = 0
while not (control / "stop").exists():
    step += 1
    held = (control / "hold").exists() and int((control / "hold").read_text()) == step
    if held:
        connection.execute("BEGIN IMMEDIATE")
        (control / "holding").write_text(str(step))
        while not (control / "release").exists():
            time.sleep(0.05)
        connection.execute("COMMIT")
    connection.execute(
        "INSERT INTO session_message VALUES(?,?,?,?,?,?)",
        (f"m{step}", "live", "user", step, 1790726400000 + step,
         json.dumps({"text": f"opencodelive{step} marker"})),
    )
    history.write(json.dumps({
        "type": "user", "sessionId": "claude-live", "uuid": f"c{step}",
        "cwd": "/work/live", "timestamp": f"2026-10-28T09:{step // 60 % 60:02d}:{step % 60:02d}Z",
        "message": {"role": "user", "content": f"claudelive{step} marker"},
    }) + "\n")
    history.flush()
    (control / "step").write_text(str(step))
    time.sleep(0.05)
history.close()
connection.close()
"""


def mash(binary: Path, root: Path, *arguments: str) -> dict:
    result = subprocess.run(
        [
            str(binary),
            "--config",
            str(root / "config.toml"),
            "--db",
            str(root / "index.sqlite3"),
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


def written(control: Path) -> int:
    """The last step the writer finished; it rewrites the file in place, so a
    read can catch it empty and simply retries."""
    path = control / "step"
    while True:
        try:
            return int(path.read_text()) if path.exists() else 0
        except ValueError:
            time.sleep(0.01)


def wait_for(condition, what: str) -> None:
    deadline = time.monotonic() + 30
    while not condition():
        if time.monotonic() > deadline:
            raise SystemExit(f"timed out waiting for {what}")
        time.sleep(0.05)


def found(binary: Path, root: Path, marker: str) -> bool:
    reply = mash(binary, root, "search", marker, "--local")
    return marker in json.dumps(reply)


def upstream_rows(database: Path) -> list:
    connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    try:
        return connection.execute(
            "SELECT * FROM session_message ORDER BY seq"
        ).fetchall()
    finally:
        connection.close()


def main() -> int:
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        control = root / "control"
        control.mkdir()
        database = root / "opencode" / "opencode.db"
        database.parent.mkdir()
        claude = root / "claude" / "projects" / "live"
        claude.mkdir(parents=True)
        jsonl = claude / "claude-live.jsonl"
        connection = sqlite3.connect(database)
        connection.executescript(SCHEMA)
        connection.close()
        (root / "config.toml").write_text(
            "version = 1\n"
            f"[providers.opencode]\nroots = [{json.dumps(str(database))}]\n"
            f"[providers.claude-code]\nroots = [{json.dumps(str(claude.parent))}]\n"
            # No models: words match exactly, as `mash init` configures it.
            '[views.build]\nrepresentations = "lexical-only"\n'
            '[views.query]\nintent = "exact-mention"\n',
            encoding="utf-8",
        )
        writer = subprocess.Popen(
            [sys.executable, "-c", WRITER, str(database), str(jsonl), str(control)]
        )
        try:
            wait_for(lambda: written(control) >= 5, "the writer's first records")
            # 1. Index while the writer keeps committing and appending.
            before = written(control)
            mash(binary, root, "index")
            for marker in (f"opencodelive{before}", f"claudelive{before}"):
                assert found(binary, root, marker), (
                    f"{marker} committed before indexing was not found"
                )
            print(f"indexed while writing: records through step {before} found")
            # 2. Index while the writer holds SQLite's write lock throughout.
            hold = written(control) + 2
            (control / "hold").write_text(str(hold))
            wait_for(
                lambda: (control / "holding").exists(),
                "the writer to hold its write lock",
            )
            mash(binary, root, "index")
            assert found(binary, root, f"opencodelive{hold - 1}"), (
                "a record committed before the held lock was not found"
            )
            (control / "release").write_text("")
            print("indexed while the writer held its write lock")
            # 3. A later index picks up what was written meanwhile.
            wait_for(
                lambda: written(control) >= hold + 5,
                "records after the lock was released",
            )
            after = written(control)
            mash(binary, root, "index")
            for marker in (f"opencodelive{after}", f"claudelive{after}"):
                assert found(binary, root, marker), (
                    f"{marker} was not picked up by a later index"
                )
            print(f"a later index picked up records through step {after}")
        finally:
            (control / "stop").write_text("")
            try:
                writer.wait(timeout=30)
            except subprocess.TimeoutExpired:
                writer.kill()
                raise SystemExit("writer did not stop") from None
        if writer.returncode != 0:
            raise SystemExit(f"writer failed with {writer.returncode}")
        # Mash only reads: every row the writer committed is still there.
        rows = upstream_rows(database)
        assert len(rows) == written(control), (len(rows), written(control))
        print(f"upstream database intact: {len(rows)} rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
