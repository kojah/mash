"""Paired label/full-topic/direct lexical retrieval on native grounded queries."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import resource
import time
from types import SimpleNamespace
import numpy as np
from scipy import sparse
from discovery_inputs import read,digest
from episode_blocks import block_counts,lexical
from label_noise import EXCLUDED
from query_navigation import Arm,query,retrieve,metrics


CONTROL='c32f7abd20ff9950097a4ad3f2d95cf23839cf91f7cab1e19ba673eb6b04e5e9'
PINS=['qmsum_query_navigation.py','QUERY_NAVIGATION_PROTOCOL.md','query_navigation.py','discovery_inputs.py','episode_blocks.py',
    'label_noise.py','stable_lexical_frame.py','qmsum.py','qmsum_capture.py','stage_parity.py','tiage_capture.py','uv.lock']


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINS}
    cp=root/'topic-qmsum-source-support-first-20261002.json'
    if digest(cp)!=CONTROL:raise ValueError('frozen group/label control required')
    control=json.loads(cp.read_text())
    for name,pin in control['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('frozen label implementation drift')
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    data=read(SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json'))
    gp=folder('block-geometry-first')/'results.json'
    if data.source!=control['source_sha256'] or digest(gp)!=control['geometry_sha256']:raise ValueError('source/geometry drift')
    geometry=json.loads(gp.read_text());owners=np.full(len(data.ready),-1,dtype=np.int64);source_meeting=np.full(len(owners),-1,dtype=np.int64)
    groups=[];labels={Arm.BLOCK:[],Arm.MESSAGE:[]};group_meeting=[]
    for meeting,g,r in zip(data.meetings,geometry['records'],control['records']):
        assert meeting.ordinal==g['meeting']==r['meeting']
        offset=g['source_offset'];size=len(meeting.turns);source_meeting[offset:offset+size]=meeting.ordinal
        block_group={b:len(groups)+i for i,group in enumerate(r['groups']) for b in group}
        for b in g['blocks']:owners[offset+b['start']:offset+b['end']+1]=block_group[b['ordinal']]
        for i,group in enumerate(r['groups']):
            groups.append({'id':len(groups),'meeting':meeting.ordinal,'blocks':group});group_meeting.append(meeting.ordinal)
            for arm,name in [(Arm.BLOCK,'physical_block'),(Arm.MESSAGE,'original_message')]:labels[arm].append(r['labels'][name][i])
    if not np.array_equal(owners>=0,data.ready) or len(groups)!=1296 or (source_meeting<0).any():raise ValueError('full source/group ownership required')
    positions={t:i for i,t in enumerate(data.terms)};begin=time.perf_counter()
    clean=data.counts.multiply([t not in EXCLUDED for t in data.terms]).tocsr();clean.eliminate_zeros()
    source_vectors=lexical(clean,data.idf).tocsr();source_vectors.sort_indices()
    group_counts=block_counts(clean,owners,len(groups));group_vectors=lexical(group_counts,data.idf).tocsr();group_vectors.sort_indices()
    label_vectors={}
    for arm,rows in labels.items():
        ii=[];jj=[]
        for i,row in enumerate(rows):
            for term in row:ii.append(i);jj.append(positions[term['term']])
        bank=sparse.csr_matrix((np.ones(len(ii)),(ii,jj)),shape=(len(groups),len(positions)))
        label_vectors[arm]=lexical(bank,data.idf).tocsr();label_vectors[arm].sort_indices()
    features_ms=(time.perf_counter()-begin)*1000;setup_ms=(time.perf_counter()-start)*1000
    records=[];times=[];summary={scope:{arm:Counter() for arm in Arm} for scope in ['meeting','global']};support=Counter()
    ready_ids=np.flatnonzero(data.ready);group_ids=np.arange(len(groups),dtype=np.int64);group_meeting=np.array(group_meeting)
    for meeting,g in zip(data.meetings,geometry['records']):
        offset=g['source_offset'];scopes={'meeting':(ready_ids[source_meeting[ready_ids]==meeting.ordinal],group_ids[group_meeting==meeting.ordinal]),'global':(ready_ids,group_ids)}
        for q in meeting.specific_queries:
            begin=time.perf_counter();vector,metadata=query(q.query,positions,data.idf);vector.sort_indices()
            source_scores=(source_vectors@vector.T).toarray().ravel();group_scores={Arm.GROUP:(group_vectors@vector.T).toarray().ravel()}
            group_scores.update({arm:(v@vector.T).toarray().ravel() for arm,v in label_vectors.items()});encode_ms=(time.perf_counter()-begin)*1000
            wanted=sorted({offset+i for span in q.spans for i in range(span.start,span.end+1)});cases={};rank_ms={}
            # Original relevant spans enter only after source/topic features and query scores are fixed.
            for scope,(ids,gids) in scopes.items():
                cases[scope]={};rank_ms[scope]={}
                for arm in Arm:
                    begin=time.perf_counter();scores=group_scores.get(arm,np.zeros(len(groups)))
                    result=retrieve(source_scores[ids],ids,owners[ids],scores[gids],gids,arm)
                    selected=[v['id'] for v in result['selected_groups']]
                    candidates=ids if arm==Arm.DIRECT else ids[np.isin(owners[ids],selected)]
                    result['metrics']=metrics(result,wanted,candidates.tolist());cases[scope][arm]=result
                    rank_ms[scope][arm]=(time.perf_counter()-begin)*1000;summary[scope][arm].update(result['metrics'])
            records.append({'meeting':meeting.ordinal,'query':q.ordinal,'query_sha256':hashlib.sha256(q.query.encode()).hexdigest(),'query_terms':metadata,
                'spans':[[s.start,s.end,s.original_start,s.original_end] for s in q.spans],'relevant_sources':wanted,
                'ready_relevant_sources':int(data.ready[wanted].sum()),'cases':cases})
            times.append({'meeting':meeting.ordinal,'query':q.ordinal,'query_projection_ms':encode_ms,'ranking_and_metrics_ms':rank_ms})
            support.update(queries=1,spans=len(q.spans),relevant_source_contributions=len(wanted),ready_relevant_source_contributions=int(data.ready[wanted].sum()),
                zero_known_term_queries=int(metadata['known_terms']==0))
        if meeting.ordinal%20==0:print(json.dumps({'processed_meetings':meeting.ordinal+1}),flush=True)
    if support['queries']!=1095 or support['spans']!=1206:raise ValueError('complete query denominators required')
    for scope,arms in summary.items():
        for arm,m in arms.items():summary[scope][arm]={'totals':dict(m),'macro':{k:m[k]/support['queries'] for k in ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    if pins!={n:digest(Path(__file__).with_name(n)) for n in PINS} or digest(cp)!=CONTROL:raise ValueError('source changed during experiment')
    return {'schema_version':1,'scope':'grounded_query_navigation_training_not_mash_search_or_semantic_quality','source_sha256':data.source,'capture_sha256':data.capture,
        'control_sha256':CONTROL,'groups':groups,'support':dict(support)|{'sources':len(owners),'ready_sources':len(ready_ids),'groups':len(groups)},
        'summary':summary,'records':records,'code_pins':pins,'protocol':{'top_groups':2,'top_sources':20,'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False},
        'timings':{'setup_ms':setup_ms,'features_ms':features_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'queries':times}}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a.artifacts)
    if a.compare and {k:v for k,v in json.loads(a.compare.read_text()).items() if k!='timings'}!={k:v for k,v in result.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary'],'support':result['support']}),flush=True)


if __name__=='__main__':main()
