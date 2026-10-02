"""Budgeted insert-only author-reference trial with graph/source checks."""
from dataclasses import dataclass
import time

import numpy as np

from links_state import snapshot
from topic_routing import unit_matrix


@dataclass
class Trial:
    complete: bool
    attempted: int | None
    model: object
    arrivals: list[int]
    events: list[dict]
    checkpoints: list[dict]
    latencies_ms: list[float]
    graph_events: list[dict]


def run(reference,vectors,*,reverse=False,maximum=8192):
    vectors=unit_matrix(vectors)
    if type(reverse) is not bool or type(maximum) is not int or not 1<=maximum<=8192 or len(vectors)>100000:
        raise ValueError('bounded original source stream required')
    graph_events=[]
    class ObservedGraph(reference.SubclusterGraph):
        def add_node(self,i):
            if i not in self.adj:
                super().add_node(i);graph_events.append({'action':'node-add','node':i})
        def remove_node(self,i):
            if i in self.adj:
                super().remove_node(i);graph_events.append({'action':'node-remove','node':i})
        def add_edge(self,i,j):
            if not self.has_edge(i,j):
                super().add_edge(i,j);graph_events.append({'action':'edge-add','left':i,'right':j})
        def remove_edge(self,i,j):
            if self.has_edge(i,j):
                super().remove_edge(i,j);graph_events.append({'action':'edge-remove','left':i,'right':j})
    model=reference.LinksClusterer(tc=.5,ts=.8,tp=1.,normalize_input=False,use_theoretical_norm=False)
    model._graph=ObservedGraph();events=[];arrivals=[];checkpoints=[];latencies=[];attempted=None
    for i in (range(len(vectors)-1,-1,-1) if reverse else range(len(vectors))):
        begin=time.perf_counter();n=model.n_subclusters_;nearest=None;score=None
        if n:
            scores=model._centroids[:n]@vectors[i];j=int(np.argmax(scores));nearest=model._subclusters[j].id;score=float(scores[j])
        # Creating a subcluster cannot trigger a later merge in this code path.
        if n==maximum and score<model.ts:
            events.append({'document':i,'nearest':nearest,'score':score,'online':None,'inserted':False})
            attempted=i;break
        start=len(graph_events);online=int(model.predict_next(vectors[i]));arrivals.append(i)
        latencies.append((time.perf_counter()-begin)*1000)
        events.append({'document':i,'nearest':nearest,'score':score,'online':online,'inserted':True,
                       'subclusters':model.n_subclusters_,'clusters':model.n_clusters_,
                       'graph_start':start,'graph_end':len(graph_events)})
        if model.n_subclusters_>maximum:raise ValueError('author creation budget drift')
        if len(arrivals)%256==0:
            state=snapshot(model,vectors,arrivals)
            if len(state.edges)>16384:raise ValueError('graph edge budget exceeded')
            checkpoints.append({'inserted':len(arrivals),'subclusters':len(state.node_ids),
                                'clusters':model.n_clusters_,'edges':len(state.edges)})
    snapshot(model,vectors,arrivals)
    return Trial(attempted is None,attempted,model,arrivals,events,checkpoints,latencies,graph_events)
