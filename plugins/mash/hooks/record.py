"""Record distilled project knowledge into Mash and advance distillation watermarks.

Input on stdin, every quote an exact substring of its cited message:

    {"records": [{"kind": "decision", "statement": "...",
                  "evidence": [{"message_id": "...", "quote": "..."}],
                  "supersedes": [{"id": "record/..."}, {"statement": "..."}]}],
     "covered": [{"chat_id": "...", "message_id": "newest message read"}]}

Each quote is checked against `mash view MESSAGE_ID`; a record with any
unverifiable quote is dropped, and a decision or preference must quote at least
one of the user's own messages (an assistant proposal alone is a suggestion).
`supersedes` (optional) names earlier records of this project, by record ID or
by exact statement (with an optional `kind`, default the record's own), that the
new record replaces. Survivors are committed to local Mash silver with
`mash index --connector-batch` (source `mash-recall`); each replaced record is
rewritten as kind `recall:superseded` with a "Superseded by" line, keeping its
text, evidence and time. Watermarks advance only for the covered chats, and only
after the write succeeded.

Each record lists the UTC days it was recorded ("Confirmed:"), so re-recording a
statement that still holds confirms it and SessionStart ranks it higher. Before
writing, statements are compared with the project's recent current records: one
equal to a stored statement apart from case, punctuation and spacing re-records
that statement, and a close rewording (IDF-weighted cosine) is reported under
`similar` so it can be superseded or kept deliberately.

`--history RECORD_ID` prints a record's supersession history without writing.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import sys
import tempfile
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any, NamedTuple, Optional

import recall_common as common

KINDS = ("decision", "preference", "fact", "open_question")
# Kinds that state what the user wants: they need the user's own words.
USER_KINDS = ("decision", "preference")
PROJECT_KIND = "recall:project"
SUPERSEDED_KIND = "recall:superseded"
AUTHOR = {"id": "mash-recall", "kind": "bot", "name": "Mash recall"}
MIN_QUOTE = 12
MAX_SUPERSEDES = 4
# Bounds a history walk over records that only this helper links.
HISTORY_LIMIT = 64
CONFLICT_EXIT = 12
# Duplicate detection reads this many records around the project's latest one.
DUPLICATE_WINDOW = 64
# A rewording shares at least this many words and this IDF-weighted cosine,
# and its differing words carry no digits (versions, sizes and IDs).
SIMILAR_WORDS, SIMILAR_COSINE = 2, 0.8


class Stored(NamedTuple):
    """A record as `view` returns it; kind and metadata are not readable."""

    id: str  # Without the connector entry prefix: record/<project key>/<hash>.
    content: str
    created_at: int


class Accepted(NamedTuple):
    record: Any
    id: str
    replaces: list[tuple[str, str]]  # (record ID, its heading)


Load = Callable[[str], Optional[Stored]]


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
        if "\n" in record["statement"].strip():
            raise ValueError("a statement is one line")
        if not isinstance(evidence, list) or not 1 <= len(evidence) <= 4:
            raise ValueError("each record cites 1 to 4 evidence quotes")
        if not all(texts(item, "message_id", "quote") for item in evidence):
            raise ValueError("evidence items have message_id and quote")
        targets = record.get("supersedes", [])
        if not isinstance(targets, list) or len(targets) > MAX_SUPERSEDES:
            raise ValueError(f"supersedes lists at most {MAX_SUPERSEDES} records")
        for target in targets:
            fields = [key for key in ("id", "statement") if key in target]
            if len(fields) != 1 or not texts(target, *fields):
                raise ValueError(
                    "each supersedes item has exactly one of id or statement"
                )
            if target.get("kind", record["kind"]) not in KINDS:
                raise ValueError(f"a superseded statement's kind is one of {KINDS}")
    if not all(texts(item, "chat_id", "message_id") for item in covered):
        raise ValueError("covered items have chat_id and message_id")
    return records, covered


def record_id(key: str, record: Any) -> str:
    identity = f"{record['kind']}\0{record['statement'].strip()}"
    return f"record/{key}/{hashlib.sha256(identity.encode()).hexdigest()[:24]}"


def connector_kind(kind: str) -> str:
    return "recall:" + kind.replace("_", "-")


def heading(kind: str, statement: str) -> str:
    return f"{kind.replace('_', ' ').capitalize()}: {statement.strip()}"


def current_heading(content: str) -> str:
    """A record's heading as first written, without any superseded marker."""
    first = content.partition("\n")[0]
    if common.superseded_by(content) is None:
        return first
    rest = first[len(common.SUPERSEDED) :]
    return rest[:1].upper() + rest[1:]


