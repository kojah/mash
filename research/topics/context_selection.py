"""Source-only causal window controls; annotations are used only after selection."""
import argparse
from enum import StrEnum
import json
from pathlib import Path
import time

from reference_review import Resolution, digest, validate


class Selector(StrEnum):
    ASSISTANT = "nearest-assistant"
    EXCHANGE = "preceding-exchange"
    CHAIN = "captured-three-user-chain"


def select(context: dict, method: Selector) -> tuple[dict, ...]:
    """No text recognizers, reviewer labels, evidence ordinals or lookahead."""
    candidates = []
    for entry in reversed(context["display"]):
        if method == Selector.ASSISTANT:
            if entry["role"] != "assistant":
                continue
            # Do not leap over missing/redacted nearest assistant prose.
            if entry["display_status"] != "included":
                return ()
            return (entry,)
        if entry["display_status"] == "included":
            candidates.append(entry)
        if method == Selector.EXCHANGE and entry["role"] == "user":
            break
    return tuple(reversed(candidates))


def bounded_exchange(context: dict, maximum_records: int = 4) -> tuple[dict, ...]:
    """Return the whole intact exchange, or abstain; never shorten around a gap."""
    if maximum_records < 1:
        raise ValueError("exchange record bound must be positive")
    entries = select(context, Selector.EXCHANGE)
    interval = []
    for entry in reversed(context["display"]):
        interval.append(entry)
        if entry["role"] == "user":
            break
    if (not entries or len(entries) != len(interval) or len(entries) > maximum_records
            or (context["stop"] == "record-limit" and not any(e["role"] == "user" for e in interval))):
        return ()
    return entries


def evaluate(contexts: dict, judgments: tuple) -> dict:
    by_anchor = {(case["thread"], c["anchor"]["ordinal"]): c
                 for case in contexts["cases"] for c in case["contexts"]}
    reports = {}
    for method in Selector:
        # Compute all selections before consulting reference judgments.
        start = time.perf_counter()
        selected = {key: select(context, method) for key, context in by_anchor.items()}
        elapsed = (time.perf_counter() - start) * 1000
        rows = []
        for judgment in judgments:
            key = (judgment.thread, judgment.ordinal)
            entries = selected[key]
            ordinals = {e["ordinal"] for e in entries}
            required = set(judgment.evidence_ordinals)
            rows.append({"thread": judgment.thread, "ordinal": judgment.ordinal,
                         "resolution": judgment.resolution,
                         "selected_ordinals": sorted(ordinals),
                         "required_records": len(required), "covered_records": len(required & ordinals),
                         "all_evidence_retained": bool(required) and required <= ordinals,
                         "extra_records": len(ordinals - required),
                         "selected_bytes": sum(len(e["text"].encode()) for e in entries),
                         "capture_stop": by_anchor[key]["stop"]})
        resolved = [r for r in rows if r["resolution"] in (Resolution.TASK, Resolution.METADATA)]
        reports[method] = {"resolved_cases": len(resolved),
                           "all_evidence_cases": sum(r["all_evidence_retained"] for r in resolved),
                           "required_records": sum(r["required_records"] for r in resolved),
                           "covered_records": sum(r["covered_records"] for r in resolved),
                           "selected_bytes": sum(r["selected_bytes"] for r in rows),
                           "extra_records": sum(r["extra_records"] for r in rows),
                           "context_on_interaction_or_unresolved": sum(
                               bool(r["selected_ordinals"]) for r in rows
                               if r["resolution"] in (Resolution.INTERACTION, Resolution.UNRESOLVED)),
                           "selected_contexts": len(selected),
                           "selection_ms": elapsed, "rows": rows}
    return reports


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("contexts", "source-review", "annotations", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private evaluation reports must stay outside Git")
    context_bytes, review_bytes = args.contexts.read_bytes(), args.source_review.read_bytes()
    annotation_bytes = args.annotations.read_bytes()
    judgments = validate(context_bytes, review_bytes, json.loads(annotation_bytes))
    report = {"schema_version": 1, "scope": "development-context-evidence-retention",
              "context_sha256": digest(context_bytes), "source_review_sha256": digest(review_bytes),
              "annotation_sha256": digest(annotation_bytes),
              "code_sha256": digest(Path(__file__).read_bytes()),
              "limitations": ["oracle original referential cohort; not an admission classifier",
                              "evidence retention is not semantic resolution or topic accuracy",
                              "extra records are not necessarily irrelevant",
                              "single exposed coding-assistant judgments; not independent human truth",
                              "selection timing excludes capture, encoding, ingestion and persistence"],
              "methods": evaluate(json.loads(context_bytes), judgments)}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2)
    print(json.dumps({method: {k: v for k, v in values.items() if k != "rows"}
                      for method, values in report["methods"].items()}))


if __name__ == "__main__":
    main()
