"""Source-pinned user-turn boundary pilot; no clustering or model inference."""
import argparse
from dataclasses import asdict, dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import sqlite3
import tomllib

import numpy as np

from data import load, normalized


class Judgment(StrEnum):
    SAME = "same"
    CHANGE = "change"
    UNCERTAIN = "uncertain"


class Method(StrEnum):
    WINDOW = "window3"
    ADJACENT = "adjacent055"
    CENTROID = "centroid055"


@dataclass(frozen=True)
class Turn:
    id: str
    ordinal: int
    text: str
    vector: np.ndarray | None


@dataclass(frozen=True)
class Boundary:
    left: int
    right: int
    judgment: Judgment
    rationale: str


@dataclass(frozen=True)
class Case:
    thread: str
    provider: str
    source_sha256: str
    turns: tuple[Turn, ...]
    boundaries: tuple[Boundary, ...]


def source_hash(turns: tuple[Turn, ...]) -> str:
    source = [(t.id, t.ordinal, t.text) for t in turns]
    return hashlib.sha256(json.dumps(source, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()


def read_turns(connection: sqlite3.Connection, thread: str, generation: str) -> tuple[Turn, ...]:
    rows = connection.execute(
        "SELECT m.id,m.ordinal,COALESCE(m.search_projection,m.content),e.vector,e.generation "
        "FROM messages m LEFT JOIN message_embeddings e ON e.message_id=m.id "
        "WHERE m.conversation_id=? AND m.role='user' ORDER BY m.ordinal,m.id", (thread,))
    turns = []
    for identity, ordinal, text, raw, space in rows:
        if not text.strip():
            continue
        if raw is not None and (space != generation or len(raw) != 384):
            raise ValueError("incompatible episode vector")
        vector = normalized(np.frombuffer(raw, dtype=np.int8).astype(np.float64)) if raw is not None else None
        turns.append(Turn(identity, ordinal, text, vector))
    if len(turns) < 2 or len({t.ordinal for t in turns}) != len(turns):
        raise ValueError("invalid or ambiguous user-turn sequence")
    return tuple(turns)


def read_cases(database: Path, partition: Path, annotations: Path) -> tuple[str, tuple[Case, ...]]:
    dataset = load(database, partition)
    specification = tomllib.loads(annotations.read_text())
    if set(specification) != {"schema_version", "generation", "cases"} or specification["schema_version"] != 1:
        raise ValueError("invalid annotation schema")
    if specification["generation"] != dataset.generation:
        raise ValueError("annotation generation mismatch")
    eligible = {thread.id: thread for thread in dataset.threads}
    cases = []
    with sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("BEGIN")
        for entry in specification["cases"]:
            if set(entry) != {"thread", "source_sha256", "boundaries"}:
                raise ValueError("invalid annotation case")
            thread = eligible[entry["thread"]]
            if thread.group[0] % 5 == 0:
                raise ValueError("development annotations must not consume reserved groups")
            turns = read_turns(connection, thread.id, dataset.generation)
            if source_hash(turns) != entry["source_sha256"]:
                raise ValueError("annotation source changed")
            boundaries = []
            for boundary in entry["boundaries"]:
                if set(boundary) != {"left", "right", "judgment", "rationale"}:
                    raise ValueError("invalid boundary annotation")
                boundaries.append(Boundary(boundary["left"], boundary["right"], Judgment(boundary["judgment"]), boundary["rationale"]))
            expected = [(a.ordinal, b.ordinal) for a, b in zip(turns, turns[1:])]
            if [(b.left, b.right) for b in boundaries] != expected:
                raise ValueError("annotations must cover every consecutive visible user boundary")
            cases.append(Case(thread.id, thread.provider, entry["source_sha256"], turns, tuple(boundaries)))
    if not cases or len({c.thread for c in cases}) != len(cases):
        raise ValueError("empty or duplicated annotation cases")
    return dataset.fingerprint, tuple(cases)


def predictions(turns: tuple[Turn, ...], method: Method) -> tuple[bool | None, ...]:
    result = []
    pending = []
    for i, turn in enumerate(turns):
        if i == 0:
            pending = [turn.vector] if turn.vector is not None else []
            continue
        if method == Method.WINDOW:
            changed = i % 3 == 0
        elif turn.vector is None or turns[i - 1].vector is None:
            changed = None
        else:
            center = turns[i - 1].vector if method == Method.ADJACENT else normalized(np.mean(pending, axis=0))
            changed = bool(center @ turn.vector < 0.55)
        result.append(changed)
        # Missing vectors break the accumulated segment, rather than bridging a
        # potentially important unembedded user turn silently.
        if changed is True or changed is None:
            pending = []
        if turn.vector is not None:
            pending.append(turn.vector)
    return tuple(result)


def score(judgments: list[Judgment], predicted: list[bool | None]) -> dict[str, int | float | None]:
    known = [(j == Judgment.CHANGE, p) for j, p in zip(judgments, predicted) if j != Judgment.UNCERTAIN]
    tp = sum(j and p is True for j, p in known)
    fp = sum(not j and p is True for j, p in known)
    fn = sum(j and p is False for j, p in known)
    positives = sum(j for j, _ in known)
    return {"known": len(known), "changes": positives, "abstained": sum(p is None for _, p in known),
            "tp": tp, "fp": fp, "fn": fn, "precision": tp / (tp + fp) if tp + fp else None,
            "recall_including_abstentions": tp / positives if positives else None}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError("use a fresh output path")
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    reports = []
    for method in Method:
        judgments, predicted, details = [], [], []
        for case in cases:
            choices = predictions(case.turns, method)
            judgments.extend(b.judgment for b in case.boundaries)
            predicted.extend(choices)
            details.append({"thread": case.thread, "provider": case.provider, "source_sha256": case.source_sha256,
                            "boundaries": [dict(asdict(b), predicted=p) for b, p in zip(case.boundaries, choices)]})
        reports.append({"method": method, "metrics": score(judgments, predicted), "cases": details})
    report = {"schema_version": 1, "input_fingerprint": fingerprint,
              "annotations_sha256": hashlib.sha256(args.annotations.read_bytes()).hexdigest(),
              "scope": "development_user_only_linear_boundary_pilot", "reports": reports}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
        output.write("\n")


if __name__ == "__main__":
    main()
