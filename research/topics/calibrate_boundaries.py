"""Leave-canonical-group-out dense cutoff calibration; development only."""
import argparse
from dataclasses import asdict, dataclass
import json
from pathlib import Path
import time

import numpy as np

from episodes import Judgment, read_cases, score
from lexical_boundaries import Features, Method, admissions, boundary_score, sha


@dataclass(frozen=True)
class Row:
    group: str
    thread: str
    provider: str
    left: int
    right: int
    judgment: Judgment
    score: float | None


def threshold(training: tuple[Row, ...]) -> tuple[float | None, dict]:
    known = [row for row in training if row.judgment != Judgment.UNCERTAIN and row.score is not None]
    if any(not np.isfinite(row.score) or not 0 <= row.score <= 2 for row in known):
        raise ValueError("invalid cosine dissimilarity")
    positives = sum(row.judgment == Judgment.CHANGE for row in known)
    negatives = len(known) - positives
    details = {"training_known_scored": len(known), "training_changes": positives,
               "objective": "balanced_accuracy", "tie_break": "higher_cutoff_fewer_splits"}
    if not positives or not negatives:
        return None, {**details, "balanced_accuracy": None, "reason": "insufficient_training_classes"}
    values = sorted({row.score for row in known})
    # Only training-score midpoints and endpoints determine candidate cutoffs.
    candidates = [float(np.nextafter(values[0], -np.inf)),
                  *[(a + b) / 2 for a, b in zip(values, values[1:])],
                  float(np.nextafter(values[-1], np.inf))]

    def objective(cutoff):
        true_positive = sum(row.score > cutoff and row.judgment == Judgment.CHANGE for row in known)
        true_negative = sum(row.score <= cutoff and row.judgment == Judgment.SAME for row in known)
        # Integer numerator avoids floating-point perturbations of exact ties.
        return true_positive * negatives + true_negative * positives, cutoff

    selected = max(candidates, key=objective)
    balanced = objective(selected)[0] / (2 * positives * negatives)
    return selected, {**details, "balanced_accuracy": balanced, "reason": "trained"}


def fold(rows: tuple[Row, ...], group: str) -> tuple[float | None, dict]:
    if not any(row.group == group for row in rows):
        raise ValueError("unknown evaluation group")
    return threshold(tuple(row for row in rows if row.group != group))


def evaluate(rows: tuple[Row, ...], predictions: list[bool | None]) -> dict:
    result = score([row.judgment for row in rows], predictions)
    known = [(row, prediction) for row, prediction in zip(rows, predictions) if row.judgment != Judgment.UNCERTAIN]
    true_negative = sum(row.judgment == Judgment.SAME and prediction is False for row, prediction in known)
    negatives = sum(row.judgment == Judgment.SAME for row, _ in known)
    result.update(tn=true_negative, specificity_including_abstentions=true_negative / negatives if negatives else None)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private reports must stay outside the repository")
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    groups = json.loads(args.partition_report.read_text())["partition"]["source_ids"]
    features = Features({}, admissions(cases, args.source_review))
    rows = tuple(Row(bytes(groups[case.thread]).hex(), case.thread, case.provider, boundary.left, boundary.right,
                     boundary.judgment, boundary_score(case, i, Method.DENSE, features))
                 for case in cases for i, boundary in enumerate(case.boundaries, 1))
    started = time.perf_counter()
    decisions = []
    folds = []
    for group in sorted({row.group for row in rows}):
        cutoff, training = fold(rows, group)
        folds.append({"group": group, "cutoff_dissimilarity": cutoff, "cosine_equivalent": 1 - cutoff if cutoff is not None else None,
                      "training": training, "training_groups": sorted({r.group for r in rows if r.group != group}),
                      "test_known": sum(r.group == group and r.judgment != Judgment.UNCERTAIN for r in rows)})
    by_group = {entry["group"]: entry["cutoff_dissimilarity"] for entry in folds}
    for row in rows:
        cutoff = by_group[row.group]
        calibrated = row.score > cutoff if row.score is not None and cutoff is not None else None
        fixed = row.score > 0.45 if row.score is not None else None
        decisions.append({**asdict(row), "fixed": fixed, "calibrated": calibrated})
    calibration_ms = (time.perf_counter() - started) * 1000
    methods = []
    for method in ("fixed", "calibrated"):
        predicted = [decision[method] for decision in decisions]
        methods.append({"method": method, "metrics": evaluate(rows, predicted),
                        "by_provider": {provider: evaluate(tuple(row for row in rows if row.provider == provider),
                            [decision[method] for row, decision in zip(rows, decisions) if row.provider == provider])
                            for provider in sorted({row.provider for row in rows})}})
    report = {"schema_version": 1, "scope": "leave_canonical_development_group_out_dense_cutoff",
              "dataset_fingerprint": fingerprint, "annotation_sha256": sha(args.annotations.read_bytes()),
              "admission_sha256": sha(args.source_review.read_bytes()), "code_sha256": sha(Path(__file__).read_bytes()),
              "lock_sha256": sha(Path(__file__).with_name("uv.lock").read_bytes()), "calibration_ms": calibration_ms,
              "policy": "maximize training balanced accuracy;higher cutoff breaks ties;strict dissimilarity greater-than",
              "folds": folds, "decisions": decisions, "methods": methods, "inference_calls": 0,
              "limitations": ["exploratory familiar development cohort and single-assistant labels;not independent validation",
                              "source-reviewed oracle admission;no referent/native-branch resolution",
                              "no per-provider or phrase rules;no lexical vocabulary fitting",
                              "not probability calibration;no semantic-topic or complete-ingestion gate measured"]}
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps(methods))


if __name__ == "__main__":
    main()
