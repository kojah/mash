"""Paired capacity/scheduling controls on frozen admitted sources; no inference."""
import argparse
from collections import Counter
import json
from pathlib import Path
import resource
import time
from topic_cohort_demo import load,digest,PINS as COHORT_PINS
from topic_stream_demo import run,pair_members,encoded
from topic_stream import Stream
from topic_refresh import refresh


CASES=[('cap128_every16',128,16),('cap2000_every16',2000,16),('cap2000_final_only',2000,None)]


def stable_json(result):
    stable=dict(result);stable.pop('runtime')
    return json.dumps(stable,sort_keys=True,allow_nan=False,separators=(',',':'))


def experiment(database,selection):
    pins={name:digest(Path(__file__).with_name(name)) for name in COHORT_PINS+['topic_capacity_control.py']}
    sources,metadata,provenance,excluded,ledger=load(database,selection)
    arms={};timings={};start=time.perf_counter();cpu=time.process_time()
    for name,capacity,cadence in CASES:
        begin=time.perf_counter();model,forward,fr=run(sources,maximum_leaves=capacity,refresh_every=cadence)
        states=dict(Counter(r.state.value for r in model.records.values()))
        checkpoint=model.checkpoint()
        if refresh(Stream.restore(json.loads(encoded(checkpoint))).capture())!=forward:raise ValueError('capacity checkpoint refresh differs')
        reverse_model,reverse,rr=run(list(reversed(sources)),maximum_leaves=capacity,refresh_every=cadence)
        left=pair_members(forward);right=pair_members(reverse)
        arms[name]={'configuration':{'maximum_leaves':capacity,'refresh_every':cadence},'forward':forward,'reverse':reverse,
                    'routing_states':states,'reverse_routing_states':dict(Counter(r.state.value for r in reverse_model.records.values())),
                    'freshness':model.freshness(),'checkpoint_exact_refreshed':True,
                    'order':{'forward_pairs':len(left),'reverse_pairs':len(right),
                             'pair_jaccard':len(left&right)/len(left|right) if left|right else None},
                    'summary':{'singleton_leaves':sum(n['kind']=='leaf' and len(n['sources'])==1 for n in forward['nodes']),
                               'weak_leaves':sum(n['kind']=='leaf' and n['label_support']=='weak' for n in forward['nodes'])}}
        timings[name]={'forward':fr,'reverse':rr,'total_ms':(time.perf_counter()-begin)*1000,
                       'rusage_hwm_at_case_completion_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}
        print(json.dumps({'completed_case':name,'forward':forward['diagnostics'],'states':states,'order':arms[name]['order']}),flush=True)
    for direction in ['forward','reverse']:
        if arms['cap2000_every16'][direction]!=arms['cap2000_final_only'][direction]:
            raise ValueError('refresh scheduling changed final topic semantics')
    if pins!={name:digest(Path(__file__).with_name(name)) for name in pins}:raise ValueError('code drift during capacity control')
    return {'schema_version':1,'scope':'fixed_capacity_scheduling_control_not_production_selection','code_pins':pins,
            'selection_sha256':digest(selection),'database_sha256':digest(database),'source_ledger_sha256':ledger,
            'policy':{'source_occurrences':len(sources),'excluded_records':len(excluded),'threshold':.55,'new_inference':0,
                      'maximum_hierarchy_leaves':2000,'capacity_not_similarity_tuned':True},
            'arms':arms,'scheduling_final_semantics_exact':True,'semantic_quality_verified':False,'production_selected':False,
            'runtime':{'arms':timings,'total_ms_after_load':(time.perf_counter()-start)*1000,'cpu_ms_after_load':(time.process_time()-cpu)*1000,
                       'rusage_hwm_before_serialization_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['database','selection','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    result=experiment(args.database,args.selection)
    if args.compare and stable_json(json.loads(args.compare.read_text()))!=stable_json(result):raise ValueError('stable capacity results differ')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'stable_complete':True,'total_ms':result['runtime']['total_ms_after_load']}),flush=True)
