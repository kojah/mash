"""Explicit source-channel mass controls; no labels or reference-derived features."""
from enum import StrEnum

import numpy as np


class View(StrEnum):
    FULL = 'title-body'
    BALANCED = 'title-prose-code'
    PROSE = 'title-prose'


def weighted(title, prose, code, view):
    view = View(view)
    inputs = [np.asarray(v, dtype=float) for v in (title, prose, code)]
    if (inputs[0].ndim != 2 or not inputs[0].shape[0] or any(v.shape != inputs[0].shape for v in inputs)
            or any(not np.isfinite(v).all() or np.any(v < 0) for v in inputs)):
        raise ValueError('aligned finite nonnegative question-channel matrices required')
    title, prose, code = inputs
    sources = {View.FULL: (title, prose + code), View.BALANCED: (title, prose, code),
               View.PROSE: (title, prose)}[view]
    result = np.zeros_like(title)
    evidence = np.zeros(len(title), dtype=int)
    for source in sources:
        totals = source.sum(1, keepdims=True)
        result += np.divide(source, totals, out=np.zeros_like(source), where=totals > 0)
        evidence += (totals[:, 0] > 0)
    return np.divide(result, evidence[:, None], out=np.zeros_like(result), where=evidence[:, None] > 0)


def matrix(records, ids, words, field):
    if len(ids) != len(set(ids)) or len(words) != len(set(words)):
        raise ValueError('unique source identities and vocabulary required')
    output = np.zeros((len(ids), len(words)), dtype=np.int64)
    for row, identity in enumerate(ids):
        record = records[identity]
        if record['status'] != 'supported':
            raise ValueError('complete supported source channels required')
        counts = record['title_counts'] if field == 'title' else record['counts'][field]
        for column, word in enumerate(words):
            value = counts.get(word, 0)
            if type(value) is not int or value < 0:
                raise ValueError('nonnegative integer source counts required')
            output[row, column] = value
    return output
