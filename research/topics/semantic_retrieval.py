"""Fixed full-coverage field aggregation and rank fusion without reference labels."""
import numpy as np


def aggregate(vectors, weights):
    vectors, weights = np.asarray(vectors, dtype=np.float64), np.asarray(weights, dtype=np.float64)
    if (vectors.ndim != 2 or vectors.shape[1] != 384 or weights.shape != (len(vectors),)
            or not len(vectors) or not np.isfinite(vectors).all() or not np.isfinite(weights).all()
            or (weights < 0).any() or weights.sum() <= 0
            or not np.allclose(np.linalg.norm(vectors, axis=1), 1, atol=1e-12, rtol=0)):
        raise ValueError('complete unit directions and positive total source-token weight required')
    mean = np.average(vectors, axis=0, weights=weights)
    length = float(np.linalg.norm(mean))
    if length <= 1e-12:
        raise ValueError('canceling field directions; do not invent a topic/search vector')
    return mean / length


def rank_fusion(left, right):
    left, right = np.asarray(left), np.asarray(right)
    if left.ndim != 1 or left.shape != right.shape or not len(left) or not np.isfinite(left).all() or not np.isfinite(right).all():
        raise ValueError('aligned complete finite score arrays required')
    ranks = []
    for scores in (left, right):
        rank = np.empty(len(scores), dtype=np.int64)
        # Caller arrays use lexical original ID order for ties.
        rank[np.argsort(-scores, kind='stable')] = np.arange(1, len(scores) + 1)
        ranks.append(rank)
    return 1 / (60 + ranks[0]) + 1 / (60 + ranks[1])
