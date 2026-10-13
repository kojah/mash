"""SessionStart hook: inject project-scoped Mash recall as additional context.

Fails open: any error, a missing executable or index, or the deadline produces
no output and exit status 0. It only reads Mash and the recall state.
"""

from __future__ import annotations

import json  # Standard library only: runs in the host's interpreter.
import sys
import time
from pathlib import Path

from mash_recall import client, project, recall, state
from mash_recall.json_value import text_at

DEADLINE_SECONDS = 3.0
MAX_INPUT_BYTES = 1024 * 1024
HELPER = Path(__file__).resolve().parent / "record.py"


def context(payload: object, deadline: recall.Deadline) -> str | None:
    session_id = text_at(payload, "session_id")
    cwd = text_at(payload, "cwd")
    if session_id is None or cwd is None:
        return None
    binary = client.resolve_binary()
    if binary is None:
        return None
    resolved = project.resolve(cwd, min(1.0, deadline.left()))
    if resolved is None:
        return None
    text = recall.build(
        client.Mash(binary),
        resolved,
        session_id,
        state.state_dir(),
        HELPER,
        int(time.time() * 1000),
        deadline,
    )
    return text or None


def main() -> int:
    deadline = recall.Deadline(DEADLINE_SECONDS)
    try:
        payload = json.loads(sys.stdin.buffer.read(MAX_INPUT_BYTES) or b"null")
        text = context(payload, deadline)
    except Exception:  # Fail open: recall must never block or break a session.
        return 0
    if text and deadline.left() > 0:
        output = {
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": text,
            }
        }
        sys.stdout.write(json.dumps(output))
    return 0


if __name__ == "__main__":
    sys.exit(main())
