"""Fixed flat-vs-tree and cadence controls; immutable cohort, no inference."""
import argparse
import itertools
import json
from pathlib import Path
import resource
import time
from topic_cohort_demo import load,digest,PINS
from topic_stream_demo import run,encoded
from topic_stream import Stream
from topic_refresh import refresh
from topic_flat import refresh_flat


CASES=[('tree_final',refresh,None),('flat_every16',refresh_flat,16),
       ('flat_every256',refresh_flat,256),('flat_final',refresh_flat,None)]


def stable(result):
    value=dict(result);value.pop('runtime')
    return json.dumps(value,sort_keys=True,allow_nan=False,separators=(',',':'))


def experiment(database,selection,baseline):
    names=PINS+['topic_flat.py','topic_flat_control.py']
    pins={name:digest(Path(__file__).with_name(name)) for name in names}
    baseline_sha=digest(baseline);old=json.loads(baseline.read_text())['arms']['cap2000_final_only']
    sources,_,_,_,ledger=load(database,selection)
    arms={};timings={};start=time.perf_counter();cpu=time.process_time()
    for name,builder,cadence in CASES:
        arm={};timing={};begin=time.perf_counter()
        for direction,ordered in [('forward',sources),('reverse',list(reversed(sources)))]:
            model,snapshot,runtime=run(ordered,maximum_leaves=2000,refresh_every=cadence,builder=builder)
            restored=Stream.restore(json.loads(encoded(model.checkpoint())))
            if builder(restored.capture())!=snapshot:raise ValueError('refreshed restart differs')
            if name=='tree_final' and snapshot!=old[direction]:raise ValueError('shared refactor changed historical tree snapshot')
            if name!='tree_final':
                if snapshot['memberships']!=old[direction]['memberships'] or snapshot['versions']!=old[direction]['versions']:
                    raise ValueError('flat control changed assignments/source versions')
                topics=snapshot['topics'];tree={node['id']:node for node in old[direction]['nodes'] if node['kind']=='leaf'}
                arm[direction+'_label_changes']=sum(topic['labels']!=tree[topic['id']]['labels'] for topic in topics)
            arm[direction]=snapshot;timing[direction]=runtime
        snapshots=[arm[d] for d in ['forward','reverse']]
        pairs=[]
        for snapshot in snapshots:
            topics=snapshot.get('topics')
            if topics is None:topics=[node for node in snapshot['nodes'] if node['kind']=='leaf']
            pairs.append({pair for topic in topics for pair in itertools.combinations(sorted(topic['sources']),2)})
        arm['order']={'forward_pairs':len(pairs[0]),'reverse_pairs':len(pairs[1]),
            'pair_jaccard':len(pairs[0]&pairs[1])/len(pairs[0]|pairs[1]) if pairs[0]|pairs[1] else None}
        arm['configuration']={'maximum_leaves':2000,'refresh_every':cadence,'flat':name!='tree_final'}
        arms[name]=arm;timing['total_ms']=(time.perf_counter()-begin)*1000
        timing['cumulative_rusage_at_completion_kib']=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        timings[name]=timing
        print(json.dumps({'completed_case':name,'forward':arm['forward']['diagnostics'],'order':arm['order']}),flush=True)
    for direction in ['forward','reverse']:
        if not arms['flat_every16'][direction]==arms['flat_every256'][direction]==arms['flat_final'][direction]:
            raise ValueError('flat refresh schedules differ in final semantics')
    if pins!={name:digest(Path(__file__).with_name(name)) for name in names} or digest(baseline)!=baseline_sha:
        raise ValueError('code/baseline drift')
    return {'schema_version':1,'scope':'flat_structure_cadence_control_not_quality_selection','code_pins':pins,
        'database_sha256':digest(database),'selection_sha256':digest(selection),'baseline_sha256':baseline_sha,
        'source_ledger_sha256':ledger,'arms':arms,'historical_tree_exact':True,'flat_assignments_exact':True,
        'flat_schedule_final_exact':True,'semantic_quality_verified':False,'production_selected':False,
        'runtime':{'arms':timings,'total_ms_after_load':(time.perf_counter()-start)*1000,
            'cpu_ms_after_load':(time.process_time()-cpu)*1000,
            'rusage_before_serialization_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['database','selection','baseline','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    result=experiment(args.database,args.selection,args.baseline)
    if args.compare and stable(json.loads(args.compare.read_text()))!=stable(result):raise ValueError('stable flat results differ')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'stable_complete':True,'total_ms':result['runtime']['total_ms_after_load']}),flush=True)
