"""Exact-neighbor online average-link blocks; bounded exploratory quality control."""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import json
import math
from pathlib import Path
import resource
import time

import numpy as np
from scipy import sparse
from threadpoolctl import threadpool_limits

from block_hierarchy import evaluate
from captured_hierarchy import Topology
from discovery_inputs import read,digest
from episode_blocks import block_counts,lexical
from online_tree import Tree,Mode,Linkage
from qmsum_block_hierarchy import stored_tree
from qmsum_compressed_macro import REFERENCE
from qmsum_hierarchy import components
from qmsum_unique_blocks import GEOMETRY
from sparse_average_tree import SparseAverageTree


def fingerprint(v):
    if sparse.issparse(v):
        v=v.copy();v.sum_duplicates();v.eliminate_zeros();v.sort_indices()
        return {'shape':list(v.shape),'data_sha256':hashlib.sha256(v.data.astype('<f8').tobytes()).hexdigest(),
            'indices_sha256':hashlib.sha256(v.indices.astype('<i8').tobytes()).hexdigest(),
            'indptr_sha256':hashlib.sha256(v.indptr.astype('<i8').tobytes()).hexdigest()}
    return {'shape':list(v.shape),'data_sha256':hashlib.sha256(np.asarray(v,dtype='<f8').tobytes()).hexdigest()}


def numeric_storage(tree):
    # Numeric payload only, deduplicate aliased buffers; Python nodes excluded.
    ranges=[]
    for v in [tree.matrix,*[v for n in tree.nodes.values() for v in [n.total,n.link_vector]]]:
        candidates=[v.data,v.indices,v.indptr] if sparse.issparse(v) else [v]
        for a in candidates:
            if not a.flags.c_contiguous:raise ValueError('contiguous numeric buffers required')
            if a.size:
                start=a.__array_interface__['data'][0];ranges.append((start,start+a.nbytes))
    total=0;end=0
    for start,stop in sorted(ranges):
        total+=max(0,stop-max(start,end));end=max(end,stop)
    return total


def fit(vectors,view,mode,reverse):
    order=list(range(vectors.shape[0]));order=order[::-1] if reverse else order
    t=SparseAverageTree(mode) if view=='lexical' else Tree(mode,linkage=Linkage.AVERAGE)
    start=time.perf_counter();cpu=time.process_time();latencies=[]
    for i in order:
        begin=time.perf_counter();t.insert(str(i),vectors[i:i+1] if view=='lexical' else vectors[i]);latencies.append((time.perf_counter()-begin)*1000)
    times={'fit_ms':(time.perf_counter()-start)*1000,'fit_cpu_ms':(time.process_time()-cpu)*1000,'insert_ms':latencies}
    statistics=asdict(t.statistics);begin=time.perf_counter();t.validate();times['validation_ms']=(time.perf_counter()-begin)*1000
    begin=time.perf_counter();tree=Topology(t.root.serial,{n.serial:tuple(c.serial for c in n.children) for n in t.nodes.values() if n.children},
        {n.serial:order[n.leaf] for n in t.leaves})
    nodes=[{'serial':n.serial,'count':n.count,'total':fingerprint(n.total),'mean':fingerprint(n.link_vector)} for n in sorted(t.nodes.values(),key=lambda n:n.serial)]
    state={'order':order,'identities':t.identities,'statistics':statistics,'nodes':nodes,'numeric_payload_bytes':numeric_storage(t)}
    times['snapshot_ms']=(time.perf_counter()-begin)*1000
    return tree,state,times


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
            lo=row['block_directions_offset'];dense=directions[lo:lo+len(blocks)];features_ms=(time.perf_counter()-begin)*1000
            for view,vectors in [('lexical',lex),('dense',dense)]:
                for mode in Mode:
                    for order,reverse in [('forward',False),('reverse',True)]:
                        tree,state,t=fit(vectors,view,mode,reverse);begin=time.perf_counter()
                        metrics=evaluate(tree,owned,gold,parts,selected,len(meeting.topics));t['evaluation_ms']=(time.perf_counter()-begin)*1000
                        timings.append({'meeting':meeting.ordinal,'view':view,'mode':mode,'order':order,
                            'features_ms':features_ms if view=='lexical' and mode==Mode.GREEDY and not reverse else 0.,**t})
                        records.append({'meeting':meeting.ordinal,'split':row['split'],'view':view,'mode':mode,'order':order,'blocks':len(blocks),
                            'state':state,'tree':stored_tree(tree),'metrics':metrics})
            support.update(sources=size,ready=len(selected),known=sum(g is not None for g in gold),blocks=len(blocks))
            if meeting.ordinal%10==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support!={'sources':88336,'ready':84434,'known':77892,'blocks':8196}:raise ValueError('complete support required')
    summaries={}
    for split in ['all','development','reserved_internal']:
        summaries[split]={}
        for view in ['lexical','dense']:
            summaries[split][view]={}
            for mode in Mode:
                summaries[split][view][mode]={}
                for order in ['forward','reverse']:
                    rows=[r for r in records if r['view']==view and r['mode']==mode and r['order']==order and (split=='all' or r['split']==split)]
                    entry={}
                    for scope in ['all','far','crossspan']:
                        values=[r['metrics'][scope] for r in rows];pairs=sum(v['pairs'] for v in values);weight=math.fsum(v['purity_sum'] for v in values)
                        base=reference['summary'][split]['methods']['lexical_average'][scope]['purity']
                        entry[scope]={'pairs':pairs,'purity_sum':weight,'purity':weight/pairs if pairs else None,
                            'loss_vs_lexical_average':base-weight/pairs if pairs else None}
                    entry['numeric_payload_bytes_for_retaining_all_permeeting_trees']=sum(r['state']['numeric_payload_bytes'] for r in rows)
                    summaries[split][view][mode][order]=entry
    return {'schema_version':1,'scope':'source_preserving_exact_online_average_block_control_not_production_selection',
        'source_sha256':data.source,'source_capture_sha256':data.capture,'geometry_sha256':GEOMETRY,'macro_reference_sha256':REFERENCE,
        'support':dict(support),'summary':summaries,'records':records,
        'protocol':{'contract':'SPARSE_AVERAGE_PROTOCOL.md','new_inference':0,'llm_calls':0,'heldout_read':False,'full_ingestion_gate':False,
            'production_selection':False,'exact_neighbors':True,'operation_caps':False,'global_online_frame':False,'author_GRINCH':False},
        'code_pins':{n:digest(Path(__file__).with_name(n)) for n in ['qmsum_sparse_average.py','sparse_average_tree.py','SPARSE_AVERAGE_PROTOCOL.md',
            'online_tree.py','data.py','discovery_inputs.py','episode_blocks.py','block_hierarchy.py','captured_hierarchy.py','qmsum_hierarchy.py','qmsum.py','uv.lock']},
        'timings':{'setup_ms':setup_ms,'after_imports_ms':(time.perf_counter()-started)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'stages':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['source','vectors','units','labels','baseline','reference-manifest','geometry','macro-reference','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a)
    if a.compare:
        old=json.loads(a.compare.read_text())
        if json.dumps({k:v for k,v in old.items() if k!='timings'},sort_keys=True,allow_nan=False)!=json.dumps({k:v for k,v in result.items() if k!='timings'},sort_keys=True,allow_nan=False):raise ValueError('complete repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary']['all'],'support':result['support']}),flush=True)


if __name__=='__main__':main()
