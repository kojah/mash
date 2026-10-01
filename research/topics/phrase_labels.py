"""Source-preserving keyword controls and official YAKE; no embedding/model calls."""
import argparse
import importlib.metadata
import json
from pathlib import Path
import re
import time

import numpy as np
from sklearn.feature_extraction.text import CountVectorizer, ENGLISH_STOP_WORDS
import yake

from episodes import read_cases
from lexical_boundaries import admissions
from reference_review import digest
from subject_labels import label_scores, ranked_terms


TOKEN = re.compile(r"(?u)\b\w[\w.+:/-]*\b")


def source_terms(text: str) -> list[str]:
    result, previous, end = [], None, 0
    for match in TOKEN.finditer(text):
        token = match.group().lower()
        if token in ENGLISH_STOP_WORDS:
            previous = None
        else:
            result.append(token)
            if previous is not None and text[end:match.start()].isspace():
                result.append(previous + " " + token)
            previous = token
        end = match.end()
    return result


def supported(term: str, records: list[str]) -> bool:
    candidate = " ".join(term.split()).casefold()
    if not candidate:
        return False
    pattern = re.compile(r"(?<!\w)" + re.escape(candidate) + r"(?!\w)")
    return any(pattern.search(" ".join(text.split()).casefold()) for text in records)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "label-report", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private label reports must remain outside Git")
    started = time.perf_counter()
    fingerprint, cases = read_cases(args.database, args.partition_report, args.annotations)
    admitted = admissions(cases, args.source_review)
    baseline_bytes = args.label_report.read_bytes()
    baseline = json.loads(baseline_bytes)
    if (baseline["dataset_fingerprint"] != fingerprint
            or baseline["source_review_sha256"] != digest(args.source_review.read_bytes())
            or baseline["annotation_sha256"] != digest(args.annotations.read_bytes())):
        raise ValueError("baseline label source changed")
    source_review = json.loads(args.source_review.read_text())
    expected = {(c["thread"], i): (s["importance"], s["support_ordinals"])
                for c in source_review["cases"] for i, s in enumerate(c["subjects"])
                if s["importance"] != "uncertain"}
    actual = {(s["thread"], s["subject_index"]): (s["importance"], s["support_ordinals"])
              for s in baseline["subjects"]}
    if actual != expected or len(actual) != len(baseline["subjects"]):
        raise ValueError("baseline subject supports changed")
    turns = [t for case in cases for t in case.turns if t.id in admitted]
    positions = {t.id: i for i, t in enumerate(turns)}
    by_thread = {case.thread: {t.ordinal: t for t in case.turns} for case in cases}
    vectorizer = CountVectorizer(analyzer=source_terms, max_features=8192)
    start = time.perf_counter()
    background = vectorizer.fit_transform(t.text for t in turns).toarray()
    subjects, records = [], []
    for subject in baseline["subjects"]:
        support = [by_thread[subject["thread"]][o] for o in subject["support_ordinals"]]
        if any(t.id not in admitted for t in support):
            raise ValueError("ineligible label support")
        subjects.append({k: subject[k] for k in ("thread", "subject_index", "importance", "support_ordinals")})
        records.append([t.text for t in support])
    counts = np.stack([background[[positions[by_thread[s["thread"]][o].id] for o in s["support_ordinals"]]].sum(axis=0)
                       for s in subjects])
    scores, _ = label_scores(counts, background)
    terms = vectorizer.get_feature_names_out().tolist()
    length_weights = np.array([len(t.split()) for t in terms])
    source_feature_ms = (time.perf_counter() - start) * 1000
    extractor = yake.KeywordExtractor(lan="en", n=2, dedupLim=0.9, dedupFunc="seqm", windowsSize=1, top=32)
    start = time.perf_counter()
    baseline_keywords = baseline_noncontiguous = 0
    for i, subject in enumerate(subjects):
        for label in baseline["subjects"][i]["labels"]["ctfidf"]:
            baseline_keywords += 1
            baseline_noncontiguous += not supported(label["term"], records[i])
        joined = "\n.\n".join(records[i])
        if len(joined.encode()) > 65536:
            raise ValueError("bounded label document cap exceeded")
        candidates = extractor.extract_keywords(joined)
        valid = [{"term": term, "score": float(value)} for term, value in candidates if supported(term, records[i])]
        subject["labels"] = {"source-adjacent-ctfidf": ranked_terms(scores[i], terms),
                             "source-adjacent-length-weighted": ranked_terms(scores[i] * length_weights, terms, diverse=True),
                             "yake-source-supported": valid[:8]}
        subject["yake_unsupported_candidates"] = len(candidates) - len(valid)
        for method, labels in subject["labels"].items():
            if any(not supported(label["term"], records[i]) for label in labels):
                raise ValueError("keyword is not a contiguous source phrase")
    ranking_and_yake_ms = (time.perf_counter() - start) * 1000
    report = {"schema_version": 1, "scope": "development-oracle-source-phrase-labels",
              "dataset_fingerprint": fingerprint, "baseline_sha256": digest(baseline_bytes),
              "yake_version": importlib.metadata.version("yake"),
              "parameters": {"source_terms": "technical unigram/bigram; stopword and non-whitespace boundaries",
                             "maximum_features": 8192, "length_weight": "number of phrase tokens",
                             "yake": {"lan": "en", "n": 2, "dedupLim": 0.9, "dedupFunc": "seqm", "windowsSize": 1, "top": 32},
                             "output_top": 8, "record_separator": "newline-period-newline",
                             "source_support": "casefolded-whitespace-normalized-substring-with-word-boundaries"},
              "source_sha256": {name: digest(Path(__file__).with_name(name).read_bytes())
                                 for name in ("phrase_labels.py", "subject_labels.py", "lexical_boundaries.py",
                                              "reference_review.py", "episodes.py", "data.py", "uv.lock")},
              "background_records": len(turns), "vocabulary": len(terms), "subjects": subjects,
              "baseline_keywords": baseline_keywords, "baseline_noncontiguous": baseline_noncontiguous,
              "source_feature_ms": source_feature_ms, "ranking_and_yake_ms": ranking_and_yake_ms,
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000, "inference_calls": 0,
              "limitations": ["oracle overlapping supports and exposed development reviewer; not discovery accuracy",
                              "no reference names or reviewer rationales in candidate inputs",
                              "length weighting is a fixed heuristic, not RAKE or semantic MMR",
                              "source contiguity does not establish meaningfulness or endorsement",
                              "YAKE document concatenation changes position statistics",
                              "not full ingestion or online label-maintenance performance"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"subjects": len(subjects), "vocabulary": len(terms), "source_feature_ms": source_feature_ms,
                      "ranking_and_yake_ms": ranking_and_yake_ms, "inference_calls": 0}))


if __name__ == "__main__":
    main()
