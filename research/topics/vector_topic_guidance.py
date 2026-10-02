"""Vectorized exact parent-rank guidance; preserves complete rank pools."""
import numpy as np


def ordered(scores,ids):
    scores=np.asarray(scores);ids=np.asarray(ids)
    if (scores.shape!=ids.shape or scores.ndim!=1 or ids.dtype.kind not in 'iu' or (ids<0).any()
            or len(np.unique(ids))!=len(ids) or not np.isfinite(scores).all()):
        raise ValueError('unique nonnegative identities and finite aligned scores required')
    positive=np.flatnonzero(scores>0)
    return scores,ids,positive[np.lexsort((ids[positive],-scores[positive]))]


def retrieve(source_scores,source_ids,owners,group_scores,group_ids):
    sources,ids,source_order=ordered(source_scores,source_ids)
    groups,gids,group_order=ordered(group_scores,group_ids);owners=np.asarray(owners)
    if owners.shape!=ids.shape or owners.dtype.kind not in 'iu' or (owners<0).any():raise ValueError('integer aligned parent ownership required')
    sorted_groups=np.argsort(gids);positions=np.searchsorted(gids[sorted_groups],owners)
    if (positions>=len(gids)).any() or not np.array_equal(gids[sorted_groups][positions],owners):raise ValueError('all source parents must be within scope')
    parent_positions=sorted_groups[positions];group_ranks=np.zeros(len(gids),dtype=np.int64)
    group_ranks[group_order]=np.arange(1,len(group_order)+1,dtype=np.int64)
    parent_ranks=group_ranks[parent_positions[source_order]]
    extra=np.divide(1.,60+parent_ranks.astype(float),out=np.zeros(len(source_order)),where=parent_ranks>0)
    fused=1/(60+np.arange(1,len(source_order)+1,dtype=float))+extra
    winners=np.lexsort((ids[source_order],-fused))[:20];rows=[]
    for slot in winners:
        source=source_order[slot];parent=parent_positions[source];pr=int(group_ranks[parent])
        rows.append({'id':int(ids[source]),'score':float(fused[slot]),'source_score':float(sources[source]),'source_rank':int(slot)+1,
            'parent_group':int(owners[source]),'parent_rank':pr if pr else None,'parent_score':float(groups[parent]) if pr else 0.})
    return {'returned_sources':rows,'candidate_sources':len(source_order),'positive_groups':len(group_order)}
