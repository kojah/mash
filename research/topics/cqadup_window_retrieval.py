"""Frozen complete source-window maximum versus document-direction retrieval."""
import argparse
from dataclasses import asdict
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from cqadup_filtered_topics import verified
from cqadup_semantic import digest
from cqadup_topics import SEMANTIC,REPEAT,PREFLIGHT
from embedding_windows import Field
from external_context import compare
from lexical_retrieval import ScoreKind,evaluate
from topic_field_evidence import candidates
from topic_routing import unit_matrix
from window_retrieval import maximum,ownership


def stratum(n):return '1' if n==1 else '2-4' if n<=4 else '5+'


def experiment(first,second,preflight_path,source,destination):
    started=time.perf_counter();cpu=time.process_time();data=verified(first,second,(SEMANTIC,REPEAT))
    if digest(preflight_path)!=PREFLIGHT or data['preflight_sha256']!=PREFLIGHT:raise ValueError('pinned complete preflight required')
    preflight=json.loads(preflight_path.read_text());ids=data['document_ids'];qids=data['query_ids']
    docs=candidates(source,data['capture_sha256'],ids);windows=ownership(preflight,docs)
    if [r['source'] for r in preflight['ledgers']['query']]!=qids:raise ValueError('development query ownership changed')
    total=sum(len(r['windows']) for rows in preflight['ledgers'].values() for r in rows)
    units=np.load(first/'unit-vectors.npy',mmap_mode='r',allow_pickle=False)
    if units.shape!=(total,384):raise ValueError('complete original unit matrix required')
    values=unit_matrix(units[:len(windows)])
    query=unit_matrix(np.load(first/'query-vectors.npy',mmap_mode='r',allow_pickle=False))
    body=unit_matrix(np.load(first/'body-vectors.npy',mmap_mode='r',allow_pickle=False))
    balanced=unit_matrix(np.load(first/'balanced-vectors.npy',mmap_mode='r',allow_pickle=False))
    if query.shape!=(len(qids),384) or body.shape!=(len(ids),384) or balanced.shape!=body.shape:raise ValueError('complete field dimensions required')
    owners=np.array([w.document for w in windows],dtype=np.int64)
    body_start=next(i for i,w in enumerate(windows) if w.field==Field.BODY)
    body_counts=np.bincount(owners[body_start:],minlength=len(ids));positions={d:i for i,d in enumerate(ids)}
    del preflight
    methods=['balanced-cosine','body-cosine','all-window-max','body-window-max']
    records={m:{} for m in methods};rows={m:[] for m in methods};winrows={m:[] for m in methods[2:]}
    supports={m:{} for m in methods[2:]};composition={m:[] for m in methods};timings=[]
    setup_ms=(time.perf_counter()-started)*1000
    for qi,qid in enumerate(qids):
        begin=time.perf_counter();unit_scores=values@query[qi]
        all_scores,all_winners=maximum(unit_scores,owners,len(ids))
        body_scores,body_winners=maximum(unit_scores[body_start:],owners[body_start:],len(ids));body_winners+=body_start
        scores={'balanced-cosine':balanced@query[qi],'body-cosine':body@query[qi],
                'all-window-max':all_scores,'body-window-max':body_scores}
        winners={'all-window-max':all_winners,'body-window-max':body_winners}
        relevant=set(data['results']['balanced-cosine']['records'][qid]['relevant_ranks'])
        for method in methods:
            record=evaluate(scores[method],ids,relevant,score_kind=ScoreKind.COSINE)
            if method=='balanced-cosine' and record!=data['results'][method]['records'][qid]:raise ValueError('archived balanced baseline changed')
            records[method][qid]=record;rows[method].append(scores[method])
            top=[positions[r['id']] for r in record['top100']]
            composition[method].append({'body_window_strata':{g:sum(stratum(int(body_counts[i]))==g for i in top) for g in ['1','2-4','5+']}})
            if method in winners:
                winning=winners[method];winrows[method].append(winning)
                supports[method][qid]=[{'document':ids[i],'window':int(winning[i])} for i in top]
                composition[method][-1].update(title_winners=sum(windows[int(winning[i])].field==Field.TITLE for i in top),
                    later_body_winners=sum(windows[int(winning[i])].field==Field.BODY and windows[int(winning[i])].ordinal>0 for i in top))
        timings.append((time.perf_counter()-begin)*1000)
    files={};results={}
    for method in methods:
        matrix=np.stack(rows[method]);path=destination/(method+'.npy');np.save(path,matrix,allow_pickle=False);files[path.name]=digest(path)
        if method in winrows:
            path=destination/(method+'-winners.npy');np.save(path,np.stack(winrows[method]),allow_pickle=False);files[path.name]=digest(path)
        metrics={k:float(np.mean([r['metrics'][k] for r in records[method].values()])) for k in next(iter(records[method].values()))['metrics']}
        strata={}
        for g in ['1','2-4','5+']:
            conditional=[];links=0
            for record in records[method].values():
                ranks=[rank for d,rank in record['relevant_ranks'].items() if stratum(int(body_counts[positions[d]]))==g]
                if ranks:
                    conditional.append({f'recall{k}':sum(r<=k for r in ranks)/len(ranks) for k in [10,100]});links+=len(ranks)
            strata[g]={'supported_queries':len(conditional),'explicit_positive_links':links,
                       'metrics':{f'recall{k}':float(np.mean([r[f'recall{k}'] for r in conditional])) if conditional else None for k in [10,100]}}
        results[method]={'metrics':metrics,'records':records[method],'conditional_positive_recall':strata,
                         'top100_composition':composition[method]}
        if method in supports:results[method]['winning_source_windows']=supports[method]
        print(json.dumps({'method':method,'metrics':metrics,'strata':strata}),flush=True)
    path=destination/'body-window-counts.npy';np.save(path,body_counts,allow_pickle=False);files[path.name]=digest(path)
    return {'schema_version':1,'scope':'development-source-window-retrieval-not-topic-gold',
            'python':platform.python_version(),'semantic_sha256':SEMANTIC,'semantic_repeat_sha256':REPEAT,
            'preflight_sha256':PREFLIGHT,'source_capture_sha256':data['capture_sha256'],
            'document_ids':ids,'query_ids':qids,'source_windows':[asdict(w) for w in windows],
            'source_window_count':len(windows),'body_window_count':len(windows)-body_start,
            'parameters':{'aggregation':'maximum signed cosine, no normalization or calibration',
                          'window_tie':'earliest original encoded row: title then body, source ID, ordinal',
                          'document_tie':'lexical original ID','strata':['1','2-4','5+']},
            'inference_calls':0,'reserved_queries_scored':0,'results':results,'files':files,
            'vector_work_per_query':{'source_windows':len(windows),'body_centroids':len(ids),'balanced_centroids':len(ids)},
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                ['cqadup_window_retrieval.py','window_retrieval.py','topic_field_evidence.py','lexical_retrieval.py','uv.lock']},
            'timings':{'setup_ms':setup_ms,'combined_score_rank_metrics_ms':timings,
                       'combined_p50_ms':float(np.median(timings)),'combined_p95_ms':float(np.percentile(timings,95)),
                       'total_ms':(time.perf_counter()-started)*1000,'process_cpu_ms':(time.process_time()-cpu)*1000,
                       'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['semantic','semantic-repeat','preflight','source','output-directory']:p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--compare',type=Path);a=p.parse_args();dst=a.output_directory
    if dst.exists() or dst.resolve().is_relative_to(Path(__file__).resolve().parents[2]):raise ValueError('fresh outside-Git output required')
    dst.mkdir(parents=True);r=json.loads(json.dumps(experiment(a.semantic,a.semantic_repeat,a.preflight,a.source,dst),allow_nan=False))
    if a.compare:compare(json.loads(a.compare.read_text()),r)
    with (dst/'results.json').open('x') as f:json.dump(r,f,indent=2,allow_nan=False)
    print(json.dumps({'output':str(dst),'timings':{k:v for k,v in r['timings'].items() if k!='combined_score_rank_metrics_ms'}}),flush=True)


if __name__=='__main__':main()
