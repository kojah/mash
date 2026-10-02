"""Prespecified whole-meeting, pooled-query paired bootstrap."""
import numpy as np


def paired(meetings, deltas, meeting_count):
    indices = np.asarray(meetings);values = np.asarray(deltas,dtype=np.float64)
    if (indices.ndim != 1 or indices.shape != values.shape or indices.dtype.kind not in 'iu'
            or (indices < 0).any() or (indices >= meeting_count).any() or not np.isfinite(values).all()):
        raise ValueError('complete finite paired per-query meeting deltas required')
    counts = np.bincount(indices,minlength=meeting_count);sums = np.bincount(indices,weights=values,minlength=meeting_count)
    if (counts == 0).any():raise ValueError('every original meeting must retain its query support')
    draws = np.random.Generator(np.random.PCG64(20261002)).integers(0,meeting_count,size=(10000,meeting_count))
    means = sums[draws].sum(axis=1)/counts[draws].sum(axis=1)
    lower,upper = np.percentile(means,[2.5,97.5])
    return {'point':float(values.mean()),'lower95':float(lower),'upper95':float(upper),
            'resamples':10000,'seed':20261002,'meetings':meeting_count,'queries':len(values)}
