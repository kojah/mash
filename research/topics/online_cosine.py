"""Insert-only threshold-growth cosine control; no semantic tree or reassignment."""
from dataclasses import dataclass
import time

import numpy as np

from topic_routing import unit_matrix


@dataclass(frozen=True)
class Event:
    document: int
    nearest: int | None
    best_score: float | None
    assigned: int | None
    created: bool


@dataclass
class Discovery:
    complete: bool
    sums: np.ndarray
    counts: np.ndarray
    assignments: np.ndarray
    events: list[Event]
    comparisons: int
    latencies_ms: list[float]


def discover(vectors, *, reverse=False, threshold=.55, maximum=8192):
    vectors=unit_matrix(vectors)
    if (len(vectors)>100000 or type(reverse) is not bool
            or type(maximum) is not int or not 1<=maximum<=8192
            or not np.isfinite(threshold) or not 0<threshold<=1):
        raise ValueError('bounded stream, positive cosine cutoff and cluster cap required')
    sums=np.zeros((maximum,vectors.shape[1]),dtype=np.float64)
    centers=np.zeros_like(sums);counts=np.zeros(maximum,dtype=np.int64)
    assigned=np.full(len(vectors),-1,dtype=np.int64)
    events=[];latencies=[];comparisons=0;clusters=0;complete=True
    for index in (range(len(vectors)-1,-1,-1) if reverse else range(len(vectors))):
        begin=time.perf_counter();vector=vectors[index]
        nearest=None;best=None
        if clusters:
            scores=centers[:clusters]@vector;comparisons+=clusters
            nearest=int(np.argmax(scores));best=float(scores[nearest])
        created=best is None or best<threshold
        if created and clusters==maximum:
            events.append(Event(index,nearest,best,None,False))
            latencies.append((time.perf_counter()-begin)*1000);complete=False;break
        target=clusters if created else nearest
        updated=sums[target]+vector;length=float(np.linalg.norm(updated))
        if not np.isfinite(length) or length<=1e-12:
            raise ValueError('canceling centroid; no invented direction')
        sums[target]=updated;centers[target]=updated/length;counts[target]+=1
        if created:clusters+=1
        assigned[index]=target;events.append(Event(index,nearest,best,target,created))
        latencies.append((time.perf_counter()-begin)*1000)
    return Discovery(complete,sums[:clusters].copy(),counts[:clusters].copy(),
                     assigned,events,comparisons,latencies)


def diagnostics(vectors, result):
    """All inserted sources, including singletons; no semantic-quality estimator."""
    vectors=unit_matrix(vectors);valid=result.assignments>=0
    lengths=np.linalg.norm(result.sums,axis=1)
    centers=result.sums/lengths[:,None]
    scores=np.einsum('ij,ij->i',vectors[valid],centers[result.assignments[valid]])
    pairs=result.counts*(result.counts-1)
    pair_mean=np.full(len(result.counts),np.nan)
    np.divide(lengths*lengths-result.counts,pairs,out=pair_mean,where=pairs>0)
    defined=pair_mean[np.isfinite(pair_mean)]
    return {'inserted':int(valid.sum()),'unprocessed':int((~valid).sum()),
            'clusters':len(result.counts),'singletons':int((result.counts==1).sum()),
            'singleton_document_fraction':float((result.counts==1).sum()/valid.sum()) if valid.any() else None,
            'size_median':float(np.median(result.counts)),
            'size_p95':float(np.percentile(result.counts,95)),'size_max':int(result.counts.max()),
            'member_final_cosine_median':float(np.median(scores)),
            'member_final_cosine_p05':float(np.percentile(scores,5)),
            'members_below_insertion_cutoff':int((scores<.55).sum()),
            'nonsingleton_topics':len(defined),
            'macro_pair_cosine':float(np.mean(defined)) if len(defined) else None}
