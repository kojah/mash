"""Actual incremental BIRCH and GRINCH-style discovery at frozen operating points."""
from dataclasses import asdict,dataclass
from enum import StrEnum
import hashlib
import inspect
from pathlib import Path
import time

import numpy as np
from sklearn.cluster import Birch
from threadpoolctl import threadpool_limits

from online_tree import Tree,Mode


BIRCH_SOURCE='92070c94072b679e22dd298c07e5d0c1387b7fa7237bfe38d27a35a130a28de3'


class Method(StrEnum):
    BIRCH='birch'
    GRINCH='grinch_style'


@dataclass(frozen=True)
class Discovery:
    assignments: list[int]
    clusters: list[list[int]]
    state: dict
    timings: dict


def vector_hash(v):return hashlib.sha256(np.asarray(v,dtype='<f8').tobytes()).hexdigest()


def canonical(groups,size):
    groups=sorted([sorted(group) for group in groups]);flat=[i for group in groups for i in group]
    if (type(size) is not int or size<1 or not groups or any(not group for group in groups)
            or any(type(i) is not int for i in flat) or sorted(flat)!=list(range(size))):raise ValueError('complete unique source partition required')
    assigned=[-1]*size
    for label,group in enumerate(groups):
        for i in group:assigned[i]=label
    return groups,assigned


def discover(vectors,method,*,reverse=False):
    vectors=np.asarray(vectors,dtype=np.float64);method=Method(method)
    if (vectors.ndim!=2 or not 0<vectors.shape[0]<=2000 or not 0<vectors.shape[1]<=1024
            or type(reverse) is not bool or not np.isfinite(vectors).all()
            or not np.allclose(np.linalg.norm(vectors,axis=1),1,rtol=0,atol=1e-12)):
        raise ValueError('bounded finite unit source matrix required')
    source=Path(inspect.getsourcefile(Birch))
    if hashlib.sha256(source.read_bytes()).hexdigest()!=BIRCH_SOURCE:raise ValueError('pinned BIRCH implementation required')
    arrival=list(range(len(vectors)))
    if reverse:arrival.reverse()
    model=Birch(threshold=.4,branching_factor=64,n_clusters=None) if method==Method.BIRCH else None
    tree=Tree(Mode.GRINCH) if model is None else None
    latencies=[]
    with threadpool_limits(limits=1):
        start=time.perf_counter();cpu=time.process_time()
        for i in arrival:
            begin=time.perf_counter()
            if model is not None:model.partial_fit(vectors[i:i+1].copy())
            else:tree.insert(str(i),vectors[i])
            latencies.append((time.perf_counter()-begin)*1000)
        fit_ms=(time.perf_counter()-start)*1000;fit_cpu_ms=(time.process_time()-cpu)*1000;begin=time.perf_counter()
        if model is not None:
            native=model.predict(vectors).tolist()
            groups=[[i for i,label in enumerate(native) if label==value] for value in sorted(set(native))]
            subclusters=[cf for leaf in model._get_leaves() for cf in leaf.subclusters_]
            if sum(cf.n_samples_ for cf in subclusters)!=len(vectors):raise ValueError('incomplete CF source accounting')
            state={'native_final_predictions':native,'subcluster_centers_sha256':vector_hash(model.subcluster_centers_),
                   'subcluster_centers_shape':list(model.subcluster_centers_.shape),'leaf_count':len(model._get_leaves()),
                   'subclusters':[{'samples':int(cf.n_samples_),'linear_sum_sha256':vector_hash(cf.linear_sum_),
                                   'centroid_sha256':vector_hash(cf.centroid_),'squared_sum':float(cf.squared_sum_),
                                   'squared_centroid_norm':float(cf.sq_norm_),'radius':float(cf.radius)} for cf in subclusters]}
        else:
            fit_statistics=asdict(tree.statistics);tree.validate();cut=tree.cut(.7)
            groups=[[arrival[i] for i in group] for group in cut]
            state={'root':tree.root.serial,'arrival':arrival,'fit_statistics':fit_statistics,
                   'final_statistics':asdict(tree.statistics),'nodes':[{'serial':n.serial,'parent':n.parent.serial if n.parent else None,
                          'children':[c.serial for c in n.children] if n.children else None,'leaf':n.leaf,'count':n.count,
                          'total_sha256':vector_hash(n.total),'link_sha256':vector_hash(n.link_vector),
                          'merger_cosine':float(np.clip(n.children[0].link_vector@n.children[1].link_vector,-1,1)) if n.children else None}
                         for n in sorted(tree.nodes.values(),key=lambda n:n.serial)]}
        groups,assigned=canonical(groups,len(vectors));evaluation_ms=(time.perf_counter()-begin)*1000
    return Discovery(assigned,groups,state,{'fit_ms':fit_ms,'fit_cpu_ms':fit_cpu_ms,'evaluation_ms':evaluation_ms,'insert_latencies_ms':latencies})