def heading_kind(text: str) -> str | None:
    return next((k for k in KINDS if text.startswith(heading(k, ""))), None)


def mark(content: str, by: str, by_heading: str) -> str:
    """`content` replaced by record `by`; an already marked text is unchanged."""
    if common.superseded_by(content) is not None:
        return content
    first, _, rest = content.partition("\n")
    lines = [
        common.SUPERSEDED + first[:1].lower() + first[1:],
        f"{common.SUPERSEDED_BY}{common.ENTRY_PREFIX}{by} ({by_heading})",
    ]
    return "\n".join(lines + ([rest] if rest else []))


def marker_heading(content: str) -> str:
    """The replacing record's heading from a superseded record's marker line."""
    marker = content.split("\n")[1]
    return marker.partition(" (")[2].removesuffix(")")


def links(stored: Stored | None) -> list[tuple[str, str]]:
    """The (record ID, heading) pairs a stored record says it replaced."""
    if stored is None:
        return []
    header = stored.content.partition("\nEvidence:")[0].split("\n")
    lines = [line for line in header if line.startswith(common.SUPERSEDES)]
    pairs = [line[len(common.SUPERSEDES) :].partition(" (") for line in lines]
    return [(common.short_id(i), text.removesuffix(")")) for i, _, text in pairs]


def target_id(key: str, record: Any, target: Any) -> str | None:
    """The record a supersedes item names, or None outside this project."""
    if "id" in target:
        found = common.short_id(target["id"])
        return found if found.startswith(f"record/{key}/") else None
    kind = target.get("kind", record["kind"])
    return record_id(key, {"kind": kind, "statement": target["statement"]})


def load(binary: str, rid: str) -> Stored | None:
    """A stored record, or None when absent. Read failures propagate: guessing
    absence could resurrect a superseded record."""
    full = common.ENTRY_PREFIX + rid
    view = common.run_mash(binary, ["view", full, "--context", "0"], 15.0)
    for message in view.get("messages") or []:
        if message.get("id") == full:
            return Stored(rid, message.get("content") or "", message["created_at"])
    return None


