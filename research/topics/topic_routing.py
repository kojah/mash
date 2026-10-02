"""Bounded insert-only BIRCH and automatic topic-routing research controls."""
from dataclasses import dataclass
from enum import StrEnum
import hashlib

import numpy as np
from sklearn.cluster import Birch


class Membership(StrEnum):
    HARD = 'hard-balanced'
    OVERLAP = 'overlap-max-window'


@dataclass(frozen=True)
class Match:
    topic: int
    score: float


class DiscoveryBudget(ValueError):
    def __init__(self, maximum, trace):
        super().__init__('BIRCH leaf budget exceeded; do not truncate sources')
        self.maximum = maximum
        self.trace = trace


def unit_matrix(values):
    values = np.asarray(values, dtype=np.float64)
    if (values.ndim != 2 or not len(values) or not 1 <= values.shape[1] <= 1024
            or not np.isfinite(values).all()
            or not np.allclose(np.linalg.norm(values, axis=1), 1, atol=1e-12, rtol=0)):
        raise ValueError('finite complete unit directions required')
    return values


def discover(vectors, reverse=False, maximum=8192):
    vectors = unit_matrix(vectors)
    if len(vectors) > 100000 or type(reverse) is not bool or type(maximum) is not int or not 1 <= maximum <= 8192:
        raise ValueError('bounded complete arrival stream required')
    model = Birch(threshold=.55, branching_factor=50, n_clusters=None, compute_labels=False)
    order = np.arange(len(vectors))[::-1] if reverse else np.arange(len(vectors))
    trace = []
    for start in range(0, len(order), 256):
        model.partial_fit(vectors[order[start:start+256]])
        count = len(model.subcluster_centers_)
        trace.append({'inserted': min(start+256, len(order)), 'centroids': count})
        if count > maximum:
            raise DiscoveryBudget(maximum, trace)
    raw = model.subcluster_centers_.copy()
    lengths = np.linalg.norm(raw, axis=1)
    if not np.isfinite(raw).all() or (lengths <= 1e-12).any():
        raise ValueError('canceling or nonfinite centroid; no invented topic')
    ids = [hashlib.sha256(row.astype('<f8').tobytes()).hexdigest() for row in raw]
    if len(set(ids)) != len(ids):
        raise ValueError('duplicate centroid snapshot identities')
    canonical = np.argsort(ids, kind='stable')
    return raw[canonical], raw[canonical]/lengths[canonical, None], [ids[i] for i in canonical], trace


def select(scores, *, overlap=True):
    scores = np.asarray(scores, dtype=np.float64)
    if scores.ndim != 1 or not 1 <= len(scores) <= 8192 or not np.isfinite(scores).all() or (np.abs(scores) > 1+1e-12).any():
        raise ValueError('complete finite centroid cosines required')
    order = np.argsort(-scores, kind='stable')
    if not overlap:
        return (Match(int(order[0]), float(scores[order[0]])),)
    best = float(scores[order[0]])
    if best < .55:
        return ()
    return tuple(Match(int(i), float(scores[i])) for i in order[:3] if scores[i] >= .55 and scores[i] >= .85*best)


def merge_top(left, right):
    """Top-three maxima compose exactly with canonical topic-index ties."""
    scores = {}
    for item in (*left, *right):
        scores[item.topic] = max(scores.get(item.topic, -np.inf), item.score)
    return tuple(Match(i, score) for i, score in sorted(scores.items(), key=lambda pair: (-pair[1], pair[0]))[:3])


