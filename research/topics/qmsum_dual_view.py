"""Dense snapshot CF compression plus unchanged lexical weighted macro reference."""
import argparse
from collections import Counter
import json
from pathlib import Path
import resource
import time

import numpy as np
from sklearn.metrics import adjusted_rand_score
from threadpoolctl import threadpool_limits

from block_hierarchy import evaluate
from compressed_macro import compress,macro
from discovery_inputs import read,digest
from dual_view import fidelity,gates
from episode_blocks import block_counts,lexical
from qmsum_block_hierarchy import stored_tree
from qmsum_compressed_macro import REFERENCE
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY


TIMED_GATES=['cf_mean_fit_cpu_ms_per_block','cf_insert_p95_ms','cf_cpu_target_met','cf_p95_target_met']


def experiment(a):
    start=time.perf_counter();cpu=time.process_time()
    if digest(a.geometry/'results.json')!=GEOMETRY or digest(a.macro_reference)!=REFERENCE:raise ValueError('immutable full references required')
    original=json.loads((a.geometry/'results.json').read_text());reference=json.loads(a.macro_reference.read_text());data=read(a)
    if original['source_sha256']!=data.source or reference['source_sha256']!=data.source or original['support']['cancelled_blocks']!=0:
        raise ValueError('matched source required')
    for capture in [original,reference]:
        for name,pin in capture['code_pins'].items():
            if digest(Path(__file__).with_name(name))!=pin:raise ValueError('immutable reference code drift')
    pin=original['files']['block-directions.npy']['sha256']
    if digest(a.geometry/'block-directions.npy')!=pin:raise ValueError('dense direction file drift')
    directions=np.load(a.geometry/'block-directions.npy',mmap_mode='r',allow_pickle=False)
    setup=(time.perf_counter()-start)*1000;records=[];timings=[];support=Counter();orders=[]
    with threadpool_limits(limits=1):
        for meeting,row in zip(data.meetings,original['records']):
            size=len(meeting.turns);offset=row['source_offset'];blocks=row['blocks'];owner=np.full(size,-1,dtype=np.int64)
            for b in blocks:owner[b['start']:b['end']+1]=b['ordinal']
            if row['meeting']!=meeting.ordinal or not np.array_equal(owner>=0,data.ready[offset:offset+size]):raise ValueError('complete source lineage required')
            selected=np.flatnonzero(owner>=0).tolist();gold,parts=components(meeting,selected);owned=owner[selected].tolist()
            lo=row['block_directions_offset'];dense=directions[lo:lo+len(blocks)];begin=time.perf_counter()
            lex=lexical(block_counts(data.counts[offset:offset+size],owner,len(blocks)),data.idf);features_ms=(time.perf_counter()-begin)*1000
            known=np.asarray([i for i,g in enumerate(gold) if g is not None]);known_ord=np.asarray(selected)[known]
            known_blocks=np.asarray(owned)[known];kg=np.asarray([gold[i] for i in known]);kp=np.asarray([parts[i] for i in known]);i,j=np.triu_indices(len(known),1)
            returns=(kg[i]==kg[j])&(kp[i]!=kp[j])&(known_ord[j]-known_ord[i]>4)&(known_blocks[i]!=known_blocks[j]);assigned=[]
            for order,reverse in [('forward',False),('reverse',True)]:
                result=compress(dense,reverse);begin=time.perf_counter();diagnostic=fidelity(dense,result.groups);fidelity_ms=(time.perf_counter()-begin)*1000
                begin=time.perf_counter();tree,merges=macro(lex,result.groups);macro_ms=(time.perf_counter()-begin)*1000
                begin=time.perf_counter();metrics=evaluate(tree,owned,gold,parts,selected,len(meeting.topics));evaluation_ms=(time.perf_counter()-begin)*1000
                labels=np.asarray(result.assignments);within=int((returns&(labels[known_blocks[i]]==labels[known_blocks[j]])).sum())
                records.append({'meeting':meeting.ordinal,'split':row['split'],'order':order,'blocks':len(blocks),'groups':result.groups,
                    'assignments':result.assignments,'state':result.state,'fidelity':diagnostic,'merges':merges,'tree':stored_tree(tree),
                    'within_compressed_group_crossspan_pairs':within,'metrics':metrics})
                timings.append({'meeting':meeting.ordinal,'order':order,'split':row['split'],'features_ms':features_ms if not reverse else 0.,
                    'compression':result.timings,'fidelity_ms':fidelity_ms,'macro_ms':macro_ms,'evaluation_ms':evaluation_ms});assigned.append(result.assignments)
            orders.append({'meeting':meeting.ordinal,'block_partition_ari':float(adjusted_rand_score(*assigned))})
            support.update(sources=size,ready=len(selected),known=len(known),blocks=len(blocks))
            if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('complete source support drift')
    summaries={};timed={}
    for split in ['all','development','reserved_internal']:
        summaries[split]={};timed[split]={}
        for order in ['forward','reverse']:
            rows=[r for r in records if r['order']==order and (split=='all' or r['split']==split)]
            ts=[t for t in timings if t['order']==order and (split=='all' or t['split']==split)]
            result=gates(rows,reference['summary'][split]['methods']['lexical_average'],ts)
            timed[split][order]={key:result.pop(key) for key in TIMED_GATES};summaries[split][order]=result
    return {'schema_version':1,'scope':'full_training_dense_compression_lexical_macro_snapshot_not_online_selection',
        'source_sha256':data.source,'source_capture_sha256':data.capture,'geometry_sha256':GEOMETRY,'macro_reference_sha256':REFERENCE,
        'block_file_sha256':pin,'support':dict(support),'protocol':{'compression':'unchanged384dim_dense_singleton_BIRCH_radius.4_branch64_bothorders',
            'macro':'unchanged18316dim_lexical_population_weighted_mean_crossblock_cosine',
            'partition':'final_nearest_CF_predictions_not_insertion_ownership','fidelity':'unit_cosine_to_recomputed_snapshot_group_mean_not_live_CF_centroid',
            'fidelity_decision':'confirmed_if_cosine_minus.9_greater_than1e-12_uncertain_band_not_passing',
            'within_group_lifting':'source_order_balanced_diagnostic_not_semantic_ancestry','budget':'frozen_COMPRESSION_BUDGET.md',
            'state_lower_bound':'(2physicalCF_plus_physicalCF_samples_gt1)*384*8_for_retaining_all_permeeting_model_banks',
            'state_exclusions':['node_caches','metadata','scalars','lexical','macro','labels','ownership','temporaries'],
            'partition_evaluation':'already_explored_internal_not_pristine','new_inference':0,'llm_calls':0,'held_out_read':False,
            'grids':False,'full_ingestion_gate':False,'labels_measured':False,'production_selection':False},
        'summary':summaries,'order':orders,'records':records,'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
            ['qmsum_dual_view.py','dual_view.py','compressed_macro.py','COMPRESSION_BUDGET.md','block_hierarchy.py','qmsum_block_hierarchy.py',
             'qmsum_compressed_macro.py','discovery_controls.py','discovery_inputs.py','episode_blocks.py','qmsum_hierarchy.py','qmsum.py','uv.lock']},
        'timings':{'setup_ms':setup,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings,'gate_results':timed}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['source','vectors','units','labels','baseline','reference-manifest','geometry','macro-reference','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if json.dumps({k:v for k,v in old.items() if k!='timings'},sort_keys=True,allow_nan=False)!=json.dumps({k:v for k,v in result.items() if k!='timings'},sort_keys=True,allow_nan=False):raise ValueError('complete dual-view repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'support':result['support'],'summary':result['summary']['all'],'timing_gates':result['timings']['gate_results']['all']}),flush=True)


if __name__=='__main__':main()
