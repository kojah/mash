"""Frozen dense/lexical/half-fusion flat diagnostic; no model calls or tuning."""
import argparse
from dataclasses import dataclass
import itertools
import json
from pathlib import Path
import resource
import time
import numpy as np
from topic_cohort_demo import load,digest,PINS
from topic_features import lexical_counts
from episode_blocks import lexical
from topic_flat import flat_snapshot
from topic_kernel import route,combine
from topic_stream import Source


@dataclass(frozen=True)
class Evidence:
    source: Source
    version: int


def stable(result):
    value=dict(result);value.pop('runtime')
    return json.dumps(value,sort_keys=True,allow_nan=False,separators=(',',':'))


def experiment(database,selection,baseline):
    names=PINS+['topic_flat.py','topic_kernel.py','topic_kernel_control.py']
    pins={name:digest(Path(__file__).with_name(name)) for name in names}
    baseline_sha=digest(baseline);old=json.loads(baseline.read_text())['arms']['flat_final']
    sources,_,_,_,ledger=load(database,selection);original_order=[s.id for s in sources]
    sources=sorted(sources,key=lambda source:source.id);position={s.id:i for i,s in enumerate(sources)}
    order=[position[identity] for identity in original_order]
    evidence=tuple(Evidence(source,i+1) for i,source in enumerate(sources));ready=list(range(len(sources)))
    start=time.perf_counter();cpu=time.process_time()
    vocabulary,counts,df,idf=lexical_counts(evidence,ready)
    features=lexical(counts,idf).tocsr();dense=np.stack([s.vector for s in sources])
    dense_kernel=dense@dense.T;lexical_kernel=(features@features.T).toarray()
    kernels={'dense':dense_kernel,'lexical':lexical_kernel,'half_fusion':combine(dense_kernel,lexical_kernel)}
    setup=(time.perf_counter()-start)*1000;arms={};timings={};dense_score_error=0.
    for name,kernel in kernels.items():
        arm={};timing={}
        for direction,arrival in [('forward',order),('reverse',list(reversed(order)))]:
            begin=time.perf_counter();result=route(kernel,arrival);route_ms=(time.perf_counter()-begin)*1000
            versions={i:step+1 for step,i in enumerate(arrival)}
            records=tuple(Evidence(s,versions[i]) for i,s in enumerate(sources))
            memberships={s.id:[{'topic':'leaf-'+str(j),'score':score} for j,score in result.memberships[i]] for i,s in enumerate(sources)}
            source_sets=[set() for _ in range(result.topic_count)]
            for i,tags in enumerate(result.memberships):
                for j,_ in tags:source_sets[j].add(i)
            snapshot=flat_snapshot(records,len(sources),tuple(range(result.topic_count)),ready,memberships,source_sets)
            arm[direction]=snapshot;arm[direction+'_unroutable']=result.unroutable
            timing[direction]={'route_ms':route_ms,'through_snapshot_ms':(time.perf_counter()-begin)*1000}
            if name=='dense':
                expected=old[direction]
                for sid,tags in memberships.items():
                    prior=expected['memberships'][sid]
                    if [t['topic'] for t in tags]!=[t['topic'] for t in prior]:raise ValueError('dense Gram decisions differ from original vector control')
                    for left,right in zip(tags,prior):
                        error=abs(left['score']-right['score']);dense_score_error=max(dense_score_error,error)
                        if error>1e-12:raise ValueError('dense Gram score exceeds tolerance')
                for field in ['versions','topics','threads','diagnostics']:
                    if snapshot[field]!=expected[field]:raise ValueError('dense control changed source/labels/thread semantics')
        pairs=[{pair for topic in arm[d]['topics'] for pair in itertools.combinations(sorted(topic['sources']),2)} for d in ['forward','reverse']]
        arm['order']={'forward_pairs':len(pairs[0]),'reverse_pairs':len(pairs[1]),
                      'pair_jaccard':len(pairs[0]&pairs[1])/len(pairs[0]|pairs[1]) if pairs[0]|pairs[1] else None}
        arms[name]=arm;timings[name]=timing
        print(json.dumps({'completed_case':name,'forward':arm['forward']['diagnostics'],'unroutable':arm['forward_unroutable'],'order':arm['order']}),flush=True)
    if pins!={name:digest(Path(__file__).with_name(name)) for name in names} or digest(baseline)!=baseline_sha:
        raise ValueError('code/baseline drift')
    return {'schema_version':1,'scope':'fixed_corpus_lexical_geometry_diagnostic_not_online_or_quality_selection',
        'code_pins':pins,'database_sha256':digest(database),'selection_sha256':digest(selection),'baseline_sha256':baseline_sha,
        'source_ledger_sha256':ledger,'policy':{'threshold':.55,'relative':.8,'maximum_tags':3,'lexical_weight':.5,
            'lexical_frame':'all admitted sources, corpus-fitted vocabulary/IDF; transductive development diagnostic',
            'maximum_vocabulary':len(vocabulary),'source_occurrences':len(sources),'new_inference':0},
        'dense_control_decisions_exact':True,'maximum_dense_score_error':dense_score_error,'arms':arms,
        'semantic_quality_verified':False,'production_selected':False,
        'runtime':{'setup_ms':setup,'arms':timings,'total_ms_after_load':(time.perf_counter()-start)*1000,
                   'cpu_ms_after_load':(time.process_time()-cpu)*1000,
                   'rusage_before_serialization_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['database','selection','baseline','output']:parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    result=experiment(args.database,args.selection,args.baseline)
    if args.compare and stable(json.loads(args.compare.read_text()))!=stable(result):raise ValueError('kernel repeat differs')
    with args.output.open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    args.output.chmod(0o600)
    print(json.dumps({'stable_complete':True,'total_ms':result['runtime']['total_ms_after_load']}),flush=True)
