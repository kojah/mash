"""Matched fine-span versus pooled-unit grouping; labels never guide discovery."""
from collections import defaultdict
from dataclasses import dataclass
from enum import StrEnum
import time

import numpy as np
from sklearn.cluster import AgglomerativeClustering, Birch, HDBSCAN
from sklearn.feature_extraction.text import CountVectorizer, HashingVectorizer
from sklearn.metrics import average_precision_score, roc_auc_score
from threadpoolctl import threadpool_limits

from lexical_boundaries import direction
from online_tree import Mode, Tree
from pair_review import Key, Pair, Relation
from phrase_labels import source_terms, supported
from span_units import Unit
from subject_labels import label_scores, ranked_terms


class Representation(StrEnum):
    SPAN = "exact-spans"
    RECORD = "matched-record-span-means"
    THREAD = "matched-thread-span-means"


class Method(StrEnum):
    BIRCH = "birch"
    GRINCH = "grinch"
    AVERAGE = "average"
    HDBSCAN = "hdbscan"


class Geometry(StrEnum):
    DENSE = "dense"
    LEXICAL = "lexical-hash512"
    HYBRID = "equal-dense-lexical"


def geometries(units: tuple[Unit, ...], dense: dict[Key, np.ndarray | None]) -> dict:
    """Fixed lexical basis, no vocabulary fit and no review-derived features.

    All geometries use the intersection of encoded and nonzero lexical spans,
    so comparisons retain exactly the same source support.
    """
    lexical = HashingVectorizer(analyzer=source_terms, n_features=512,
                               alternate_sign=False, norm="l2").transform(u.text for u in units).toarray()
    result = {g: {} for g in Geometry}
    for unit, sparse in zip(units, lexical):
        vector = dense[unit.key]
        available = vector is not None and np.linalg.norm(sparse) > 0
        result[Geometry.DENSE][unit.key] = vector if available else None
        result[Geometry.LEXICAL][unit.key] = sparse if available else None
        result[Geometry.HYBRID][unit.key] = np.concatenate((vector, sparse)) / np.sqrt(2) if available else None
    return result


@dataclass(frozen=True)
class Point:
    keys: tuple[Key, ...]
    vector: np.ndarray


def points(units: tuple[Unit, ...], vectors: dict[Key, np.ndarray | None],
           representation: Representation) -> tuple[Point, ...]:
    groups = defaultdict(list)
    for unit in units:
        if vectors[unit.key] is None:
            continue
        group = (unit.key.thread, unit.key.ordinal, unit.key.index if representation == Representation.SPAN else -1)
        if representation == Representation.THREAD:
            group = (unit.key.thread, -1, -1)
        groups[group].append(unit.key)
    result = []
    for group in sorted(groups):
        keys = tuple(groups[group])
        vector = direction([vectors[k] for k in keys])
        if vector is not None:
            result.append(Point(keys, vector))
    return tuple(result)


def cluster(points: tuple[Point, ...], method: Method, reverse: bool) -> tuple[np.ndarray, float]:
    if not points:
        return np.empty(0, dtype=np.int64), 0.0
    order = list(range(len(points)))
    if reverse:
        order.reverse()
    train = np.stack([points[i].vector for i in order])
    started = time.perf_counter()
    with threadpool_limits(limits=1):
        if method == Method.HDBSCAN and len(points) < 5:
            labels = np.full(len(points), -1, dtype=np.int64)
        elif len(points) == 1:
            labels = np.asarray([0])
        elif method == Method.BIRCH:
            labels = Birch(threshold=.4, branching_factor=64, n_clusters=None).fit_predict(train)
        elif method == Method.AVERAGE:
            labels = AgglomerativeClustering(n_clusters=None, distance_threshold=.3, metric="cosine", linkage="average").fit_predict(train)
        elif method == Method.HDBSCAN:
            labels = HDBSCAN(min_cluster_size=5, min_samples=5, metric="euclidean",
                             cluster_selection_method="eom", n_jobs=1, copy=False).fit_predict(train)
        else:
            tree = Tree(Mode.GRINCH)
            for i, vector in enumerate(train):
                tree.insert(str(i), vector)
            tree.validate()
            labels = np.empty(len(points), dtype=np.int64)
            for label, members in enumerate(tree.cut(.7)):
                labels[list(members)] = label
    elapsed = (time.perf_counter() - started) * 1000
    restored = np.empty(len(points), dtype=np.int64)
    restored[order] = labels
    return restored, elapsed


