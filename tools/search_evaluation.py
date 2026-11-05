#!/usr/bin/env python3
"""Score saved thread searches against private, independently judged TOML.

Offline only: no inference, provider access, or automatic relevance judgments.
Unjudged returned threads prevent scoring rather than becoming negatives.
"""

import argparse
import hashlib
import json
import math
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

import tomllib


class Category(str, Enum):
    EXACT = "exact"
    SEMANTIC = "semantic"
    REFERENTIAL = "referential"
    THREAD_SCOPED = "thread-scoped"
    FILTERED = "filtered"
    NEGATIVE = "negative"


class Split(str, Enum):
    DEVELOPMENT = "development"
    HELD_OUT = "held-out"


@dataclass(frozen=True, order=True)
class Chat:
    provider: str
    chat_id: str

    @classmethod
    def decode(cls, value):
        return cls(string(value["provider"]), string(value["chat_id"]))


def string(value):
    if not isinstance(value, str) or not value.strip():
        raise ValueError("expected a nonempty string")
    return value


@dataclass(frozen=True)
class Query:
    id: str
    text: str
    category: Category
    split: Split
    intent: str
    scope: str
    grades: dict[Chat, int]

    @classmethod
    def decode(cls, value):
        required = {"id", "text", "category", "split", "intent", "scope", "judgments"}
        if set(value) != required:
            raise ValueError("query fields differ from the evaluation schema")
        grades = {}
        for judgment in value["judgments"]:
            if set(judgment) != {"provider", "chat_id", "grade"}:
                raise ValueError("unexpected judgment fields")
            thread = Chat.decode(judgment)
            grade = judgment["grade"]
            if type(grade) is not int or grade not in (0, 1, 2) or thread in grades:
                raise ValueError("duplicate thread or invalid relevance grade")
            grades[thread] = grade
        category = Category(value["category"])
        if not grades or (category != Category.NEGATIVE and not any(grades.values())):
            raise ValueError(
                "positive query needs a relevant thread; all queries need judgments"
            )
        if category == Category.NEGATIVE and any(grades.values()):
            raise ValueError("negative query cannot have relevant judgments")
        if value["intent"] not in ("exact-mention", "related-discussion"):
            raise ValueError("invalid intent")
        if value["scope"] not in ("local", "shared", "combined"):
            raise ValueError("invalid scope")
        return cls(
            string(value["id"]),
            string(value["text"]),
            category,
            Split(value["split"]),
            value["intent"],
            value["scope"],
            grades,
        )


def unique(values, key):
    result = {}
    for value in values:
        identity = key(value)
        if identity in result:
            raise ValueError("duplicate query identity")
        result[identity] = value
    return result


def score(query, response):
    if (
        response["query"] != query.text
        or response["intent"] != query.intent
        or response["unit"] != "chats"
    ):
        raise ValueError("capture does not match the declared query/intent/unit")
    diagnostics = response["diagnostics"]
    if (
        type(diagnostics["limit"]) is not int
        or type(diagnostics["shared_fallback"]) is not bool
        or type(diagnostics["shared_context_incomplete"]) is not bool
    ):
        raise ValueError("invalid diagnostic field types")
    if diagnostics["requested_scope"] != query.scope:
        raise ValueError("capture scope differs from the declared query")
    if diagnostics["limit"] < 10:
        raise ValueError("capture must request at least ten results")
    if (
        diagnostics["shared_fallback"]
        or diagnostics["shared_context_incomplete"]
        or response.get("shared_error")
        or response.get("shared_context_error")
    ):
        raise ValueError("degraded searches cannot qualify as a complete evaluation")
    expected = {
        "local": {"local"},
        "shared": {"shared"},
        "combined": {"local", "shared"},
    }
    if set(diagnostics["completed_sources"]) != expected[query.scope]:
        raise ValueError("requested sources did not all complete")
    threads = [Chat.decode(hit) for hit in response["results"][:10]]
    if len(set(threads)) != len(threads):
        raise ValueError("duplicate logical threads in the ranked top ten")
    missing = [thread for thread in threads if thread not in query.grades]
    if missing:
        return {
            "id": query.id,
            "category": query.category.value,
            "complete": False,
            "unjudged": [vars(thread) for thread in missing],
            "metrics": None,
        }
    grades = [query.grades[thread] for thread in threads]
    relevant = sum(grade > 0 for grade in query.grades.values())
    metrics: dict[str, bool | float] = {"abstained": not threads}
    if relevant:
        metrics["mrr_at_10"] = next(
            (1 / (i + 1) for i, grade in enumerate(grades) if grade > 0), 0.0
        )
        ideal = sorted(query.grades.values(), reverse=True)

        def gains(values: list[int], cutoff: int) -> float:
            return sum(
                (2**grade - 1) / math.log2(i + 2)
                for i, grade in enumerate(values[:cutoff])
            )

        for k in (5, 10):
            metrics[f"precision_at_{k}"] = sum(g > 0 for g in grades[:k]) / k
            metrics[f"recall_at_{k}"] = sum(g > 0 for g in grades[:k]) / relevant
            metrics[f"ndcg_at_{k}"] = gains(grades, k) / gains(ideal, k)
    return {
        "id": query.id,
        "category": query.category.value,
        "complete": True,
        "unjudged": [],
        "metrics": metrics,
    }


