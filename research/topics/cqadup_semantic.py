"""Singleton local semantic retrieval with complete preflight windows and fixed fusion."""
import argparse
import hashlib
import json
from pathlib import Path
import resource
import time

import numpy as np

from cqadup_source import load
from cqadup_window_preflight import CAPTURE, COHORT, MODEL, TOKENIZER
from embedding_windows import Field, Window
from external_context import compare
from lexical_retrieval import ScoreKind, evaluate
from local_embeddings import Encoder
from semantic_retrieval import aggregate, rank_fusion
from window_inference import infer


LEXICAL = '22ce86f9ec04daa7a8c41bc174b399faad93c1b2cb91066c5c5735a156658c0d'


def digest(path):
    # Large vector matrices must not be copied wholesale just to compute a pin.
    sha = hashlib.sha256()
    with path.open('rb') as file:
        while raw := file.read(1024*1024):
            sha.update(raw)
    return sha.hexdigest()


def experiment(root, preflight, preflight_pin, snapshot, lexical, destination):
    started = time.perf_counter()
    if digest(preflight) != preflight_pin or digest(lexical / 'results.json') != LEXICAL:
        raise ValueError('explicit pinned complete preflight and lexical baseline required')
    capture = json.loads(preflight.read_text())
    if (capture['capture_sha256'] != CAPTURE or capture['cohort_sha256'] != COHORT
            or capture['model_sha256'] != MODEL or capture['tokenizer_sha256'] != TOKENIZER
            or capture['encoder_calls'] != 0 or capture['retrieval_outcomes'] is not None):
        raise ValueError('complete pre-outcome installed-asset contract required')
    documents, queries, references, _ = load(root, CAPTURE)
    cohort_raw = (root/'cohort-first.json').read_bytes()
    if hashlib.sha256(cohort_raw).hexdigest() != COHORT:
        raise ValueError('prospective cohort drift')
    document_ids = sorted(documents)
    cohort = json.loads(cohort_raw)['cohort']['phases']
    query_ids = sorted(q for g in cohort['development'] for q in g['queries'])
    for field, expected in [('title',document_ids), ('body',document_ids), ('query',query_ids)]:
        if [r['source'] for r in capture['ledgers'][field]] != expected:
            raise ValueError('complete field/development identities required')
    old = json.loads((lexical/'results.json').read_text())
    if old['document_ids'] != document_ids or old['query_ids'] != query_ids:
        raise ValueError('matched full-corpus lexical identities required')
    bm25_path = lexical/'word-bm25.npy'
    if digest(bm25_path) != old['results']['word-bm25']['score_sha256']:
        raise ValueError('complete pinned lexical score matrix required')
    bm25 = np.load(bm25_path, mmap_mode='r', allow_pickle=False)
    if bm25.shape != (len(query_ids),len(document_ids)):
        raise ValueError('complete lexical matrix dimensions required')
    encoder = Encoder(snapshot)
    if encoder.statistics.model_sha256 != MODEL or encoder.statistics.tokenizer_sha256 != TOKENIZER:
        raise ValueError('installed asset drift; no downloads')
    ledgers = [(field,r) for field in ('title','body','query') for r in capture['ledgers'][field]]
    total = sum(len(r['windows']) for _,r in ledgers)
    matrix = np.lib.format.open_memmap(destination/'unit-vectors.npy', mode='w+', dtype=np.float64, shape=(total,384))
    position, inference_ms = 0, 0
    fields = {'title':[], 'body':[], 'query':[]}
    build_started = time.perf_counter()
    for field, record in ledgers:
        source = (queries if field=='query' else documents)[record['source']]
        text = source.title if field=='title' else source.text
        if hashlib.sha256(text.encode()).hexdigest() != record['source_text_sha256']:
            raise ValueError('original field text drift')
        start, weights = position, []
        for value in record['windows']:
            unit = Window(**(value | {'field':Field(value['field']), 'ids':tuple(value['ids']), 'types':tuple(value['types'])}))
            original = text[unit.start:unit.end]
            if (hashlib.sha256(original.encode()).hexdigest() != unit.text_sha256
                    or tuple(encoder.tokenizer.encode(original).ids) != unit.ids):
                raise ValueError('source-bound complete input IDs required')
            begin = time.perf_counter()
            _, vector = infer(encoder.session,[unit])
            inference_ms += (time.perf_counter()-begin)*1000
            matrix[position] = vector[0]
            position += 1
            weights.append(unit.payload_tokens)
            if position % 1000 == 0:
                matrix.flush()
                print(json.dumps({'stage':'encoding-progress','field':field,'completed':position,
                                  'total':total,'elapsed_seconds':time.perf_counter()-build_started}),flush=True)
        fields[field].append(aggregate(matrix[start:position],weights))
    if position != total:
        raise ValueError('complete source-unit inference required')
    matrix.flush()
    build_ms = (time.perf_counter()-build_started)*1000
    title, body, query = [np.stack(fields[f]) for f in ('title','body','query')]
    balanced = np.stack([aggregate([a,b],[1,1]) for a,b in zip(title,body)])
    files = {}
    for name,values in [('title-vectors',title),('body-vectors',body),('balanced-vectors',balanced),('query-vectors',query)]:
        path = destination/(name+'.npy')
        np.save(path,values,allow_pickle=False)
        files[path.name] = digest(path)
    files['unit-vectors.npy'] = digest(destination/'unit-vectors.npy')
    relevant = {q:{r.document for r in references if r.query==q} for q in query_ids}
    results, timings = {}, {}
    for method in ('title-cosine','balanced-cosine','balanced-bm25-rrf60'):
        records,rows,latencies = {},[],[]
        for i,q in enumerate(query_ids):
            begin = time.perf_counter()
            scores = (title if method=='title-cosine' else balanced) @ query[i]
            if method.endswith('rrf60'):
                scores = rank_fusion(scores,bm25[i])
            kind = ScoreKind.NONNEGATIVE if method.endswith('rrf60') else ScoreKind.COSINE
            records[q] = evaluate(scores,document_ids,relevant[q],score_kind=kind)
            latencies.append((time.perf_counter()-begin)*1000)
            rows.append(scores)
        path = destination/(method+'.npy')
        np.save(path,np.stack(rows),allow_pickle=False)
        files[path.name] = digest(path)
        metrics = {key:float(np.mean([r['metrics'][key] for r in records.values()]))
                   for key in next(iter(records.values()))['metrics']}
        results[method] = {'metrics':metrics,'records':records,'score_kind':str(kind)}
        timings[method] = {'score_full_rank_and_metrics_ms':latencies,
                          'p50_ms':float(np.median(latencies)), 'p95_ms':float(np.percentile(latencies,95))}
        print(json.dumps({'stage':'control-complete','method':method,'metrics':metrics}),flush=True)
    timings.update(build_ms=build_ms,inference_ms=inference_ms,total_ms=(time.perf_counter()-started)*1000,
                   peak_rss_kib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    return {'version':1,'scope':'development-only-complete-corpus-singleton-semantic-retrieval',
            'capture_sha256':CAPTURE,'cohort_sha256':COHORT,'preflight_sha256':preflight_pin,
            'lexical_sha256':LEXICAL,'model_sha256':MODEL,'tokenizer_sha256':TOKENIZER,
            'document_ids':document_ids,'query_ids':query_ids,'encoded_windows':total,
            'inference':'singleton-CPU-one-intra/inter-op-thread-mean-float32-int8-direction',
            'field_aggregation':'payload-token-weighted-unit-direction-mean-then-normalize',
            'balanced_aggregation':'equal-title/body-direction-mean-then-normalize',
            'fusion':'complete-rank-RRF60-balanced-cosine-plus-word-BM25-no-sweeps',
            'reserved_queries_scored':0,'files':files,'results':results,'timings':timings,
            'code_pins':{n:digest(Path(__file__).with_name(n)) for n in
                         ('cqadup_semantic.py','semantic_retrieval.py','window_inference.py','local_embeddings.py',
                          'lexical_retrieval.py','cqadup_source.py','embedding_windows.py','uv.lock')}}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('root','preflight','snapshot','lexical','output-directory'):
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--preflight-sha256',required=True)
    p.add_argument('--compare',type=Path)
    args = p.parse_args()
    if args.output_directory.exists() or args.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git semantic output required')
    args.output_directory.mkdir()
    result = experiment(args.root,args.preflight,args.preflight_sha256,args.snapshot,args.lexical,args.output_directory)
    if args.compare:
        compare(json.loads(args.compare.read_text()),result)
    (args.output_directory/'results.json').write_text(json.dumps(result,sort_keys=True,allow_nan=False))
    print(json.dumps({'stage':'complete','encoded_windows':result['encoded_windows'],'timings':
                      {k:v for k,v in result['timings'].items() if not isinstance(v,dict)}}))


if __name__=='__main__':
    main()
