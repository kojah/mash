"""Fixed parent-rank bonus, preserving all direct source eligibility."""
import argparse
from collections import Counter
import json
from pathlib import Path
import resource
import time
from types import SimpleNamespace
import numpy as np
from discovery_inputs import read,digest
from navigation_features import build
from query_navigation import query,metrics,rank
from soft_topic_guidance import retrieve


CONTROL='096ec1a15eebb5b0adbe43656075e89b2a2776daf2c1df0b1e54befe126d56e6'
PINS=['qmsum_soft_topic_guidance.py','SOFT_TOPIC_GUIDANCE_PROTOCOL.md','soft_topic_guidance.py','navigation_features.py',
    'query_navigation.py','discovery_inputs.py','episode_blocks.py','label_noise.py','stable_lexical_frame.py','qmsum.py','uv.lock']


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINS}
    cp=root/'topic-qmsum-query-navigation-first-20261002.json'
    if digest(cp)!=CONTROL:raise ValueError('frozen query/navigation control required')
    control=json.loads(cp.read_text())
    for name,pin in control['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('navigation control code drift')
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    data=read(SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json'))
    if data.source!=control['source_sha256']:raise ValueError('source drift')
    previous=json.loads((root/'topic-qmsum-source-support-first-20261002.json').read_text());gp=folder('block-geometry-first')/'results.json'
    if digest(root/'topic-qmsum-source-support-first-20261002.json')!=control['control_sha256'] or digest(gp)!=previous['geometry_sha256']:raise ValueError('ownership controls drift')
    geometry=json.loads(gp.read_text());groups=control['groups'];owners=np.full(len(data.ready),-1,dtype=np.int64);source_meeting=np.full(len(owners),-1,dtype=np.int64)
    for meeting,g,r in zip(data.meetings,geometry['records'],previous['records']):
        offset=g['source_offset'];source_meeting[offset:offset+len(meeting.turns)]=meeting.ordinal
        gids=[v['id'] for v in groups if v['meeting']==meeting.ordinal]
        for gid,blocks in zip(gids,r['groups']):
            for b in g['blocks']:
                if b['ordinal'] in blocks:owners[offset+b['start']:offset+b['end']+1]=gid
    if not np.array_equal(owners>=0,data.ready) or (source_meeting<0).any():raise ValueError('complete frozen ownership required')
    begin=time.perf_counter();features=build(data.counts,data.idf,data.terms,owners,len(groups));feature_ms=(time.perf_counter()-begin)*1000
    setup_ms=(time.perf_counter()-start)*1000;positions={t:i for i,t in enumerate(data.terms)};ready_ids=np.flatnonzero(data.ready)
    group_ids=np.arange(len(groups),dtype=np.int64);group_meeting=np.array([g['meeting'] for g in groups]);records=[];timings=[]
    summary={scope:{arm:Counter() for arm in ['direct_sources','parent_rrf']} for scope in ['meeting','global']}
    paired={scope:Counter() for scope in summary}
    for previous_query in control['records']:
        meeting=data.meetings[previous_query['meeting']];q=meeting.specific_queries[previous_query['query']]
        begin=time.perf_counter();vector,metadata=query(q.query,positions,data.idf);vector.sort_indices();source_scores,group_scores=features.score(vector)
        projection_ms=(time.perf_counter()-begin)*1000
        if metadata!=previous_query['query_terms']:raise ValueError('query token accounting drift')
        wanted=previous_query['relevant_sources'];cases={};stages={}
        scopes={'meeting':(ready_ids[source_meeting[ready_ids]==meeting.ordinal],group_ids[group_meeting==meeting.ordinal]),'global':(ready_ids,group_ids)}
        for scope,(ids,gids) in scopes.items():
            begin=time.perf_counter();direct={'selected_groups':[],'returned_sources':rank(source_scores[ids],ids,20),'candidate_sources':len(ids)}
            direct['metrics']=metrics(direct,wanted,ids.tolist())
            if direct!=previous_query['cases'][scope]['direct_sources']:raise ValueError('direct baseline scores/identities/metrics drift')
            baseline_ms=(time.perf_counter()-begin)*1000;begin=time.perf_counter()
            candidate=retrieve(source_scores[ids],ids,owners[ids],group_scores[gids],gids);candidate['metrics']=metrics(candidate,wanted,ids.tolist())
            if len(candidate['returned_sources'])!=len(direct['returned_sources']):raise ValueError('direct eligibility/zero cases changed')
            stages[scope]={'baseline_ms':baseline_ms,'parent_rrf_ms':(time.perf_counter()-begin)*1000};cases[scope]={'direct_sources':direct,'parent_rrf':candidate}
            for arm,result in cases[scope].items():summary[scope][arm].update(result['metrics'])
            delta=candidate['metrics']['recall_at_20']-direct['metrics']['recall_at_20'];paired[scope].update(improved=int(delta>0),declined=int(delta<0),tied=int(delta==0))
        records.append({k:v for k,v in previous_query.items() if k!='cases'}|{'cases':cases})
        timings.append({'meeting':meeting.ordinal,'query':q.ordinal,'projection_ms':projection_ms,'scopes':stages})
        if len(records)%200==0:print(json.dumps({'processed_queries':len(records)}),flush=True)
    if len(records)!=1095 or pins!={n:digest(Path(__file__).with_name(n)) for n in PINS} or digest(cp)!=CONTROL:raise ValueError('full query support/code controls changed')
    for scope,arms in summary.items():
        for arm,m in arms.items():summary[scope][arm]={'totals':dict(m),'macro':{k:m[k]/len(records) for k in ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    return {'schema_version':1,'scope':'parent_rank_bonus_training_not_production_rrf_or_topic_selection','source_sha256':data.source,'control_sha256':CONTROL,
        'support':control['support'],'summary':summary,'paired_recall':paired,'records':records,'code_pins':pins,
        'protocol':{'rrf_k':60,'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False,'candidate_pruning':False},
        'timings':{'setup_ms':setup_ms,'features_ms':feature_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'queries':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a.artifacts)
    if a.compare and {k:v for k,v in json.loads(a.compare.read_text()).items() if k!='timings'}!={k:v for k,v in result.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary'],'paired_recall':result['paired_recall']}),flush=True)


if __name__=='__main__':main()
