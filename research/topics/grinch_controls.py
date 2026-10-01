"""Private, development-only online grouping and order-stability controls."""
import argparse
from collections import Counter
from dataclasses import asdict
from enum import StrEnum
import importlib.metadata
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
from sklearn.cluster import Birch
from sklearn.metrics import adjusted_rand_score
from threadpoolctl import threadpool_limits

from data import load, normalized
from hierarchy_metrics import dendrogram_purity, triplet_signature
from online_tree import Mode, Tree
from reference_review import digest


class Method(StrEnum):
    GREEDY = "greedy"
    ROTATE = "rotate"
    GRINCH = "grinch"
    BIRCH = "birch"


class Order(StrEnum):
    FORWARD = "forward"
    REVERSE = "reverse"
    SHUFFLED = "shuffled"


class Source(StrEnum):
    SESSIONS = "sessions"
    SYNTHETIC = "synthetic"


def synthetic() -> tuple[tuple[str, ...], np.ndarray, dict[str, int]]:
    identities, vectors, labels = [], [], {}
    for label in range(6):
        for offset in range(16):
            identity = f"class-{label}-point-{offset:02d}"
            vector = np.zeros(102)
            vector[label * 17 + offset:label * 17 + offset + 2] = 1
            identities.append(identity)
            vectors.append(normalized(vector))
            labels[identity] = label
    return tuple(identities), np.stack(vectors), labels


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", choices=list(Source), type=Source, required=True)
    parser.add_argument("--method", choices=list(Method), type=Method, required=True)
    parser.add_argument("--order", choices=list(Order), type=Order, required=True)
    parser.add_argument("--database", type=Path)
    parser.add_argument("--partition-report", type=Path)
    parser.add_argument("--reference-manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError("private grouping captures must remain outside Git")
    if args.output.exists():
        raise ValueError("use a fresh output path")
    started = time.perf_counter()
    reference = json.loads(args.reference_manifest.read_bytes())
    if reference["repository"] != "https://github.com/iesl/grinch" or reference["commit"] != "a2747d3c5d3405cf27afc839bc28352b99c3e309":
        raise ValueError("unexpected author-reference capture")
    for path, expected in reference["files"].items():
        if digest((args.reference_manifest.parent / Path(path).name).read_bytes()) != expected:
            raise ValueError("author-reference source changed")
    labels, providers, dataset = None, {}, None
    if args.source == Source.SESSIONS:
        if args.database is None or args.partition_report is None:
            raise ValueError("sessions require explicit frozen database and partition")
        dataset = load(args.database, args.partition_report)
        development = tuple(t for t in dataset.threads if t.group[0] % 5 != 0)
        identities = tuple(t.id for t in development)
        vectors = np.stack([t.vector for t in development])
        providers = {t.id: t.provider for t in development}
    else:
        identities, vectors, labels = synthetic()
    indices = np.arange(len(identities))
    if args.order == Order.REVERSE:
        indices = indices[::-1]
    elif args.order == Order.SHUFFLED:
        indices = np.random.default_rng(17).permutation(len(identities))
    ordered = tuple(identities[i] for i in indices)
    train = vectors[indices]
    timings = []
    tree = None if args.method == Method.BIRCH else Tree(Mode(args.method))
    model = Birch(threshold=.4, branching_factor=64, n_clusters=None) if tree is None else None
    cpu = time.process_time()
    wall = time.perf_counter()
    with threadpool_limits(limits=1):
        for identity, vector in zip(ordered, train):
            insert_started = time.perf_counter()
            if tree is not None:
                tree.insert(identity, vector)
            else:
                model.partial_fit(vector.reshape(1, -1))
            timings.append((time.perf_counter() - insert_started) * 1000)
    fit_wall_ms, fit_cpu_ms = (time.perf_counter() - wall) * 1000, (time.process_time() - cpu) * 1000
    statistics = asdict(tree.statistics) if tree is not None else None
    evaluate_started = time.perf_counter()
    if tree is not None:
        tree.validate()
        cut = tree.cut(.7)
        clusters = tuple(tuple(sorted(ordered[i] for i in members)) for members in cut)
        clades = sorted(digest(json.dumps(sorted(ordered[i] for i in tree.leaf_indices(n)), separators=(",", ":")).encode())
                        for n in tree.nodes.values() if n.children is not None and n is not tree.root)
        hierarchy = {"live_nodes": len(tree.nodes), "clade_sha256": clades,
                     "maximum_leaf_depth": max(len(tree.ancestors(n)) - 1 for n in tree.leaves),
                     "triplet_signature": triplet_signature(tree),
                     "dendrogram_purity": dendrogram_purity(tree, labels) if labels is not None else None}
    else:
        assigned = model.predict(train)
        clusters = tuple(tuple(sorted(ordered[i] for i in range(len(ordered)) if assigned[i] == label))
                         for label in sorted(set(int(label) for label in assigned)))
        hierarchy = None
    clusters = tuple(sorted(clusters))
    fit_labels = {identity: i for i, members in enumerate(clusters) for identity in members}
    evaluation_ms = (time.perf_counter() - evaluate_started) * 1000
    peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    report = {"schema_version": 1, "scope": "development-online-grouping-control", "source": args.source,
              "method": args.method, "order": args.order, "shuffle_seed": 17,
              "input_policy": "raw-compatible-user-thread-means" if dataset else "disjoint-synthetic-path-vectors",
              "generation": dataset.generation if dataset else None,
              "dataset_fingerprint": dataset.fingerprint if dataset else None,
              "partition_sha256": dataset.partition_sha256 if dataset else None,
              "reserved_groups_excluded": len(dataset.threads) - len(identities) if dataset else 0,
              "training_threads": len(ordered), "dimensions": train.shape[1],
              "canonical_identities": sorted(identities), "arrival_identities": ordered,
              "parameters": {"tree_cut": "all-internal-cosines-at-least-0.7", "leaf_bound": 2000,
                             "neighbor_search": "exact-leaves-excluding-descendants-for-graft", "float": "float64",
                             "birch": {"threshold": .4, "branching_factor": 64, "n_clusters": None}},
              "versions": {name: importlib.metadata.version(name) for name in ("numpy", "scikit-learn", "threadpoolctl")},
              "python": platform.python_version(), "platform": platform.platform(),
              "reference": reference, "reference_manifest_sha256": digest(args.reference_manifest.read_bytes()),
              "code_sha256": {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                              ("grinch_controls.py", "online_tree.py", "hierarchy_metrics.py", "data.py", "uv.lock")},
              "fit_wall_ms": fit_wall_ms, "fit_cpu_ms": fit_cpu_ms,
              "insertion_ms": {"p50": float(np.percentile(timings, 50)), "p95": float(np.percentile(timings, 95)),
                               "maximum": max(timings), "samples": timings},
              "statistics": statistics, "evaluation_ms": evaluation_ms, "hierarchy": hierarchy,
              "clusters": clusters, "cluster_sizes": sorted(len(c) for c in clusters),
              "mixed_provider_clusters": sum(len({providers[i] for i in c}) > 1 for c in clusters) if providers else None,
              "provider_counts": dict(Counter(providers.values())),
              "synthetic_native_cut_ari": adjusted_rand_score([labels[i] for i in sorted(identities)],
                                                              [fit_labels[i] for i in sorted(identities)]) if labels else None,
              "inference_calls": 0, "elapsed_ms_after_imports": (time.perf_counter() - started) * 1000,
              "peak_rss_mib": peak / (1024 * 1024 if platform.system() == "Darwin" else 1024),
              "limitations": ["independent GRINCH-style Python implementation, not author-reference execution or byte parity",
                              "float64 stable arrival ties, zero-sum direction zero, strict mutual graft preference",
                              "rotations implemented as pre-insertion ascent; restructuring bounded to post-graft LCA",
                              "native tree cut and BIRCH radius are different uncalibrated operating points",
                              "real inputs are single raw thread means, not multi-topic episodes or labels",
                              "synthetic purity is not real topic quality or a proof that model separation holds",
                              "insert-only; persistence, edits/deletes, overlap and semantic ancestors unimplemented",
                              "fits exclude validation/cuts/signatures; no full ingestion gate or production scalability claim"]}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x") as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({"method": args.method, "source": args.source, "order": args.order,
                      "training": len(ordered), "fit_ms": fit_wall_ms, "p95_ms": report["insertion_ms"]["p95"],
                      "clusters": len(clusters), "statistics": statistics,
                      "synthetic_purity": hierarchy["dendrogram_purity"] if hierarchy else None}))


if __name__ == "__main__":
    main()
