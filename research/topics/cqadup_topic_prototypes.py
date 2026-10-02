"""Admission-only original-space prototype control; fixed document topics/labels."""
import argparse
from dataclasses import asdict
from enum import StrEnum
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from cqadup_filtered_topics import FIRST, SECOND, verified
from cqadup_semantic import digest
from cqadup_topics import SEMANTIC, REPEAT
from external_context import compare
from topic_filtered_retrieval import score_union
from topic_prototypes import complete_union, fit
from topic_routing import Match, routed_metrics, select


BASELINE='076172dfeb1f9f78b428ec58f6249fb9ece70c99cf6c0f18506bc3b01b6086f0'
BASELINE_REPEAT='7c32194bd6d74c4d0bd2cbdbf66ae4be6f7c196feea496b5594ac853295fff1d'


class State(StrEnum):
    COMPLETE='complete'
    BUDGET='vector-budget-exceeded'


def experiment(topics, topic_repeat, semantic, semantic_repeat, baseline_path, baseline_repeat):
    started=time.perf_counter()
    topic=verified(topics,topic_repeat,(FIRST,SECOND))
    reference=verified(semantic,semantic_repeat,(SEMANTIC,REPEAT))
    if digest(baseline_path)!=BASELINE or digest(baseline_repeat)!=BASELINE_REPEAT:
        raise ValueError('audited packing-only baseline required')
    baseline=json.loads(baseline_path.read_text());compare(baseline,json.loads(baseline_repeat.read_text()))
    ids=reference['document_ids'];queries=reference['query_ids']
    if (topic['document_ids']!=ids or baseline['document_ids']!=ids
            or topic['query_ids']!=queries or baseline['query_ids']!=queries
            or ids!=sorted(set(ids)) or queries!=sorted(set(queries))):
        raise ValueError('complete identity alignment required')
    vectors=np.load(semantic/'balanced-vectors.npy',mmap_mode='r',allow_pickle=False)
    qvectors=np.load(semantic/'query-vectors.npy',mmap_mode='r',allow_pickle=False)
    if vectors.shape!=(len(ids),384) or qvectors.shape!=(len(queries),384):
        raise ValueError('complete original vector shape required')
    setup_ms=(time.perf_counter()-started)*1000;results=[];timings={}
    for capture in topic['captures']:
        for mode,control in capture['modes'].items():
            members=[tuple(Match(**m) for m in row) for row in control['memberships']]
            begin=time.perf_counter();p=fit(vectors,members,len(capture['topic_ids']))
            fit_ms=(time.perf_counter()-begin)*1000;records={};observations=[]
            for qi,qid in enumerate(queries):
                begin=time.perf_counter()
                cosines=p.directions@qvectors[qi];cosines[~p.active]=-1
                selected=select(cosines)
                union,visits=complete_union(selected,members)
                # Reference identities are read only after candidate admission.
                relevant=set(reference['results']['balanced-cosine']['records'][qid]['relevant_ranks'])
                within=len(union)<=8192
                if within:
                    scores,candidates=score_union(qvectors[qi],vectors,union)
                    ranking=routed_metrics(candidates,[s for _,s in candidates],ids,relevant)
                else:
                    scores=None;candidates=None;ranking=None
                observations.append((time.perf_counter()-begin)*1000)
                records[qid]={'status':str(State.COMPLETE if within else State.BUDGET),
                    'topic_scores':cosines.tolist(),'topics':[asdict(m) for m in selected],
                    'union_documents':[ids[i] for i in union],'posting_visits':visits,
                    'union_reference_coverage':len(relevant&{ids[i] for i in union})/len(relevant),
                    'vector_scores':scores.tolist() if within else None,
                    'candidates':[ids[i] for i,_ in candidates] if within else None,
                    'ranking':ranking,'scored_vectors':len(union) if within else 0}
            completed=[r for r in records.values() if r['status']==State.COMPLETE]
            summary={metric:float(np.mean([r['ranking']['metrics'][metric] for r in completed])) if completed else None
                     for metric in ('mrr10','ndcg10','recall10','recall100')}
            old=next(r for r in baseline['results'] if r['order']==capture['order'] and r['mode']==mode)
            result={'order':capture['order'],'mode':mode,'prototype_sums':p.sums.tolist(),
                    'prototype_masses':p.masses.tolist(),'prototype_directions':p.directions.tolist(),
                    'prototype_active':p.active.tolist(),'records':records,
                    'summary':{'completed_queries':len(completed),'budget_failures':len(records)-len(completed),
                     'query_abstentions':sum(not r['topics'] for r in records.values()),
                     'all_query_metrics':summary if len(completed)==len(records) else None,
                     'completed_query_metrics':summary,'completed_candidate_recall':float(np.mean([r['ranking']['candidate_recall'] for r in completed])) if completed else None,
                     'all_query_union_reference_coverage':float(np.mean([r['union_reference_coverage'] for r in records.values()])),
                     'vector_scores_total':sum(r['scored_vectors'] for r in records.values()),
                     'maximum_union':max(len(r['union_documents']) for r in records.values())},
                    'projected_admission_baseline':old['summary']['query-union']}
            results.append(result)
            timings[capture['order']+'/'+mode]={'fit_ms':fit_ms,'observations_ms':observations,
                                               'p50_ms':float(np.median(observations)),'p95_ms':float(np.percentile(observations,95))}
            print(json.dumps({'order':capture['order'],'mode':mode,'summary':result['summary']}),flush=True)
    timings.update(setup_ms=setup_ms,total_ms=(time.perf_counter()-started)*1000,
                   peak_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    return {'schema_version':1,'scope':'development-admission-only-offline-prototypes-not-topic-quality',
            'topic_sha256':FIRST,'topic_repeat_sha256':SECOND,'semantic_sha256':SEMANTIC,
            'semantic_repeat_sha256':REPEAT,'baseline_sha256':BASELINE,'baseline_repeat_sha256':BASELINE_REPEAT,
            'python':platform.python_version(),'inference_calls':0,'reserved_queries_scored':0,
            'document_ids':ids,'query_ids':queries,
            'parameters':{'prototype_mass':'equal document mass across retained memberships','zero_norm_cutoff':1e-12,
                          'absolute_cosine':.55,'relative_best':.85,'topic_budget':3,'vector_work_cap':8192,
                          'candidate_cap':1000,'score_chunk':256,'tie':'original topic index; source ID for documents'},
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                         ('cqadup_topic_prototypes.py','topic_prototypes.py','cqadup_filtered_topics.py',
                          'topic_filtered_retrieval.py','topic_routing.py','cqadup_topics.py',
                          'cqadup_semantic.py','external_context.py','uv.lock')},
            'results':results,'timings':timings}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('topics','topic-repeat','semantic','semantic-repeat','baseline','baseline-repeat','output'):
        parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git capture required')
    result=json.loads(json.dumps(experiment(args.topics,args.topic_repeat,args.semantic,args.semantic_repeat,
                                           args.baseline,args.baseline_repeat),allow_nan=False))
    if args.compare: compare(json.loads(args.compare.read_text()),result)
    with args.output.open('x') as f: json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'output':str(args.output),'total_ms':result['timings']['total_ms']}),flush=True)


if __name__=='__main__': main()
