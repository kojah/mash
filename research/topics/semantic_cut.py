"""Semantic coherence diagnostic with explicit singleton/unassigned ownership."""
from dataclasses import dataclass
import math
import numpy as np
from captured_hierarchy import traversal


@dataclass(frozen=True)
class Cut:
    mean:float
    population_sd:float
    threshold:float
    groups:list[list[int]]
    unassigned:list[int]
    nodes:list[dict]


def select(tree,vectors):
    vectors=np.asarray(vectors,dtype=np.float64);n=len(tree.leaves)
    if (vectors.ndim!=2 or vectors.shape[0]!=n or not 1<=vectors.shape[1]<=8192
            or not np.isfinite(vectors).all() or not np.allclose(np.linalg.norm(vectors,axis=1),1,rtol=0,atol=1e-12)):
        raise ValueError('complete frozen finite unit semantic directions required')
    post,_,_=traversal(tree,n);cosines=vectors@vectors.T;values=cosines[np.triu_indices(n,1)]
    mean=math.fsum(values)/len(values) if len(values) else 0.
    sd=math.sqrt(math.fsum((float(v)-mean)**2 for v in values)/len(values)) if len(values) else 0.
    threshold=mean+sd;members={};scores={};nodes=[];totals={};norm_sums={}
    for node in post:
        members[node]=[tree.leaves[node]] if node in tree.leaves else sorted(members[tree.children[node][0]]+members[tree.children[node][1]])
        g=members[node]
        if node in tree.leaves:
            totals[node]=vectors[tree.leaves[node]];norm_sums[node]=math.fsum(float(x)**2 for x in totals[node])
        else:
            a,b=tree.children[node];totals[node]=totals[a]+totals[b];norm_sums[node]=norm_sums[a]+norm_sums[b]
        score=(math.fsum(float(x)**2 for x in totals[node])-norm_sums[node])/(len(g)*(len(g)-1)) if len(g)>1 else None;scores[node]=score
        nodes.append({'serial':node,'blocks':g,'distinct_pair_mean':score,'qualifies':score is not None and score>threshold+1e-12})
    groups=[];unassigned=[];stack=[tree.root]
    while stack:
        node=stack.pop();score=scores[node]
        if score is not None and score>threshold+1e-12:groups.append(members[node])
        elif node in tree.leaves:unassigned.append(tree.leaves[node])
        else:stack.extend(tree.children[node])
    groups.sort();unassigned.sort()
    if sorted(unassigned+sum(groups,[]))!=list(range(n)):raise ValueError('complete disjoint source ownership required')
    return Cut(mean,sd,threshold,groups,unassigned,nodes)
