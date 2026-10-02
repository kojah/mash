"""Test whether discovered-topic guidance improves over plain thread context."""
import argparse
from collections import Counter
import json
from pathlib import Path
import resource
import time
import numpy as np
from guidance_corpus import load,REFERENCE
from discovery_inputs import digest
from navigation_features import build
from query_navigation import query,rank,metrics
from vector_topic_guidance import retrieve


PINS=['qmsum_thread_context.py','THREAD_CONTEXT_PROTOCOL.md','guidance_corpus.py','navigation_features.py','vector_topic_guidance.py',
    'query_navigation.py','discovery_inputs.py','episode_blocks.py','label_noise.py','stable_lexical_frame.py','qmsum.py','uv.lock']


def experiment(root):
    start=time.perf_counter();cpu=time.process_time();pins={n:digest(Path(__file__).with_name(n)) for n in PINS}
    c=load(root);data=c.data;reference=c.reference;begin=time.perf_counter()
    topics=build(data.counts,data.idf,data.terms,c.topic_owner,len(c.topic_meeting))
    thread_owner=np.where(data.ready,c.source_meeting,-1);threads=build(data.counts,data.idf,data.terms,thread_owner,len(data.meetings))
    if (topics.sources!=threads.sources).nnz:raise ValueError('source feature frame differs between parent controls')
    features_ms=(time.perf_counter()-begin)*1000;setup_ms=(time.perf_counter()-start)*1000
    positions={t:i for i,t in enumerate(data.terms)};ids=np.flatnonzero(data.ready);gids=np.arange(len(c.topic_meeting),dtype=np.int64);tids=np.arange(len(data.meetings),dtype=np.int64)
    records=[];timings=[];summary={s:{a:Counter() for a in ['direct_sources','topic_parent','thread_parent']} for s in ['meeting','global']}
    paired={s:{baseline:Counter() for baseline in ['direct_sources','thread_parent']} for s in summary}
    for previous in reference['records']:
        meeting=data.meetings[previous['meeting']];q=meeting.specific_queries[previous['query']]
        begin=time.perf_counter();direction,metadata=query(q.query,positions,data.idf);direction.sort_indices();sources,group_scores=topics.score(direction)
        thread_scores=(threads.groups@direction.T).toarray().ravel();projection_ms=(time.perf_counter()-begin)*1000
        if metadata!=previous['query_terms']:raise ValueError('query metadata drift')
        scopes={'meeting':(ids[c.source_meeting[ids]==meeting.ordinal],gids[c.topic_meeting==meeting.ordinal],np.array([meeting.ordinal],dtype=np.int64)),
            'global':(ids,gids,tids)};cases={};stages={}
        for scope,(source_ids,topic_ids,thread_ids) in scopes.items():
            begin=time.perf_counter();direct={'selected_groups':[],'returned_sources':rank(sources[source_ids],source_ids,20),'candidate_sources':len(source_ids)}
            direct['metrics']=metrics(direct,previous['relevant_sources'],source_ids.tolist())
            if direct!=previous['cases'][scope]['direct_sources']:raise ValueError('direct reference drift')
            direct_ms=(time.perf_counter()-begin)*1000;begin=time.perf_counter()
            topic=retrieve(sources[source_ids],source_ids,c.topic_owner[source_ids],group_scores[topic_ids],topic_ids)
            topic['metrics']=metrics(topic,previous['relevant_sources'],source_ids.tolist());topic_ms=(time.perf_counter()-begin)*1000
            if topic!=previous['cases'][scope]['parent_rrf']:raise ValueError('topic reference drift')
            begin=time.perf_counter();thread=retrieve(sources[source_ids],source_ids,c.source_meeting[source_ids],thread_scores[thread_ids],thread_ids)
            thread['metrics']=metrics(thread,previous['relevant_sources'],source_ids.tolist());thread_ms=(time.perf_counter()-begin)*1000
            if scope=='meeting' and ([r['id'] for r in thread['returned_sources']]!=[r['id'] for r in direct['returned_sources']] or thread['metrics']!=direct['metrics']):raise ValueError('sole-parent direct-order invariant failed')
            cases[scope]={'direct_sources':direct,'topic_parent':topic,'thread_parent':thread};stages[scope]={'direct_ms':direct_ms,'topic_ms':topic_ms,'thread_ms':thread_ms}
            for arm,result in cases[scope].items():summary[scope][arm].update(result['metrics'])
            for baseline,pairs in paired[scope].items():
                delta=topic['metrics']['recall_at_20']-cases[scope][baseline]['metrics']['recall_at_20'];pairs.update(improved=int(delta>0),declined=int(delta<0),tied=int(delta==0))
        records.append({k:v for k,v in previous.items() if k!='cases'}|{'cases':cases});timings.append({'meeting':meeting.ordinal,'query':q.ordinal,'projection_ms':projection_ms,'scopes':stages})
        if len(records)%200==0:print(json.dumps({'processed_queries':len(records)}),flush=True)
    if (len(records)!=1095 or digest(root/'topic-qmsum-soft-guidance-first-20261002.json')!=REFERENCE
            or pins!={n:digest(Path(__file__).with_name(n)) for n in PINS}):raise ValueError('query/source pins changed')
    for scope,arms in summary.items():
        for arm,m in arms.items():summary[scope][arm]={'totals':dict(m),'macro':{k:m[k]/len(records) for k in ['recall_at_20','precision_at_20','mrr','ndcg_at_20','candidate_recall','returned_sources']}}
    return {'schema_version':1,'scope':'thread_context_control_training_not_topic_quality_or_production_selection','source_sha256':data.source,
        'reference_capture_sha256':REFERENCE,'control_sha256':reference['control_sha256'],'support':reference['support']|{'thread_parents':162,'topic_parents':1296},
        'summary':summary,'paired_topic_recall':paired,'records':records,'code_pins':pins,'protocol':{'rrf_k':60,'new_inference':0,'llm_calls':0,'heldout_read':False,'production_selection':False},
        'timings':{'setup_ms':setup_ms,'features_ms':features_ms,'after_imports_ms':(time.perf_counter()-start)*1000,'cpu_ms':(time.process_time()-cpu)*1000,
            'rusage_peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,'queries':timings}}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--artifacts',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--compare',type=Path);a=p.parse_args()
    if a.output.exists() or a.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    result=experiment(a.artifacts)
    if a.compare and {k:v for k,v in json.loads(a.compare.read_text()).items() if k!='timings'}!={k:v for k,v in result.items() if k!='timings'}:raise ValueError('complete non-timing repeat differs')
    with a.output.open('x') as f:json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'summary':result['summary'],'paired':result['paired_topic_recall']}),flush=True)


if __name__=='__main__':main()
