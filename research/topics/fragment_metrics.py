"""Parent-aspect evidence from multiple exact source fragments."""
from collections import defaultdict

import numpy as np

from fragments import Fragment
from lexical_boundaries import direction
from pair_review import Pair
from unit_grouping import Point, pair_metrics


def evaluate(pairs: tuple[Pair, ...], fragments: tuple[Fragment, ...],
             points: tuple[Point, ...], labels: np.ndarray) -> dict:
    parents = {f.unit.key: f.parent for f in fragments}
    positions = defaultdict(list)
    for i, point in enumerate(points):
        for key in point.keys:
            positions[parents[key]].append(i)
    ids = sorted(set(int(label) for label in labels if label >= 0))
    centers = {label: direction([p.vector for p, fit in zip(points, labels) if fit == label]) for label in ids}
    affiliations = [{label for label, center in centers.items() if center is not None and p.vector @ center >= .7}
                    for p in points]
    rows = []
    for pair in pairs:
        left, right = positions[pair.left], positions[pair.right]
        available = bool(left and right)
        native_left = {int(labels[i]) for i in left if labels[i] >= 0}
        native_right = {int(labels[i]) for i in right if labels[i] >= 0}
        multi_left = set().union(*(affiliations[i] for i in left))
        multi_right = set().union(*(affiliations[i] for i in right))
        candidates = [(float(np.clip(points[i].vector @ points[j].vector, -1, 1)), i, j)
                      for i in left for j in right]
        best = max(candidates, key=lambda candidate: candidate[0]) if candidates else None
        rows.append({"left": vars(pair.left), "right": vars(pair.right), "relation": pair.relation,
                     "left_vectors": len(left), "right_vectors": len(right),
                     "cosine": best[0] if best else None,
                     "best_fragment_keys": [vars(points[i].keys[0]) for i in best[1:]] if best else None,
                     "native_coassigned": bool(native_left & native_right) if available else None,
                     "centroid07_shared": bool(multi_left & multi_right) if available else None})
    return {"metrics": pair_metrics(rows), "rows": rows,
            "policy": "maximum fragment-pair cosine; any shared nonnoise cluster or diagnostic >=0.7 centroid affinity"}
