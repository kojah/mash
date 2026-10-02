"""Explicit mask-aware local window inference; batching fidelity is measured."""
from collections.abc import Sequence

import numpy as np
from numpy.typing import NDArray

from embedding_windows import Window
from local_embeddings import quantized_direction


def inputs(units: Sequence[Window]) -> dict[str, NDArray[np.int64]]:
    if not 1 <= len(units) <= 16 or any(not 2 <= len(w.ids) <= 256 or len(w.ids) != len(w.types) for w in units):
        raise ValueError('bounded complete window batch required')
    length = max(len(w.ids) for w in units)
    ids = np.zeros((len(units), length), dtype=np.int64)
    mask, types = np.zeros_like(ids), np.zeros_like(ids)
    for i, unit in enumerate(units):
        ids[i, :len(unit.ids)], types[i, :len(unit.ids)] = unit.ids, unit.types
        mask[i, :len(unit.ids)] = 1
    return {'input_ids': ids, 'attention_mask': mask, 'token_type_ids': types}


def pooling(hidden: NDArray[np.float32], mask: NDArray[np.int64]) -> NDArray[np.float32]:
    if (hidden.ndim != 3 or hidden.shape[:2] != mask.shape or hidden.shape[2] != 384
            or not np.isfinite(hidden).all() or not np.isin(mask, [0, 1]).all()
            or (mask.sum(1) < 2).any()):
        raise ValueError('finite aligned model output and binary attended mask required')
    # Match existing singleton mean exactly: select attended tokens per row,
    # rather than allowing padding or a changed summation order into the mean.
    return np.stack([row[attention.astype(bool)].mean(0, dtype=np.float32)
                     for row, attention in zip(hidden, mask)])


def infer(session, units: Sequence[Window]) -> tuple[NDArray[np.float32], NDArray[np.float64]]:
    batch = inputs(units)
    hidden = session.run(['last_hidden_state'], batch)[0]
    pooled = pooling(hidden, batch['attention_mask'])
    return pooled, np.stack([quantized_direction(row) for row in pooled])
