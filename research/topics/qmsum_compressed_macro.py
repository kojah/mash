"""Full-source fixed CF compression control with a separate weighted macro tree."""
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
from episode_blocks import block_counts,lexical
from qmsum_block_hierarchy import stored_tree
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY


REFERENCE='8d11deaf3abde425b1ed972bb63460d5a54a59c04aaa28c26e35fea7d3246e1c'


def experiment(a):
    start=time.perf_counter();cpu=time.process_time()
    if digest(a.geometry/'results.json')!=GEOMETRY or digest(a.macro_reference)!=REFERENCE:
        raise ValueError('immutable full block and macro references required')
    old=json.loads((a.geometry/'results.json').read_text());reference=json.loads(a.macro_reference.read_text());data=read(a)
    if old['source_sha256']!=data.source or reference['source_sha256']!=data.source or old['support']['cancelled_blocks']!=0:
        raise ValueError('matched source required')
    for capture in [old,reference]:
        for name,pin in capture['code_pins'].items():
            if digest(Path(__file__).with_name(name))!=pin:raise ValueError('immutable reference code drift')
    setup=(time.perf_counter()-start)*1000;records=[];timings=[];support=Counter();orders=[]
    with threadpool_limits(limits=1):
        for meeting,original in zip(data.meetings,old['records']):
            offset=original['source_offset'];size=len(meeting.turns);blocks=original['blocks'];owner=np.full(size,-1,dtype=np.int64)
            for b in blocks:owner[b['start']:b['end']+1]=b['ordinal']
            if meeting.ordinal!=original['meeting'] or not np.array_equal(owner>=0,data.ready[offset:offset+size]):raise ValueError('complete lineage drift')
            selected=np.flatnonzero(owner>=0).tolist();gold,parts=components(meeting,selected);owners=owner[selected].tolist()
            begin=time.perf_counter();lex=lexical(block_counts(data.counts[offset:offset+size],owner,len(blocks)),data.idf)
            features_ms=(time.perf_counter()-begin)*1000;known=np.asarray([i for i,g in enumerate(gold) if g is not None])
            known_gold=np.asarray([gold[i] for i in known]);known_parts=np.asarray([parts[i] for i in known]);known_ord=np.asarray(selected)[known]
            known_blocks=np.asarray(owners)[known];i,j=np.triu_indices(len(known),1)
            returns=(known_gold[i]==known_gold[j])&(known_parts[i]!=known_parts[j])&(known_ord[j]-known_ord[i]>4)&(known_blocks[i]!=known_blocks[j])
            assignments=[]
            for order,reverse in [('forward',False),('reverse',True)]:
                result=compress(lex,reverse);begin=time.perf_counter();tree,merges=macro(lex,result.groups);macro_ms=(time.perf_counter()-begin)*1000
                begin=time.perf_counter();metrics=evaluate(tree,owners,gold,parts,selected,len(meeting.topics));evaluation_ms=(time.perf_counter()-begin)*1000
                nearest=np.asarray(result.assignments);within=int((returns&(nearest[known_blocks[i]]==nearest[known_blocks[j]])).sum())
                records.append({'meeting':meeting.ordinal,'split':original['split'],'order':order,'blocks':len(blocks),
                    'groups':result.groups,'assignments':result.assignments,'state':result.state,'merges':merges,'tree':stored_tree(tree),
                    'within_compressed_group_crossspan_pairs':within,'metrics':metrics})
                timings.append({'meeting':meeting.ordinal,'order':order,'features_ms':features_ms if not reverse else 0.,
                    'compression':result.timings,'macro_ms':macro_ms,'evaluation_ms':evaluation_ms})
                assignments.append(result.assignments)
            orders.append({'meeting':meeting.ordinal,'block_partition_ari':float(adjusted_rand_score(*assignments))})
            support.update(sources=size,ready=len(selected),known=len(known),blocks=len(blocks))
            if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('complete source required')
    summaries={}
    for order in ['forward','reverse']:
        subset=[r for r in records if r['order']==order];summary={}
        for scope in ['all','far','crossspan']:
            values=[r['metrics'][scope] for r in subset];pairs=sum(v['pairs'] for v in values);weight=sum(v['purity_sum'] for v in values)
            summary[scope]={'pairs':pairs,'purity':weight/pairs,'root_only_purity':sum(v['root_only_purity_sum'] for v in values)/pairs,
                'offline_lexical_purity':reference['summary']['all']['methods']['lexical_average'][scope]['purity']}
        summary['groups']=sum(len(r['groups']) for r in subset);summary['physical_subclusters']=sum(len(r['state']['physical_subclusters']) for r in subset)
        summary['within_compressed_group_crossspan_pairs']=sum(r['within_compressed_group_crossspan_pairs'] for r in subset)
        summaries[order]=summary
    return {'schema_version':1,'scope':'full_training_fixed_cf_snapshot_weighted_macro_control_not_online_selection',
        'source_sha256':data.source,'source_capture_sha256':data.capture,'geometry_sha256':GEOMETRY,'macro_reference_sha256':REFERENCE,
        'support':dict(support),'protocol':{'features':'unchanged_original18316dim_fulltrainingIDF_sparse_unit_block_features',
            'compression':'sklearn_BIRCH_singleton_radius.4_branch64_both_orders','partition':'final_nearest_CF_predictions_not_insertion_ownership',
            'macro':'unrenormalized_group_means_exact_mean_crossblock_cosine_population_weighted_average',
            'ties':'round12_distance_then_earliest_original_block_pair','within_group_lifting':'source_order_balanced_diagnostic_not_semantic_ancestry',
            'metric':'unchanged_source_weighted_physical_block_LCA_all_far_crossspan_conditional_known',
            'snapshot_frame':'frozen_full_training_vocabulary_IDF_not_online_IDF_validation','threshold_grid':False,
            'new_inference':0,'llm_calls':0,'held_out_read':False,'frontier_selection':False,'full_ingestion_gate':False},
        'summary':summaries,'order':orders,'records':records,'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
            ['qmsum_compressed_macro.py','compressed_macro.py','qmsum_block_hierarchy.py','block_hierarchy.py','captured_hierarchy.py',
             'discovery_controls.py','discovery_inputs.py','episode_blocks.py','qmsum_hierarchy.py','qmsum.py','uv.lock']},
        'timings':{'setup_ms':setup,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['source','vectors','units','labels','baseline','reference-manifest','geometry','macro-reference','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if json.dumps({k:v for k,v in old.items() if k!='timings'},sort_keys=True,allow_nan=False)!=json.dumps({k:v for k,v in result.items() if k!='timings'},sort_keys=True,allow_nan=False):raise ValueError('complete compression repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'support':result['support'],'summary':result['summary'],'timings':{k:v for k,v in result['timings'].items() if k!='stages'}}),flush=True)


if __name__=='__main__':main()
