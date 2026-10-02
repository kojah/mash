"""LDAvis-inspired relevance on hard source-count groups; not LDA inference."""
from dataclasses import dataclass
import math
import numpy as np
from scipy import sparse
from label_noise import EXCLUDED
from stable_lexical_frame import State
from supported_labels import Background,prepare_background


WEIGHT=.6


@dataclass(frozen=True)
class Corpus:
    background:Background
    frequency:np.ndarray
    total:float


def prepare(counts,terms):
    background=prepare_background(counts,terms)
    clean=counts.multiply([t not in EXCLUDED for t in terms])
    frequency=np.asarray(clean.sum(axis=0)).ravel();frequency.flags.writeable=False
    return Corpus(background,frequency,float(math.fsum(frequency)))


def score(probability,background_probability):
    if not 0<probability<=1 or not 0<background_probability<=1:raise ValueError('positive finite probability required')
    return WEIGHT*math.log(probability)+(1-WEIGHT)*math.log(probability/background_probability)


def labels(groups,counts,terms,states,corpus):
    flat=sum(groups,[])
    if (not sparse.isspmatrix_csr(counts) or not isinstance(corpus,Corpus) or corpus.background.terms!=tuple(terms)
            or counts.shape[1]!=len(terms) or len(states)!=counts.shape[0] or len(flat)!=len(set(flat))
            or any(not g or g!=sorted(set(g)) for g in groups) or any(type(j) is not int or not 0<=j<counts.shape[0] for j in flat)
            or not np.isfinite(counts.data).all() or (counts.data<0).any() or any(State(s)==State.EMPTY for s in states)):
        raise ValueError('complete aligned source-word ownership required')
    clean=(sparse.diags([float(State(s)==State.READY) for s in states])@counts.multiply([t not in EXCLUDED for t in terms])).tocsr();clean.eliminate_zeros();out=[]
    for group in groups:
        row=sparse.csr_matrix(clean[group].sum(axis=0));member_df=np.asarray((clean[group]>0).sum(axis=0)).ravel();total=float(math.fsum(row.data));candidates=[]
        for j,tf in zip(row.indices,row.data):
            if not any(c.isalpha() for c in terms[j]) or member_df[j]<2 or corpus.background.df[j]>.4*corpus.background.documents:continue
            p=float(tf)/total;q=float(corpus.frequency[j])/corpus.total
            candidates.append({'term':terms[j],'score':score(p,q),'member_blocks':int(member_df[j]),'background_blocks':int(corpus.background.df[j]),
                'within_probability':p,'background_probability':q,'lift':p/q})
        out.append(sorted(candidates,key=lambda c:(-c['score'],c['term']))[:8])
    return out
