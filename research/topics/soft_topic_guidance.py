"""Adapted parent-rank RRF bonus; direct candidate eligibility is unchanged."""
import numpy as np
from query_navigation import rank


K=60


def retrieve(source_scores,source_ids,owners,group_scores,group_ids):
    source_ids=np.asarray(source_ids);owners=np.asarray(owners)
    if source_ids.shape!=owners.shape:raise ValueError('aligned parent ownership required')
    direct=rank(source_scores,source_ids,max(1,len(source_ids)))
    groups=rank(group_scores,group_ids,max(1,len(group_ids)))
    parents={g['id']:(i+1,g['score']) for i,g in enumerate(groups)}
    owned=dict(zip(source_ids.tolist(),owners.tolist()));out=[]
    if not set(owned.values())<=set(np.asarray(group_ids).tolist()):raise ValueError('all source parents must be within scope')
    for i,row in enumerate(direct):
        group=int(owned[row['id']]);parent=parents.get(group)
        score=1/(K+i+1)+(1/(K+parent[0]) if parent else 0.)
        out.append({'id':row['id'],'score':score,'source_score':row['score'],'source_rank':i+1,
            'parent_group':group,'parent_rank':parent[0] if parent else None,'parent_score':parent[1] if parent else 0.})
    selected=sorted(out,key=lambda r:(-r['score'],r['id']))[:20]
    return {'returned_sources':selected,'candidate_sources':len(direct),'positive_groups':len(groups)}