def supersede(
    key: str, records: list[Any], lookup: Load
) -> tuple[list[Accepted], dict[str, str], dict[str, Stored | None], list[Any]]:
    """Check supersession in submission order. A record may replace stored
    records or earlier records of this submission that are current or already
    replaced by it; a replaced record may be re-recorded but stays replaced, so
    replacements form chains (A -> B -> C) and never cycles or forks. Returns the
    accepted records, replaced ID -> replacing ID for this submission, the
    records read, and the dropped records with reasons."""
    accepted: dict[str, Accepted] = {}
    by: dict[str, str] = {}
    stored: dict[str, Stored | None] = {}
    dropped: list[Any] = []

    def read(rid: str) -> Stored | None:
        if rid not in stored:
            stored[rid] = lookup(rid)
        return stored[rid]

    def replacer(rid: str) -> str | None:
        if rid in by:
            return by[rid]
        found = read(rid)
        return common.superseded_by(found.content) if found else None

    def successors(rid: str | None) -> list[str]:
        chain: list[str] = []
        while rid and rid not in chain and len(chain) < HISTORY_LIMIT:
            chain.append(rid)
            rid = replacer(rid)
        return chain

    def check(rid: str, holder: str | None, tid: str | None) -> str | None:
        if tid is None:
            return "supersedes may only name records of this project"
        if tid == rid:
            return "a record cannot supersede itself"
        if tid in successors(holder):
            return f"superseding {tid} would create a supersession cycle"
        current = replacer(tid)
        if tid not in accepted and read(tid) is None:
            return f"record {tid} not found (in one submission, list it first)"
        if current not in (None, rid):
            return f"record {tid} is already superseded by {current}; supersede that"
        if holder and current != rid:
            return f"this record is superseded by {holder}; supersede from that"
        return None

    for record in records:
        rid = record_id(key, record)
        holder = replacer(rid)
        replaces: list[tuple[str, str]] = []
        reason = None
        for target in record.get("supersedes", []):
            tid = target_id(key, record, target)
            if reason := check(rid, holder, tid):
                break
            assert tid is not None
            if tid in accepted:
                prior = accepted[tid].record
                text = heading(prior["kind"], prior["statement"])
            else:
                found = read(tid)
                assert found is not None
                text = current_heading(found.content)
            if all(tid != seen for seen, _ in replaces):
                replaces.append((tid, text))
        if reason:
            dropped.append({"statement": record["statement"], "reason": reason})
            continue
        by.update((tid, rid) for tid, _ in replaces)
        # Re-recording keeps the links an earlier write established.
        previous = read(rid)
        named = {tid for tid, _ in replaces}
        kept = [link for link in links(previous) if link[0] not in named]
        accepted[rid] = Accepted(record, rid, replaces + kept)
    return list(accepted.values()), by, stored, dropped


SCHEMA = {
    "op": "declare-schema",
    "kinds": [{"kind": PROJECT_KIND, "role": "container", "root": True}]
    + [
        {"kind": kind, "role": "entry", "parents": [PROJECT_KIND]}
        for kind in [*map(connector_kind, KINDS), SUPERSEDED_KIND]
    ],
}


