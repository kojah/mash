"""Fixed lexical navigation diagnostic; no gold annotations in retrieval."""
from enum import StrEnum
import math
import numpy as np
from scipy import sparse
from episode_blocks import lexical
from label_noise import EXCLUDED
from stable_lexical_frame import counts


class Arm(StrEnum):
    DIRECT='direct_sources'
    GROUP='group_words'
    BLOCK='block_labels'
    MESSAGE='message_labels'


def query(text,positions,idf):
    words={t:n for t,n in counts(text).items() if t not in EXCLUDED}
    known=sorted((positions[t],n) for t,n in words.items() if t in positions)
    row=sparse.csr_matrix(([n for j,n in known],([0]*len(known),[j for j,n in known])),shape=(1,len(positions)))
    return lexical(row,idf).tocsr(),{'filtered_terms':len(words),'known_terms':len(known),'oov_terms':len(words)-len(known)}


def rank(scores,ids,limit):
    values=np.asarray(scores);ids=np.asarray(ids)
    if (values.shape!=ids.shape or values.ndim!=1 or ids.dtype.kind not in 'iu' or len(ids)!=len(set(ids.tolist()))
            or (ids<0).any() or not np.isfinite(values).all() or type(limit) is not int or limit<1):
        raise ValueError('unique nonnegative identities and finite aligned scores required')
    eligible=np.flatnonzero(values>0);order=np.lexsort((ids[eligible],-values[eligible]))[:limit]
    return [{'id':int(ids[i]),'score':float(values[i])} for i in eligible[order]]


def retrieve(source_scores,source_ids,source_groups,group_scores,group_ids,arm):
    arm=Arm(arm);ids=np.asarray(source_ids);owners=np.asarray(source_groups)
    if ids.shape!=owners.shape:raise ValueError('source-group alignment required')
    selected=[]
    if arm!=Arm.DIRECT:selected=rank(group_scores,group_ids,2)
    keep=np.ones(len(ids),dtype=bool) if arm==Arm.DIRECT else np.isin(owners,[g['id'] for g in selected])
    result=rank(np.asarray(source_scores)[keep],ids[keep],20)
    return {'selected_groups':selected,'returned_sources':result,'candidate_sources':int(keep.sum())}


def metrics(result,relevant,candidate_ids):
    if not relevant or len(relevant)!=len(set(relevant)) or len(candidate_ids)!=len(set(candidate_ids)):
        raise ValueError('unique original relevant/candidate identities required')
    relevant=set(relevant);returned=[r['id'] for r in result['returned_sources']]
    if len(returned)>20 or len(returned)!=len(set(returned)) or not set(returned)<=set(candidate_ids):raise ValueError('valid returned source subset required')
    hits=[i+1 for i,j in enumerate(returned) if j in relevant];dcg=sum(1/math.log2(i+1) for i in hits)
    ideal=sum(1/math.log2(i+1) for i in range(1,min(20,len(relevant))+1))
    return {'relevant_sources':len(relevant),'hits':len(hits),'recall_at_20':len(hits)/len(relevant),'precision_at_20':len(hits)/20,
        'mrr':1/hits[0] if hits else 0.,'ndcg_at_20':dcg/ideal,'candidate_relevant_sources':len(relevant&set(candidate_ids)),
        'candidate_recall':len(relevant&set(candidate_ids))/len(relevant),'returned_sources':len(returned),'zero_results':int(not returned)}