def evaluate(pairs: tuple[Pair, ...], points: tuple[Point, ...], labels: np.ndarray) -> dict:
    positions = {key: i for i, point in enumerate(points) for key in point.keys}
    ids = sorted(set(int(label) for label in labels if label >= 0))
    centers = {label: direction([p.vector for p, fit in zip(points, labels) if fit == label]) for label in ids}
    memberships = [{label for label, center in centers.items() if center is not None and p.vector @ center >= .7}
                   for p in points]
    rows = []
    for pair in pairs:
        left, right = positions.get(pair.left), positions.get(pair.right)
        available = left is not None and right is not None
        rows.append({"left": vars(pair.left), "right": vars(pair.right), "relation": pair.relation,
                     "cosine": float(np.clip(points[left].vector @ points[right].vector, -1, 1)) if available else None,
                     "native_coassigned": bool(labels[left] >= 0 and labels[left] == labels[right]) if available else None,
                     "centroid07_shared": bool(memberships[left] & memberships[right]) if available else None})
    known = [r for r in rows if r["relation"] != Relation.UNCERTAIN]
    ranked = [r for r in known if r["cosine"] is not None]
    expected = [r["relation"] == Relation.OVERLAP for r in ranked]
    metrics = {"known_pairs": len(known), "ranked_pairs": len(ranked),
               "uncertain_excluded": len(rows) - len(known),
               "average_precision": float(average_precision_score(expected, [r["cosine"] for r in ranked])) if any(expected) else None,
               "roc_auc": float(roc_auc_score(expected, [r["cosine"] for r in ranked])) if len(set(expected)) == 2 else None}
    for method in ("native_coassigned", "centroid07_shared"):
        actual = [r for r in known if r[method] is not None]
        metrics[method] = {"true_positive": sum(r["relation"] == Relation.OVERLAP and r[method] for r in actual),
                           "false_positive": sum(r["relation"] == Relation.DISTINCT and r[method] for r in actual),
                           "false_negative": sum(r["relation"] == Relation.OVERLAP and not r[method] for r in actual),
                           "true_negative": sum(r["relation"] == Relation.DISTINCT and not r[method] for r in actual),
                           "abstentions": len(known) - len(actual)}
    return {"metrics": metrics, "rows": rows,
            "centroid07_policy": "shared diagnostic cosine>=0.7 affiliations; not HDBSCAN prediction or calibrated confidence"}


def keywords(units: tuple[Unit, ...], points: tuple[Point, ...], labels: np.ndarray) -> list[dict]:
    ids = sorted(set(int(label) for label in labels if label >= 0))
    if not ids:
        return []
    source = {u.key: u for u in units}
    active = sorted({k for p in points for k in p.keys})
    positions = {key: i for i, key in enumerate(active)}
    vectorizer = CountVectorizer(analyzer=source_terms, max_features=8192)
    background = vectorizer.fit_transform(source[k].text for k in active).toarray()
    groups = [tuple(sorted(k for p, fit in zip(points, labels) if fit == label for k in p.keys)) for label in ids]
    counts = np.stack([background[[positions[k] for k in group]].sum(axis=0) for group in groups])
    scores, _ = label_scores(counts, background)
    terms = vectorizer.get_feature_names_out().tolist()
    result = []
    for i, keys in enumerate(groups):
        names = ranked_terms(scores[i], terms)
        if any(not supported(name["term"], [source[k].text for k in keys]) for name in names):
            raise ValueError("keyword is not an exact source phrase")
        result.append({"members": [vars(k) for k in keys], "keywords": names})
    return result
