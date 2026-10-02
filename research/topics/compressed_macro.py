"""Fixed CF snapshot partition plus population-weighted macro linkage reference."""
from dataclasses import dataclass
import hashlib
import inspect
from pathlib import Path
import time

import numpy as np
from scipy import sparse
from sklearn.cluster import Birch

from captured_hierarchy import Topology,traversal
from discovery_controls import BIRCH_SOURCE,canonical,vector_hash


@dataclass(frozen=True)
class Compression:
    groups: list[list[int]]
    assignments: list[int]
    state: dict
    timings: dict


def compress(vectors,reverse=False):
    vectors=sparse.csr_matrix(vectors,dtype=np.float64)
    if (not 1<=vectors.shape[0]<=2000 or not 1<=vectors.shape[1]<=20000
            or not np.isfinite(vectors.data).all() or type(reverse) is not bool
            or not np.allclose(np.asarray(vectors.multiply(vectors).sum(axis=1)).ravel(),1,atol=1e-12,rtol=0)):
        raise ValueError('bounded finite unit lexical matrix required')
    if hashlib.sha256(Path(inspect.getsourcefile(Birch)).read_bytes()).hexdigest()!=BIRCH_SOURCE:
        raise ValueError('pinned BIRCH reference required')
    model=Birch(threshold=.4,branching_factor=64,n_clusters=None)
    arrival=list(range(vectors.shape[0]))
    if reverse:arrival.reverse()
    start=time.perf_counter();cpu=time.process_time();latencies=[]
    for i in arrival:
        begin=time.perf_counter();model.partial_fit(vectors[i:i+1].copy());latencies.append((time.perf_counter()-begin)*1000)
    fit_ms=(time.perf_counter()-start)*1000;fit_cpu_ms=(time.process_time()-cpu)*1000;begin=time.perf_counter()
    native=model.predict(vectors).tolist();groups,assignments=canonical([[i for i,g in enumerate(native) if g==label] for label in sorted(set(native))],len(native))
    physical=[cf for leaf in model._get_leaves() for cf in leaf.subclusters_]
    if sum(cf.n_samples_ for cf in physical)!=len(native):raise ValueError('complete physical CF samples required')
    state={'arrival':arrival,'native_final_predictions':native,'physical_leaf_count':len(model._get_leaves()),
        'physical_centers_sha256':vector_hash(model.subcluster_centers_),
        'physical_subclusters':[{'samples':int(cf.n_samples_),'linear_sum_sha256':vector_hash(cf.linear_sum_),
            'centroid_sha256':vector_hash(cf.centroid_),'radius':float(cf.radius),'squared_sum':float(cf.squared_sum_)} for cf in physical]}
    return Compression(groups,assignments,state,{'fit_ms':fit_ms,'fit_cpu_ms':fit_cpu_ms,
        'snapshot_ms':(time.perf_counter()-begin)*1000,'insert_ms':latencies})


def macro(vectors,groups):
    """Exact cross-block mean distances, with constrained initial CF groups.

    Balanced source-order lifting inside groups permits complete diagnostic
    scores only. It supplies no semantic hierarchy or insertion ownership.
    """
    vectors=sparse.csr_matrix(vectors,dtype=np.float64);size=vectors.shape[0]
    canonical_groups,_=canonical(groups,size)
    if groups!=canonical_groups or not np.isfinite(vectors.data).all():raise ValueError('canonical complete groups required')
    means=sparse.vstack([sparse.csr_matrix(vectors[g].sum(axis=0)/len(g)) for g in groups],format='csr')
    count=len(groups);distance=np.full((2*count-1,2*count-1),np.inf)
    distance[:count,:count]=1-np.clip((means@means.T).toarray(),-1,1);np.fill_diagonal(distance,np.inf)
    children={};serial=size
    def join(a,b):
        nonlocal serial
        node=serial;serial+=1;children[node]=(a,b);return node
    def balanced(group):
        if len(group)==1:return group[0]
        mid=len(group)//2;return join(balanced(group[:mid]),balanced(group[mid:]))
    roots={i:balanced(g) for i,g in enumerate(groups)};populations={i:len(g) for i,g in enumerate(groups)}
    first={i:g[0] for i,g in enumerate(groups)};active=list(range(count));merges=[]
    while len(active)>1:
        a,b=min(((a,b) for i,a in enumerate(active) for b in active[i+1:]),
                key=lambda pair:(round(float(distance[pair]),12),min(first[pair[0]],first[pair[1]]),max(first[pair[0]],first[pair[1]])))
        if first[a]>first[b]:a,b=b,a
        node=count+len(merges);population=populations[a]+populations[b]
        merges.append({'node':node,'children':[a,b],'distance':float(distance[a,b]),'blocks':population})
        active.remove(a);active.remove(b)
        for c in active:
            value=(populations[a]*distance[a,c]+populations[b]*distance[b,c])/population
            distance[node,c]=value;distance[c,node]=value
        populations[node]=population;first[node]=min(first[a],first[b]);roots[node]=join(roots[a],roots[b]);active.append(node)
    tree=Topology(roots[active[0]],children,{i:i for i in range(size)});traversal(tree,size)
    return tree,merges
