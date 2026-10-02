"""Full original positive support; unassigned sources are never a shared cluster."""
import numpy as np


def evaluate(groups,owners,gold,parts,ordinals):
    n=len(owners);blocks=sorted(set(owners));flat=sum(groups,[])
    if (not n==len(gold)==len(parts)==len(ordinals) or blocks!=list(range(len(blocks)))
            or sorted(set(ordinals))!=ordinals or len(flat)!=len(set(flat))
            or any(type(b) is not int or b not in blocks for b in flat)
            or any((g is None)!=(p is None) for g,p in zip(gold,parts))):
        raise ValueError('aligned complete original source ownership required')
    assignments={b:i for i,g in enumerate(groups) for b in g};assigned=np.array([o in assignments for o in owners])
    known=np.array([g is not None for g in gold]);selected=np.flatnonzero(known);source_groups=np.array([assignments.get(owners[i],-1) for i in selected])
    source_gold=np.array([gold[i] for i in selected]);source_parts=np.array([parts[i] for i in selected]);source_owners=np.array([owners[i] for i in selected]);source_ordinals=np.array([ordinals[i] for i in selected])
    a,b=np.triu_indices(len(selected),1);distinct=source_owners[a]!=source_owners[b];same=source_gold[a]==source_gold[b];far=source_ordinals[b]-source_ordinals[a]>4
    recurring=source_parts[a]!=source_parts[b];predicted=(source_groups[a]>=0)&(source_groups[a]==source_groups[b]);out={}
    for scope,eligible in [('all',distinct),('far',distinct&far),('crossspan',distinct&far&(~same|recurring))]:
        positives=eligible&same;tp=int((positives&predicted).sum());support=int(positives.sum());pred=int((eligible&predicted).sum())
        out[scope]={'gold_pairs':support,'predicted_pairs':pred,'true_positive_pairs':tp,
            'precision':tp/pred if pred else None,'recall':tp/support if support else None,'f1':2*tp/(pred+support) if pred+support else None}
    out['coverage']={'ready_sources':n,'singleton_known_sources':int(known.sum()),'assigned_sources':int(assigned.sum()),
        'assigned_known_sources':int((assigned&known).sum()),'physical_blocks':len(blocks),'assigned_blocks':len(flat),'unassigned_blocks':len(blocks)-len(flat),
        'groups':len(groups),'group_sizes':sorted(map(len,groups))}
    return out
