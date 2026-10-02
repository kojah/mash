"""Bounded causal vector aggregation; not semantic episode segmentation."""
from dataclasses import dataclass
from enum import StrEnum

import numpy as np


class Status(StrEnum):
    READY = 'ready'
    UNSUPPORTED = 'unsupported_source'
    CANCELLED = 'cancelled_direction'


class Limit(StrEnum):
    ROOT = 'meeting_root'
    TURNS = 'five_turn_limit'
    TOKENS = 'whole_turn_token_limit'
    GAP = 'unsupported_prior_source'
    SOURCE = 'unsupported_current_source'


@dataclass(frozen=True)
class Unit:
    ordinal: int
    start: int | None
    tokens: int
    status: Status
    limit: Limit


def aggregate(vectors, ready, tokens):
    vectors = np.asarray(vectors, dtype=np.float64)
    ready = np.asarray(ready);tokens = np.asarray(tokens)
    if (vectors.ndim != 2 or not 0 < vectors.shape[0] <= 2048 or not 0 < vectors.shape[1] <= 20000
            or ready.shape != (len(vectors),) or ready.dtype != np.bool_
            or tokens.shape != ready.shape or not np.issubdtype(tokens.dtype, np.integer)
            or (tokens < 2).any() or (tokens[ready] > 512).any() or not np.isfinite(vectors).all()
            or not np.allclose(np.linalg.norm(vectors[ready], axis=1), 1, rtol=0, atol=1e-12)):
        raise ValueError('complete finite source directions, boolean support and model token counts required')
    output = np.zeros_like(vectors);units = []
    for end in range(len(vectors)):
        if not ready[end]:
            units.append(Unit(end, None, 0, Status.UNSUPPORTED, Limit.SOURCE));continue
        start = end;total = int(tokens[end]);limit = None
        while start > 0 and end-start < 4:
            if not ready[start-1]:limit = Limit.GAP;break
            if total+int(tokens[start-1]) > 512:limit = Limit.TOKENS;break
            start -= 1;total += int(tokens[start])
        if limit is None:limit = Limit.ROOT if start == 0 else Limit.TURNS
        summed = np.sum(vectors[start:end+1], axis=0, dtype=np.float64);norm = float(np.linalg.norm(summed))
        status = Status.CANCELLED if norm <= 1e-12 else Status.READY
        if status == Status.READY:output[end] = summed/norm
        units.append(Unit(end, start, total, status, limit))
    return output, units
