"""No-inference development boundary rankings with frozen oracle admission."""
import argparse
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
import sklearn
from sklearn.feature_extraction.text import TfidfVectorizer
from sklearn.metrics import average_precision_score, roc_auc_score

from episodes import Case, Judgment, read_cases


class Admission(StrEnum):
    CONTENT = "content"
    REFERENTIAL = "referential"
    PROTOCOL = "protocol"
    ATTACHMENT = "attachment"
    SENSITIVE = "sensitive"


class Method(StrEnum):
    DENSE = "dense-adjacent"
    DENSE_BLOCKS = "dense-disjoint2"
    LEXICAL = "tfidf-adjacent"
    LEXICAL_BLOCKS = "tfidf-disjoint2"
    HYBRID = "equal-dense-lexical-adjacent"


@dataclass(frozen=True)
class Features:
    lexical: dict[str, np.ndarray]
    admitted: frozenset[str]


def sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def admissions(cases: tuple[Case, ...], path: Path) -> frozenset[str]:
    if path.stat().st_size > 8 * 1024 * 1024:
        raise ValueError("admission sidecar exceeds cap")
    raw = json.loads(path.read_text())
    if raw.get("schema_version") != 1 or len(raw.get("cases", [])) != len(cases):
        raise ValueError("invalid admission schema or scope")
    by_thread = {entry["thread"]: entry for entry in raw["cases"]}
    if len(by_thread) != len(cases) or set(by_thread) != {case.thread for case in cases}:
        raise ValueError("admission cases must match frozen annotation scope")
    accepted = set()
    for case in cases:
        entry = by_thread[case.thread]
        if entry["source_sha256"] != case.source_sha256:
            raise ValueError("admission source changed")
        judgments = entry["admission_judgments"]
        if len(judgments) != len(case.turns):
            raise ValueError("admission must cover every user record")
        for turn, judgment in zip(case.turns, judgments):
            if (judgment["message"], judgment["ordinal"], judgment["text_sha256"]) != (
                    turn.id, turn.ordinal, sha(turn.text.encode())):
                raise ValueError("admission identity or text changed")
            if Admission(judgment["kind"]) in (Admission.CONTENT, Admission.REFERENTIAL):
                accepted.add(turn.id)
    return frozenset(accepted)


def direction(rows: list[np.ndarray | None]) -> np.ndarray | None:
    if not rows or any(row is None for row in rows):
        return None
    center = np.mean(rows, axis=0)
    norm = np.linalg.norm(center)
    if not np.isfinite(norm) or norm <= np.finfo(np.float64).eps:
        return None
    return center / norm


def boundary_score(case: Case, index: int, method: Method, features: Features) -> float | None:
    if not 1 <= index < len(case.turns):
        raise ValueError("invalid boundary index")
    blocks = method in (Method.DENSE_BLOCKS, Method.LEXICAL_BLOCKS)
    left = case.turns[max(0, index - (2 if blocks else 1)):index]
    right = case.turns[index:index + (2 if blocks else 1)]
    # Keep original sequence and abstain at inadmissible records: never bridge
    # generated framing or a secret by silently compressing the conversation.
    if any(turn.id not in features.admitted for turn in (*left, *right)):
        return None

    def similarity(lexical: bool) -> float | None:
        a, b = [direction([features.lexical.get(turn.id) if lexical else turn.vector for turn in group])
                for group in (left, right)]
        return float(np.clip(a @ b, -1, 1)) if a is not None and b is not None else None

    if method == Method.HYBRID:
        dense, lexical = similarity(False), similarity(True)
        return 1 - (dense + lexical) / 2 if dense is not None and lexical is not None else None
    value = similarity(method in (Method.LEXICAL, Method.LEXICAL_BLOCKS))
    return 1 - value if value is not None else None


