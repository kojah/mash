"""Typed recall records, quote verification and their connector representation.

Records are ordinary connector objects in Mash silver under source `mash-recall`:
one `recall:project` container per project and one entry per record, kind
`recall:decision`, `recall:preference`, `recall:fact` or `recall:open-question`.
Each entry's text names the cited message IDs and quotes, so lexical search
finds it; metadata repeats them with the project for exact references.
"""

from __future__ import annotations

import enum
import hashlib
from dataclasses import dataclass
from typing import Callable

from .client import Message
from .json_value import items, mapping
from .project import Project

SOURCE_ID = "mash-recall"
PROJECT_KIND = "recall:project"
AUTHOR = {"id": "mash-recall", "kind": "bot", "name": "Mash recall"}
MAX_RECORDS = 20
MAX_EVIDENCE = 4
MAX_STATEMENT = 600
MIN_QUOTE = 12
MAX_QUOTE = 400


class RecordKind(enum.Enum):
    decision = "decision"
    preference = "preference"
    fact = "fact"
    open_question = "open_question"

    @property
    def connector_kind(self) -> str:
        return "recall:" + self.value.replace("_", "-")

    @property
    def label(self) -> str:
        return self.value.replace("_", " ").capitalize()


@dataclass(frozen=True)
class Evidence:
    message_id: str
    quote: str


@dataclass(frozen=True)
class Record:
    kind: RecordKind
    statement: str
    evidence: tuple[Evidence, ...]


@dataclass(frozen=True)
class Covered:
    """A chat the agent read, through `message_id` (the newest message read)."""

    chat_id: str
    message_id: str


@dataclass(frozen=True)
class Submission:
    records: tuple[Record, ...]
    covered: tuple[Covered, ...]


class InvalidSubmission(ValueError):
    pass


def text(value: object, name: str, limit: int) -> str:
    if not isinstance(value, str) or not value.strip():
        raise InvalidSubmission(f"{name} must be a nonempty string")
    if len(value) > limit:
        raise InvalidSubmission(f"{name} exceeds {limit} characters")
    return value


def fields(
    value: object, allowed: set[str], required: set[str], what: str
) -> dict[str, object]:
    """An object with exactly the allowed keys, all required keys present."""
    members = mapping(value)
    if members is None or set(members) - allowed or required - set(members):
        raise InvalidSubmission(what)
    return members


def bounded_list(value: object, low: int, high: int, what: str) -> list[object]:
    if not isinstance(value, list) or not low <= len(value) <= high:
        raise InvalidSubmission(what)
    return items(value)


def decode_record(value: object) -> Record:
    keys = {"kind", "statement", "evidence"}
    raw = fields(value, keys, keys, "each record has kind, statement and evidence")
    try:
        kind = RecordKind(raw["kind"])
    except ValueError:
        raise InvalidSubmission(
            "kind must be decision, preference, fact or open_question"
        ) from None
    evidence = bounded_list(
        raw["evidence"],
        1,
        MAX_EVIDENCE,
        f"each record cites 1 to {MAX_EVIDENCE} evidence quotes",
    )
    cited = []
    for item in evidence:
        pair = {"message_id", "quote"}
        quote = fields(item, pair, pair, "evidence items have message_id and quote")
        cited.append(
            Evidence(
                text(quote["message_id"], "message_id", 4096),
                text(quote["quote"], "quote", MAX_QUOTE),
            )
        )
    statement = text(raw["statement"], "statement", MAX_STATEMENT).strip()
    return Record(kind, statement, tuple(cited))