def changes(
    key: str,
    toplevel: str,
    repository: str | None,
    accepted: list[Accepted],
    covered: list[Any],
    by: dict[str, str] | None = None,
    stored: dict[str, Stored | None] | None = None,
) -> list[dict[str, Any]]:
    """Connector mutations: schema, the project container, one entry per
    accepted record and a rewrite of each stored record it replaced."""
    by, stored = by or {}, stored or {}
    label, container = repository or toplevel, f"project/{key}"
    now_ms = int(time.time() * 1000)
    where = {"project": toplevel, "repository": repository}
    headings = {
        item.id: heading(item.record["kind"], item.record["statement"])
        for item in accepted
    }
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

    def entry(rid: str, kind: str, content: str, created_at: int, metadata: Any):
        return dict(
            op="upsert-object",
            id=rid,
            role="entry",
            kind=kind,
            parent=container,
            author=AUTHOR,
            content=content,
            created_at=created_at,
            metadata=metadata,
        )

    for offset, item in enumerate(accepted):
        record = item.record
        kind, statement = record["kind"], record["statement"].strip()
        cited = [
            {"message_id": e["message_id"], "quote": e["quote"]}
            for e in record["evidence"]
        ]
        quotes = [f'- message {e["message_id"]}: "{e["quote"]}"' for e in cited]
        replaced = [
            f"{common.SUPERSEDES}{common.ENTRY_PREFIX}{tid} ({text})"
            for tid, text in item.replaces
        ]
        previous = stored.get(item.id)
        days = []
        if previous is not None:
            days = common.confirmations(previous.content, previous.created_at)
        days = [d for d in days if d != common.day(now_ms)] + [common.day(now_ms)]
        confirmed = common.CONFIRMED + ", ".join(days[-common.CONFIRMATIONS :])
        lines = [headings[item.id], f"Project: {label}", *replaced, confirmed]
        content = "\n".join([*lines, "Evidence:", *quotes])
        # Distinct, ordered times keep `view` context in submission order.
        created_at, connector = now_ms + offset, connector_kind(kind)
        holder = by.get(item.id)
        if holder:
            content = mark(content, holder, headings[holder])
        elif previous and (holder := common.superseded_by(previous.content)):
            # Re-recording a replaced record keeps it replaced, in place.
            content = mark(content, holder, marker_heading(previous.content))
            created_at = previous.created_at
        if holder:
            connector = SUPERSEDED_KIND
        metadata = dict(where, record_kind=kind, statement=statement)
        metadata.update(evidence=cited, covered_chats=[c["chat_id"] for c in covered])
        metadata.update(supersedes=[tid for tid, _ in item.replaces])
        metadata.update(superseded_by=holder)
        result.append(entry(item.id, connector, content, created_at, metadata))
    for tid, rid in by.items():
        previous = stored.get(tid)
        if tid in headings or previous is None:
            continue  # Accepted records were marked above.
        if common.superseded_by(previous.content) == rid:
            continue  # Already rewritten by an earlier submission.
        # `view` cannot return the stored metadata; the text keeps the evidence.
        text = current_heading(previous.content)
        kind = heading_kind(text)
        statement = text.partition(": ")[2] if kind else text
        metadata = dict(where, record_kind=kind, statement=statement)
        metadata.update(superseded_by=rid)
        content = mark(previous.content, rid, headings[rid])
        result.append(
            entry(tid, SUPERSEDED_KIND, content, previous.created_at, metadata)
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


def words(text: str) -> list[str]:
    """Lowercase words without punctuation; identifiers keep underscores."""
    return re.sub(r"[^\w\s]+", " ", text.lower()).split()


def statement_of(content: str) -> tuple[str | None, str]:
    """A stored record's kind and statement, read from its heading."""
    text = current_heading(content)
    kind = heading_kind(text)
    return kind, text.partition(": ")[2] if kind else text


def similarity(a: list[str], b: list[str], idf: Callable[[str], float]) -> float:
    """IDF-weighted cosine of two statements' words."""
    left, right = set(a), set(b)
    dot = sum(idf(w) ** 2 for w in left & right)
    norm = math.sqrt(sum(idf(w) ** 2 for w in left) * sum(idf(w) ** 2 for w in right))
    return dot / norm if norm else 0.0


def duplicates(records: list[Any], existing: list[Stored]) -> list[Any]:
    """Point each record that restates a stored current record at that record's
    exact statement, so writing it re-records (confirms) the stored one; return
    the close rewordings, which are written as new records."""
    stored = []
    for item in existing:
        kind, statement = statement_of(item.content)
        if kind:
            stored.append((item, kind, statement, words(statement)))
    corpus = [found for *_, found in stored] + [
        words(record["statement"]) for record in records
    ]
    frequency: dict[str, int] = {}
    for found in corpus:
        for word in set(found):
            frequency[word] = frequency.get(word, 0) + 1

    def idf(word: str) -> float:
        return math.log(1 + len(corpus) / (frequency.get(word, 0) + 0.5))

    similar = []
    for record in records:
        mine = words(record["statement"])
        for item, kind, statement, theirs in stored:
            if kind != record["kind"] or not mine:
                continue
            if mine == theirs:
                record["statement"] = statement
                break
            differing = set(mine) ^ set(theirs)
            if len(set(mine) & set(theirs)) < SIMILAR_WORDS or any(
                char.isdigit() for word in differing for char in word
            ):
                continue
            score = similarity(mine, theirs, idf)
            if score >= SIMILAR_COSINE:
                similar.append(
                    {
                        "statement": record["statement"],
                        "similar_to": common.ENTRY_PREFIX + item.id,
                        "heading": current_heading(item.content),
                        "score": round(score, 3),
                    }
                )
    return similar


def by_user(message: Any) -> bool:
    """Native user turns, or human-authored connector messages."""
    return message.get("role") == "user" or message.get("author_kind") == "human"


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

    def authorship(record: Any) -> str | None:
        if record["kind"] not in USER_KINDS:
            return None
        if any(by_user(fetch(e["message_id"])) for e in record["evidence"]):
            return None
        return (
            f"a {record['kind']} must quote the user's own message; an assistant"
            " proposal alone is a suggestion: cite the user's approval, or record"
            " it as a fact or open_question"
        )

    kept, dropped = [], []
    for record in records:
        reason = next(filter(None, map(problem, record["evidence"])), None)
        if reason := reason or authorship(record):
            dropped.append({"statement": record["statement"], "reason": reason})
        else:
            kept.append(record)
    root = common.state_dir()
    path = root / "projects" / f"{key}.json"
    state = common.read_state(path)
    try:
        rows = common.project_records(
            binary, state.get("latest_record"), DUPLICATE_WINDOW, 15.0
        )
        existing = [
            Stored(common.short_id(r["id"]), r.get("content") or "", r["created_at"])
            for r in rows
        ]
    except Exception:
        existing = []  # Duplicate detection is advisory; recording still works.
    similar = duplicates(kept, existing)
    unique = list({record_id(key, record): record for record in kept}.values())
    accepted, by, stored, refused = supersede(
        key, unique, lambda rid: load(binary, rid)
    )
    dropped += refused
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
    written = [common.ENTRY_PREFIX + item.id for item in accepted]
    current = [
        common.ENTRY_PREFIX + item.id
        for item in accepted
        if item.id not in by
        and not ((s := stored.get(item.id)) and common.superseded_by(s.content))
    ]
    if accepted:
        mutations = changes(key, toplevel, repository, accepted, covered, by, stored)
        deliver(binary, root, mutations)
        if current:
            state["latest_record"] = current[-1]
    watermarks = state.get("watermarks") or {}
    advanced = []
    for chat_id, watermark in marks.items():
        previous = (watermarks.get(chat_id) or {}).get("updated_at", -1)
        if watermark["updated_at"] > previous:
            watermarks[chat_id] = watermark
            advanced.append(chat_id)
    state.update(
        project=toplevel,
        repository=repository,
        watermarks=dict(sorted(watermarks.items())),
    )
    common.write_state(path, state)
    return {
        "project": repository or toplevel,
        "verified": len(accepted),
        "dropped": dropped,
        "uncovered": uncovered,
        "written": written,
        "current": current,
        "superseded": [
            {"id": common.ENTRY_PREFIX + tid, "by": common.ENTRY_PREFIX + rid}
            for tid, rid in by.items()
        ],
        "advanced": advanced,
        "similar": similar,
    }


def history(record: str) -> dict[str, Any]:
    """Every record linked to `record` by supersession, oldest first, and the
    current ones. Reads only; follows links in both directions."""
    binary = common.resolve_binary()
    if binary is None:
        raise RuntimeError("no mash executable found; set MASH_BIN")
    found: dict[str, Any] = {}
    queue = [common.short_id(record)]
    while queue and len(found) < HISTORY_LIMIT:
        rid = queue.pop()
        if rid in found:
            continue
        stored = load(binary, rid)
        if stored is None:
            if not found:
                raise ValueError(f"record {record} not found")
            continue
        replaced_by = common.superseded_by(stored.content)
        replaces = [link for link, _ in links(stored)]
        found[rid] = {
            "id": common.ENTRY_PREFIX + rid,
            "heading": current_heading(stored.content),
            "created_at": stored.created_at,
            "superseded_by": replaced_by and common.ENTRY_PREFIX + replaced_by,
            "supersedes": [common.ENTRY_PREFIX + item for item in replaces],
        }
        queue += [item for item in [replaced_by, *replaces] if item]
    records = sorted(found.values(), key=lambda item: item["created_at"])
    current = [item["id"] for item in records if item["superseded_by"] is None]
    return {"current": current, "records": records}


def main() -> int:
    parser = argparse.ArgumentParser(description="Record verified project recall.")
    parser.add_argument("--cwd", default=os.getcwd(), help="a directory in the project")
    parser.add_argument(
        "--history",
        metavar="RECORD_ID",
        help="print the record's supersession history instead of recording",
    )
    arguments = parser.parse_args()
    try:
        if arguments.history:
            result = history(arguments.history)
        else:
            result = run(arguments.cwd, sys.stdin.buffer.read())
    except Exception as error:
        print(json.dumps({"error": str(error) or type(error).__name__}))
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