def evaluate(judgments, captures, split):
    if (
        set(judgments) != {"version", "queries"}
        or type(judgments["version"]) is not int
        or judgments["version"] != 1
    ):
        raise ValueError("unsupported judgment schema")
    if (
        set(captures) != {"version", "queries"}
        or type(captures["version"]) is not int
        or captures["version"] != 1
    ):
        raise ValueError("unsupported capture schema")
    all_queries = unique(
        [Query.decode(q) for q in judgments["queries"]], lambda q: q.id
    )
    queries = {
        identity: query
        for identity, query in all_queries.items()
        if query.split == split
    }
    if not queries:
        raise ValueError("selected split contains no queries")
    for capture in captures["queries"]:
        if set(capture) != {"id", "response"}:
            raise ValueError("unexpected capture fields")
    saved = unique(captures["queries"], lambda c: string(c["id"]))
    if set(saved) != set(queries):
        raise ValueError("capture identities do not exactly match the selected split")
    records = [
        score(query, saved[identity]["response"]) for identity, query in queries.items()
    ]
    complete = all(record["complete"] for record in records)
    means = {}
    if complete:
        for category in ["all"] + [c.value for c in Category]:
            group = [
                r for r in records if category == "all" or r["category"] == category
            ]
            positive = [r["metrics"] for r in group if "mrr_at_10" in r["metrics"]]
            negative = [
                r["metrics"] for r in group if r["category"] == Category.NEGATIVE.value
            ]
            means[category] = {
                "queries": len(group),
                "positive_queries": len(positive),
                "negative_queries": len(negative),
            }
            if positive:
                for name in positive[0]:
                    if name != "abstained":
                        means[category][name] = sum(m[name] for m in positive) / len(
                            positive
                        )
            if negative:
                means[category]["abstention_rate"] = sum(
                    m["abstained"] for m in negative
                ) / len(negative)
    return {
        "version": 1,
        "split": split.value,
        "complete": complete,
        "categories": means if complete else None,
        "queries": records,
        "limitations": [
            "thread relevance only; no passage or task-completion judgments",
            "recall denominator is the judged pool, not exhaustive corpus relevance",
            "human independence, corpus/configuration pins and selection gates require review",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--judgments", type=Path, required=True)
    parser.add_argument("--captures", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--split", choices=[s.value for s in Split], default=Split.HELD_OUT.value
    )
    args = parser.parse_args()
    output = args.output.resolve()
    if output.is_relative_to(Path(__file__).resolve().parent.parent):
        parser.error("private evaluation output must be outside the repository")
    judgment_bytes = args.judgments.read_bytes()
    capture_bytes = args.captures.read_bytes()
    report = evaluate(
        tomllib.loads(judgment_bytes.decode()),
        json.loads(capture_bytes),
        Split(args.split),
    )
    report["input_sha256"] = {
        "judgments": hashlib.sha256(judgment_bytes).hexdigest(),
        "captures": hashlib.sha256(capture_bytes).hexdigest(),
        "scorer": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    }
    output.write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps({"complete": report["complete"], "queries": len(report["queries"])})
    )
    return 0 if report["complete"] else 2


if __name__ == "__main__":
    raise SystemExit(main())