def decode_submission(value: object) -> Submission:
    """Validate the helper's JSON input once, at the boundary."""
    top = fields(
        value,
        {"records", "covered"},
        set(),
        "expected an object with records and covered",
    )
    raw_records = bounded_list(
        top.get("records", []),
        0,
        MAX_RECORDS,
        f"records must be a list of at most {MAX_RECORDS}",
    )
    raw_covered = bounded_list(
        top.get("covered", []), 0, 16, "covered must be a list of at most 16 chats"
    )
    covered = []
    for raw in raw_covered:
        pair = {"chat_id", "message_id"}
        item = fields(raw, pair, pair, "covered items have chat_id and message_id")
        covered.append(
            Covered(
                text(item["chat_id"], "chat_id", 4096),
                text(item["message_id"], "message_id", 4096),
            )
        )
    records = tuple(decode_record(raw) for raw in raw_records)
    return Submission(records, tuple(covered))


def quote_problem(
    evidence: Evidence, fetch: Callable[[str], Message | None]
) -> str | None:
    """Why a quote is not an exact substring of its cited message, or None."""
    if len(evidence.quote.strip()) < MIN_QUOTE:
        return f"quote shorter than {MIN_QUOTE} characters"
    message = fetch(evidence.message_id)
    if message is None:
        return f"message {evidence.message_id} not found"
    if evidence.quote not in message.content:
        return f"quote is not an exact substring of message {evidence.message_id}"
    return None


def verify(
    records: tuple[Record, ...], fetch: Callable[[str], Message | None]
) -> tuple[list[Record], list[tuple[Record, str]]]:
    """Keep records whose every quote verifies; report the rest with a reason."""
    kept: list[Record] = []
    dropped: list[tuple[Record, str]] = []
    for record in records:
        problems = [
            problem
            for item in record.evidence
            if (problem := quote_problem(item, fetch))
        ]
        if problems:
            dropped.append((record, problems[0]))
        else:
            kept.append(record)
    return kept, dropped


def container_id(project: Project) -> str:
    return f"project/{project.key}"


def record_id(project: Project, record: Record) -> str:
    digest = hashlib.sha256(
        f"{record.kind.value}\0{record.statement}".encode()
    ).hexdigest()[:24]
    return f"record/{project.key}/{digest}"


def anchor(object_id: str) -> str:
    """The `view` lookup Mash assigns a connector entry."""
    return f"connector#entry#{SOURCE_ID}#{object_id}"


def content(project: Project, record: Record) -> str:
    lines = [
        f"{record.kind.label}: {record.statement}",
        f"Project: {project.label}",
        "Evidence:",
    ]
    lines += [
        f'- message {item.message_id}: "{item.quote}"' for item in record.evidence
    ]
    return "\n".join(lines)


def schema_change() -> dict[str, object]:
    kinds: list[dict[str, object]] = [
        {"kind": PROJECT_KIND, "role": "container", "root": True}
    ]
    kinds += [
        {"kind": kind.connector_kind, "role": "entry", "parents": [PROJECT_KIND]}
        for kind in RecordKind
    ]
    return {"op": "declare-schema", "kinds": kinds}


def changes(
    project: Project, records: list[Record], covered: tuple[Covered, ...], now_ms: int
) -> list[dict[str, object]]:
    """Connector mutations for one submission: schema, project container, entries."""
    project_metadata = {"project": project.toplevel, "repository": project.repository}
    result: list[dict[str, object]] = [
        schema_change(),
        {
            "op": "upsert-object",
            "id": container_id(project),
            "role": "container",
            "kind": PROJECT_KIND,
            "title": f"Project recall: {project.label}",
            "updated_at": now_ms,
            "metadata": project_metadata,
        },
    ]
    for offset, record in enumerate(records):
        result.append(
            {
                "op": "upsert-object",
                "id": record_id(project, record),
                "role": "entry",
                "kind": record.kind.connector_kind,
                "parent": container_id(project),
                "author": AUTHOR,
                "content": content(project, record),
                # Distinct, ordered times keep `view` context in submission order.
                "created_at": now_ms + offset,
                "metadata": {
                    **project_metadata,
                    "record_kind": record.kind.value,
                    "statement": record.statement,
                    "evidence": [
                        {"message_id": item.message_id, "quote": item.quote}
                        for item in record.evidence
                    ],
                    "covered_chats": [item.chat_id for item in covered],
                },
            }
        )
    return result
