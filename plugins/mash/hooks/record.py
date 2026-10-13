"""Record distilled project knowledge into Mash and advance distillation watermarks.

Input on stdin, every quote an exact substring of its cited message:

    {"records": [{"kind": "decision", "statement": "...",
                  "evidence": [{"message_id": "...", "quote": "..."}]}],
     "covered": [{"chat_id": "...", "message_id": "newest message read"}]}

Each quote is checked against `mash view MESSAGE_ID`; a record with any
unverifiable quote is dropped. Survivors are committed to local Mash silver
with `mash index --connector-batch` (source `mash-recall`). Watermarks advance
only for the covered chats, and only after the write succeeded.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

import recall_common as common

KINDS = ("decision", "preference", "fact", "open_question")
PROJECT_KIND = "recall:project"
AUTHOR = {"id": "mash-recall", "kind": "bot", "name": "Mash recall"}
MIN_QUOTE = 12
CONFLICT_EXIT = 12


def texts(item: Any, *keys: str) -> bool:
    return all(isinstance(item.get(k), str) and item[k].strip() for k in keys)


def parse(raw: bytes) -> tuple[list[Any], list[Any]]:
    """The submission's records and covered chats, checked once here."""
    value = json.loads(raw)
    records, covered = value.get("records", []), value.get("covered", [])
    for record in records:
        evidence = record.get("evidence")
        if record.get("kind") not in KINDS or not texts(record, "statement"):
            raise ValueError(f"each record needs a kind in {KINDS} and a statement")
        if not isinstance(evidence, list) or not 1 <= len(evidence) <= 4:
            raise ValueError("each record cites 1 to 4 evidence quotes")
        if not all(texts(item, "message_id", "quote") for item in evidence):
            raise ValueError("evidence items have message_id and quote")
    if not all(texts(item, "chat_id", "message_id") for item in covered):
        raise ValueError("covered items have chat_id and message_id")
    return records, covered


def record_id(key: str, record: Any) -> str:
    identity = f"{record['kind']}\0{record['statement'].strip()}"
    return f"record/{key}/{hashlib.sha256(identity.encode()).hexdigest()[:24]}"


def connector_kind(kind: str) -> str:
    return "recall:" + kind.replace("_", "-")


SCHEMA = {
    "op": "declare-schema",
    "kinds": [{"kind": PROJECT_KIND, "role": "container", "root": True}]
    + [
        {"kind": connector_kind(kind), "role": "entry", "parents": [PROJECT_KIND]}
        for kind in KINDS
    ],
}


def changes(
    key: str,
    toplevel: str,
    repository: str | None,
    records: list[Any],
    covered: list[Any],
) -> list[dict[str, Any]]:
    """Connector mutations: schema, the project container and one entry per record."""
    label, container = repository or toplevel, f"project/{key}"
    now_ms = int(time.time() * 1000)
    where = {"project": toplevel, "repository": repository}
    result: list[dict[str, Any]] = [SCHEMA]
    result.append(
        dict(
            op="upsert-object",
            id=container,
            role="container",
            kind=PROJECT_KIND,
            title=f"Project recall: {label}",
            updated_at=now_ms,
            metadata=where,
        )
    )
    for offset, record in enumerate(records):
        kind, statement = record["kind"], record["statement"].strip()
        cited = [
            {"message_id": item["message_id"], "quote": item["quote"]}
            for item in record["evidence"]
        ]
        quotes = [f'- message {e["message_id"]}: "{e["quote"]}"' for e in cited]
        heading = f"{kind.replace('_', ' ').capitalize()}: {statement}"
        content = "\n".join([heading, f"Project: {label}", "Evidence:", *quotes])
        chats = [item["chat_id"] for item in covered]
        metadata = dict(where, record_kind=kind, statement=statement)
        metadata.update(evidence=cited, covered_chats=chats)
        # Distinct, ordered times keep `view` context in submission order.
        result.append(
            dict(
                op="upsert-object",
                id=record_id(key, record),
                role="entry",
                kind=connector_kind(kind),
                parent=container,
                author=AUTHOR,
                content=content,
                created_at=now_ms + offset,
                metadata=metadata,
            )
        )
    return result


