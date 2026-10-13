"""Record distilled project knowledge into Mash and advance distillation watermarks.

Input (stdin or --input), all quotes exact substrings of the cited messages:

    {"records": [{"kind": "decision", "statement": "...",
                  "evidence": [{"message_id": "...", "quote": "..."}]}],
     "covered": [{"chat_id": "...", "message_id": "newest message read"}]}

Each quote is checked against `mash view MESSAGE_ID`; a record with any
unverifiable quote is dropped. Survivors are committed to local Mash silver
with `mash index --connector-batch` (source `mash-recall`). Watermarks advance
only for the covered chats, and only after any delivery succeeded.
"""

from __future__ import annotations

import argparse  # Standard library only: runs in the host's interpreter.
import hashlib
import json
import os
import sys
import time
from pathlib import Path
from typing import Callable

from mash_recall import client, project, records, state
from mash_recall.json_value import items, mapping

VIEW_TIMEOUT = 15.0
DELIVERY_TIMEOUT = 60.0
BATCH_VERSION = 2


class Failure(Exception):
    pass


def fetcher(mash: client.Mash) -> Callable[[str], client.Message | None]:
    cache: dict[str, client.Message | None] = {}

    def fetch(message_id: str) -> client.Message | None:
        if message_id not in cache:
            try:
                messages = mash.view(message_id, 0, VIEW_TIMEOUT)
            except client.MashError:
                messages = []
            cache[message_id] = next(
                (message for message in messages if message.id == message_id), None
            )
        return cache[message_id]

    return fetch


def batch(
    source_state: state.SourceState, changes: list[dict[str, object]]
) -> dict[str, object]:
    body = json.dumps(changes, sort_keys=True).encode()
    digest = hashlib.sha256(body + str(source_state.revision).encode()).hexdigest()[:32]
    return {
        "schema_version": BATCH_VERSION,
        "source_id": records.SOURCE_ID,
        "batch_id": f"recall-{digest}",
        "expected_revision": source_state.revision,
        "trigger": "change",
        "changes": changes,
    }


def send(mash: client.Mash, root: Path, value: dict[str, object]) -> client.Receipt:
    path = root / f".batch-{os.getpid()}.json"
    path.write_bytes(json.dumps(value).encode())
    os.chmod(path, 0o600)
    try:
        return mash.deliver(path, DELIVERY_TIMEOUT)
    finally:
        path.unlink(missing_ok=True)


def deliver(
    mash: client.Mash,
    root: Path,
    source_state: state.SourceState,
    value: dict[str, object],
) -> None:
    """Persist the batch before sending; resend it unchanged until acknowledged."""
    source_state.pending = value
    state.save_source(root, source_state)
    try:
        receipt = send(mash, root, value)
    except client.MashError as error:
        if error.exit_code != client.CONFLICT_EXIT or value["expected_revision"] == 0:
            if error.exit_code in (2, 7, 9, 12):
                # Permanent rejection: discard so later submissions are not blocked.
                source_state.pending = None
                state.save_source(root, source_state)
            raise Failure(f"Mash rejected the records: {error}") from None
        # Only this helper writes the source and it holds the lock, so a conflict
        # means a new or replaced database; start its source from revision 0.
        source_state.revision = 0
        changes = [mapping(change) or {} for change in items(value.get("changes"))]
        deliver(mash, root, source_state, batch(source_state, changes))
        return
    source_state.revision = receipt.source_revision
    source_state.pending = None
    state.save_source(root, source_state)


def run(arguments: argparse.Namespace, raw: bytes) -> dict[str, object]:
    try:
        submission = records.decode_submission(json.loads(raw))
    except ValueError as error:
        raise Failure(f"invalid input: {error}") from None
    binary = client.resolve_binary()
    if binary is None:
        raise Failure("no mash executable found; set MASH_BIN")
    resolved = project.resolve(os.path.abspath(arguments.cwd), 5.0)
    if resolved is None:
        raise Failure("cannot resolve the project directory")
    mash = client.Mash(binary)
    fetch = fetcher(mash)
    kept, dropped = records.verify(submission.records, fetch)
    unique = list(
        {records.record_id(resolved, record): record for record in kept}.values()
    )
    marks: dict[str, state.Watermark] = {}
    unreadable: list[str] = []
    for item in submission.covered:
        message = fetch(item.message_id)
        if message is None or message.created_at is None:
            unreadable.append(item.chat_id)
        else:
            marks[item.chat_id] = state.Watermark(message.created_at, message.id)
    summary: dict[str, object] = {
        "project": resolved.label,
        "verified": len(unique),
        "dropped": [
            {"statement": record.statement, "reason": reason}
            for record, reason in dropped
        ],
        "uncovered": unreadable,
    }
    if arguments.check:
        return {**summary, "written": [], "advanced": []}
    root = state.state_dir()
    with state.locked(root):
        source_state = state.load_source(root)
        if source_state.pending is not None:
            deliver(mash, root, source_state, source_state.pending)
        project_state = state.load_project(
            root, resolved.key, resolved.toplevel, resolved.repository
        )
        if unique:
            now_ms = int(time.time() * 1000)
            changes = records.changes(resolved, unique, submission.covered, now_ms)
            deliver(mash, root, source_state, batch(source_state, changes))
            project_state.latest_record = records.anchor(
                records.record_id(resolved, unique[-1])
            )
        advanced = [
            chat_id
            for chat_id, mark in marks.items()
            if project_state.advance(chat_id, mark)
        ]
        state.save_project(root, resolved.key, project_state)
    written = [records.anchor(records.record_id(resolved, record)) for record in unique]
    return {**summary, "written": written, "advanced": advanced}


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--cwd", default=os.getcwd(), help="a directory inside the project"
    )
    parser.add_argument(
        "--input", type=Path, help="JSON submission file (default: stdin)"
    )
    parser.add_argument(
        "--check", action="store_true", help="verify quotes without writing"
    )
    arguments = parser.parse_args()
    raw = arguments.input.read_bytes() if arguments.input else sys.stdin.buffer.read()
    try:
        result = run(arguments, raw)
    except Failure as error:
        print(json.dumps({"error": str(error)}))
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
