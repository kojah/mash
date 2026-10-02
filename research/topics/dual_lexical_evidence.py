"""Fixed equal-weight full-list RRF for two source-aligned evidence views."""
import numpy as np


def fuse(current, context, identities):
    identities = np.asarray(identities)
    views = [np.asarray(v, dtype=np.float64) for v in [current, context]]
    if (identities.ndim != 1 or not len(identities) or not np.issubdtype(identities.dtype, np.integer)
            or len(np.unique(identities)) != len(identities) or (identities < 0).any()
            or any(v.shape != identities.shape or not np.isfinite(v).all() for v in views)):
        raise ValueError('complete finite aligned views and unique source identities required')
    result = np.zeros(len(identities), dtype=np.float64)
    for scores in views:
        order = np.lexsort((identities, -scores))
        ranks = np.empty(len(order), dtype=np.int64);ranks[order] = np.arange(1, len(order)+1)
        result += 1/(60+ranks)
    return result
