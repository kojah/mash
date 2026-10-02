"""Source ownership and graph invariants for an explicit Links research snapshot."""
from collections import defaultdict
from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class Snapshot:
    node_ids: tuple[int,...]
    cluster_ids: tuple[int,...]
    sums: np.ndarray
    counts: np.ndarray
    members: tuple[tuple[int,...],...]
    edges: tuple[tuple[int,int],...]
    labels: np.ndarray


def snapshot(model,vectors,arrivals):
    if (len(arrivals)!=model.n_samples_seen_ or len(set(arrivals))!=len(arrivals)
            or any(type(i) is not int or not 0<=i<len(vectors) for i in arrivals)):
        raise ValueError('complete canonical arrival ownership required')
    nodes=sorted(model.subclusters_,key=lambda s:s.id)
    by_id={s.id:s for s in nodes};expected=set(by_id)
    if (expected!=set(model._subcluster_map) or expected!=set(model._graph.adj)
            or len(nodes)!=model.n_subclusters_):raise ValueError('graph/map nodes disagree')
    components=defaultdict(set);owned=[];members=[];edges=[]
    for s in nodes:
        if model._subclusters[s.index] is not s or model._subcluster_map[s.id] is not s:
            raise ValueError('swap/pop storage identity mismatch')
        indices=s.vector_indices
        if (s.count!=len(indices) or not indices or any(type(i) is not int or not 0<=i<len(arrivals) for i in indices)):
            raise ValueError('invalid subcluster source ownership')
        owned.extend(indices);source=tuple(arrivals[i] for i in indices);members.append(source)
        summed=np.sum(vectors[list(source)],axis=0)
        if not np.allclose(summed,s.sum_vector,atol=1e-10,rtol=0):raise ValueError('source sum drift')
        length=float(np.linalg.norm(s.sum_vector))
        if (not np.isfinite(length) or length<=1e-12
                or not np.allclose(s.centroid,s.sum_vector/length,atol=1e-12,rtol=0)
                or not np.array_equal(model._centroids[s.index],s.centroid)):
            raise ValueError('empirical centroid/storage mismatch')
        components[s.cluster_id].add(s.id)
        for neighbor in model._graph.adj[s.id]:
            if neighbor not in expected or neighbor==s.id or s.id not in model._graph.adj[neighbor]:
                raise ValueError('invalid graph edge')
            other=by_id[neighbor]
            if other.cluster_id!=s.cluster_id:raise ValueError('edge crosses published component identity')
            if float(s.centroid@other.centroid)+1e-12<model.s_tilde(s.count,other.count):
                raise ValueError('invalid retained edge threshold')
            if s.id<neighbor:edges.append((s.id,neighbor))
    if sorted(owned)!=list(range(len(arrivals))):raise ValueError('lost or duplicate source ownership')
    actual={c:set(ids) for c,ids in model._cluster_subclusters.items()}
    if dict(components)!=actual or len(components)!=model.n_clusters_:raise ValueError('component registry mismatch')
    for ids in components.values():
        todo=[min(ids)];visited=set()
        while todo:
            i=todo.pop()
            if i in visited:continue
            visited.add(i);todo.extend(model._graph.adj[i]-visited)
        if visited!=ids:raise ValueError('component registry spans disconnected graph')
    labels=np.full(len(vectors),-1,dtype=np.int64)
    for s,sources in zip(nodes,members):labels[list(sources)]=s.cluster_id
    if not np.array_equal(labels[arrivals],model.get_final_labels()):raise ValueError('final labels disagree with ownership')
    dim=vectors.shape[1]
    return Snapshot(tuple(s.id for s in nodes),tuple(s.cluster_id for s in nodes),
                    np.stack([s.sum_vector for s in nodes]) if nodes else np.empty((0,dim)),
                    np.array([s.count for s in nodes],dtype=np.int64),tuple(members),tuple(sorted(edges)),labels)
