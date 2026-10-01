"""Source-only span similarity ranking, evaluated with private pilot judgments."""
import argparse
from collections import Counter
from dataclasses import asdict, dataclass
from enum import StrEnum
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from context_selection import Selector, select
from data import normalized
from episodes import read_cases
from lexical_boundaries import admissions
from local_embeddings import Encoder
from reference_review import digest, exact_keys, validate
from source_spans import structural_spans


class Relation(StrEnum):
    SUPPORT = "topic-support"
    BACKGROUND = "background"
    PROCEDURE = "procedure"
    METADATA = "metadata"
    MIXED = "mixed-rejection"
    REJECTED = "rejected-proposal"


class Query(StrEnum):
    CURRENT = "current-user"
    PREVIOUS = "preceding-user"
    CHAIN = "captured-user-chain-mean"


def query_inputs(anchor: str, context: dict, method: Query) -> tuple[str, ...]:
    if method == Query.CURRENT:
        return (anchor,)
    users = [e for e in context["display"] if e["role"] == "user"]
    users = users[-1:] if method == Query.PREVIOUS else users
    if not users or len(users) > 3:
        return ()
    for entry in users:
        identity = {k: entry[k] for k in context["anchor"]}
        if (entry["display_status"] != "included" or identity not in context["traversed"]
                or entry["ordinal"] >= context["anchor"]["ordinal"]
                or digest(entry["text"].encode()) != entry["searchable_sha256"]):
            return ()
    texts = tuple(e["text"] for e in users)
    return texts if method == Query.PREVIOUS else (*texts, anchor)


@dataclass(frozen=True)
class Pilot:
    thread: str
    ordinal: int
    assistant_ordinal: int
    relations: tuple[Relation, ...]


def review_pilot(annotation: dict, spans_bytes: bytes, contexts_bytes: bytes) -> tuple[Pilot, ...]:
    """Require complete scalar judgments for each pinned candidate record."""
    exact_keys(annotation, {"schema_version", "scope", "annotator", "spans_sha256",
                            "contexts_sha256", "cases"})
    if (annotation["schema_version"] != 1 or annotation["scope"] != "development-span-selection-pilot"
            or not isinstance(annotation["annotator"], str) or not annotation["annotator"].strip()):
        raise ValueError("unsupported pilot or missing exposure disclosure")
    if annotation["spans_sha256"] != digest(spans_bytes) or annotation["contexts_sha256"] != digest(contexts_bytes):
        raise ValueError("pilot source changed")
    span_report, context_report = json.loads(spans_bytes), json.loads(contexts_bytes)
    if span_report["dataset_fingerprint"] != context_report["dataset_fingerprint"]:
        raise ValueError("span/context dataset changed")
    rows = {(r["thread"], r["identity"]["ordinal"]): r for r in span_report["rows"]}
    contexts = {(c["thread"], x["anchor"]["ordinal"]): x
                for c in context_report["cases"] for x in c["contexts"]}
    if len(rows) != len(span_report["rows"]):
        raise ValueError("duplicate span record")
    pilots, seen = [], set()
    for case in annotation["cases"]:
        exact_keys(case, {"thread", "ordinal", "anchor", "assistant", "judgments"})
        key = (case["thread"], case["ordinal"])
        if key in seen or key not in contexts:
            raise ValueError("duplicate or unexpected pilot anchor")
        seen.add(key)
        context = contexts[key]
        assistant, = select(context, Selector.ASSISTANT)
        identity = {k: assistant[k] for k in context["anchor"]}
        if case["anchor"] != context["anchor"] or case["assistant"] != identity:
            raise ValueError("pilot causal identity changed")
        row = rows[(case["thread"], assistant["ordinal"])]
        text = assistant["text"]
        if (row["identity"] != identity or digest(text.encode()) != identity["searchable_sha256"]
                or identity not in context["traversed"]):
            raise ValueError("assistant is not intact causal source")
        rebuilt = [{**asdict(s), "text_sha256": digest(text[s.start:s.end].encode()),
                    "source_bytes": len(text[s.start:s.end].encode())} for s in structural_spans(text)]
        if rebuilt != row["spans"]:
            raise ValueError("captured source spans changed")
        if len(case["judgments"]) != len(rebuilt):
            raise ValueError("incomplete span judgments")
        relations = []
        for i, judgment in enumerate(case["judgments"]):
            exact_keys(judgment, {"index", "text_sha256", "relation"})
            if type(judgment["index"]) is not int or judgment["index"] != i or judgment["text_sha256"] != rebuilt[i]["text_sha256"]:
                raise ValueError("duplicate, reordered or edited span judgment")
            relations.append(Relation(judgment["relation"]))
        pilots.append(Pilot(*key, assistant["ordinal"], tuple(relations)))
    if not pilots:
        raise ValueError("empty pilot")
    return tuple(pilots)


