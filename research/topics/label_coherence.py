"""Document-presence label coherence diagnostics, never semantic quality gold."""
import math

import numpy as np


def npmi(left, right, joint, documents):
    if (any(type(n) is not int for n in (left,right,joint,documents)) or documents<1
            or not 0<left<=documents or not 0<right<=documents
            or not max(0,left+right-documents)<=joint<=min(left,right)):
        raise ValueError('valid document-presence contingency counts required')
    if joint==0: return -1.
    if joint==documents: return None
    return math.log(joint*documents/(left*right))/-math.log(joint/documents)


def presence_statistics(counts, columns):
    if (counts.ndim!=2 or not counts.shape[0] or len(columns)>2048
            or columns!=sorted(set(columns)) or any(type(c) is not int or not 0<=c<counts.shape[1] for c in columns)
            or not np.isfinite(counts.data).all() or (counts.data<0).any()):
        raise ValueError('bounded aligned nonnegative sparse counts required')
    presence=(counts[:,columns]>0).astype(np.int64).tocsr()
    df=np.asarray(presence.sum(axis=0)).ravel()
    joint=(presence.T@presence).toarray()
    if not np.array_equal(np.diag(joint),df): raise ValueError('document-presence conservation failed')
    return df,joint


def diagnose(terms, positions, df, joint, documents):
    if len(terms)>8 or len(set(terms))!=len(terms) or any(t not in positions for t in terms):
        raise ValueError('frozen unique bounded label terms required')
    pairs=[]
    for i,left in enumerate(terms):
        for right in terms[i+1:]:
            a,b=positions[left],positions[right]
            x,y,z=int(df[a]),int(df[b]),int(joint[a,b])
            atoms,other=set(left.split()),set(right.split())
            pairs.append({'left':left,'right':right,'left_df':x,'right_df':y,'joint_df':z,
                          'npmi':npmi(x,y,z,documents),'lexical_containment':atoms<=other or other<=atoms})
    defined=[p['npmi'] for p in pairs if p['npmi'] is not None]
    return {'terms':terms,'pairs':pairs,'mean_npmi':sum(defined)/len(pairs) if pairs and len(defined)==len(pairs) else None,
            'zero_joint_pairs':sum(p['joint_df']==0 for p in pairs),
            'undefined_pairs':len(pairs)-len(defined),
            'containment_pairs':sum(p['lexical_containment'] for p in pairs)}
