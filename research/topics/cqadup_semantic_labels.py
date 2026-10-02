"""Bounded singleton semantic MMR labels on frozen topic memberships."""
import argparse
from dataclasses import asdict
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np
from scipy import sparse

from cqadup_filtered_topics import FIRST,SECOND,verified
from cqadup_semantic import digest
from cqadup_source import parse_source
from cqadup_window_preflight import MODEL,TOKENIZER
from external_context import compare
from label_coherence import diagnose,presence_statistics
from local_embeddings import Encoder
from semantic_mmr import select
from subject_labels import ranked_terms
from topic_labels import scores,support
from topic_review import decode


def csr(root,prefix,shape):
    return sparse.csr_matrix(tuple(np.load(root/(prefix+'-'+n+'.npy'),allow_pickle=False)
                                   for n in ('data','indices','indptr')),shape=tuple(shape))


def experiment(topics,repeat,source,snapshot,destination):
    started=time.perf_counter();cpu_started=time.process_time()
    data=verified(topics,repeat,(FIRST,SECOND));terms=data['vocabulary'];positions={t:i for i,t in enumerate(terms)}
    counts=csr(topics,'source-counts',data['source_counts_shape']);plans=[]
    for c in data['captures']:
        for mode,control in c['modes'].items():
            aggregate=csr(topics,c['order']+'-'+mode+'-classes',control['class_counts_shape'])
            _,contrastive=scores(aggregate,counts)
            for i,t in enumerate(control['topics']):
                row=contrastive.getrow(i);pool=ranked_terms(row.data,[terms[j] for j in row.indices],limit=32)
                if pool[:8]!=t['labels']['corpus-idf']: raise ValueError('frozen lexical baseline changed')
                plans.append({'order':c['order'],'mode':mode,'topic':i,'pool':pool,
                    'members':{doc for doc,row in enumerate(control['memberships']) if any(m['topic']==i for m in row)},
                    'archived_diverse_outside_pool':[k['term'] for k in t['labels']['corpus-idf-diverse'] if k['term'] not in {k['term'] for k in pool}]})
    phrases=sorted({k['term'] for p in plans for k in p['pool']})
    if not phrases or len(phrases)>1024: raise ValueError('phrase inference budget exceeded; no truncation')
    print(json.dumps({'stage':'phrase-budget-verified','topics':len(plans),'unique_phrases':len(phrases)}),flush=True)
    # Read only candidate sources, never query metadata or duplicate judgments.
    if digest(source/'capture.json')!=data['source_capture_sha256']: raise ValueError('source capture changed')
    manifest=decode((source/'capture.json').read_bytes());path=source/'corpus.jsonl';pin=manifest['files']['corpus.jsonl']
    if digest(path)!=pin['sha256'] or path.stat().st_size!=pin['size']: raise ValueError('complete source bytes changed')
    docs={}
    with path.open('rb') as f:
        while raw:=f.readline(4*1024*1024+1):
            if len(raw)>4*1024*1024 or len(docs)>=100000: raise ValueError('source budgets exceeded')
            doc=parse_source(decode(raw))
            if doc.id in docs: raise ValueError('duplicate source identity')
            docs[doc.id]=doc
    if sorted(docs)!=data['document_ids']: raise ValueError('complete source identity alignment required')
    document_ids=data['document_ids'];inverted=counts.tocsc()
    selected=sorted(positions[t] for t in phrases);df,joint=presence_statistics(counts,selected)
    local={terms[c]:i for i,c in enumerate(selected)}
    setup_ms=(time.perf_counter()-started)*1000
    begin=time.perf_counter();encoder=Encoder(snapshot);startup_ms=(time.perf_counter()-begin)*1000
    if encoder.statistics.model_sha256!=MODEL or encoder.statistics.tokenizer_sha256!=TOKENIZER:
        raise ValueError('pinned installed encoder required; no downloads')
    token_ids={t:encoder.tokenizer.encode(t).ids for t in phrases}
    if any(not 2<=len(ids)<=512 for ids in token_ids.values()): raise ValueError('complete phrase token budget exceeded')
    begin=time.perf_counter();vectors=np.stack([encoder.encode(t) for t in phrases]);encoding_ms=(time.perf_counter()-begin)*1000
    vector_path=destination/'phrase-vectors.npy';np.save(vector_path,vectors,allow_pickle=False)
    phrase_positions={t:i for i,t in enumerate(phrases)};results=[]
    begin=time.perf_counter()
    for p in plans:
        pool=p['pool'];pool_terms=[k['term'] for k in pool];relevance=np.asarray([k['score'] for k in pool])
        rows=[phrase_positions[t] for t in pool_terms];directions=vectors[rows]
        variants={'corpus-idf':pool[:8],
                  'corpus-idf-contained':ranked_terms(relevance,pool_terms,diverse=True),
                  'semantic-mmr':select(pool_terms,relevance,directions)}
        labels={}
        for name,keywords in variants.items():
            chosen=[k['term'] for k in keywords];indices=[phrase_positions[t] for t in chosen]
            similarity=vectors[indices]@vectors[indices].T;mask=np.triu(np.ones(similarity.shape,dtype=bool),k=1)
            evidence={}
            for t in chosen:
                col=positions[t];owners=[int(d) for d in inverted.indices[inverted.indptr[col]:inverted.indptr[col+1]] if int(d) in p['members']]
                if not owners: raise ValueError('selected phrase lacks topic source support')
                evidence[t]={'supporting_documents':len(owners),'first_source':support(docs[document_ids[owners[0]]],t)}
            labels[name]={'keywords':keywords,'source_support':evidence,
                'coherence':diagnose(chosen,local,df,joint,len(document_ids)),
                'mean_pair_cosine':float(similarity[mask].mean()) if mask.any() else None,
                'mean_relative_relevance':float(np.mean([k['score']/relevance.max() for k in keywords]))}
        results.append({k:p[k] for k in ('order','mode','topic','pool','archived_diverse_outside_pool')}|{'labels':labels})
    labels_ms=(time.perf_counter()-begin)*1000;summary=[]
    for order,mode in sorted({(r['order'],r['mode']) for r in results}):
        group=[r for r in results if r['order']==order and r['mode']==mode]
        for variant in ('corpus-idf','corpus-idf-contained','semantic-mmr'):
            values=[r['labels'][variant] for r in group];slots=[k['term'] for v in values for k in v['keywords']]
            summary.append({'order':order,'mode':mode,'variant':variant,'topics':len(group),
                'macro_npmi':float(np.mean([v['coherence']['mean_npmi'] for v in values])),
                'macro_mean_pair_cosine':float(np.mean([v['mean_pair_cosine'] for v in values])),
                'macro_relative_relevance':float(np.mean([v['mean_relative_relevance'] for v in values])),
                'unique_term_ratio':len(set(slots))/len(slots)})
    for r in summary: print(json.dumps(r),flush=True)
    statistics=asdict(encoder.statistics)
    inference_ms=statistics.pop('inference_ms');tokenization_ms=statistics.pop('tokenization_ms')
    return {'schema_version':1,'scope':'development-source-grounded-label-only-mmr-not-semantic-quality',
            'topic_sha256':FIRST,'topic_repeat_sha256':SECOND,'source_capture_sha256':data['source_capture_sha256'],
            'python':platform.python_version(),'parameters':{'pool':32,'unique_phrase_cap':1024,'lambda':.6,
                'labels':8,'relevance':'positive corpus-IDF / maximum in pool','redundancy':'maximum signed cosine; zero for first',
                'ties':'lexical phrase','inference':'singleton existing quantized-direction encoder; no truncation'},
            'phrases':phrases,'phrase_token_ids':token_ids,'encoder':statistics,'inference_calls':statistics['calls'],
            'reserved_queries_scored':0,'files':{'phrase-vectors.npy':digest(vector_path)},
            'results':results,'summary':summary,
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                ('cqadup_semantic_labels.py','semantic_mmr.py','local_embeddings.py','data.py','topic_labels.py',
                 'subject_labels.py','label_coherence.py','cqadup_source.py','cqadup_filtered_topics.py','uv.lock')},
            'timings':{'setup_ms':setup_ms,'encoder_startup_ms':startup_ms,'encoding_ms':encoding_ms,
                'inference_ms':inference_ms,'tokenization_ms':tokenization_ms,'labels_ms':labels_ms,
                'total_ms':(time.perf_counter()-started)*1000,'process_cpu_ms':(time.process_time()-cpu_started)*1000,
                'peak_rss_kib':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for n in ('topics','topic-repeat','source','snapshot','output-directory'): parser.add_argument('--'+n,type=Path,required=True)
    parser.add_argument('--compare',type=Path);args=parser.parse_args();dst=args.output_directory
    if dst.exists() or dst.resolve().is_relative_to(Path(__file__).resolve().parents[2]): raise ValueError('fresh outside-Git output required')
    dst.mkdir(parents=True)
    result=json.loads(json.dumps(experiment(args.topics,args.topic_repeat,args.source,args.snapshot,dst),allow_nan=False))
    if args.compare: compare(json.loads(args.compare.read_text()),result)
    with (dst/'results.json').open('x') as f: json.dump(result,f,indent=2,allow_nan=False)
    print(json.dumps({'output':str(dst),'timings':result['timings']}),flush=True)


if __name__=='__main__': main()
