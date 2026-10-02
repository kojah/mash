"""Same average-linkage discovery, different immutable source evidence units."""
import argparse
from collections import Counter
from dataclasses import asdict
import json
import math
from pathlib import Path
import resource
import time
from types import SimpleNamespace
import numpy as np
from block_hierarchy import average
from discovery_inputs import read,digest
from matched_unit_hierarchy import evaluate
from qmsum_block_hierarchy import stored_tree
from qmsum_compressed_macro import REFERENCE
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY
from semantic_units import aggregate,Status


PINS=['qmsum_unit_granularity.py','UNIT_GRANULARITY_PROTOCOL.md','matched_unit_hierarchy.py','block_hierarchy.py',
    'captured_hierarchy.py','discovery_inputs.py','qmsum.py','qmsum_hierarchy.py','qmsum_block_hierarchy.py',
    'qmsum_compressed_macro.py','qmsum_unique_blocks.py','semantic_units.py','uv.lock']


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINS}
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    gp=folder('block-geometry-first')/'results.json';rp=root/'topic-qmsum-block-hierarchy-final-first-20261002.json'
    if digest(gp)!=GEOMETRY or digest(rp)!=REFERENCE:raise ValueError('immutable source geometry/reference required')
    geometry=json.loads(gp.read_text());reference=json.loads(rp.read_text());vp=gp.parent/'block-directions.npy'
    for n,pin in geometry['code_pins'].items():
        if digest(Path(__file__).with_name(n))!=pin:raise ValueError('geometry implementation drift')
    if digest(vp)!=geometry['files'][vp.name]['sha256']:raise ValueError('block vector drift')
    pooled=np.load(vp,mmap_mode='r',allow_pickle=False)
    data=read(SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json'))
    if any(x['source_sha256']!=data.source for x in [geometry,reference]):raise ValueError('source drift')
    setup=(time.perf_counter()-start)*1000;records=[];timings=[];lineage=[];support=Counter()
    for meeting,g in zip(data.meetings,geometry['records']):
        offset=g['source_offset'];size=len(meeting.turns);owner=np.full(size,-1,dtype=np.int64)
        for b in g['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        ready=data.ready[offset:offset+size];raw=data.raw[offset:offset+size]
        if not np.array_equal(owner>=0,ready):raise ValueError('common source drift')
        positions=np.flatnonzero(ready).tolist();gold,parts=components(meeting,positions);physical=owner[positions].tolist()
        begin=time.perf_counter();context,units=aggregate(raw,ready,data.tokens[offset:offset+size]);aggregation_ms=(time.perf_counter()-begin)*1000
        if any(u.status!=Status.READY for u in units if ready[u.ordinal]):raise ValueError('context cancellation changes common support')
        original={'meeting':meeting.ordinal,'source_offset':offset,'units':[asdict(u) for u in units]}
        if original!=data.lineage[meeting.ordinal]:raise ValueError('original context lineage drift')
        lineage.append(original);lo=g['block_directions_offset'];n=len(g['blocks'])
        for name,vectors,owners in [('raw_message',raw[positions],list(range(len(positions)))),
                                    ('causal_context',context[positions],list(range(len(positions)))),
                                    ('physical_block',pooled[lo:lo+n],physical)]:
            begin=time.perf_counter();tree,merges=average(vectors@vectors.T);fit_ms=(time.perf_counter()-begin)*1000
            begin=time.perf_counter();metrics=evaluate(tree,owners,physical,gold,parts,positions);evaluation_ms=(time.perf_counter()-begin)*1000
            if name=='physical_block':
                old=next(r for r in reference['records'] if r['meeting']==meeting.ordinal and r['method']=='dense_average')
                if stored_tree(tree)!=old['tree'] or merges!=old['merges']:raise ValueError('original block topology replay differs')
                for scope in metrics:
                    if metrics[scope]['pairs']!=old['metrics'][scope]['pairs'] or not math.isclose(metrics[scope]['purity_sum'],old['metrics'][scope]['purity_sum'],rel_tol=1e-12,abs_tol=1e-12):raise ValueError('original block metric replay differs')
            records.append({'meeting':meeting.ordinal,'representation':name,'source_ordinals':positions,'discovery_owners':owners,
                'original_physical_owners':physical,'tree':stored_tree(tree),'merges':merges,'metrics':metrics})
            timings.append({'meeting':meeting.ordinal,'representation':name,'aggregation_ms':aggregation_ms if name=='causal_context' else 0.,'fit_ms':fit_ms,'evaluation_ms':evaluation_ms})
        support.update(sources=size,ready=len(positions),known=sum(g is not None for g in gold),blocks=n)
        if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('original source support changed')
    summary={}
    for name in ['raw_message','causal_context','physical_block']:
        summary[name]={}
        for scope,expected in [('all',7978214),('far',7923483),('crossspan',84323)]:
            values=[r['metrics'][scope] for r in records if r['representation']==name];pairs=sum(v['pairs'] for v in values);weight=math.fsum(v['purity_sum'] for v in values)
            if pairs!=expected:raise ValueError('matched positive denominator changed')
            summary[name][scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs}
    if pins!={n:digest(Path(__file__).with_name(n)) for n in PINS}:raise ValueError('source changed during experiment')
    return {'schema_version':1,'scope':'offline_matched_dense_unit_granularity_not_production_selection','source_sha256':data.source,'capture_sha256':data.capture,
        'geometry_sha256':GEOMETRY,'reference_sha256':REFERENCE,'block_vectors_sha256':geometry['files'][vp.name]['sha256'],
        'support':dict(support),'summary':summary,'records':records,'context_lineage':lineage,'code_pins':pins,
        'protocol':{'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False,'full_ingestion_gate':False,'labels_evaluated':False},
        'timings':{'setup_ms':setup,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a.artifacts)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if {k:v for k,v in old.items() if k!='timings'}!={k:v for k,v in result.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary'],'support':result['support']}),flush=True)


if __name__=='__main__':main()
