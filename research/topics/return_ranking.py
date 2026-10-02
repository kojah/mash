"""Exact conditional return ranking with explicit zero-score abstention and ties."""
import hashlib

import numpy as np


def rank(scores,ordinals):
    scores = np.round(np.asarray(scores,dtype=np.float64),12);ordinals = np.asarray(ordinals)
    if scores.shape != ordinals.shape or scores.ndim != 1 or not np.isfinite(scores).all():raise ValueError('aligned finite scores required')
    keep = np.flatnonzero(scores>0)
    return keep[np.lexsort((ordinals[keep],-scores[keep]))]


def fusion(dense,lexical,ordinals):
    output = np.zeros(len(ordinals),dtype=np.float64)
    for scores in [dense,lexical]:
        ordered = rank(scores,ordinals);output[ordered] += 1/(60+np.arange(1,len(ordered)+1))
    return output


def evaluate(scores,ordinals,relevant):
    relevant = np.asarray(relevant)
    if relevant.dtype != np.bool_ or not relevant.any():raise ValueError('explicit positive return support required')
    ordered = rank(scores,ordinals)
    if relevant.shape != np.asarray(scores).shape:raise ValueError('aligned relevance required')
    positives = np.flatnonzero(relevant[ordered])+1;total = int(relevant.sum());ap = float(np.sum(np.arange(1,len(positives)+1)/positives)/total)
    return {'relevant':total,'returned':len(ordered),'relevant_ranks':positives.tolist(),'ap':ap,
            'rr':1/int(positives[0]) if len(positives) else 0.,'hit10':bool(len(positives) and positives[0]<=10),
            'top10':np.asarray(ordinals)[ordered[:10]].tolist(),
            'ranked_sha256':hashlib.sha256(np.asarray(np.asarray(ordinals)[ordered],dtype='<i8').tobytes()).hexdigest()}
