"""Offline contextual boundary controls; source text only, no reviewer summaries."""
import argparse
from dataclasses import asdict
from enum import StrEnum
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from context_selection import Selector, select
from episodes import read_cases, score
from lexical_boundaries import admissions, direction, metrics
from local_embeddings import Encoder
from reference_review import digest, validate


class Representation(StrEnum):
    USER = "user-only"
    JOINED = "referential-exchange-joined"
    MEAN = "referential-exchange-record-mean"


def inputs(text: str, context: dict, referential: bool, method: Representation) -> tuple[str, ...]:
    if not referential or method == Representation.USER:
        return (text,)
    entries = select(context, Selector.EXCHANGE)
    # Do not silently turn an incomplete/missing display into a shorter exchange.
    interval = []
    for entry in reversed(context["display"]):
        interval.append(entry)
        if entry["role"] == "user":
            break
    if (not entries or len(entries) != len(interval) or len(entries) > 4
            or (context["stop"] == "record-limit" and not any(e["role"] == "user" for e in interval))):
        return ()
    texts = tuple(e["text"] for e in entries) + (text,)
    return ("\n\n".join(texts),) if method == Representation.JOINED else texts


def embed(texts: tuple[str, ...], encoder: Encoder) -> tuple[np.ndarray | None, tuple[int, ...]]:
    lengths = tuple(len(encoder.tokenizer.encode(text).ids) for text in texts)
    if not lengths or any(not 2 <= length <= 512 for length in lengths):
        return None, lengths
    return direction([encoder.encode(text) for text in texts]), lengths


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "contexts",
                 "reference-review", "snapshot", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private reports must remain outside Git")
    started = time.perf_counter()
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    admitted = admissions(cases, args.source_review)
    context_bytes, review_bytes = args.contexts.read_bytes(), args.source_review.read_bytes()
    reference_bytes = args.reference_review.read_bytes()
    validate(context_bytes, review_bytes, json.loads(reference_bytes))
    context_report = json.loads(context_bytes)
    if context_report["dataset_fingerprint"] != fingerprint:
        raise ValueError("context dataset changed")
    contexts = {(case["thread"], c["anchor"]["ordinal"]): c
                for case in context_report["cases"] for c in case["contexts"]}
    referential = {j["message"] for case in json.loads(review_bytes)["cases"]
                   for j in case["admission_judgments"] if j["kind"] == "referential"}
    encoder = Encoder(args.snapshot)
    reports = []
    for method in Representation:
        before = encoder.statistics.calls
        vectors, coverage = {}, []
        for case in cases:
            for turn in case.turns:
                if turn.id not in admitted:
                    continue
                context = contexts[(case.thread, turn.ordinal)]
                anchor = context["anchor"]
                if anchor["id"] != turn.id or anchor["searchable_sha256"] != digest(turn.text.encode()):
                    raise ValueError("context anchor source changed")
                texts = inputs(turn.text, context, turn.id in referential, method)
                vector, lengths = embed(texts, encoder)
                vectors[turn.id] = vector
                coverage.append({"thread": case.thread, "ordinal": turn.ordinal,
                                 "referential": turn.id in referential, "encoded": vector is not None,
                                 "input_records": len(texts), "input_tokens": lengths,
                                 "status": "encoded" if vector is not None else
                                           "over-model-window" if lengths else "incomplete-or-over-record-cap"})
        rows = []
        for case in cases:
            for i, boundary in enumerate(case.boundaries, 1):
                left, right = (vectors.get(t.id) for t in case.turns[i - 1:i + 1])
                value = 1 - float(np.clip(left @ right, -1, 1)) if left is not None and right is not None else None
                rows.append({"thread": case.thread, "provider": case.provider, "left": boundary.left,
                             "right": boundary.right, "judgment": boundary.judgment, "score": value})
        reports.append({"method": method, "new_encoder_calls": encoder.statistics.calls - before,
                        "coverage": coverage, "metrics": metrics(rows), "rows": rows})
    common = [i for i in range(len(reports[0]["rows"])) if all(r["rows"][i]["score"] is not None for r in reports)]
    for report in reports:
        shared = [report["rows"][i] for i in common]
        report["common_support_metrics"] = metrics(shared)
        report["common_by_provider"] = {provider: metrics([r for r in shared if r["provider"] == provider])
                                        for provider in sorted({case.provider for case in cases})}
        report["fixed_cosine055_metrics"] = score(
            [r["judgment"] for r in report["rows"]],
            [r["score"] > 0.45 if r["score"] is not None else None for r in report["rows"]])
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    sources = ("context_embeddings.py", "context_selection.py", "reference_review.py", "local_embeddings.py",
               "lexical_boundaries.py", "episodes.py", "data.py", "uv.lock")
    report = {"schema_version": 1, "scope": "development_contextual_boundary_rankings",
              "dataset_fingerprint": fingerprint, "context_sha256": digest(context_bytes),
              "reference_review_sha256": digest(reference_bytes), "source_review_sha256": digest(review_bytes),
              "annotations_sha256": digest(args.annotations.read_bytes()),
              "code_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in sources},
              "policy": {"admission": "oracle-original-content-and-referential", "context": "preceding-exchange",
                         "context_records_cap": 4, "context_only_for_original_referential": True,
                         "overflow": "abstain-no-truncation", "mean": "equal-normalized-record-directions",
                         "reference_resolutions_used_for_selection": False,
                         "fixed_comparator_cosine": 0.55},
              "encoder": asdict(encoder.statistics), "reports": reports,
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "peak_rss_mib": peak / (1024 * 1024 if platform.system() == "Darwin" else 1024),
              "limitations": ["frozen user-only exploratory boundaries, not full-dialogue topic judgments",
                              "oracle admission, no automatic referent resolution",
                              "source preceding assistant proposals may contain unendorsed subjects",
                              "linear comparison of user anchors, not native-branch segmentation",
                              "not ingestion overhead or semantic topic quality",
                              "singleton one-thread CPU control, not Rust worker or CoreML throughput"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"encoder": report["encoder"], "reports": [
        {"method": r["method"], "metrics": r["metrics"], "common": r["common_support_metrics"],
         "encoded": sum(c["encoded"] for c in r["coverage"]), "inputs": len(r["coverage"]),
         "new_calls": r["new_encoder_calls"]} for r in reports]}))


if __name__ == "__main__":
    main()