def rank_spans(queries: tuple[str, ...], texts: tuple[str, ...], encoder: Encoder) -> tuple[tuple[int, float], ...]:
    """Rank original strings only; no oracle judgment, rationale or task label."""
    lengths = [len(encoder.tokenizer.encode(t).ids) for t in (*queries, *texts)]
    if not queries or any(not 2 <= length <= 512 for length in lengths):
        raise ValueError("pilot exceeds model window; no truncation or partial ranking")
    query = normalized(np.mean([encoder.encode(q) for q in queries], axis=0))
    scored = [(i, float(np.clip(query @ encoder.encode(t), -1, 1))) for i, t in enumerate(texts)]
    return tuple(sorted(scored, key=lambda pair: (-pair[1], pair[0])))


def selection_counts(indices: tuple[int, ...], relations: tuple[Relation, ...]) -> dict:
    counts = Counter(relations[i] for i in indices)
    return {"selected": len(indices), "topic_support_hits": counts[Relation.SUPPORT],
            "topic_support_total": relations.count(Relation.SUPPORT),
            "mixed_or_rejected": counts[Relation.MIXED] + counts[Relation.REJECTED],
            "metadata": counts[Relation.METADATA], "procedure": counts[Relation.PROCEDURE],
            "background": counts[Relation.BACKGROUND]}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "contexts",
                 "reference-review", "spans", "pilot", "snapshot", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private reports must remain outside Git")
    started = time.perf_counter()
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    admitted = admissions(cases, args.source_review)
    contexts_bytes, spans_bytes = args.contexts.read_bytes(), args.spans.read_bytes()
    validate(contexts_bytes, args.source_review.read_bytes(), json.loads(args.reference_review.read_bytes()))
    contexts_report = json.loads(contexts_bytes)
    if contexts_report["dataset_fingerprint"] != fingerprint:
        raise ValueError("frozen dataset changed")
    pilots = review_pilot(json.loads(args.pilot.read_bytes()), spans_bytes, contexts_bytes)
    turns = {(c.thread, t.ordinal): t for c in cases for t in c.turns if t.id in admitted}
    contexts = {(c["thread"], x["anchor"]["ordinal"]): x
                for c in contexts_report["cases"] for x in c["contexts"]}
    encoder = Encoder(args.snapshot)
    reports = []
    for pilot in pilots:
        key = (pilot.thread, pilot.ordinal)
        turn, context = turns[key], contexts[key]
        if turn.id != context["anchor"]["id"] or digest(turn.text.encode()) != context["anchor"]["searchable_sha256"]:
            raise ValueError("pilot anchor source changed")
        assistant, = select(context, Selector.ASSISTANT)
        text = assistant["text"]
        spans = structural_spans(text)
        texts = tuple(text[s.start:s.end] for s in spans)
        methods = []
        for method in Query:
            queries = query_inputs(turn.text, context, method)
            lengths = [len(encoder.tokenizer.encode(t).ids) for t in (*queries, *texts)]
            if not queries or any(not 2 <= n <= 512 for n in lengths):
                methods.append({"method": method, "status": "incomplete-or-over-model-window",
                                "input_tokens": lengths, "ranking": [], "controls": {}})
                continue
            ranking = rank_spans(queries, texts, encoder)
            methods.append({"method": method, "status": "ranked", "input_tokens": lengths,
                            "ranking": [{"index": i, "cosine": value, "relation": pilot.relations[i]} for i, value in ranking],
                            "controls": {f"top-{k}": selection_counts(tuple(i for i, _ in ranking[:k]), pilot.relations)
                                         for k in (1, 3)}})
        reports.append({"thread": pilot.thread, "ordinal": pilot.ordinal,
                        "assistant_ordinal": pilot.assistant_ordinal,
                        "all": selection_counts(tuple(range(len(spans))), pilot.relations), "methods": methods})
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    report = {"schema_version": 1, "scope": "development-source-span-similarity-pilot",
              "dataset_fingerprint": fingerprint,
              "input_sha256": {name: digest(getattr(args, name).read_bytes()) for name in
                               ("annotations", "source_review", "contexts", "reference_review", "spans", "pilot", "partition_report")},
              "code_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                              ("span_selection.py", "source_spans.py", "context_selection.py", "reference_review.py",
                               "local_embeddings.py", "episodes.py", "lexical_boundaries.py", "data.py", "uv.lock")},
              "encoder": asdict(encoder.statistics), "reports": reports,
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "peak_rss_mib": peak / (1024 * 1024 if platform.system() == "Darwin" else 1024),
              "limitations": ["three deliberately selected known hard cases, not representative or held-out",
                              "single coding-assistant span judgments with prior source/result exposure",
                              "topic-support means current reply's subject evidence, not verified topic labels",
                              "mixed-rejection spans contain useful subject and rejected details; not safe borrowing",
                              "whole admitted assistant candidate scope, source-only similarity, no endorsement model",
                              "preceding-user and captured-chain queries are bounded source-only controls, not referent resolution",
                              "additional non-generative encoder work; not full ingestion or guided-search evaluation"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"cases": len(reports), "encoder": asdict(encoder.statistics),
                      "controls": [{"ordinal": r["ordinal"], "all": r["all"], "methods": [
                          {"method": m["method"], "status": m["status"], "controls": m["controls"]} for m in r["methods"]]}
                                   for r in reports]}))


if __name__ == "__main__":
    main()
