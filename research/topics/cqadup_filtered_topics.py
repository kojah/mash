"""Frozen topic admission, centrality packing versus exact union query retrieval."""
import argparse
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from cqadup_semantic import digest
from cqadup_topics import SEMANTIC, REPEAT
from external_context import compare
from topic_filtered_retrieval import admitted_union, score_union
from topic_routing import Match, posting_lists, routed_metrics, select


FIRST = 'ee0f9aa1fe26857ed3fdab10d6ca92d27fa1e8fbc1ccc820f26efe43cfeccaa4'
SECOND = '5f7bb9d39068d9250fe0b448d0c28308c984c630de199d137cb0c904983958c9'


def verified(first, second, expected):
    if digest(first/'results.json') != expected[0] or digest(second/'results.json') != expected[1]:
        raise ValueError('frozen captures required')
    a=json.loads((first/'results.json').read_text());b=json.loads((second/'results.json').read_text())
    compare(a,b)
    for name,sha in a['files'].items():
        if digest(first/name)!=sha or digest(second/name)!=sha:
            raise ValueError('complete captured arrays changed')
    return a


def experiment(topics, topic_repeat, semantic, semantic_repeat):
    started=time.perf_counter()
    topic=verified(topics,topic_repeat,(FIRST,SECOND))
    reference=verified(semantic,semantic_repeat,(SEMANTIC,REPEAT))
    ids=reference['document_ids'];queries=reference['query_ids'];positions={d:i for i,d in enumerate(ids)}
    if (topic['document_ids']!=ids or topic['query_ids']!=queries
            or ids!=sorted(set(ids)) or queries!=sorted(set(queries))
            or topic['geometry']['mode']!='fixed-bootstrap-pca25'):
        raise ValueError('complete frozen identity/geometry alignment required')
    vectors=np.load(semantic/'balanced-vectors.npy',mmap_mode='r',allow_pickle=False)
    qvectors=np.load(semantic/'query-vectors.npy',mmap_mode='r',allow_pickle=False)
    projected=np.load(topics/'projection-query-directions.npy',mmap_mode='r',allow_pickle=False)
    if vectors.shape!=(len(ids),384) or qvectors.shape!=(len(queries),384):
        raise ValueError('complete original vector dimensions required')
    setup_ms=(time.perf_counter()-started)*1000
    results=[];timings={}
    for capture in topic['captures']:
        order=capture['order']
        centers=np.load(topics/(order+'-centroids.npy'),allow_pickle=False)
        for mode,control in capture['modes'].items():
            posts=posting_lists([tuple(Match(**m) for m in row) for row in control['memberships']],len(centers))
            records={};latencies={'centrality':[],'query-union':[]}
            for qi,qid in enumerate(queries):
                archived=control['query_records'][qid]
                chosen_topics=tuple(Match(**m) for m in archived['topics'])
                if chosen_topics!=select(centers @ projected[qi]):
                    raise ValueError('frozen query admission changed')
                relevant=set(reference['results']['balanced-cosine']['records'][qid]['relevant_ranks'])
                begin=time.perf_counter()
                union,visits=admitted_union(chosen_topics,posts,len(ids))
                if len(union)!=archived['union_before_cap'] or visits!=archived['posting_visits']:
                    raise ValueError('archived admitted union changed')
                scores,candidates=score_union(qvectors[qi],vectors,union)
                record=routed_metrics(candidates,[s for _,s in candidates],ids,relevant)
                latencies['query-union'].append((time.perf_counter()-begin)*1000)
                begin=time.perf_counter()
                old=[(positions[d['id']],d['routing_score']) for d in archived['candidate_documents']]
                baseline_scores=vectors[[i for i,_ in old]] @ qvectors[qi]
                baseline=routed_metrics(old,baseline_scores,ids,relevant)
                for field in ('metrics','candidate_recall','relevant_ranks'):
                    if baseline[field]!=archived[field]: raise ValueError('centrality baseline changed')
                if ([d['id'] for d in baseline['top100']]!=[d['id'] for d in archived['top100']]
                        or not np.allclose([d['score'] for d in baseline['top100']],
                                           [d['score'] for d in archived['top100']],atol=1e-12,rtol=0)):
                    raise ValueError('baseline ranks/scores changed')
                latencies['centrality'].append((time.perf_counter()-begin)*1000)
                records[qid]={'topics':archived['topics'],'posting_visits':visits,
                    'union_documents':[ids[i] for i in union],'union_scores':scores.tolist(),
                    'union_reference_coverage':len(relevant & {ids[i] for i in union})/len(relevant),
                    'query_union':record,'centrality':baseline,
                    'scored_vectors':{'query-union':len(union),'centrality':len(old)},
                    'query_union_candidates':[ids[i] for i,_ in candidates]}
            summary={}
            for name,key in [('query-union','query_union'),('centrality','centrality')]:
                summary[name]={metric:float(np.mean([r[key]['metrics'][metric] for r in records.values()]))
                               for metric in ('mrr10','ndcg10','recall10','recall100')}
                summary[name]['candidate_recall']=float(np.mean([r[key]['candidate_recall'] for r in records.values()]))
                summary[name]['scored_vectors_total']=sum(r['scored_vectors'][name] for r in records.values())
            results.append({'order':order,'mode':mode,'records':records,'summary':summary})
            timings[order+'/'+mode]={name:{'observations_ms':v,'p50_ms':float(np.median(v)),
                                          'p95_ms':float(np.percentile(v,95))} for name,v in latencies.items()}
            print(json.dumps({'order':order,'mode':mode,'summary':summary}),flush=True)
    timings.update(setup_ms=setup_ms,total_ms=(time.perf_counter()-started)*1000,
                   peak_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    return {'schema_version':1,'scope':'development-packing-only-not-equal-work-or-agent-navigation',
            'python':platform.python_version(),'topic_sha256':FIRST,'topic_repeat_sha256':SECOND,
            'semantic_sha256':SEMANTIC,'semantic_repeat_sha256':REPEAT,
            'parameters':{'vector_work_cap':8192,'candidate_cap':1000,'score_chunk':256,
                          'dimensions':384,'tie':'signed cosine descending then original ID ascending'},
            'inference_calls':0,'reserved_queries_scored':0,'document_ids':ids,'query_ids':queries,
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                         ('cqadup_filtered_topics.py','topic_filtered_retrieval.py','topic_routing.py',
                          'cqadup_semantic.py','external_context.py','uv.lock')},
            'results':results,'timings':timings}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('topics','topic-repeat','semantic','semantic-repeat','output'):
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path)
    args=parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result=json.loads(json.dumps(experiment(args.topics,args.topic_repeat,args.semantic,args.semantic_repeat),allow_nan=False))
    if args.compare: compare(json.loads(args.compare.read_text()),result)
    with args.output.open('x') as f: json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'output':str(args.output),'total_ms':result['timings']['total_ms']}),flush=True)


if __name__=='__main__': main()
