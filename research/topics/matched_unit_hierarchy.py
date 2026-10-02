"""Conditional source-pair purity with a separate, immutable exclusion unit."""
from collections import Counter
import math
import numpy as np
from captured_hierarchy import traversal


def evaluate(tree,owners,excluded_units,gold,parts,ordinals):
    n=len(tree.leaves);post,_,_=traversal(tree,n)
    if (not len(owners)==len(excluded_units)==len(gold)==len(parts)==len(ordinals)
            or sorted(set(ordinals))!=ordinals or set(owners)!=set(range(n))
            or any(type(i) is not int or i<0 for i in [*owners,*excluded_units,*ordinals])
            or any((g is None)!=(p is None) for g,p in zip(gold,parts))):
        raise ValueError('aligned complete source ownership and exclusion units required')
    boundaries={}
    for owner,unit in zip(owners,excluded_units):
        if owner in boundaries and boundaries[owner]!=unit:raise ValueError('discovery leaf crosses exclusion boundary')
        boundaries[owner]=unit
    counts={node:Counter() for node in tree.leaves};leaves={i:node for node,i in tree.leaves.items()}
    for o,g in zip(owners,gold):
        if g is not None:counts[leaves[o]][g]+=1
    members={};lcas=np.full((n,n),-1,dtype=np.int64)
    for node in post:
        if node in tree.leaves:members[node]=[tree.leaves[node]];continue
        a,b=tree.children[node];counts[node]=counts[a]+counts[b];left,right=members[a],members[b]
        lcas[np.ix_(left,right)]=node;lcas[np.ix_(right,left)]=node;members[node]=left+right
    known=np.flatnonzero([g is not None for g in gold]);o=np.array(owners)[known];x=np.array(excluded_units)[known]
    topics=np.array(gold,dtype=object)[known];components=np.array(parts,dtype=object)[known];positions=np.array(ordinals)[known]
    result={}
    for topic in sorted(set(topics)):
        ids=np.flatnonzero(topics==topic);a,b=np.triu_indices(len(ids),1);a,b=ids[a],ids[b]
        distinct=x[a]!=x[b];far=positions[b]-positions[a]>4;returning=components[a]!=components[b]
        for scope,eligible in [('all',distinct),('far',distinct&far),('crossspan',distinct&far&returning)]:
            nodes=lcas[o[a[eligible]],o[b[eligible]]]
            if (nodes<0).any():raise ValueError('exclusion groups must not split a discovery leaf')
            events=Counter(map(int,nodes));pairs=len(nodes)
            weighted=math.fsum(pop*counts[node][topic]/sum(counts[node].values()) for node,pop in events.items())
            result.setdefault(scope,[]).append({'topic':int(topic),'pairs':pairs,'purity_sum':weighted})
    out={}
    for scope in ['all','far','crossspan']:
        rows=result.get(scope,[]);pairs=sum(r['pairs'] for r in rows);weight=math.fsum(r['purity_sum'] for r in rows)
        out[scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,'topics':rows}
    return out
