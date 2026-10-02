"""Source-word support diagnostics, without new bespoke noise words or inference."""
from dataclasses import dataclass
from enum import StrEnum
from types import MappingProxyType
import numpy as np
from scipy import sparse
from label_noise import EXCLUDED
from stable_lexical_frame import State
from subject_labels import ranked_terms


class Profile(StrEnum):
    STATIC='static_noise_ctfidf'
    DISTINCTIVE='source_supported_corpus_idf'


class Support(StrEnum):
    SUPPORTED='at_least_two_source_supported_label_terms'
    WEAK='insufficient_label_terms'


@dataclass(frozen=True)
class Proposal:
    blocks:list[int]
    eligible_blocks:list[int]
    labels:list[dict]
    support:Support


@dataclass(frozen=True)
class Background:
    documents:int
    terms:tuple[str,...]
    df:np.ndarray
    idf:np.ndarray
    positions:MappingProxyType


def prepare_background(counts,terms):
    if (not sparse.isspmatrix_csr(counts) or not 1<=counts.shape[0]<=10000 or counts.shape[1]!=len(terms)
            or not 1<=len(terms)<=20000 or terms!=sorted(set(terms))
            or not np.isfinite(counts.data).all() or (counts.data<0).any()):raise ValueError('complete canonical nonnegative source corpus required')
    df=np.asarray((counts>0).sum(axis=0)).ravel();idf=np.log((1+counts.shape[0])/(1+df))+1;df.flags.writeable=False;idf.flags.writeable=False
    return Background(counts.shape[0],tuple(terms),df,idf,MappingProxyType({t:i for i,t in enumerate(terms)}))


def label(groups,counts,terms,states,background,profile):
    profile=Profile(profile)
    flat=sum(groups,[])
    if (not sparse.isspmatrix_csr(counts) or not isinstance(background,Background)
            or counts.shape[1]!=len(terms) or background.terms!=tuple(terms) or len(states)!=counts.shape[0]
            or not np.isfinite(counts.data).all() or (counts.data<0).any() or len(flat)!=len(set(flat))
            or any(not g or g!=sorted(set(g)) for g in groups)
            or any(type(i) is not int or not 0<=i<counts.shape[0] for i in flat)
            or any(State(s)==State.EMPTY for s in states)):
        raise ValueError('aligned complete source-word counts and unique proposal ownership required')
    if not groups:return []
    eligible=np.array([State(s)==State.READY for s in states]);clean=(sparse.diags(eligible.astype(float))@counts.multiply([t not in EXCLUDED for t in terms])).tocsr();clean.eliminate_zeros()
    classes=sparse.vstack([sparse.csr_matrix(clean[g].sum(axis=0)) for g in groups],format='csr')
    lengths=np.asarray(classes.sum(axis=1)).ravel();frequency=np.asarray(classes.sum(axis=0)).ravel()
    rows=np.repeat(np.arange(len(lengths)),np.diff(classes.indptr));base=classes.copy()
    base.data=base.data/np.maximum(lengths[rows],1)*np.log1p(lengths.mean()/frequency[base.indices])
    df=background.df;out=[]
    for i,g in enumerate(groups):
        row=base[i];member_df=np.asarray((clean[g]>0).sum(axis=0)).ravel()
        if profile==Profile.DISTINCTIVE:
            keep=np.array([any(c.isalpha() for c in terms[j]) and member_df[j]>=2 and df[j]<=.4*background.documents for j in row.indices],dtype=bool)
            indices=row.indices[keep];values=row.data[keep]*background.idf[indices]
        else:indices,values=row.indices,row.data
        labels=ranked_terms(values,[terms[j] for j in indices],limit=8)
        for term in labels:
            j=background.positions[term['term']];term.update(member_blocks=int(member_df[j]),background_blocks=int(df[j]))
        out.append(Proposal(g.copy(),[j for j in g if eligible[j]],labels,Support.SUPPORTED if len(labels)>=2 else Support.WEAK))
    return out
