"""Descriptive geometry controls, not an automatic topic-quality winner selector."""
import argparse
from dataclasses import asdict, dataclass
from enum import StrEnum
import importlib.metadata
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
from sklearn.cluster import AgglomerativeClustering, Birch, HDBSCAN
from sklearn.feature_extraction.text import CountVectorizer
from threadpoolctl import threadpool_limits

from data import load, normalized


class Method(StrEnum):
    BIRCH = "birch"
    HDBSCAN = "hdbscan"
    AVERAGE = "average"


@dataclass(frozen=True)
class Cluster:
    id: int
    training_threads: tuple[str, ...]
    terms: tuple[str, ...]


@dataclass(frozen=True)
class Assignment:
    thread: str
    provider: str
    reserved: bool
    fit_cluster: int | None
    nearest_cluster: int | None
    cosine: float | None


@dataclass(frozen=True)
class Report:
    schema_version: int
    input_policy: str
    method: Method
    parameters: dict[str, float | int | str]
    generation: str
    fingerprint: str
    partition_sha256: str
    versions: dict[str, str]
    reserved_discovery: bool
    reversed: bool
    stored_user_vectors: int
    duplicate_messages: int
    duplicate_threads: int
    independent_threads: int
    training_threads: int
    reserved_threads: int
    fit_noise_threads: int
    fit_ms: float
    labeling_ms: float
    process_peak_rss_bytes: int
    clusters: tuple[Cluster, ...]
    assignments: tuple[Assignment, ...]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", type=Path, required=True)
    parser.add_argument("--partition-report", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--method", choices=list(Method), type=Method, required=True)
    parser.add_argument("--holdout", action="store_true")
    parser.add_argument("--reverse", action="store_true")
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError("use a fresh output path")
    dataset = load(args.database, args.partition_report)
    reserved = [thread.group[0] % 5 == 0 for thread in dataset.threads]
    indices = [i for i, held in enumerate(reserved) if not (args.holdout and held)]
    if args.reverse:
        indices.reverse()
    train = np.stack([dataset.threads[i].vector for i in indices])
    parameters: dict[str, float | int | str]
    if args.method == Method.BIRCH:
        parameters = {"threshold": 0.4, "branching_factor": 64, "n_clusters": "none"}
        model = Birch(threshold=0.4, branching_factor=64, n_clusters=None)
    elif args.method == Method.HDBSCAN:
        parameters = {"min_cluster_size": 5, "min_samples": 5, "metric": "euclidean", "cluster_selection_method": "eom"}
        model = HDBSCAN(min_cluster_size=5, min_samples=5, metric="euclidean", cluster_selection_method="eom", n_jobs=1)
    else:
        parameters = {"distance_threshold": 0.3, "metric": "cosine", "linkage": "average"}
        model = AgglomerativeClustering(n_clusters=None, distance_threshold=0.3, metric="cosine", linkage="average")
    started = time.perf_counter()
    with threadpool_limits(limits=1):
        labels = model.fit_predict(train)
    fit_ms = (time.perf_counter() - started) * 1000
    ids = sorted(set(int(label) for label in labels if label >= 0))
    centers = [normalized(train[labels == label].mean(axis=0)) for label in ids]
    started = time.perf_counter()
    # Label-only unigram/bigram control; stop words never alter document vectors.
    clusters = []
    if ids:
        texts = ["\n".join(dataset.threads[indices[i]].text for i in range(len(indices)) if labels[i] == label) for label in ids]
        vectorizer = CountVectorizer(stop_words="english", ngram_range=(1, 2), max_features=20_000)
        counts = vectorizer.fit_transform(texts).astype(np.float64)
        lengths = np.asarray(counts.sum(axis=1)).ravel()
        frequency = np.asarray(counts.sum(axis=0)).ravel()
        idf = np.log1p(lengths.mean() / np.maximum(frequency, 1))
        scores = counts.multiply(1 / np.maximum(lengths, 1)[:, None]).multiply(idf).tocsr()
        terms = vectorizer.get_feature_names_out()
        for row, label in enumerate(ids):
            ranked = scores[row].toarray().ravel()
            top = sorted(np.flatnonzero(ranked), key=lambda i: (-ranked[i], terms[i]))[:8]
            members = tuple(sorted(dataset.threads[indices[i]].id for i in range(len(indices)) if labels[i] == label))
            clusters.append(Cluster(label, members, tuple(str(terms[i]) for i in top)))
    labeling_ms = (time.perf_counter() - started) * 1000
    fit_labels = {index: int(label) for index, label in zip(indices, labels)}
    assignments = []
    for i, thread in enumerate(dataset.threads):
        scores = np.asarray([center @ thread.vector for center in centers])
        best = int(np.argmax(scores)) if len(scores) else None
        # Common diagnostic assignment for held-out vectors. It is not HDBSCAN
        # prediction, calibrated confidence, or overlapping episode assignment.
        cluster = ids[best] if best is not None and scores[best] >= 0.7 else None
        assignments.append(Assignment(thread.id, thread.provider, reserved[i], fit_labels.get(i), cluster, float(scores[best]) if best is not None else None))
    rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if platform.system() != "Darwin":
        rss *= 1024
    report = Report(1, "raw_compatible_user_thread_means", args.method, parameters, dataset.generation, dataset.fingerprint, dataset.partition_sha256, {"python": platform.python_version(), "numpy": importlib.metadata.version("numpy"), "scikit-learn": importlib.metadata.version("scikit-learn"), "platform": platform.platform()}, args.holdout, args.reverse, dataset.stored_user_vectors, dataset.duplicate_messages, dataset.duplicate_threads, len(dataset.threads), len(indices), sum(reserved), int(np.sum(labels < 0)), fit_ms, labeling_ms, rss, tuple(clusters), tuple(assignments))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("x") as output:
        json.dump(asdict(report), output, indent=2, allow_nan=False)
        output.write("\n")


if __name__ == "__main__":
    main()
