"""Distinct-parent local-pair support; repeated windows never add parent votes."""
from collections import Counter, defaultdict
from enum import StrEnum
from itertools import combinations

import numpy as np

from word_graph import Graph


class Weight(StrEnum):
    WINDOW = 'window-count'
    PARENT = 'parent-count'


def supports(records, parents, vocabulary_size, pairs):
    if (len(records) > 4096 or not parents or len(parents) != len(set(parents))
            or vocabulary_size < 0 or vocabulary_size > 1024):
        raise ValueError('bounded source windows and unique parent identities required')
    known = set(parents)
    evidence = defaultdict(set)
    events = 0
    for record in records:
        parent, words = record['parent'], record['words']
        if parent not in known or words != sorted(set(words)) or any(type(w) is not int or w < 0 or w >= vocabulary_size for w in words):
            raise ValueError('canonical window words and known parent required')
        events += len(words) * (len(words) - 1) // 2
        if events > 2_000_000:
            raise ValueError('pair expansion cap exceeded')
        evidence[parent].update(combinations(words, 2))
    totals = Counter(pair for values in evidence.values() for pair in values)
    if any(len(p) != 2 or p[0] < 0 or p[0] >= p[1] or p[1] >= vocabulary_size for p in pairs):
        raise ValueError('unordered source word pairs required')
    return np.array([totals[tuple(pair)] for pair in pairs], dtype=np.int64), events


def replicated(graph, support, weight):
    weight = Weight(weight)
    support = np.asarray(support)
    if (support.shape != graph.counts.shape or not np.issubdtype(support.dtype, np.integer)
            or np.any(support < 0) or np.any(support > graph.counts)):
        raise ValueError('nonnegative aligned parent counts bounded by window counts required')
    # probabilities/DF retain their original window-null meaning. The graph's
    # counts below are only Infomap weights, not recalculated probability tests.
    return Graph(graph.df, graph.pairs, graph.counts if weight == Weight.WINDOW else support,
                 graph.probabilities, graph.selected & (support >= 2), graph.cutoff)
