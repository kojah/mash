"""Frozen lexical granularity comparison; unchanged dense arms remain controls."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import resource
import time
from types import SimpleNamespace
import numpy as np
from block_hierarchy import average
from discovery_inputs import read,digest
from episode_blocks import block_counts,lexical
from matched_unit_hierarchy import evaluate
from qmsum_block_hierarchy import stored_tree
from qmsum_compressed_macro import REFERENCE
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY
from semantic_units import Unit,Status,Limit
from unit_source_counts import pool


DENSE='61e11897f620da4215e5f2305e6605919d1a0e86fe774826d1e8a4598af49294'
PINS=['qmsum_lexical_granularity.py','LEXICAL_GRANULARITY_PROTOCOL.md','unit_source_counts.py',
    'matched_unit_hierarchy.py','block_hierarchy.py','captured_hierarchy.py','discovery_inputs.py','episode_blocks.py',
    'semantic_units.py','qmsum.py','qmsum_hierarchy.py','qmsum_block_hierarchy.py','qmsum_unique_blocks.py','uv.lock']


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINS}
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    gp=folder('block-geometry-first')/'results.json';rp=root/'topic-qmsum-block-hierarchy-final-first-20261002.json';dp=root/'topic-qmsum-unit-granularity-first-20261002.json'
    if digest(gp)!=GEOMETRY or digest(rp)!=REFERENCE or digest(dp)!=DENSE:raise ValueError('immutable source/control captures required')
    geometry=json.loads(gp.read_text());reference=json.loads(rp.read_text());dense=json.loads(dp.read_text())
    data=read(SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json'))
    if any(x['source_sha256']!=data.source for x in [geometry,reference,dense]):raise ValueError('original source drift')
    setup=(time.perf_counter()-start)*1000;records=[];timings=[];support=Counter()
    for meeting,g in zip(data.meetings,geometry['records']):
        offset=g['source_offset'];size=len(meeting.turns);owner=np.full(size,-1,dtype=np.int64)
        for b in g['blocks']:owner[b['start']:b['end']+1]=b['ordinal']
        if not np.array_equal(owner>=0,data.ready[offset:offset+size]):raise ValueError('inherited ready source drift')
        positions=np.flatnonzero(owner>=0).tolist();gold,parts=components(meeting,positions);physical=owner[positions].tolist();n=len(g['blocks'])
        original=data.lineage[meeting.ordinal]
        if original!=dense['context_lineage'][meeting.ordinal]:raise ValueError('frozen context boundaries differ')
        units=[Unit(u['ordinal'],u['start'],u['tokens'],Status(u['status']),Limit(u['limit'])) for u in original['units']]
        source_counts=data.counts[offset:offset+size];begin=time.perf_counter();context=pool(source_counts,units);blocks=block_counts(source_counts,owner,n);pool_ms=(time.perf_counter()-begin)*1000
        for name,counts,owners in [('raw_message',source_counts[positions],list(range(len(positions)))),
                                  ('causal_context',context[positions],list(range(len(positions)))),('physical_block',blocks,physical)]:
            begin=time.perf_counter();v=lexical(counts,data.idf).tocsr();features_ms=(time.perf_counter()-begin)*1000
            zero=int((v.getnnz(axis=1)==0).sum());begin=time.perf_counter();tree,merges=average((v@v.T).toarray());fit_ms=(time.perf_counter()-begin)*1000
            begin=time.perf_counter();metrics=evaluate(tree,owners,physical,gold,parts,positions);evaluation_ms=(time.perf_counter()-begin)*1000
            if name=='physical_block':
                old=next(r for r in reference['records'] if r['meeting']==meeting.ordinal and r['method']=='lexical_average')
                if stored_tree(tree)!=old['tree'] or merges!=old['merges']:raise ValueError('original lexical block topology/merge replay differs')
                for scope in metrics:
                    if metrics[scope]['pairs']!=old['metrics'][scope]['pairs'] or not math.isclose(metrics[scope]['purity_sum'],old['metrics'][scope]['purity_sum'],rel_tol=1e-12,abs_tol=1e-12):raise ValueError('original lexical block metrics differ')
            records.append({'meeting':meeting.ordinal,'representation':name,'source_ordinals':positions,'discovery_owners':owners,
                'original_physical_owners':physical,'zero_lexical_units':zero,'feature_nnz':v.nnz,'tree':stored_tree(tree),'merges':merges,'metrics':metrics})
            timings.append({'meeting':meeting.ordinal,'representation':name,'pool_ms':pool_ms if name=='causal_context' else 0.,
                'features_ms':features_ms,'fit_ms':fit_ms,'evaluation_ms':evaluation_ms})
        support.update(sources=size,ready=len(positions),known=sum(x is not None for x in gold),blocks=n)
        if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('full original support changed')
    summary={}
    for name in ['raw_message','causal_context','physical_block']:
        rows=[r for r in records if r['representation']==name];summary[name]={'zero_lexical_units':sum(r['zero_lexical_units'] for r in rows),'feature_nnz':sum(r['feature_nnz'] for r in rows),'scopes':{}}
        for scope,expected in [('all',7978214),('far',7923483),('crossspan',84323)]:
            values=[r['metrics'][scope] for r in rows];pairs=sum(v['pairs'] for v in values);weight=math.fsum(v['purity_sum'] for v in values)
            if pairs!=expected:raise ValueError('matched source-pair support changed')
            summary[name]['scopes'][scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs,'dense_reference':dense['summary'][name][scope]['purity']}
    if pins!={n:digest(Path(__file__).with_name(n)) for n in PINS}:raise ValueError('source changed during experiment')
    return {'schema_version':1,'scope':'offline_matched_lexical_granularity_not_production_selection','source_sha256':data.source,'capture_sha256':data.capture,
        'geometry_sha256':GEOMETRY,'reference_sha256':REFERENCE,'dense_sha256':DENSE,'support':dict(support),'summary':summary,'records':records,'code_pins':pins,
        'protocol':{'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False,'labels_evaluated':False,
            'full_ingestion_gate':False,'idf':'original_full_training_snapshot_not_causal_online'},
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
