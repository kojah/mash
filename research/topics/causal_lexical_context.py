"""Source-only bounded causal count aggregation and explicit retrieval metrics."""
from dataclasses import dataclass
from enum import StrEnum

import numpy as np
from scipy import sparse


class Limit(StrEnum):
    ROOT = 'meeting_root'
    TURNS = 'five_turn_limit'
    TOKENS = 'whole_turn_token_limit'
    CURRENT = 'current_turn_over_budget_kept_intact'


@dataclass(frozen=True)
class Context:
    start: int
    end: int
    tokens: int
    limit: Limit


def contexts(tokens, lengths):
    tokens = np.asarray(tokens)
    if (tokens.ndim != 1 or not 0 < len(tokens) <= 100000 or not np.issubdtype(tokens.dtype, np.integer)
            or (tokens < 0).any() or (tokens > 100000).any() or not lengths
            or any(type(n) is not int or not 1 <= n <= 10000 for n in lengths) or sum(lengths) != len(tokens)):
        raise ValueError('complete bounded lexical lengths and original meeting sizes required')
    result = [];offset = 0
    for length in lengths:
        for end in range(offset, offset+length):
            start = end;total = int(tokens[end]);limit = Limit.CURRENT if total > 1024 else None
            if limit is None:
                while start > offset and end-start < 4:
                    if total+int(tokens[start-1]) > 1024:limit = Limit.TOKENS;break
                    start -= 1;total += int(tokens[start])
                if limit is None:limit = Limit.ROOT if start == offset else Limit.TURNS
            result.append(Context(start, end, total, limit))
        offset += length
    return result


def aggregate(counts, records):
    if (not sparse.isspmatrix_csr(counts) or counts.shape[0] != len(records) or not len(records)
            or not np.issubdtype(counts.dtype, np.integer) or (counts.data <= 0).any()):
        raise ValueError('complete positive integer sparse source counts required')
    columns = [];rows = []
    for i, record in enumerate(records):
        if not isinstance(record, Context) or record.end != i or not 0 <= record.start <= i or i-record.start > 4:
            raise ValueError('complete bounded original context ownership required')
        columns.extend(range(record.start, i+1));rows.extend([i]*(i-record.start+1))
    incidence = sparse.csr_matrix((np.ones(len(rows), dtype=np.int64), (rows, columns)), shape=(len(records), len(records)))
    result = incidence @ counts;result.sum_duplicates();result.sort_indices()
    return result


def rank(scores, candidates, relevant):
    scores = np.asarray(scores, dtype=np.float64);candidates = np.asarray(candidates);relevant = np.asarray(relevant)
    if (scores.ndim != 1 or not len(scores) or candidates.shape != scores.shape or relevant.shape != scores.shape
            or relevant.dtype != np.bool_ or not relevant.any() or not np.isfinite(scores).all()
            or not np.issubdtype(candidates.dtype, np.integer) or len(np.unique(candidates)) != len(candidates)):
        raise ValueError('complete finite candidate scores and nonempty boolean relevance required')
    order = np.lexsort((candidates, -scores));matches = relevant[order];total = int(relevant.sum())
    first = int(np.flatnonzero(matches)[0])+1
    return {'mrr': 1/first, 'first_relevant_rank': first, 'hit1': int(matches[0]), 'hit10': int(matches[:10].any()),
            'recall1': int(matches[:1].sum())/total, 'recall10': int(matches[:10].sum())/total,
            'top10_ordinals': candidates[order[:10]].tolist(), 'top10_scores': scores[order[:10]].tolist()}
