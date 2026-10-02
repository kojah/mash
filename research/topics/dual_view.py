"""Snapshot dense fidelity and conservative gates for a lexical macro experiment."""
import math

import numpy as np

from discovery_controls import canonical


def fidelity(vectors,groups):
    vectors=np.asarray(vectors,dtype=np.float64)
    if (vectors.ndim!=2 or not 1<=vectors.shape[0]<=2000 or not 1<=vectors.shape[1]<=1024
            or not np.isfinite(vectors).all() or groups!=canonical(groups,len(vectors))[0]):
        raise ValueError('bounded canonical complete finite source vectors required')
    norms=np.linalg.norm(vectors,axis=1)
    if not np.allclose(norms,1,atol=1e-12,rtol=0):raise ValueError('original unit directions required')
    values=[None]*len(vectors);cancelled=[]
    for label,group in enumerate(groups):
        mean=np.sum(vectors[group],axis=0,dtype=np.float64)/len(group);norm=float(np.linalg.norm(mean))
        if norm<=1e-12:cancelled.append(label);continue
        cosine=np.clip((vectors[group]@(mean/norm))/norms[group],-1,1)
        for i,c in zip(group,cosine):values[i]=float(c)
    passing=[i for i,c in enumerate(values) if c is not None and c-.9>1e-12]
    uncertain=[i for i,c in enumerate(values) if c is not None and abs(c-.9)<=1e-12]
    failing=[i for i,c in enumerate(values) if c is None or c-.9<-1e-12]
    if sorted(passing+uncertain+failing)!=list(range(len(values))):raise ValueError('complete unambiguous numeric classification required')
    return {'cosines':values,'confirmed_passing':passing,'uncertain':uncertain,'confirmed_failing':failing,'cancelled_group_means':cancelled}


def gates(records,reference,timings):
    size=sum(r['blocks'] for r in records);groups=sum(len(r['groups']) for r in records)
    passing=sum(len(r['fidelity']['confirmed_passing']) for r in records)
    cf=sum(len(r['state']['physical_subclusters']) for r in records)
    merged=sum(c['samples']>1 for r in records for c in r['state']['physical_subclusters'])
    numeric=(2*cf+merged)*384*8;state_budget=16*1024*1024*size/10000
    quality={}
    for scope in ['all','far','crossspan']:
        pairs=sum(r['metrics'][scope]['pairs'] for r in records);weight=math.fsum(r['metrics'][scope]['purity_sum'] for r in records)
        baseline=reference[scope]['purity'];purity=weight/pairs if pairs else None
        quality[scope]={'pairs':pairs,'purity':purity,'offline_lexical_purity':baseline,
            'loss':baseline-purity if purity is not None else None,'within_loss_target':purity is not None and baseline-purity<=.02}
    cpu=math.fsum(r['compression']['fit_cpu_ms'] for r in timings)/size
    inserts=[v for r in timings for v in r['compression']['insert_ms']];p95=float(np.percentile(inserts,95))
    return {'blocks':size,'groups':groups,'physical_subclusters':cf,'group_fraction':groups/size,'retention_target_met':groups<=size//2,
        'confirmed_passing_blocks':passing,'fidelity_fraction_lower_bound':passing/size,'fidelity_target_met_conservatively':passing*20>=size*19,
        'uncertain_blocks':sum(len(r['fidelity']['uncertain']) for r in records),'quality':quality,
        'cf_mean_fit_cpu_ms_per_block':cpu,'cf_insert_p95_ms':p95,'cf_cpu_target_met':cpu<=.5,'cf_p95_target_met':p95<=1,
        'known_numeric_state_lower_bound_bytes':numeric,'persistent_state_budget_bytes':state_budget,
        'state_lower_bound_already_exceeds_budget':numeric>state_budget,
        'within_group_crossspan_pairs':sum(r['within_compressed_group_crossspan_pairs'] for r in records),
        'complete_state_gate':False,'complete_ingestion_gate':False,'semantic_quality_gate':False,'production_selection':False}
