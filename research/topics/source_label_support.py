"""Compare physical-block and unique original-message support, with fixed scores."""
from enum import StrEnum
import numpy as np
from scipy import sparse
from episode_blocks import block_counts
from label_noise import EXCLUDED
from stable_lexical_frame import State
from supported_labels import Background


class Unit(StrEnum):
    BLOCK='physical_block'
    MESSAGE='original_message'


def labels(groups,counts,owners,source_ids,terms,states,background,unit):
    unit=Unit(unit);flat=sum(groups,[]);owners=np.asarray(owners)
    if (not sparse.isspmatrix_csr(counts) or counts.shape!=(len(source_ids),len(terms))
            or len(source_ids)!=len(set(source_ids)) or any(type(i) is not int or i<0 for i in source_ids)
            or owners.shape!=(counts.shape[0],) or owners.dtype.kind not in 'iu'
            or not isinstance(background,Background) or background.terms!=tuple(terms)
            or not np.isfinite(counts.data).all() or (counts.data<0).any()
            or any(State(s)==State.EMPTY for s in states) or set(flat)!=set(range(len(states)))
            or len(flat)!=len(set(flat)) or any(g!=sorted(set(g)) or not g for g in groups)
            or any(type(i) is not int for i in flat) or (owners<-1).any() or (owners>=len(states)).any()
            or set(owners[owners>=0])!=set(range(len(states)))):
        raise ValueError('unique original sources and complete block ownership required')
    bank=block_counts(counts,owners,len(states))
    eligible=np.array([State(s)==State.READY for s in states])
    clean=(sparse.diags(eligible.astype(float))@bank.multiply([t not in EXCLUDED for t in terms])).tocsr();clean.eliminate_zeros()
    original=counts.multiply([t not in EXCLUDED for t in terms]).tocsr();original.eliminate_zeros()
    classes=sparse.vstack([sparse.csr_matrix(clean[g].sum(axis=0)) for g in groups],format='csr')
    lengths=np.asarray(classes.sum(axis=1)).ravel();frequency=np.asarray(classes.sum(axis=0)).ravel()
    rows=np.repeat(np.arange(len(lengths)),np.diff(classes.indptr));base=classes.copy()
    base.data=base.data/np.maximum(lengths[rows],1)*np.log1p(lengths.mean()/frequency[base.indices])
    out=[]
    for i,group in enumerate(groups):
        source_rows=np.flatnonzero(np.isin(owners,[j for j in group if eligible[j]]))
        message_df=np.asarray((original[source_rows]>0).sum(axis=0)).ravel()
        block_df=np.asarray((clean[group]>0).sum(axis=0)).ravel();member=block_df if unit==Unit.BLOCK else message_df
        row=base[i];candidates=[]
        for j,value in zip(row.indices,row.data):
            if not any(c.isalpha() for c in terms[j]) or member[j]<2 or background.df[j]>.4*background.documents:continue
            candidates.append({'term':terms[j],'score':float(value*background.idf[j]),'member_blocks':int(block_df[j]),
                'member_sources':int(message_df[j]),'background_blocks':int(background.df[j])})
        out.append(sorted(candidates,key=lambda t:(-t['score'],t['term']))[:8])
    return out
