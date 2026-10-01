"""Offline bounded multi-vector aspect experiment on exact frozen sources."""
import argparse
from dataclasses import asdict
import importlib.metadata
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from fragment_metrics import evaluate
from fragments import Mode, split
from local_embeddings import Encoder
from pair_review import Relation
from reference_review import digest
from span_grouping import load_scope
from unit_grouping import Method, Representation, cluster, keywords, points


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("database", "partition-report", "annotations", "source-review", "contexts",
                 "reference-review", "spans", "pair-review", "snapshot", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]) or args.output.exists():
        raise ValueError("use a fresh private output path outside Git")
    started = time.perf_counter()
    cpu_started = time.process_time()
    fingerprint, documents, units, pairs = load_scope(args)
    encoder = Encoder(args.snapshot)
    captures = []
    for mode in Mode:
        segmentation_start = time.perf_counter()
        fragments = split(units, mode)
        segmentation_ms = (time.perf_counter() - segmentation_start) * 1000
        before = asdict(encoder.statistics)
        encode_start = time.perf_counter()
        vectors, coverage, latencies = {}, [], []
        for fragment in fragments:
            request_started = time.perf_counter()
            tokens = len(encoder.tokenizer.encode(fragment.unit.text).ids)
            valid = 2 <= tokens <= 512
            vectors[fragment.unit.key] = encoder.encode(fragment.unit.text) if valid else None
            latencies.append((time.perf_counter() - request_started) * 1000)
            coverage.append({**fragment.metadata(), "tokens": tokens, "encoded": valid})
        after = asdict(encoder.statistics)
        encoding_ms = (time.perf_counter() - encode_start) * 1000
        represented = points(tuple(f.unit for f in fragments), vectors, Representation.SPAN)
        represented_keys = {k for p in represented for k in p.keys}
        reports = []
        for method in Method:
            for reverse in (False, True):
                labels, grouping_ms = cluster(represented, method, reverse)
                label_started = time.perf_counter()
                names = keywords(tuple(f.unit for f in fragments), represented, labels)
                labeling_ms = (time.perf_counter() - label_started) * 1000
                evaluation_started = time.perf_counter()
                result = evaluate(pairs, fragments, represented, labels)
                evaluation_ms = (time.perf_counter() - evaluation_started) * 1000
                reports.append({"method": method, "reverse": reverse, "points": len(represented),
                                "cluster_count": len(names), "noise_points": int((labels < 0).sum()),
                                "clusters": names, "grouping_ms": grouping_ms,
                                "labeling_ms": labeling_ms, "evaluation_ms": evaluation_ms, **result})
        captures.append({"mode": mode, "coverage": coverage, "segmentation_ms": segmentation_ms,
                         "encoding_ms": encoding_ms, "encoding_p95_ms": float(np.percentile(latencies, 95)),
                         "encoder_delta": {k: after[k] - before[k] for k in
                             ("calls", "cache_hits", "inference_ms", "tokenization_ms")},
                         "represented_parents": len({f.parent for f in fragments if f.unit.key in represented_keys}),
                         "reports": reports})
    known = [p for p in pairs if p.relation != Relation.UNCERTAIN]
    threads = {k.thread for p in known for k in (p.left, p.right)}
    report = {"schema_version": 1, "scope": "development-bounded-source-fragment-aspects",
              "dataset_fingerprint": fingerprint, "source_records": len(documents), "source_units": len(units),
              "input_sha256": {name: digest(getattr(args, name).read_bytes()) for name in
                  ("partition_report", "annotations", "source_review", "contexts", "reference_review", "spans", "pair_review")},
              "source_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                  ("fragment_experiment.py", "fragments.py", "fragment_metrics.py", "span_grouping.py", "span_units.py",
                   "unit_grouping.py", "pair_review.py", "local_embeddings.py", "source_spans.py", "online_tree.py",
                   "lexical_boundaries.py", "phrase_labels.py", "subject_labels.py", "reference_review.py",
                   "preceding_context.py", "episodes.py", "data.py", "uv.lock")},
              "python": platform.python_version(), "platform": platform.platform(),
              "versions": {name: importlib.metadata.version(name) for name in ("numpy", "scikit-learn")},
              "encoder": asdict(encoder.statistics), "captures": captures,
              "parameters": {"sentence_rules": "terminal .!? followed by whitespace+uppercase/digit; semicolon+whitespace; protect paired inline backticks/double quotes and unmatched suffix",
                             "windows": {"words": 16, "overlap": 4, "cap": 8, "oversize": "increase width to preserve all source words"},
                             "code_headings": "intact", "cutoffs": "BIRCH radius.4; GRINCH merge cosine.7; average distance.3; HDBSCAN minimum cluster/sample5 EOM",
                             "pair_score": "maximum fragment pair cosine; any shared nonnoise native cluster; any shared cosine>=.7 centroid",
                             "labels": "unchanged source-adjacent c-TF-IDF candidates, max8192/top8"},
              "calibration": {"performed": False, "reviewed_threads": len(threads),
                              "reason": "three reviewed threads with cross-case dependence and concentrated family/summary pairs; insufficient independent support for robust cutoff calibration"},
              "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "cpu_ms": (time.process_time() - cpu_started) * 1000,
              "peak_rss_mib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024 * 1024 if platform.system() == "Darwin" else 1024),
              "limitations": ["same exposed purposive50-pair review, oracle source admission/context, not representative or independent evaluation",
                              "max-of-many cosine can inflate unrelated scores; fragments do not resolve grammatical negation scope or endorsement",
                              "source windows can cross clause boundaries; sentence rules are not a full English parser",
                              "code/headings remain intact and sentence caps merge source pieces; no semantic guarantee",
                              "shared encoder cache across modes; deltas are marginal costs and not standalone ingestion measurements",
                              "encoding p95 includes preflight and cache-hit requests; overlapping windows duplicate source counts in candidate labels",
                              "no cutoff tuning, semantic ancestors, persistence, Rust promotion or guided-search gain"]}
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"modes": len(captures), "controls": sum(len(c["reports"]) for c in captures),
                      "encoder": report["encoder"], "elapsed_ms": report["elapsed_ms_after_imports"]}))


if __name__ == "__main__":
    main()