def memberships(vectors, centers, units, owners):
    vectors, centers, units = map(unit_matrix, (vectors, centers, units))
    owners = np.asarray(owners)
    if (vectors.shape[1] != centers.shape[1] or units.shape[1] != centers.shape[1]
            or len(centers) > 8192 or owners.shape != (len(units),) or owners.dtype.kind not in 'iu'
            or (owners < 0).any() or (owners >= len(vectors)).any()
            or set(owners.tolist()) != set(range(len(vectors)))):
        raise ValueError('complete window ownership and aligned dimensions required')
    hard = []
    for start in range(0, len(vectors), 128):
        scores = vectors[start:start+128] @ centers.T
        hard.extend(select(row, overlap=False) for row in scores)
    maximum = [() for _ in vectors]
    for start in range(0, len(units), 128):
        scores = units[start:start+128] @ centers.T
        order = np.argsort(-scores, axis=1, kind='stable')[:, :3]
        for row, selected, owner in zip(scores, order, owners[start:start+128]):
            maximum[owner] = merge_top(maximum[owner], tuple(Match(int(i), float(row[i])) for i in selected))
    overlap = [tuple(m for m in matches if m.score >= .55 and m.score >= .85*matches[0].score)
               if matches and matches[0].score >= .55 else () for matches in maximum]
    return {Membership.HARD: tuple(hard), Membership.OVERLAP: tuple(overlap)}


def posting_lists(members, topics):
    if not 1 <= topics <= 8192:
        raise ValueError('bounded topic count required')
    result = [[] for _ in range(topics)]
    for doc, matches in enumerate(members):
        if len(matches) > 3 or len({m.topic for m in matches}) != len(matches):
            raise ValueError('unique bounded memberships required')
        for match in matches:
            if not 0 <= match.topic < topics or not np.isfinite(match.score) or abs(match.score) > 1+1e-12:
                raise ValueError('valid centroid membership required')
            result[match.topic].append((doc, match.score))
    return result


def route(query, centers, postings, budget=1000):
    centers = unit_matrix(centers)
    query = unit_matrix(np.asarray(query).reshape(1, -1))[0]
    if len(postings) != len(centers) or type(budget) is not int or not 1 <= budget <= 1000:
        raise ValueError('aligned postings and bounded routing budget required')
    topics = select(centers @ query)
    candidates = {}
    visits = 0
    for match in topics:
        for doc, confidence in postings[match.topic]:
            value = match.score * confidence
            candidates[doc] = max(candidates.get(doc, -np.inf), value)
            visits += 1
    # Document indices are lexical original-ID order. No dense prescoring.
    order = sorted(candidates, key=lambda doc: (-candidates[doc], doc))[:budget]
    return topics, [(i, candidates[i]) for i in order], len(candidates), visits


def routed_metrics(candidates, dense_scores, ids, relevant):
    if ids != sorted(set(ids)) or not relevant or not set(relevant) <= set(ids):
        raise ValueError('complete source identities and explicit relevance required')
    dense_scores = np.asarray(dense_scores)
    chosen = [i for i, _ in candidates]
    if len(set(chosen)) != len(chosen) or any(i < 0 or i >= len(ids) for i in chosen) or not np.isfinite(dense_scores).all() or dense_scores.shape != (len(chosen),):
        raise ValueError('scores only for unique routed candidates required')
    order = sorted(range(len(chosen)), key=lambda i: (-float(dense_scores[i]), ids[chosen[i]]))
    ranked = [ids[chosen[i]] for i in order]
    ranks = {doc: ranked.index(doc)+1 if doc in ranked else None for doc in sorted(relevant)}
    known = [rank for rank in ranks.values() if rank is not None]
    import math
    first = min(known, default=None)
    return {'metrics': {'mrr10': 1/first if first is not None and first <= 10 else 0,
                        'ndcg10': sum(1/math.log2(rank+1) for rank in known if rank <= 10)/sum(1/math.log2(i+1) for i in range(1,min(10,len(relevant))+1)),
                        'recall10': sum(rank <= 10 for rank in known)/len(relevant),
                        'recall100': sum(rank <= 100 for rank in known)/len(relevant)},
            'candidate_recall': len(known)/len(relevant), 'relevant_ranks': ranks,
            'top100': [{'id': ids[chosen[i]], 'score': float(dense_scores[i])} for i in order[:100]]}
