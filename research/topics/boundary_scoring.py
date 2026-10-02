"""Label-free online boundary scores and explicitly training-only calibration."""
from enum import StrEnum
from fractions import Fraction

import numpy as np


class Method(StrEnum):
    ADJACENT = 'adjacent'
    PREVIOUS_TWO = 'previous_two'


def score_turns(vectors):
    vectors=np.asarray(vectors,dtype=np.float64)
    if (vectors.ndim!=2 or not 2<=len(vectors)<=100000 or not 1<=vectors.shape[1]<=1024
            or not np.isfinite(vectors).all()
            or not np.allclose(np.linalg.norm(vectors,axis=1),1,atol=1e-12,rtol=0)):
        raise ValueError('complete finite unit turn directions required')
    result=np.full((len(vectors)-1,2),np.nan);abstentions=[]
    for i in range(1,len(vectors)):
        result[i-1,0]=vectors[i-1]@vectors[i]
        # Only already-arrived turns; never consult the next response or labels.
        mean=vectors[max(0,i-2):i].mean(axis=0);length=float(np.linalg.norm(mean))
        if length<=1e-12:abstentions.append({'ordinal':i,'method':Method.PREVIOUS_TWO,'reason':'canceling_context'})
        else:result[i-1,1]=(mean/length)@vectors[i]
    return result,abstentions


def confusion(labels,predictions):
    labels=np.asarray(labels);predictions=np.asarray(predictions)
    if labels.ndim!=1 or labels.dtype!=np.bool_ or predictions.shape!=labels.shape or predictions.dtype!=np.bool_:
        raise ValueError('aligned boolean boundary labels and predictions required')
    tp=int(np.count_nonzero(labels&predictions));fp=int(np.count_nonzero(~labels&predictions))
    fn=int(np.count_nonzero(labels&~predictions));tn=int(np.count_nonzero(~labels&~predictions))
    return counts(tp,fp,fn,tn)


def counts(tp,fp,fn,tn):
    if any(type(n) is not int or n<0 for n in [tp,fp,fn,tn]):raise ValueError('nonnegative integer confusion counts required')
    return {'tp':tp,'fp':fp,'fn':fn,'tn':tn,'support':tp+fp+fn+tn,
            'precision':tp/(tp+fp) if tp+fp else None,'recall':tp/(tp+fn) if tp+fn else None,
            'f1':2*tp/(2*tp+fp+fn) if 2*tp+fp+fn else 0.0}


def objective(record):
    denominator=2*record['tp']+record['fp']+record['fn']
    return Fraction(2*record['tp'],denominator) if denominator else Fraction(0)


def calibrate(scores,labels):
    scores=np.asarray(scores,dtype=np.float64);labels=np.asarray(labels)
    if (scores.ndim!=1 or not len(scores) or len(scores)>100000 or labels.shape!=scores.shape
            or labels.dtype!=np.bool_ or not np.isfinite(scores).all() or (np.abs(scores)>1+1e-12).any()):
        raise ValueError('complete supported training cosine scores and boolean labels required')
    order=np.argsort(scores,kind='stable');ordered=scores[order];truth=labels[order]
    positives=int(np.count_nonzero(labels));negatives=len(labels)-positives;tp=fp=0;candidates=[]
    start=0
    while start<len(order):
        stop=start+1
        while stop<len(order) and ordered[stop]==ordered[start]:stop+=1
        # Strict less-than: this entire equal-score group is still negative.
        candidates.append({'threshold':float(ordered[start]),'counts':counts(tp,fp,positives-tp,negatives-fp)})
        changes=int(np.count_nonzero(truth[start:stop]));tp+=changes;fp+=stop-start-changes;start=stop
    candidates.append({'threshold':float(np.nextafter(ordered[-1],np.inf)),
                       'counts':counts(tp,fp,positives-tp,negatives-fp)})
    winner=max(candidates,key=lambda r:(objective(r['counts']),-(r['counts']['tp']+r['counts']['fp']),-r['threshold']))
    return {'winner':winner,'candidates':candidates}


def choose(results):
    if set(results)!={m.value for m in Method}:raise ValueError('both declared training methods required')
    return min(results,key=lambda m:(-objective(results[m]['winner']['counts']),
                                   results[m]['winner']['counts']['tp']+results[m]['winner']['counts']['fp'],
                                   m,results[m]['winner']['threshold']))
