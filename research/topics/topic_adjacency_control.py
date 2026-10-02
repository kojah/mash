"""Paired canonical/admitted unit adjacency using frozen local vectors, no inference."""
import argparse
import hashlib
import json
from pathlib import Path
import resource
import time
import numpy as np
from topic_stream import Source
from topic_stream_demo import pair_members
from topic_units import units,route,Adjacency


BASELINE='367d53bff53eb4186123992b3fbad84fd8ea900bce6829d701dfe58783125cc7'
VECTORS='31e2f6a9e61791e57535ebddf82172775b5f140bc6a63875f0164d5d44bbdc94'
PINS=['topic_adjacency_control.py','topic_units.py','topic_stream.py','topic_refresh.py','topic_stream_demo.py',
      'topic_jobs.py','data.py','episode_blocks.py','block_hierarchy.py','captured_hierarchy.py','subject_labels.py','label_noise.py','uv.lock']


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def stable_json(result):
    stable=dict(result);stable.pop('runtime')
    return json.dumps(stable,sort_keys=True,separators=(',',':'),allow_nan=False)


def experiment(baseline,vector_path):
    if digest(baseline)!=BASELINE or digest(vector_path)!=VECTORS:raise ValueError('frozen original source/vector controls required')
    report=json.loads(baseline.read_text());vectors=json.loads(vector_path.read_text())
    if vectors['baseline_sha256']!=BASELINE:raise ValueError('vector/source ownership differs')
    pins={name:digest(Path(__file__).with_name(name)) for name in PINS}
    order={p['id']:(p['thread'],p['ordinal'],p['original_message']) for p in report['source_provenance']}
    records=sorted(report['baseline_checkpoint']['records'],key=lambda r:order[r['id']])
    arms={};timings={};start=time.perf_counter();cpu=time.process_time()
    for representation,control in sorted(vectors['arms'].items()):
        if set(control['vectors'])!={r['id'] for r in records}:raise ValueError('vector source IDs differ')
        sources=[Source(r['id'],r['thread'],r['provider'],r['text'],None if control['vectors'][r['id']] is None
                        else np.array(control['vectors'][r['id']])) for r in records]
        for policy in Adjacency:
            begin=time.perf_counter();groups=units(sources,adjacency=policy)
            forward=route(groups);reverse=route(list(reversed(groups)))
            left=pair_members(forward);right=pair_members(reverse)
            name=representation+'/'+policy.value
            arms[name]={'units':[[s.id for s in g] for g in groups],'forward':forward,'reverse':reverse,
                        'summary':{'units':len(groups),'multi_turn_units':sum(len(g)>1 for g in groups),
                                   'maximum_turns':max(map(len,groups),default=0),
                                   'forward_pairs':len(left),'reverse_pairs':len(right),
                                   'pair_jaccard':len(left&right)/len(left|right) if left|right else None,
                                   'singleton_leaves':sum(n['kind']=='leaf' and len(n['sources'])==1 for n in forward['nodes']),
                                   'weak_leaves':sum(n['kind']=='leaf' and n['label_support']=='weak' for n in forward['nodes'])}}
            if policy is Adjacency.CANONICAL:
                if forward!=control['unit_forward'] or reverse!=control['unit_reverse']:
                    raise ValueError('default policy changed frozen baseline semantics')
            timings[name]=(time.perf_counter()-begin)*1000
    if pins!={name:digest(Path(__file__).with_name(name)) for name in PINS}:raise ValueError('code drift during experiment')
    return {'schema_version':1,'baseline_sha256':BASELINE,'vector_capture_sha256':VECTORS,'code_pins':pins,
            'policy':{'threshold':.55,'maximum_turns':8,'maximum_joined_bytes':8192,'new_inference':0,
                      'excluded':'only literal empty projection AND absent vector; retained separately, never embedded',
                      'arrival':'round-robin unit completion order; reverse formed units, not native formation'},
            'arms':arms,'semantic_quality_verified':False,'production_selected':False,
            'runtime':{'arms_ms':timings,'total_ms':(time.perf_counter()-start)*1000,
                       'cpu_ms':(time.process_time()-cpu)*1000,'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['baseline','vectors','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    result=experiment(args.baseline,args.vectors)
    if args.compare and stable_json(json.loads(args.compare.read_text()))!=stable_json(result):raise ValueError('stable outputs differ')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({name:{'summary':arm['summary'],'diagnostics':arm['forward']['diagnostics']} for name,arm in result['arms'].items()}))
