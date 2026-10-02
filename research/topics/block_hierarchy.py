"""Cut-free block trees with source-weighted conditional gold, never majority labels."""
from collections import Counter
import math

import numpy as np
from scipy.cluster.hierarchy import linkage
from scipy.spatial.distance import squareform

from captured_hierarchy import Topology,traversal


def average(cosines):
    size=len(cosines)
    if (not 1<=size<=2000 or cosines.shape!=(size,size) or not np.isfinite(cosines).all()
            or not np.array_equal(cosines,cosines.T)):
        raise ValueError('bounded symmetric finite cosine matrix required')
    distance=1-np.clip(cosines,-1,1);np.fill_diagonal(distance,0)
    merges=linkage(squareform(distance,checks=True),method='average') if size>1 else np.empty((0,4))
    children={size+i:(int(row[0]),int(row[1])) for i,row in enumerate(merges)}
    tree=Topology(2*size-2,children,{i:i for i in range(size)});traversal(tree,size)
    return tree,merges.tolist()


def evaluate(tree,owners,gold,components,ordinals,topic_count):
    """Count distinct-block positive source pairs at their physical block LCA.

    Every singleton-known source contributes one to class fractions, including
    mixed leaves. Unknown/multi rows remain in tree geometry but outside gold.
    No within-block source pair contributes to any scope.
    """
    size=len(tree.leaves);post,parent,depth=traversal(tree,size)
    if (not len(owners)==len(gold)==len(components)==len(ordinals)
            or sorted(set(ordinals))!=ordinals or any(type(o) is not int or o<0 for o in ordinals)
            or any(type(b) is not int or not 0<=b<size for b in owners) or set(owners)!=set(range(size))
            or any(g is not None and (type(g) is not int or not 0<=g<topic_count) for g in gold)
            or any((g is None)!=(c is None) for g,c in zip(gold,components))):
        raise ValueError('complete aligned block/source evidence required')
    leaf_nodes={i:n for n,i in tree.leaves.items()}
    def lca(a,b):
        while depth[a]>depth[b]:a=parent[a]
        while depth[b]>depth[a]:b=parent[b]
        while a!=b:a,b=parent[a],parent[b]
        return a
    near={};near_return={};leaf_counts={b:Counter() for b in range(size)};leaf_spans={b:Counter() for b in range(size)}
    for i,g in enumerate(gold):
        if g is None:continue
        leaf_counts[owners[i]][g]+=1;leaf_spans[owners[i]][g,components[i]]+=1
        for j in range(i+1,len(gold)):
            if ordinals[j]-ordinals[i]>4:break
            if gold[j]!=g or owners[i]==owners[j]:continue
            node=lca(leaf_nodes[owners[i]],leaf_nodes[owners[j]])
            near.setdefault(node,Counter())[g]+=1
            if components[i]!=components[j]:near_return.setdefault(node,Counter())[g]+=1
    counts={};spans={};events={k:[[] for _ in range(topic_count)] for k in ['all','far','crossspan']}
    for node in post:
        if node in tree.leaves:
            b=tree.leaves[node];counts[node]=leaf_counts[b];spans[node]=leaf_spans[b];continue
        a,b=tree.children[node];counts[node]=counts[a]+counts[b];spans[node]=spans[a]+spans[b]
        known=sum(counts[node].values());same_span=Counter()
        for (g,c),n in spans[a].items():same_span[g]+=n*spans[b][g,c]
        for g,n in counts[a].items():
            pairs=n*counts[b][g]
            values={'all':pairs,'far':pairs-near.get(node,{}).get(g,0),
                    'crossspan':pairs-same_span[g]-near_return.get(node,{}).get(g,0)}
            for scope,p in values.items():
                if p<0:raise ValueError('negative LCA accounting')
                if p:events[scope][g].append((p,p*counts[node][g]/known,p*known,p if node==tree.root else 0))
    totals=counts[tree.root];known=sum(totals.values());result={}
    for scope,topics in events.items():
        support=[]
        for g,items in enumerate(topics):
            pairs=sum(r[0] for r in items);weight=math.fsum(r[1] for r in items)
            support.append({'topic':g,'known_rows':totals[g],'pairs':pairs,'purity_sum':weight,
                'lca_known_rows_sum':sum(r[2] for r in items),'root_lca_pairs':sum(r[3] for r in items),
                'purity':weight/pairs if pairs else None})
        pairs=sum(r['pairs'] for r in support);weight=math.fsum(r['purity_sum'] for r in support)
        root=math.fsum(r['pairs']*totals[r['topic']]/known for r in support) if known else 0.
        result[scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,
            'root_only_purity_sum':root,'root_only_purity':root/pairs if pairs else None,
            'lca_known_rows_sum':sum(r['lca_known_rows_sum'] for r in support),
            'root_lca_pairs':sum(r['root_lca_pairs'] for r in support),'topics':support}
    return result
