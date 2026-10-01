"""Label-only source controls with oracle subject support; no discovery or inference."""
import argparse
import heapq
import json
from pathlib import Path
import time

import numpy as np
from sklearn.feature_extraction.text import CountVectorizer

from episodes import read_cases
from lexical_boundaries import admissions
from reference_review import digest


def label_scores(counts: np.ndarray, background: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    if counts.ndim != 2 or background.ndim != 2 or counts.shape[1] != background.shape[1]:
        raise ValueError("incompatible label counts")
    if not np.isfinite(counts).all() or not np.isfinite(background).all() or np.any(counts < 0) or np.any(background < 0):
        raise ValueError("invalid label counts")
    lengths = counts.sum(axis=1)
    frequency = counts.sum(axis=0)
    baseline = counts / np.maximum(lengths[:, None], 1) * np.log1p(lengths.mean() / np.maximum(frequency, 1))
    df = (background > 0).sum(axis=0)
    corpus_idf = np.log((1 + len(background)) / (1 + df)) + 1
    return baseline, baseline * corpus_idf


def ranked_terms(scores: np.ndarray, terms: list[str], *, diverse: bool = False, limit: int = 8) -> list[dict]:
    if scores.ndim != 1 or len(scores) != len(terms) or len(set(terms)) != len(terms):
        raise ValueError("ambiguous label vocabulary")
    if limit < 1 or not np.isfinite(scores).all() or np.any(scores < 0):
        raise ValueError("invalid label ranking bounds or scores")
    result, selected = [], []
    candidates = np.flatnonzero(scores > 0)
    # Preserve lexical ties. Diversity can reject arbitrarily many candidates;
    # only the ordinary path may stop selection at the requested top-k budget.
    key = lambda i: (-scores[i], terms[i])
    ordered = sorted(candidates, key=key) if diverse else heapq.nsmallest(limit, candidates, key=key)
    for index in ordered:
        if diverse:
            atoms = frozenset(terms[index].split())
            if any(atoms <= prior or prior <= atoms for prior in selected):
                continue
            selected.append(atoms)
        result.append({"term": terms[index], "score": float(scores[index])})
        if len(result) == limit:
            break
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private labels must stay outside Git")
    started = time.perf_counter()
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    admitted = admissions(cases, args.source_review)
    review = json.loads(args.source_review.read_text())
    turns = [t for case in cases for t in case.turns if t.id in admitted]
    positions = {t.id: i for i, t in enumerate(turns)}
    subjects = []
    for case in cases:
        source = next(c for c in review["cases"] if c["thread"] == case.thread)
        by_ordinal = {t.ordinal: t for t in case.turns}
        for index, subject in enumerate(source["subjects"]):
            if subject["importance"] == "uncertain":
                continue
            support = subject["support_ordinals"]
            if not support or len(set(support)) != len(support) or any(o not in by_ordinal or by_ordinal[o].id not in admitted for o in support):
                raise ValueError("subject support must be unique admitted source records")
            subjects.append({"thread": case.thread, "subject_index": index,
                             "importance": subject["importance"],
                             "support_ordinals": support,
                             "source_indices": [positions[by_ordinal[o].id] for o in support]})
    parameters = {"ngram_range": (1, 2), "lowercase": True, "stop_words": "english",
                  "max_features": 8192, "token_pattern": r"(?u)\b\w[\w.+:/-]*\b"}
    start = time.perf_counter()
    vectorizer = CountVectorizer(**parameters)
    background = vectorizer.fit_transform(t.text for t in turns).toarray()
    # Count each source record separately: no invented cross-message bigrams.
    counts = np.stack([background[s["source_indices"]].sum(axis=0) for s in subjects])
    baseline, contrastive = label_scores(counts, background)
    terms = vectorizer.get_feature_names_out().tolist()
    for i, subject in enumerate(subjects):
        subject.pop("source_indices")
        subject["labels"] = {"ctfidf": ranked_terms(baseline[i], terms),
                             "corpus-idf": ranked_terms(contrastive[i], terms),
                             "corpus-idf-contained-term-pruning": ranked_terms(contrastive[i], terms, diverse=True)}
    fit_label_ms = (time.perf_counter() - start) * 1000
    report = {"schema_version": 1, "scope": "development-oracle-subject-label-control",
              "dataset_fingerprint": fingerprint, "source_review_sha256": digest(args.source_review.read_bytes()),
              "annotation_sha256": digest(args.annotations.read_bytes()), "parameters": parameters,
              "source_sha256": {name: digest(Path(__file__).with_name(name).read_bytes())
                                 for name in ("subject_labels.py", "lexical_boundaries.py", "episodes.py", "data.py", "uv.lock")},
              "vocabulary_sha256": digest(json.dumps(terms).encode()), "vocabulary": len(terms),
              "background_records": len(turns), "subjects": subjects, "fit_label_ms": fit_label_ms,
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "inference_calls": 0,
              "limitations": ["oracle overlapping subject supports, not discovered clusters",
                              "reference subject names and rationales are never input text",
                              "development transductive vocabulary/IDF, not an online maintenance policy",
                              "English stopwords can remove domain terms; lexical containment is not semantic MMR",
                              "ranked keywords are not validated topic names or semantic parent labels",
                              "fit timing excludes full ingestion and persistence"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"subjects": len(subjects), "background_records": len(turns), "vocabulary": len(terms),
                      "fit_label_ms": fit_label_ms, "inference_calls": 0}))


if __name__ == "__main__":
    main()
