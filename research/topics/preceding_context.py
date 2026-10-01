"""Private, bounded preceding-context packets; no inference or referent guesses."""
import argparse
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import sqlite3

from episodes import Turn, read_cases, source_hash
from review_packets import review_text


class Mode(StrEnum):
    NATIVE = "stored-native-parent"
    LINEAR = "stored-linear-order"


class Stop(StrEnum):
    USER = "preceding-visible-user"
    ROOT = "root"
    LIMIT = "record-limit"
    VISIBLE = "visible-record-limit"


class Role(StrEnum):
    USER = "user"
    ASSISTANT = "assistant"
    TOOL = "tool"
    DEVELOPER = "developer"


@dataclass(frozen=True)
class Message:
    id: str
    ordinal: int
    role: Role
    parent: int | None
    canonical: str
    searchable: str


def sha(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()


def identity(message: Message) -> dict:
    return {"id": message.id, "ordinal": message.ordinal, "role": message.role,
            "parent": message.parent, "canonical_sha256": sha(message.canonical),
            "searchable_sha256": sha(message.searchable)}


def preceding(messages: tuple[Message, ...], anchor: int, *, maximum_records: int = 64,
              maximum_message_bytes: int = 16384, maximum_display_bytes: int = 32768,
              maximum_users: int = 1, maximum_visible: int | None = None) -> dict:
    """Follow only causal predecessors; never replace a broken native edge by order."""
    if min(maximum_records, maximum_message_bytes, maximum_display_bytes, maximum_users) < 1:
        raise ValueError("context bounds must be positive")
    if maximum_visible is not None and (type(maximum_visible) is not int or maximum_visible < 1):
        raise ValueError("visible context bound must be a positive integer")
    by_ordinal = {m.ordinal: m for m in messages}
    if len(by_ordinal) != len(messages) or len({m.id for m in messages}) != len(messages):
        raise ValueError("ambiguous message identities or ordinals")
    if any(m.ordinal < 0 for m in messages):
        raise ValueError("negative message ordinal")
    current = by_ordinal[anchor]
    if current.role != "user" or not current.searchable.strip():
        raise ValueError("anchor must be a visible user record")
    mode = Mode.LINEAR if current.parent is None else Mode.NATIVE
    order = sorted(by_ordinal)
    positions = {ordinal: i for i, ordinal in enumerate(order)}
    traversed, displayed = [], []
    used = 0
    omitted = 0
    users = 0
    visible = 0
    for _ in range(maximum_records):
        if mode == Mode.NATIVE:
            parent = current.parent
            if parent is None or parent < -1 or parent >= current.ordinal:
                raise ValueError("invalid or mixed native ancestry")
            if parent == -1:
                stop = Stop.ROOT
                break
            if parent not in by_ordinal:
                raise ValueError("missing native predecessor")
        else:
            if current.parent is not None:
                raise ValueError("mixed linear and native context")
            position = positions[current.ordinal]
            if position == 0:
                stop = Stop.ROOT
                break
            parent = order[position - 1]
        current = by_ordinal[parent]
        # Validate the reached edge too, including the stopping user record.
        if mode == Mode.NATIVE and (current.parent is None or current.parent < -1 or current.parent >= current.ordinal):
            raise ValueError("invalid or mixed native ancestry")
        if mode == Mode.LINEAR and current.parent is not None:
            raise ValueError("mixed linear and native context")
        traversed.append(identity(current))
        if current.role in ("user", "assistant") and current.searchable.strip():
            size = len(current.searchable.encode())
            text, redacted = review_text(current.searchable)
            entry = identity(current)
            if size > maximum_message_bytes or used + len(text.encode()) > maximum_display_bytes:
                entry.update(text=None, display_status="omitted-size")
                omitted += 1
            else:
                used += len(text.encode())
                entry.update(text=text, display_status="redacted" if redacted else "included")
            displayed.append(entry)
            visible += 1
            if current.role == "user":
                users += 1
                if users == maximum_users:
                    stop = Stop.USER
                    break
            if maximum_visible is not None and visible == maximum_visible:
                stop = Stop.VISIBLE
                break
    else:
        stop = Stop.LIMIT
    displayed.reverse()
    pin = {"anchor": identity(by_ordinal[anchor]), "mode": mode, "traversed": traversed,
           "stop": stop, "bounds": [maximum_records, maximum_message_bytes, maximum_display_bytes]}
    if maximum_users != 1:
        pin["maximum_users"] = maximum_users
    if maximum_visible is not None:
        pin['maximum_visible'] = maximum_visible
    return {**pin, "context_sha256": sha(json.dumps(pin, sort_keys=True, separators=(",", ":"))),
            "display": displayed, "omitted_records": omitted,
            "display_bytes": used, "bounded_interval_complete": stop not in (Stop.LIMIT, Stop.VISIBLE),
            "native_log_verified": False}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--maximum-users", type=int, choices=(1, 3), default=1)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private packets must stay outside the repository")
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    packets = []
    with sqlite3.connect(args.database.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("BEGIN")
        for case in cases:
            messages = tuple(Message(row[0], row[1], Role(row[2]), *row[3:]) for row in connection.execute(
                "SELECT id,ordinal,role,context_parent,content,COALESCE(search_projection,content) "
                "FROM messages WHERE conversation_id=? ORDER BY ordinal", (case.thread,)))
            visible = tuple(Turn(m.id, m.ordinal, m.searchable, None) for m in messages
                            if m.role == Role.USER and m.searchable.strip())
            if source_hash(visible) != case.source_sha256:
                raise ValueError("annotation source changed before context capture")
            packets.append({"thread": case.thread, "provider": case.provider,
                            "source_sha256": case.source_sha256,
                            "contexts": [preceding(messages, turn.ordinal, maximum_users=args.maximum_users) for turn in case.turns]})
    scope = "preceding_interval_to_previous_visible_user_or_root" if args.maximum_users == 1 else "preceding_chain_up_to_three_visible_users_or_root"
    report = {"schema_version": 1, "scope": scope,
              "limitations": ["stored ancestry, not independent native-log verification",
                              "linear histories may contain compaction or inherited context",
                              "display guard is not a general secret scanner",
                              "no semantic referent resolution or boundary judgments"],
              "dataset_fingerprint": fingerprint, "annotation_sha256": sha(args.annotations.read_text()),
              "code_sha256": sha(Path(__file__).read_text()), "cases": packets}
    # Never overwrite frozen packets or display private source text on stdout.
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(report, output, ensure_ascii=False, indent=2)
    print(json.dumps({"cases": len(cases), "contexts": sum(len(c["contexts"]) for c in packets)}))


if __name__ == "__main__":
    main()
