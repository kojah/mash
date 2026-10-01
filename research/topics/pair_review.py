"""Pinned source-span overlap judgments; evaluation data never becomes model text."""
from dataclasses import dataclass
from enum import StrEnum
import json

from reference_review import digest, exact_keys


class Relation(StrEnum):
    OVERLAP = "overlapping-task-aspect"
    DISTINCT = "distinct-task-aspects"
    UNCERTAIN = "uncertain-granularity"


@dataclass(frozen=True, order=True)
class Key:
    thread: str
    ordinal: int
    index: int


@dataclass(frozen=True)
class Pair:
    left: Key
    right: Key
    relation: Relation


def validate_pairs(annotation: dict, spans_bytes: bytes) -> tuple[Pair, ...]:
    exact_keys(annotation, {"schema_version", "scope", "annotator", "granularity", "spans_sha256", "pairs"})
    if annotation["schema_version"] != 1 or annotation["scope"] != "development-source-span-topic-overlap":
        raise ValueError("unsupported overlap review")
    for field in ("annotator", "granularity"):
        if not isinstance(annotation[field], str) or not annotation[field].strip():
            raise ValueError("review exposure and granularity disclosures required")
    if annotation["spans_sha256"] != digest(spans_bytes):
        raise ValueError("overlap review source changed")
    report = json.loads(spans_bytes)
    rows = {(r["thread"], r["identity"]["ordinal"]): r for r in report["rows"]}
    if len(rows) != len(report["rows"]):
        raise ValueError("duplicate source span record")

    def endpoint(value: dict) -> Key:
        exact_keys(value, {"thread", "identity", "index", "text_sha256"})
        identity = value["identity"]
        if (not isinstance(value["thread"], str) or not isinstance(identity, dict)
                or type(identity.get("ordinal")) is not int):
            raise ValueError("invalid overlap source identity")
        row = rows.get((value["thread"], identity["ordinal"]))
        if row is None:
            raise ValueError("unknown overlap source record")
        if (value["identity"] != row["identity"] or type(value["index"]) is not int
                or not 0 <= value["index"] < len(row["spans"])
                or value["text_sha256"] != row["spans"][value["index"]]["text_sha256"]):
            raise ValueError("overlap endpoint is not an exact captured source span")
        return Key(value["thread"], value["identity"]["ordinal"], value["index"])

    result, seen = [], set()
    for item in annotation["pairs"]:
        exact_keys(item, {"left", "right", "relation", "rationale"})
        left, right = endpoint(item["left"]), endpoint(item["right"])
        canonical = tuple(sorted((left, right)))
        if left == right or canonical in seen:
            raise ValueError("self or duplicate/reversed overlap pair")
        if not isinstance(item["rationale"], str) or not item["rationale"].strip():
            raise ValueError("source judgment rationale required")
        seen.add(canonical)
        result.append(Pair(left, right, Relation(item["relation"])))
    if not result:
        raise ValueError("empty overlap review")
    return tuple(result)