def metrics(rows: list[dict]) -> dict:
    known = [row for row in rows if row["judgment"] != Judgment.UNCERTAIN]
    scored = [row for row in known if row["score"] is not None]
    labels = [row["judgment"] == Judgment.CHANGE for row in scored]
    values = [row["score"] for row in scored]
    both_classes = bool(labels) and 0 < sum(labels) < len(labels)
    return {"known": len(known), "scored": len(scored), "abstained": len(known) - len(scored),
            "scored_changes": sum(labels), "positive_prevalence": sum(labels) / len(labels) if labels else None,
            "average_precision": float(average_precision_score(labels, values)) if both_classes else None,
            "roc_auc": float(roc_auc_score(labels, values)) if both_classes else None}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private reports must stay outside the repository")
    started = time.perf_counter()
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    admitted = admissions(cases, args.source_review)
    turns = [turn for case in cases for turn in case.turns if turn.id in admitted]
    parameters = {"ngram_range": (1, 2), "lowercase": True, "sublinear_tf": True,
                  "smooth_idf": True, "norm": "l2", "stop_words": None, "min_df": 1,
                  "max_df": 1.0, "max_features": 8192, "token_pattern": r"(?u)\b\w[\w.+:/-]*\b"}
    vectorizer = TfidfVectorizer(**parameters)
    fit_start = time.perf_counter()
    matrix = vectorizer.fit_transform(turn.text for turn in turns)
    fit_ms = (time.perf_counter() - fit_start) * 1000
    features = Features({turn.id: matrix[i].toarray().ravel() for i, turn in enumerate(turns)}, admitted)
    reports = []
    for method in Method:
        score_start = time.perf_counter()
        rows = [{"thread": case.thread, "provider": case.provider, "left": boundary.left, "right": boundary.right,
                 "judgment": boundary.judgment, "score": boundary_score(case, i, method, features)}
                for case in cases for i, boundary in enumerate(case.boundaries, 1)]
        reports.append({"method": method, "scoring_ms": (time.perf_counter() - score_start) * 1000,
                        "metrics": metrics(rows), "rows": rows})
    # Compare rankings on the same support, rather than rewarding abstention.
    common = [i for i in range(len(reports[0]["rows"])) if all(r["rows"][i]["score"] is not None for r in reports)]
    for report in reports:
        shared = [report["rows"][i] for i in common]
        report["common_support_metrics"] = metrics(shared)
        report["common_by_provider"] = {provider: metrics([row for row in shared if row["provider"] == provider])
                                        for provider in sorted({case.provider for case in cases})}
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    result = {"schema_version": 1, "scope": "development_user_only_oracle_admission_rankings",
              "dataset_fingerprint": fingerprint, "annotation_sha256": sha(args.annotations.read_bytes()),
              "admission_sha256": sha(args.source_review.read_bytes()), "parameters": parameters,
              "code_sha256": sha(Path(__file__).read_bytes()), "lock_sha256": sha(Path(__file__).with_name("uv.lock").read_bytes()),
              "python": platform.python_version(), "sklearn": sklearn.__version__, "numpy": np.__version__,
              "fit_documents": len(turns), "vocabulary": len(vectorizer.vocabulary_),
              "features_sha256": sha(json.dumps(vectorizer.get_feature_names_out().tolist()).encode()),
              "fit_ms": fit_ms, "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "peak_rss_mib": peak / (1024 * 1024 if platform.system() == "Darwin" else 1024),
              "inference_calls": 0, "reports": reports,
              "limitations": ["single-assistant exploratory admission/boundary judgments;no independent human labels",
                              "TF-IDF fitted transductively to admitted development records;not an online IDF policy",
                              "one-turn future lookahead for disjoint blocks;no native branch/context resolution",
                              "threshold-free diagnostics,not calibrated predictions or semantic topic quality",
                              "no ingestion/persistence/label-maintenance cost or production overhead gate measured"]}
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(result, output, indent=2, allow_nan=False)
    print(json.dumps({"fit_documents": len(turns), "reports": [{"method": r["method"],
                      "metrics": r["common_support_metrics"]} for r in reports]}))


if __name__ == "__main__":
    main()
