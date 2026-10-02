"""Independent direct original-source pair audit, reusable across vector views."""
import math
import numpy as np
from linkage_minimum import verify


def record(row,memberships,distance):
    n=len(distance);tree=row['tree'];children={int(k):v for k,v in tree['children'].items()};leaves={int(k):v for k,v in tree['leaves'].items()}
    assert leaves=={i:i for i in range(n)} and tree['root']==2*n-2
    positions=np.array(row['source_ordinals']);owners=np.array(row['discovery_owners']);physical=np.array(row['original_physical_owners'])
    known=np.array([i for i,p in enumerate(positions) if len(memberships[p])==1],dtype=np.int64)
    gold=np.array([memberships[positions[i]][0][0] for i in known],dtype=np.int64);parts=np.array([memberships[positions[i]][0][1] for i in known],dtype=np.int64)
    counts=np.zeros((2*n-1,max(gold,default=-1)+1),dtype=np.int64);lca=np.full((n,n),-1,dtype=np.int64);members={i:[i] for i in range(n)}
    for i,t in zip(known,gold):counts[owners[i],t]+=1
    maximum=0.
    def close(a,b):
        nonlocal maximum
        if a is None:assert b is None;return
        maximum=max(maximum,abs(a-b));assert math.isclose(a,b,rel_tol=1e-12,abs_tol=1e-12),(a,b)
    for k,merge in enumerate(row['merges']):
        node=n+k;a,b=int(merge[0]),int(merge[1]);assert children[node]==[a,b] and a<node and b<node
        left,right=members[a],members[b];assert not set(left)&set(right)
        close(float(distance[np.ix_(left,right)].mean()),merge[2]);assert len(left)+len(right)==merge[3]
        lca[np.ix_(left,right)]=node;lca[np.ix_(right,left)]=node;members[node]=left+right;counts[node]=counts[a]+counts[b]
    assert sorted(members[tree['root']])==list(range(n));totals=counts.sum(axis=1);metrics={};pairs_checked=0
    for scope in ['all','far','crossspan']:
        topics=[]
        for topic in sorted(set(gold)):
            ids=np.flatnonzero(gold==topic);a,b=np.triu_indices(len(ids),1);a,b=ids[a],ids[b];mask=physical[known[a]]!=physical[known[b]]
            if scope!='all':mask&=positions[known[b]]-positions[known[a]]>4
            if scope=='crossspan':mask&=parts[a]!=parts[b]
            nodes=lca[owners[known[a[mask]]],owners[known[b[mask]]]];assert (nodes>=0).all()
            weight=math.fsum(float(v) for v in counts[nodes,topic]/totals[nodes]);pairs=len(nodes);pairs_checked+=pairs
            topics.append({'topic':int(topic),'pairs':pairs,'purity_sum':weight})
        pairs=sum(t['pairs'] for t in topics);weight=math.fsum(t['purity_sum'] for t in topics);m={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,'topics':topics};captured=row['metrics'][scope]
        assert pairs==captured['pairs'] and len(topics)==len(captured['topics']);close(weight,captured['purity_sum']);close(m['purity'],captured['purity'])
        for a,b in zip(topics,captured['topics']):assert a['topic']==b['topic'] and a['pairs']==b['pairs'];close(a['purity_sum'],b['purity_sum'])
        metrics[scope]=m
    minima=verify(distance,row['merges'])
    return {'metrics':metrics,'positive_pair_scope_contributions':pairs_checked,'maximum_numeric_error':maximum,'global_minimum':minima}
