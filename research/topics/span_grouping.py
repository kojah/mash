"""Offline matched-unit experiment; no reviewer text guides embeddings or groups."""
import argparse
from collections import Counter
from dataclasses import asdict
import importlib.metadata
import json
from pathlib import Path
import platform
import resource
import time

from local_embeddings import Encoder
from pair_review import validate_pairs
from reference_review import digest
from source_spans import load_documents
from span_units import reconstruct
from unit_grouping import Geometry, Method, Representation, cluster, evaluate, geometries, keywords, points


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "contexts",
                 "reference-review", "spans", "pair-review", "snapshot", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private source labels and memberships must remain outside Git")
    if args.output.exists():
        raise ValueError("use a fresh output path")
    started = time.perf_counter()
    fingerprint, documents = load_documents(args.database, args.partition_report, args.annotations,
                                           args.source_review, args.contexts, args.reference_review)
    spans_bytes, review_bytes = args.spans.read_bytes(), args.pair_review.read_bytes()
    source_report = json.loads(spans_bytes)
    if source_report["dataset_fingerprint"] != fingerprint:
        raise ValueError("source unit dataset changed")
    for name in ("annotations", "source_review", "contexts", "reference_review"):
        if source_report["input_sha256"][name] != digest(getattr(args, name).read_bytes()):
            raise ValueError("captured source inputs changed")
    units = reconstruct(documents, source_report)
    pairs = validate_pairs(json.loads(review_bytes), spans_bytes)
    encoder = Encoder(args.snapshot)
    encode_started = time.perf_counter()
    vectors, coverage = {}, []
    for unit in units:
        tokens = len(encoder.tokenizer.encode(unit.text).ids)
        encoded = 2 <= tokens <= 512
        vectors[unit.key] = encoder.encode(unit.text) if encoded else None
        coverage.append({**unit.metadata(), "tokens": tokens, "encoded": encoded,
                         "status": "encoded" if encoded else "outside-model-window"})
    encoding_ms = (time.perf_counter() - encode_started) * 1000
    feature_started = time.perf_counter()
    features = geometries(units, vectors)
    feature_ms = (time.perf_counter() - feature_started) * 1000
    reports = []
    for geometry in Geometry:
        for representation in Representation:
            pool_started = time.perf_counter()
            represented = points(units, features[geometry], representation)
            pooling_ms = (time.perf_counter() - pool_started) * 1000
            for method in Method:
                for reverse in (False, True):
                    labels, grouping_ms = cluster(represented, method, reverse)
                    label_started = time.perf_counter()
                    names = keywords(units, represented, labels)
                    labeling_ms = (time.perf_counter() - label_started) * 1000
                    evaluation_started = time.perf_counter()
                    assessment = evaluate(pairs, represented, labels)
                    evaluation_ms = (time.perf_counter() - evaluation_started) * 1000
                    reports.append({"geometry": geometry, "representation": representation, "method": method, "reverse": reverse,
                                    "points": len(represented), "represented_spans": sum(len(p.keys) for p in represented),
                                    "noise_points": int((labels < 0).sum()), "clusters": names,
                                    "cluster_count": len(names), "pooling_ms": pooling_ms,
                                    "grouping_ms": grouping_ms, "labeling_ms": labeling_ms,
                                    "evaluation_ms": evaluation_ms, **assessment})
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    report = {"schema_version": 1, "scope": "development-matched-source-unit-grouping",
              "dataset_fingerprint": fingerprint,
              "input_sha256": {name: digest(getattr(args, name).read_bytes()) for name in
                               ("partition_report", "annotations", "source_review", "contexts", "reference_review", "spans", "pair_review")},
              "source_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                                 ("span_grouping.py", "span_units.py", "unit_grouping.py", "pair_review.py", "source_spans.py",
                                  "online_tree.py", "local_embeddings.py", "phrase_labels.py", "subject_labels.py", "preceding_context.py",
                                  "lexical_boundaries.py", "reference_review.py", "episodes.py", "data.py", "uv.lock")},
              "versions": {name: importlib.metadata.version(name) for name in ("numpy", "scikit-learn")},
              "python": platform.python_version(), "platform": platform.platform(),
              "source_records": len(documents), "source_units": len(units), "coverage": coverage,
              "role_counts": dict(Counter(u.role for u in units)), "encoding_ms": encoding_ms,
              "feature_ms": feature_ms,
              "matched_spans": sum(v is not None for v in features[Geometry.DENSE].values()),
              "encoder": asdict(encoder.statistics), "reports": reports,
              "parameters": {"pooling": "equal normalized span directions, same encoded source coverage",
                             "geometry": "dense384; nonnegative fixed lexical hash512 source-adjacent terms, no fit/IDF; equal normalized concatenation896",
                             "birch": {"threshold": .4, "branching_factor": 64, "n_clusters": None},
                             "grinch": "exact leaves; all internal merge cosines>=0.7 for native cut",
                             "average": "cosine average linkage distance threshold0.3",
                             "hdbscan": {"min_cluster_size": 5, "min_samples": 5, "selection": "eom"},
                             "labels": "source-adjacent unigram/bigram c-TF-IDF, top8, English stopwords, max8192",
                             "centroid_diagnostic": "all cosine>=0.7 affiliations; not calibrated or HDBSCAN prediction"},
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "peak_rss_mib": peak / (1024 * 1024 if platform.system() == "Darwin" else 1024),
              "limitations": ["oracle admitted anchors and original reference-evidence scope, including metadata proposals",
                              "50 purposive exposed coding-assistant pair judgments, not independent or representative quality",
                              "shared fine task aspects, with uncertain granularity excluded and multi-aspect spans permitted",
                              "same encoded spans pooled by record/thread; not text-level embeddings or current Rust representations",
                              "all structural kinds retained; metadata/procedure/rejected propositions are not auto filtered",
                              "native cuts use different uncalibrated criteria; raw pair ranking is distinct from grouping",
                              "72 sequential grouping controls with shared encoder cache; transductive label vocabulary/classes",
                              "all geometries restricted to encoded and nonzero lexical span intersection; hashing collisions and no IDF",
                              "grouping_ms includes tree validation and cut extraction, unlike earlier insert-only timings",
                              "private terms are source-supported keyword candidates, not independently validated topic names",
                              "no semantic ancestors, edit/delete persistence, ingestion gate or guided-search gain established"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"source_records": len(documents), "spans": len(units),
                      "encoded": sum(c["encoded"] for c in coverage), "encoder": asdict(encoder.statistics),
                      "controls": len(reports), "elapsed_ms": report["elapsed_ms_after_imports"]}))


if __name__ == "__main__":
    main()
