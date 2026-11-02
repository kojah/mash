"""Windows-only behaviour the library tests cannot reach, against a real binary.

    python .github/scripts/windows-smoke.py path\\to\\mash.exe

1. `mash mcp` answers the MCP handshake and lists tools over stdio.
2. While it runs, the installed mash.exe can be overwritten (an upgrade):
   long-lived commands move their running image aside and leave a copy.
3. The SessionStart hook runs under Windows Python and starts its detached
   background index (Windows creation flags, not preexec_fn).
Standard library only.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HOOKS = Path(__file__).resolve().parents[2] / "plugins" / "mash" / "hooks"


def request(process: subprocess.Popen, message: dict) -> dict:
    assert process.stdin is not None and process.stdout is not None
    process.stdin.write(json.dumps(message) + "\n")
    process.stdin.flush()
    while True:
        line = process.stdout.readline()
        if not line:
            raise SystemExit("mcp closed stdout before answering")
        reply = json.loads(line)
        if reply.get("id") == message["id"]:
            return reply


def mcp_and_upgrade(binary: Path, root: Path) -> None:
    installed = root / "installed" / "mash.exe"
    installed.parent.mkdir()
    shutil.copy2(binary, installed)
    process = subprocess.Popen(
        [str(installed), "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    try:
        reply = request(
            process,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "windows-smoke", "version": "0"},
                },
            },
        )
        assert "result" in reply, reply
        assert process.stdin is not None
        process.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"
        )
        process.stdin.flush()
        tools = request(process, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        names = {tool["name"] for tool in tools["result"]["tools"]}
        assert {"search", "view", "status"} <= names, names
        # An upgrade replaces the installed file while the agent is connected;
        # mash moved its running image aside, so the installed one is free.
        replacement = root / "replacement.exe"
        shutil.copy2(binary, replacement)
        os.replace(replacement, installed)
        print("installed mash.exe replaced while mcp was running")
    finally:
        if process.stdin is not None:
            process.stdin.close()
        try:
            process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            process.kill()
            raise SystemExit("mcp did not exit after stdin closed") from None


def session_start_hook(binary: Path, root: Path) -> None:
    state = root / "recall"
    environment = {
        **os.environ,
        "MASH_BIN": str(binary),
        "MASH_RECALL_STATE_DIR": str(state),
        "MASH_AUTO_INDEX": "on",
    }
    payload = json.dumps({"session_id": "smoke", "cwd": str(root)})
    result = subprocess.run(
        [sys.executable, str(HOOKS / "session_start.py")],
        input=payload,
        capture_output=True,
        text=True,
        env=environment,
        timeout=30,
    )
    assert result.returncode == 0, result
    marker = state / "auto-index.json"
    deadline = time.monotonic() + 10
    while not marker.exists() and time.monotonic() < deadline:
        time.sleep(0.1)
    assert marker.exists(), "SessionStart did not start its background index"
    print("SessionStart hook started the background index")


def main() -> int:
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        mcp_and_upgrade(binary, root)
        session_start_hook(binary, root)
    return 0


if __name__ == "__main__":
    sys.exit(main())
