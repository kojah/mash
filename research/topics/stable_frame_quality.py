"""Fixed-frame diagnostics and source-word labels; no fitted geometry or gold cut."""
import math
import numpy as np
from scipy import sparse
from captured_hierarchy import traversal
from label_noise import EXCLUDED
from stable_lexical_frame import Policy,State
from subject_labels import ranked_terms
from topic_labels import scores


def identity_frame(counts,terms,policy):
    if (not sparse.isspmatrix_csr(counts) or counts.shape[1]!=len(terms)
            or terms!=sorted(set(terms)) or not np.isfinite(counts.data).all()
            or (counts.data<=0).any()):raise ValueError('canonical positive source counts required')
    policy=Policy(policy);used=counts.copy().astype(np.float64)
    if policy==Policy.FILTERED:
        used=used.multiply([t not in EXCLUDED for t in terms]).tocsr();used.eliminate_zeros()
    states=[]
    for i in range(counts.shape[0]):
        if not counts[i].nnz:states.append(State.EMPTY)
        elif not used[i].nnz:states.append(State.FALLBACK)
        else:states.append(State.READY)
    fallback=np.array([s==State.FALLBACK for s in states])
    used=used+sparse.diags(fallback.astype(float))@counts
    used.data=1+np.log(used.data)
    norms=np.sqrt(np.asarray(used.multiply(used).sum(axis=1)).ravel())
    result=(sparse.diags(np.divide(1.,norms,out=np.zeros_like(norms),where=norms>0))@used).tocsr()
    return result,states


def cosine_errors(hashed,identity):
    if hashed.shape[0]!=identity.shape[0] or not 1<=hashed.shape[0]<=2000:raise ValueError('matched bounded rows required')
    h=(hashed@hashed.T).toarray();v=(identity@identity.T).toarray()
    if not np.isfinite(h).all() or not np.isfinite(v).all():raise ValueError('finite cosines required')
    errors=(h-v)[np.triu_indices(len(h),1)]
    return {'pairs':len(errors),'signed_sum':math.fsum(errors),'absolute_sum':math.fsum(abs(errors)),
        'maximum_absolute':float(max(abs(errors),default=0)),
        'minimum_signed':float(min(errors,default=0)),'maximum_signed':float(max(errors,default=0))}


def label_cut(tree):
    """Display exactly min(8,n) groups, split largest leaf population first.

    Geometry topology is fixed; sizes and original leaf ordinals resolve ties.
    This is a reproducible display diagnostic, not an inferred semantic cut.
    """
    post,_,_=traversal(tree,len(tree.leaves));members={}
    for n in post:members[n]=[tree.leaves[n]] if n in tree.leaves else sorted(members[tree.children[n][0]]+members[tree.children[n][1]])
    cut=[tree.root]
    while len(cut)<min(8,len(tree.leaves)):
        candidates=[n for n in cut if n in tree.children]
        n=min(candidates,key=lambda n:(-len(members[n]),members[n]));cut.remove(n);cut.extend(tree.children[n])
    return [members[n] for n in sorted(cut,key=lambda n:members[n])]


def label_examples(tree,features,counts,terms,states):
    if features.shape[0]!=counts.shape[0] or len(states)!=counts.shape[0]:raise ValueError('matched label ownership required')
    groups=label_cut(tree);filtered=counts.multiply([t not in EXCLUDED for t in terms]).tocsr();filtered.eliminate_zeros()
    # Geometry fallback does not grant invented label support.
    eligible=np.array([s==State.READY for s in states])
    filtered=(sparse.diags(eligible.astype(float))@filtered).tocsr()
    aggregated=sparse.vstack([sparse.csr_matrix(filtered[g].sum(axis=0)) for g in groups],format='csr')
    base,_=scores(aggregated,filtered);out=[]
    for i,g in enumerate(groups):
        row=base[i];local=[terms[j] for j in row.indices];mean=sparse.csr_matrix(features[g].mean(axis=0))
        similarities=np.asarray((features[g]@mean.T).toarray()).ravel()
        representative=min(range(len(g)),key=lambda j:(-similarities[j],g[j]))
        out.append({'blocks':g,'label_eligible_blocks':[j for j in g if eligible[j]],
            'labels':ranked_terms(row.data,local,limit=8),'representative_block':g[representative],
            'representative_mean_similarity':float(similarities[representative])})
    return out