def deliver(binary: str, root: Path, mutations: list[dict[str, Any]]) -> None:
    """Commit one batch. Mash accepts it only at the source's current revision,
    which this helper alone advances: the stored revision is the normal case, the
    next one recovers a receipt lost after commit (or a concurrent helper), and 0
    a new database. The revision in the batch ID makes a resend idempotent."""
    source = root / "source.json"
    stored = common.read_state(source).get("revision", 0)
    body = json.dumps(mutations, sort_keys=True).encode()
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    for revision in dict.fromkeys([stored, stored + 1, 0]):
        digest = hashlib.sha256(body + str(revision).encode()).hexdigest()[:32]
        batch = {
            "schema_version": 2,
            "source_id": common.SOURCE_ID,
            "batch_id": f"recall-{digest}",
            "expected_revision": revision,
            "trigger": "change",
            "changes": mutations,
        }
        with tempfile.NamedTemporaryFile("w", dir=root, suffix=".json") as file:
            json.dump(batch, file)
            file.flush()
            try:
                arguments = ["index", "--connector-batch", file.name]
                receipt = common.run_mash(binary, arguments, 60.0)
            except common.MashError as error:
                if error.code == CONFLICT_EXIT:
                    continue
                raise
        common.write_state(source, {"revision": receipt["source_revision"]})
        return
    raise RuntimeError("Mash rejected the records: connector revision conflict")


def run(cwd: str, raw: bytes) -> dict[str, Any]:
    records, covered = parse(raw)
    binary = common.resolve_binary()
    if binary is None:
        raise RuntimeError("no mash executable found; set MASH_BIN")
    toplevel, repository = common.resolve_project(os.path.abspath(cwd), 5.0)
    key = common.project_key(toplevel, repository)
    messages: dict[str, Any] = {}

    def fetch(message_id: str) -> Any:
        if message_id not in messages:
            try:
                view = common.run_mash(
                    binary, ["view", message_id, "--context", "0"], 15.0
                )
                found = [m for m in view["messages"] if m.get("id") == message_id]
            except Exception:
                found = []
            messages[message_id] = found[0] if found else None
        return messages[message_id]

    def problem(evidence: Any) -> str | None:
        message_id, quote = evidence["message_id"], evidence["quote"]
        if len(quote.strip()) < MIN_QUOTE:
            return f"quote shorter than {MIN_QUOTE} characters"
        if (message := fetch(message_id)) is None:
            return f"message {message_id} not found"
        if quote not in (message.get("content") or ""):
            return f"quote is not an exact substring of message {message_id}"
        return None

    kept, dropped = [], []
    for record in records:
        reason = next(filter(None, map(problem, record["evidence"])), None)
        if reason:
            dropped.append({"statement": record["statement"], "reason": reason})
        else:
            kept.append(record)
    unique = list({record_id(key, record): record for record in kept}.values())
    marks, uncovered = {}, []
    for item in covered:
        message = fetch(item["message_id"])
        if message is None or not isinstance(message.get("created_at"), int):
            uncovered.append(item["chat_id"])
        else:
            marks[item["chat_id"]] = {
                "updated_at": message["created_at"],
                "message_id": message["id"],
            }
    root = common.state_dir()
    path = root / "projects" / f"{key}.json"
    state = common.read_state(path)
    written = [
        f"connector#entry#{common.SOURCE_ID}#{record_id(key, r)}" for r in unique
    ]
    if unique:
        deliver(binary, root, changes(key, toplevel, repository, unique, covered))
        state["latest_record"] = written[-1]
    watermarks = state.get("watermarks") or {}
    advanced = []
    for chat_id, mark in marks.items():
        if mark["updated_at"] > (watermarks.get(chat_id) or {}).get("updated_at", -1):
            watermarks[chat_id] = mark
            advanced.append(chat_id)
    state.update(
        project=toplevel,
        repository=repository,
        watermarks=dict(sorted(watermarks.items())),
    )
    common.write_state(path, state)
    return {
        "project": repository or toplevel,
        "verified": len(unique),
        "dropped": dropped,
        "uncovered": uncovered,
        "written": written,
        "advanced": advanced,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Record verified project recall.")
    parser.add_argument("--cwd", default=os.getcwd(), help="a directory in the project")
    arguments = parser.parse_args()
    try:
        result = run(arguments.cwd, sys.stdin.buffer.read())
    except Exception as error:
        print(json.dumps({"error": str(error) or type(error).__name__}))
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
