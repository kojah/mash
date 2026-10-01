"""Validate complete pinned packet-origin judgments; never build model text."""
from dataclasses import dataclass
from enum import StrEnum
import json

from reference_review import digest, exact_keys


class Origin(StrEnum):
    DIRECT = "direct-task"
    REFERENCE = "referential"
    PROTOCOL = "protocol"
    COORDINATION = "coordination"
    WRAPPED = "wrapped-task"
    CONTROL = "wrapped-control"


@dataclass(frozen=True)
class Judgment:
    thread: str
    ordinal: int
    origin: Origin
    facets: frozenset[str]


def validate(packets_bytes: bytes, review_bytes: bytes) -> tuple[Judgment, ...]:
    if max(len(packets_bytes), len(review_bytes)) > 16 * 1024 * 1024:
        raise ValueError("source packet/review exceeds cap")
    packets, review = json.loads(packets_bytes), json.loads(review_bytes)
    exact_keys(review, {"schema_version", "scope", "packets_sha256", "annotator", "facet_granularity", "cases"})
    if (review["schema_version"] != 1 or review["scope"] != "development-packet-origin-review"
            or review["packets_sha256"] != digest(packets_bytes)):
        raise ValueError("source-origin review schema or packet hash changed")
    if any(not isinstance(review[k], str) or not review[k].strip() for k in ("annotator", "facet_granularity")):
        raise ValueError("review exposure/granularity disclosure required")
    cases = {c["thread"]: c for c in packets["cases"]}
    reviewed = {c["thread"]: c for c in review["cases"]}
    if len(cases) != len(packets["cases"]) or len(reviewed) != len(review["cases"]) or set(cases) != set(reviewed):
        raise ValueError("origin review must cover the exact packet case scope")
    result = []
    for thread, case in cases.items():
        row = reviewed[thread]
        exact_keys(row, {"thread", "source_sha256", "source_group", "turns"})
        if (row["source_sha256"] != case["source_sha256"] or row["source_group"] != case["source_group"]
                or len(row["turns"]) != len(case["turns"])):
            raise ValueError("origin review source group/sequence changed")
        for source, annotation in zip(case["turns"], row["turns"]):
            exact_keys(annotation, {"id", "ordinal", "text_sha256", "kind", "rationale", "facets"})
            if type(annotation["ordinal"]) is not int or any(annotation[k] != source[k] for k in ("id", "ordinal", "text_sha256")):
                raise ValueError("origin review source identity changed")
            if not source["display_redacted"] and digest(source["text"].encode()) != source["text_sha256"]:
                raise ValueError("packet source text changed")
            if not isinstance(annotation["rationale"], str) or not annotation["rationale"].strip():
                raise ValueError("source review rationale required")
            facets = annotation["facets"]
            if (not isinstance(facets, list) or any(not isinstance(f, str) or not f.strip() for f in facets)
                    or len(set(facets)) != len(facets)):
                raise ValueError("invalid aspect-facet judgment")
            origin = Origin(annotation["kind"])
            if facets and (origin != Origin.WRAPPED or source["display_redacted"]):
                raise ValueError("facet pilot must use intact reviewed wrapped tasks only")
            result.append(Judgment(thread, source["ordinal"], origin, frozenset(facets)))
    return tuple(result)
