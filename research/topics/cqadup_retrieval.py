"""Development-only full-corpus retrieval; reserved outcomes stay unopened."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import time

import numpy as np
import scipy
import sklearn

from cqadup_source import load, partition
from external_context import compare
from lexical_retrieval import Method, evaluate, fit
from stackoverflow_source import encoded


CAPTURE = '6ed3409aaeb3734e5fd2c89d6d7d0412dca45ca0828f7f7ea654450bd513782f'
COHORT = '1f190cfcc154af6881f9b115a69d8ed9a0fc01ba93961c44be4f7ce25b8e9fbf'


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def experiment(root, destination):
    started = time.perf_counter()
    documents, queries, references, _ = load(root, CAPTURE)
    raw = (root / 'cohort-first.json').read_bytes()
    if digest(raw) != COHORT:
        raise ValueError('pinned prospective cohort required')
    frozen = json.loads(raw)
    if partition(documents, queries, references) != frozen['cohort']:
        raise ValueError('source-derived prospective cohort drift')
    document_ids = sorted(documents)
    query_ids = sorted(q for group in frozen['cohort']['phases']['development'] for q in group['queries'])
    if len(query_ids) != 64:
        raise ValueError('complete declared development query population required')
    relevant = {q: {r.document for r in references if r.query == q} for q in query_ids}
    # Reserved query text/outcomes never enter feature fitting or scoring.
    texts = [documents[d].title + '\n' + documents[d].text for d in document_ids]
    result = {'version': 1, 'scope': 'development-only-complete-corpus-lexical-retrieval',
              'capture_sha256': CAPTURE, 'cohort_sha256': COHORT,
              'document_ids': document_ids, 'query_ids': query_ids,
              'encoder_calls': 0, 'reserved_queries_scored': 0,
              'relevance_expanded': False, 'source_preprocessing': 'title-newline-text-no-cleaning',
              'versions': {'python': platform.python_version(), 'numpy': np.__version__,
                           'scipy': scipy.__version__, 'sklearn': sklearn.__version__},
              'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                            ('cqadup_retrieval.py', 'lexical_retrieval.py', 'cqadup_source.py', 'uv.lock')},
              'results': {}, 'timings': {}}
    for method in Method:
        begin = time.perf_counter()
        index = fit(texts, method)
        fit_ms = (time.perf_counter() - begin) * 1000
        records, rows, latencies = {}, [], []
        for q in query_ids:
            begin = time.perf_counter()
            scores, evidence = index.score(queries[q].text)
            record = evaluate(scores, document_ids, relevant[q])
            latencies.append((time.perf_counter() - begin) * 1000)
            records[q] = record | {'query_feature_count': evidence}
            rows.append(scores)
        scores = np.stack(rows)
        path = destination / f'{method}.npy'
        np.save(path, scores, allow_pickle=False)
        aggregate = {key: float(np.mean([r['metrics'][key] for r in records.values()]))
                     for key in next(iter(records.values()))['metrics']}
        terms = index.vectorizer.get_feature_names_out().tolist()
        result['results'][method] = {
            'parameters': {'min_df': 2, 'max_df': 1., 'max_features': 65536,
                           'lowercase': True, 'stop_words': None,
                           'analyzer': 'char_wb' if method == Method.CHARACTER else 'word',
                           'ngram_range': [3, 5] if method == Method.CHARACTER else [1, 1],
                           'token_pattern': None if method == Method.CHARACTER else r'(?u)\b\w\w+\b',
                           'k1': 1.2 if method == Method.BM25 else None,
                           'b': .75 if method == Method.BM25 else None,
                           'query_tf': 'binary' if method == Method.BM25 else '1+log(tf)',
                           'length': 'complete-analyzer-before-vocabulary-filtering'},
            'terms': terms, 'idf_sha256': digest(index.idf.tobytes()),
            'counts_nnz': int(index.counts.nnz), 'lengths_sha256': digest(index.lengths.tobytes()),
            'score_sha256': digest(path.read_bytes()), 'score_shape': list(scores.shape),
            'metrics': aggregate, 'records': records,
            'zero_evidence_queries': sum(r['query_feature_count'] == 0 for r in records.values()),
        }
        result['timings'][method] = {'fit_ms': fit_ms, 'score_and_full_rank_ms': latencies,
                                     'p50_ms': float(np.median(latencies)),
                                     'p95_ms': float(np.percentile(latencies, 95))}
        print(json.dumps({'stage': 'control-complete', 'method': method, 'metrics': aggregate,
                          'features': len(terms), 'timings': {k: v for k, v in result['timings'][method].items()
                                                            if k != 'score_and_full_rank_ms'}}), flush=True)
    result['timings']['total_ms'] = (time.perf_counter() - started) * 1000
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output-directory', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output_directory.exists() or args.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output directory required')
    args.output_directory.mkdir()
    result = experiment(args.root, args.output_directory)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    (args.output_directory / 'results.json').write_text(json.dumps(result, sort_keys=True, allow_nan=False))
    print(json.dumps({'stage': 'complete', 'total_ms': result['timings']['total_ms']}))


if __name__ == '__main__':
    main()
