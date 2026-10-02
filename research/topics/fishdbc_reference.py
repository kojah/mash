"""Pinned author FISHDBC with offline distances; not an ingestion benchmark."""
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import random
import sys
import time

import numpy as np

from captured_hierarchy import Topology,traversal


MANIFEST='bf4d0261f789837206eb752712658e24c4e0033a6da99a4c0079728e160105b3'


def author(root):
    root=Path(root).resolve();path=root/'reference-manifest.json'
    if hashlib.sha256(path.read_bytes()).hexdigest()!=MANIFEST:raise ValueError('frozen author manifest required')
    manifest=json.loads(path.read_text())
    for name,pin in manifest['files'].items():
        if hashlib.sha256((root/name).read_bytes()).hexdigest()!=pin:raise ValueError('author file drift')
    for name,version in manifest['dependencies'].items():
        if importlib.metadata.version(name)!=version:raise ValueError('author dependency drift')
    sys.path.insert(0,str(root))
    import flexible_clustering
    if Path(flexible_clustering.__file__).resolve()!=root/'flexible_clustering/__init__.py':raise ValueError('wrong author module')
    return flexible_clustering.FISHDBC


def matrix(distance):
    d=np.asarray(distance,dtype=np.float64)
    if (d.ndim!=2 or not 6<=len(d)<=2000 or d.shape!=(len(d),len(d))
            or not np.isfinite(d).all() or np.any(d<0) or not np.array_equal(d,d.T)
            or np.any(np.diag(d)!=0)):raise ValueError('bounded symmetric finite nonnegative zero-diagonal distances required')
    return d


def topology(merges,order):
    n=len(order);a=np.asarray(merges,dtype=np.float64)
    if sorted(order)!=list(range(n)) or a.shape!=(n-1,4) or not np.isfinite(a).all():raise ValueError('complete finite source tree required')
    children={}
    for i,row in enumerate(a):
        left,right=int(row[0]),int(row[1])
        if row[0]!=left or row[1]!=right or not 0<=left<n+i or not 0<=right<n+i or left==right:raise ValueError('invalid merge identities')
        children[n+i]=(left,right)
    tree=Topology(2*n-2,children,{i:v for i,v in enumerate(order)});traversal(tree,n)
    return tree


def exact(distance):
    """Independent complete-graph Kruskal; fifth OTHER neighbor matches author."""
    d=matrix(distance);n=len(d);x=d.copy();np.fill_diagonal(x,np.inf)
    cores=np.partition(x,4,axis=1)[:,4];parent=list(range(n))
    def find(i):
        while parent[i]!=i:parent[i]=parent[parent[i]];i=parent[i]
        return i
    edges=sorted((max(float(d[i,j]),float(cores[i]),float(cores[j])),i,j,float(d[i,j])) for i in range(n) for j in range(i+1,n))
    mst=[]
    for edge in edges:
        _,i,j,_=edge;a,b=find(i),find(j)
        if a==b:continue
        parent[a]=b;mst.append(edge)
        if len(mst)==n-1:break
    return cores.tolist(),[list(e) for e in mst]


def run(distance,root,reverse=False):
    d=matrix(distance);n=len(d);order=list(range(n));order=order[::-1] if reverse else order
    factory=author(root);state=random.getstate();random.seed(20261002)
    timings={'adds_ms':[],'extractions_ms':[]};prefixes=[]
    c=factory(lambda i,j:float(d[i,j]),min_samples=5,m=5,ef=50,heuristic=True,balanced_add=True,vectorized=False)
    started=time.perf_counter();cpu=time.process_time()
    try:
        for position,source in enumerate(order,1):
            begin=time.perf_counter();c.add(source);timings['adds_ms'].append((time.perf_counter()-begin)*1000)
            if position>=6 and (position%16==0 or position==n):
                begin=time.perf_counter();output=c.cluster();timings['extractions_ms'].append((time.perf_counter()-begin)*1000)
                labels,probabilities,stabilities,condensed,slt,mst=output
                prefixes.append({'size':position,'source_order':order[:position],'labels':labels.tolist(),
                    'probabilities':probabilities.tolist(),'stabilities':stabilities.tolist(),
                    'condensed':condensed.tolist(),'merges':slt.tolist(),'mst':mst.tolist()})
        timings.update(author_ms=(time.perf_counter()-started)*1000,author_cpu_ms=(time.process_time()-cpu)*1000)
    finally:random.setstate(state)
    cores=[float(-h[0][0]) for h in c._neighbor_heaps]
    if not np.isfinite(cores).all():raise ValueError('insufficient author neighbors')
    graph=c._hnsw_add.__self__._graphs
    graphs=[[[int(i),[[int(j),float(v)] for j,v in sorted(ns.items())]] for i,ns in sorted(g.items())] for g in graph]
    begin=time.perf_counter();exact_cores,exact_edges=exact(d[np.ix_(order,order)])
    from hdbscan import hdbscan_
    exact_mst=np.asarray([[i,j,w] for w,i,j,_ in exact_edges],dtype=np.float64)
    exact_merges=hdbscan_.label(exact_mst).tolist();timings['exact_ms']=(time.perf_counter()-begin)*1000
    native_edges=[list(e) for e in c._mst_edges]
    gap=math.fsum(e[0] for e in native_edges)-math.fsum(e[0] for e in exact_edges)
    return {'order':order,'prefixes':prefixes,'cores':cores,'native_edges':native_edges,'graphs':graphs,
        'cache_hits':c.cache_hits,'distance_evaluations':c.cache_misses,'exact_cores':exact_cores,
        'exact_edges':exact_edges,'exact_merges':exact_merges,'mst_weight_excess':gap,
        'maximum_core_excess':max(a-b for a,b in zip(cores,exact_cores))},timings
