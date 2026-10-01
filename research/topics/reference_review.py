"""Validate private source-backed reference judgments; never produce model inputs."""
import argparse
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path


class Resolution(StrEnum):
    TASK = "resolved-task"
    METADATA = "resolved-metadata"
    INTERACTION = "interaction-only"
    UNRESOLVED = "unresolved-reference"


@dataclass(frozen=True)
class Judgment:
    thread: str
    ordinal: int
    resolution: Resolution
    evidence_ordinals: tuple[int, ...]


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def exact_keys(value: dict, keys: set[str]) -> None:
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError("unexpected annotation fields")


def validate_anchors(context_bytes: bytes, review_bytes: bytes, annotation: dict,
                     expected: dict, context_cases: list) -> tuple[Judgment, ...]:
    """Require every originally referential anchor once, with exact causal evidence."""
    exact_keys(annotation, {"schema_version", "scope", "annotator", "context_sha256",
                            "source_review_sha256", "judgments"})
    if annotation["schema_version"] != 1 or annotation["scope"] != "bounded-preceding-reference-review":
        raise ValueError("unsupported reference review scope")
    if not isinstance(annotation["annotator"], str) or not annotation["annotator"].strip():
        raise ValueError("annotator and exposure disclosure required")
    if (annotation["context_sha256"] != digest(context_bytes)
            or annotation["source_review_sha256"] != digest(review_bytes)):
        raise ValueError("reference review source changed")
    available = {}
    for case in context_cases:
        for context in case["contexts"]:
            key = (case["thread"], context["anchor"]["ordinal"])
            if key in available:
                raise ValueError("duplicate context anchor")
            available[key] = (case["source_sha256"], context)
    judgments, seen = [], set()
    for item in annotation["judgments"]:
        exact_keys(item, {"thread", "ordinal", "anchor", "context_sha256", "resolution", "evidence", "rationale"})
        key = (item["thread"], item["ordinal"])
        if key in seen or key not in expected or key not in available:
            raise ValueError("duplicate or unexpected reference anchor")
        seen.add(key)
        source, original = expected[key]
        context_source, context = available[key]
        anchor = context["anchor"]
        if (source != context_source or item["anchor"] != anchor
                or original["message"] != anchor["id"]
                or original["text_sha256"] != anchor["searchable_sha256"]
                or item["context_sha256"] != context["context_sha256"]):
            raise ValueError("anchor identity or source mismatch")
        resolution = Resolution(item["resolution"])
        if not isinstance(item["rationale"], str) or not item["rationale"].strip():
            raise ValueError("source rationale required")
        display = {entry["ordinal"]: entry for entry in context["display"]}
        if len(display) != len(context["display"]):
            raise ValueError("ambiguous displayed evidence")
        evidence_ordinals = []
        for evidence in item["evidence"]:
            exact_keys(evidence, set(anchor))
            entry = display.get(evidence["ordinal"])
            if (entry is None or evidence["ordinal"] >= item["ordinal"]
                    or evidence["ordinal"] in evidence_ordinals
                    or entry["display_status"] != "included"
                    or not isinstance(entry["text"], str)
                    or digest(entry["text"].encode()) != evidence["searchable_sha256"]
                    or evidence not in context["traversed"]
                    or evidence != {name: entry[name] for name in evidence}):
                raise ValueError("evidence is not intact causal displayed source")
            evidence_ordinals.append(evidence["ordinal"])
        if resolution in (Resolution.TASK, Resolution.METADATA) and not evidence_ordinals:
            raise ValueError("resolved references require source evidence")
        judgments.append(Judgment(item["thread"], item["ordinal"], resolution, tuple(evidence_ordinals)))
    if seen != set(expected):
        raise ValueError("incomplete referential review")
    return tuple(judgments)


def validate(context_bytes: bytes, review_bytes: bytes, annotation: dict) -> tuple[Judgment, ...]:
    contexts, review = json.loads(context_bytes), json.loads(review_bytes)
    expected = {}
    for case in review['cases']:
        for item in case['admission_judgments']:
            if item['kind'] == 'referential':
                key = (case['thread'], item['ordinal'])
                if key in expected:
                    raise ValueError('duplicate source anchor')
                expected[key] = (case['source_sha256'], item)
    return validate_anchors(context_bytes, review_bytes, annotation, expected, contexts['cases'])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("contexts", "source-review", "annotations"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    judgments = validate(args.contexts.read_bytes(), args.source_review.read_bytes(),
                         json.loads(args.annotations.read_text()))
    print(json.dumps({"judgments": len(judgments), "resolutions": dict(Counter(j.resolution for j in judgments))}))


if __name__ == "__main__":
    main()
