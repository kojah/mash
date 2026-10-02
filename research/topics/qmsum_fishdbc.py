"""Source-preserving author FISHDBC/exact mutual-reachability quality control."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import resource
import time

import numpy as np
from threadpoolctl import threadpool_limits

from block_hierarchy import evaluate
from discovery_inputs import read,digest
from episode_blocks import block_counts,lexical
from fishdbc_reference import run,topology,MANIFEST
from qmsum_block_hierarchy import stored_tree
from qmsum_compressed_macro import REFERENCE
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY


def experiment(a):
    started=time.perf_counter();cpu=time.process_time()
    if digest(a.geometry/'results.json')!=GEOMETRY or digest(a.macro_reference)!=REFERENCE:raise ValueError('immutable source references required')
    original=json.loads((a.geometry/'results.json').read_text());reference=json.loads(a.macro_reference.read_text());data=read(a)
    for capture in [original,reference]:
        if capture['source_sha256']!=data.source:raise ValueError('source mismatch')
        for name,pin in capture['code_pins'].items():
            if digest(Path(__file__).with_name(name))!=pin:raise ValueError('original code drift')
    if digest(a.geometry/'block-directions.npy')!=original['files']['block-directions.npy']['sha256']:raise ValueError('vector drift')
    directions=np.load(a.geometry/'block-directions.npy',mmap_mode='r',allow_pickle=False)
    setup_ms=(time.perf_counter()-started)*1000;records=[];timings=[];support=Counter()
    with threadpool_limits(limits=1):
        for meeting,row in zip(data.meetings,original['records']):
            offset=row['source_offset'];size=len(meeting.turns);blocks=row['blocks'];owner=np.full(size,-1,dtype=np.int64)
            for b in blocks:owner[b['start']:b['end']+1]=b['ordinal']
            if meeting.ordinal!=row['meeting'] or not np.array_equal(owner>=0,data.ready[offset:offset+size]):raise ValueError('source ownership mismatch')
            selected=np.flatnonzero(owner>=0).tolist();gold,parts=components(meeting,selected);owned=owner[selected].tolist()
            begin=time.perf_counter();lex=lexical(block_counts(data.counts[offset:offset+size],owner,len(blocks)),data.idf)
            lo=row['block_directions_offset'];dense=directions[lo:lo+len(blocks)]
            cosines={'lexical':(lex@lex.T).toarray(),'dense':dense@dense.T};features_ms=(time.perf_counter()-begin)*1000
            for view,cosine in cosines.items():
                # Symmetric products are made exact before author/control sharing.
                distance=1-np.clip((cosine+cosine.T)/2,-1,1);np.fill_diagonal(distance,0)
                for order,reverse in [('forward',False),('reverse',True)]:
                    result,t=run(distance,a.fishdbc_reference,reverse)
                    begin=time.perf_counter();metrics={};trees={}
                    for name,merges in [('author',result['prefixes'][-1]['merges']),('exact',result['exact_merges'])]:
                        tree=topology(merges,result['order']);trees[name]=stored_tree(tree)
                        metrics[name]=evaluate(tree,owned,gold,parts,selected,len(meeting.topics))
                    t['evaluation_ms']=(time.perf_counter()-begin)*1000
                    timings.append({'meeting':meeting.ordinal,'view':view,'order':order,'features_ms':features_ms if view=='lexical' and not reverse else 0.,**t})
                    records.append({'meeting':meeting.ordinal,'split':row['split'],'view':view,'order':order,'blocks':len(blocks),
                        'distance_sha256':hashlib.sha256(distance.astype('<f8').tobytes()).hexdigest(),
                        'result':result,'trees':trees,'metrics':metrics})
            support.update(sources=size,ready=len(selected),known=sum(g is not None for g in gold),blocks=len(blocks))
            if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('complete support required')
    summaries={}
    for split in ['all','development','reserved_internal']:
        summaries[split]={}
        for view in ['lexical','dense']:
            summaries[split][view]={}
            for order in ['forward','reverse']:
                rows=[r for r in records if r['view']==view and r['order']==order and (split=='all' or r['split']==split)]
                summary={}
                for method in ['author','exact']:
                    summary[method]={}
                    for scope in ['all','far','crossspan']:
                        values=[r['metrics'][method][scope] for r in rows];pairs=sum(v['pairs'] for v in values);weight=math.fsum(v['purity_sum'] for v in values)
                        base=reference['summary'][split]['methods']['lexical_average'][scope]['purity']
                        summary[method][scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,
                            'loss_vs_lexical_average':base-weight/pairs if pairs else None}
                summary.update(distance_evaluations=sum(r['result']['distance_evaluations'] for r in rows),
                    final_noise_blocks=sum(sum(x==-1 for x in r['result']['prefixes'][-1]['labels']) for r in rows),
                    blocks=sum(r['blocks'] for r in rows),maximum_core_excess=max(r['result']['maximum_core_excess'] for r in rows),
                    mst_weight_excess_sum=math.fsum(r['result']['mst_weight_excess'] for r in rows))
                summaries[split][view][order]=summary
    return {'schema_version':1,'scope':'source_preserving_offline_distance_author_FISHDBC_not_production_selection',
        'source_sha256':data.source,'source_capture_sha256':data.capture,'geometry_sha256':GEOMETRY,'macro_reference_sha256':REFERENCE,
        'fishdbc_reference_sha256':MANIFEST,'support':dict(support),'summary':summaries,'records':records,
        'protocol':{'contract':'FISHDBC_PROTOCOL.md','new_inference':0,'llm_calls':0,'heldout_read':False,'full_ingestion_gate':False,
            'production_selection':False,'distance_precomputed':True,'exact_control_shared_hdbscan_label':True},
        'code_pins':{n:digest(Path(__file__).with_name(n)) for n in ['qmsum_fishdbc.py','fishdbc_reference.py','FISHDBC_PROTOCOL.md',
            'discovery_inputs.py','episode_blocks.py','block_hierarchy.py','captured_hierarchy.py','qmsum_hierarchy.py','qmsum.py','uv.lock']},
        'timings':{'setup_ms':setup_ms,'after_imports_ms':(time.perf_counter()-started)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['source','vectors','units','labels','baseline','reference-manifest','geometry','macro-reference','fishdbc-reference','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if json.dumps({k:v for k,v in old.items() if k!='timings'},sort_keys=True,allow_nan=False)!=json.dumps({k:v for k,v in result.items() if k!='timings'},sort_keys=True,allow_nan=False):raise ValueError('complete repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary']['all'],'support':result['support']}),flush=True)


if __name__=='__main__':main()
