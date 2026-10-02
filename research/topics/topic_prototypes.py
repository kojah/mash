"""Original-space, equal-document-mass prototypes over fixed topic memberships."""
from dataclasses import dataclass

import numpy as np

from topic_routing import Match, unit_matrix


@dataclass(frozen=True)
class Prototypes:
    sums: np.ndarray
    masses: np.ndarray
    directions: np.ndarray
    active: np.ndarray


def fit(vectors, members, topics):
    vectors=unit_matrix(vectors)
    if len(vectors)!=len(members) or type(topics) is not int or not 1<=topics<=8192:
        raise ValueError('complete source-bound memberships required')
    sums=np.zeros((topics,vectors.shape[1]),dtype=np.float64)
    masses=np.zeros(topics,dtype=np.float64)
    for vector,row in zip(vectors,members):
        if len(row)>3 or len({m.topic for m in row})!=len(row):
            raise ValueError('unique bounded memberships required')
        for m in row:
            if not isinstance(m,Match) or type(m.topic) is not int or not 0<=m.topic<topics:
                raise ValueError('source topic identity required')
            weight=1/len(row)
            sums[m.topic]+=vector*weight
            masses[m.topic]+=weight
    lengths=np.linalg.norm(sums,axis=1)
    active=(masses>0)&(lengths>1e-12)
    directions=np.zeros_like(sums)
    np.divide(sums,lengths[:,None],out=directions,where=active[:,None])
    return Prototypes(sums,masses,directions,active)


def complete_union(selected, members):
    """Complete metadata union before a separate vector-work gate; no truncation."""
    topics={m.topic for m in selected}
    if len(topics)!=len(selected) or len(selected)>3 or len(members)>100000:
        raise ValueError('bounded unique admission required')
    union=[];visits=0
    for i,row in enumerate(members):
        count=sum(m.topic in topics for m in row)
        visits+=count
        if count: union.append(i)
    return union,visits
