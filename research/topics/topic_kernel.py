"""Bounded fixed-frame Gram routing control; offline geometry diagnostic only."""
from dataclasses import dataclass
import math
import numpy as np


@dataclass(frozen=True)
class Routing:
    anchors: tuple[int | None, ...]
    memberships: tuple[tuple[tuple[int,float], ...], ...]
    topic_count: int
    unroutable: int


def route(kernel,order,threshold=.55,relative=.8,maximum_tags=3):
    kernel=np.asarray(kernel,dtype=np.float64);n=len(kernel)
    if (kernel.shape!=(n,n) or not 0<n<=2048 or not np.isfinite(kernel).all()
            or not np.allclose(kernel,kernel.T,rtol=0,atol=1e-12)
            or np.max(np.abs(kernel))>1+1e-12
            or not np.allclose(np.diag(kernel),np.where(np.diag(kernel)>.5,1.,0.),rtol=0,atol=1e-12)
            or sorted(order)!=list(range(n)) or isinstance(threshold,bool) or not math.isfinite(threshold) or not 0<threshold<=1
            or isinstance(relative,bool) or not 0<relative<=1 or type(maximum_tags) is not int or not 1<=maximum_tags<=3):
        raise ValueError('bounded unit-or-zero fixed kernel and complete arrival order required')
    # Each column is the sum of similarities to sources already in its topic.
    # This avoids inventing a low-dimensional projection for sparse features.
    totals=np.zeros((n,n));norms=np.zeros(n);sizes=np.zeros(n,dtype=int)
    anchors=[None]*n;used=0
    for i in order:
        if kernel[i,i]<.5:continue
        valid=np.flatnonzero(norms[:used]>1e-24)
        if len(valid):
            scores=totals[i,valid]/np.sqrt(norms[valid]);j=int(valid[int(np.argmax(scores))])
            if scores.max()<threshold:j=used
        else:j=used
        if j==used:used+=1
        # Sum-vector norm update is exact in the fixed feature frame.
        value=norms[j]+2*totals[i,j]+kernel[i,i]
        if value < -1e-10:raise ValueError('invalid aggregate norm')
        norms[j]=max(0.,value);totals[:,j]+=kernel[:,i];sizes[j]+=1;anchors[i]=j
    memberships=[]
    for i,own in enumerate(anchors):
        if kernel[i,i]<.5:memberships.append(());continue
        valid=norms[:used]>1e-24
        scores=np.divide(totals[i,:used],np.sqrt(norms[:used]),out=np.zeros(used),where=valid)
        if own is not None and sizes[own]>1:
            value=norms[own]-2*totals[i,own]+kernel[i,i]
            if value < -1e-10:raise ValueError('invalid leave-one-out norm')
            scores[own]=(totals[i,own]-kernel[i,i])/math.sqrt(value) if value>1e-24 else 0.
        best=float(scores.max()) if used else 0.
        choices=sorted((j for j in range(used) if valid[j] and scores[j]>=threshold and scores[j]>=best*relative),
                       key=lambda j:(-scores[j],j))[:maximum_tags]
        memberships.append(tuple((j,float(scores[j])) for j in choices))
    return Routing(tuple(anchors),tuple(memberships),used,sum(anchor is None for anchor in anchors))


def combine(dense,lexical,weight=.5):
    if not math.isfinite(weight) or not 0<=weight<=1:raise ValueError('explicit finite lexical weight required')
    dense=np.asarray(dense,dtype=float);lexical=np.asarray(lexical,dtype=float)
    if dense.shape!=lexical.shape or dense.ndim!=2:raise ValueError('compatible fixed frames required')
    kernel=(1-weight)*dense+weight*lexical
    norms=np.sqrt(np.diag(kernel));scale=np.outer(norms,norms)
    return np.divide(kernel,scale,out=np.zeros_like(kernel),where=scale>0)
