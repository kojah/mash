"""One vote/result per physical source block, with conditional known-member gains."""
from dataclasses import dataclass
import math

import numpy as np

from return_ranking import evaluate,rank


@dataclass(frozen=True)
class Groups:
    blocks: np.ndarray
    ordinals: np.ndarray
    members: tuple[np.ndarray,...]
    gains: np.ndarray


def group(block_ids,ordinals,relevant):
    block_ids=np.asarray(block_ids);ordinals=np.asarray(ordinals);relevant=np.asarray(relevant)
    if (block_ids.ndim!=1 or not len(block_ids) or len(block_ids)>2048 or ordinals.shape!=block_ids.shape
            or relevant.shape!=block_ids.shape or relevant.dtype!=np.bool_ or not relevant.any()
            or not np.issubdtype(block_ids.dtype,np.integer) or not np.issubdtype(ordinals.dtype,np.integer)
            or (block_ids<0).any() or (ordinals<0).any() or len(set(ordinals.tolist()))!=len(ordinals)):
        raise ValueError('bounded unique known-source candidates and positive relevance required')
    ids=np.unique(block_ids);members=tuple(np.flatnonzero(block_ids==i) for i in ids)
    earliest=np.array([int(ordinals[m].min()) for m in members],dtype=np.int64)
    gains=np.array([int(relevant[m].sum())/len(m) for m in members],dtype=np.float64)
    return Groups(ids,earliest,members,gains)


def maximum(scores,groups):
    scores=np.asarray(scores,dtype=np.float64)
    if (scores.ndim!=1 or not np.isfinite(scores).all()
            or sorted(int(i) for m in groups.members for i in m)!=list(range(len(scores)))):
        raise ValueError('finite score per uniquely owned candidate required')
    return np.array([float(scores[m].max()) for m in groups.members],dtype=np.float64)


def metrics(scores,groups):
    result=evaluate(scores,groups.ordinals,groups.gains>0);ordered=rank(scores,groups.ordinals)
    discounts=np.log2(np.arange(2,12,dtype=np.float64));actual=groups.gains[ordered[:10]]
    ideal=np.sort(groups.gains)[::-1][:10]
    dcg=math.fsum(float(g/d) for g,d in zip(actual,discounts));idcg=math.fsum(float(g/d) for g,d in zip(ideal,discounts))
    result['ndcg10']=dcg/idcg
    result['top10_blocks']=groups.blocks[ordered[:10]].tolist()
    return result
