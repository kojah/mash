"""Conservative necessary centroid count under a frozen angular fidelity target."""
from enum import StrEnum
import hashlib

import numpy as np


class Frame(StrEnum):
    LEXICAL='lexical'
    DENSE='dense'
    HYBRID='equal_energy_hybrid'


class Verdict(StrEnum):
    RULED_OUT='ruled_out_for_per_meeting_view_and_frozen_budget'
    OPEN='not_ruled_out_not_proven_feasible'


def graph(cosines):
    cosines=np.asarray(cosines,dtype=np.float64);size=len(cosines)
    if (not 1<=size<=2000 or cosines.shape!=(size,size) or not np.isfinite(cosines).all()
            or not np.array_equal(cosines,cosines.T) or not np.allclose(np.diag(cosines),1,atol=1e-12,rtol=0)
            or np.max(np.abs(cosines))>1+1e-12 or np.linalg.eigvalsh(cosines)[0]<-1e-10):
        raise ValueError('bounded finite unit positive-semidefinite Gram required')
    eligible=cosines>=2*.9*.9-1-1e-12;np.fill_diagonal(eligible,False)
    scores=cosines.copy();np.fill_diagonal(scores,-np.inf)
    return {'blocks':size,'gram_sha256':hashlib.sha256(np.asarray(cosines,dtype='<f8').tobytes()).hexdigest(),
        'neighbors':[np.flatnonzero(row).tolist() for row in eligible],
        'isolated':np.flatnonzero(~eligible.any(axis=1)).tolist(),
        'closest_cosine':scores.max(axis=1).tolist() if size>1 else [None]}


def bound(records):
    if not records or any(r['blocks']<1 or len(set(r['isolated']))!=len(r['isolated'])
            or any(type(i) is not int or not 0<=i<r['blocks'] for i in r['isolated']) for r in records):
        raise ValueError('complete nonempty unique isolated support required')
    size=sum(r['blocks'] for r in records);isolated=sum(len(r['isolated']) for r in records)
    bad=size//20;lower=max(len(records),isolated-bad);budget=size//2
    return {'meetings':len(records),'blocks':size,'isolated_blocks':isolated,'maximum_outliers':bad,
        'minimum_centroids_lower_bound':lower,'maximum_groups_budget':budget,
        'verdict':Verdict.RULED_OUT if lower>budget else Verdict.OPEN}
