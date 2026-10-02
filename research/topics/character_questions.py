"""Preregistered word/character source-similarity diagnostic, not topic discovery."""
import argparse
import json
from pathlib import Path
import time

import numpy as np

from channel_weights import View, matrix, weighted
from character_features import count_channels, idf_vectors, sparse_rows
from external_clustering import boolean_metrics
from external_context import compare
from graph_questions import inputs
from question_calibration import passes
from question_channels import Channel, classify
from question_transfer import rank_metrics
from reference_review import digest
from stackoverflow_source import load
from weighted_questions import CHANNEL_PIN, GRAPH_PIN, capture


def experiment(root):
    started = time.perf_counter()
    channels = capture(root, 'channels-first.json', CHANNEL_PIN)
    graph = capture(root, 'graph-first.json', GRAPH_PIN)
    ids, pairs, old_counts, _, words, _, _, archived = inputs(root)
    if ids != graph['identities'] or pairs != graph['rows'] or words != graph['words']:
        raise ValueError('original exposed cohort required')
    records = {r['id']: r for r in channels['records']}
    sources = {str(q.id): q for q in load(root).inputs}
    texts, hashes = {k: [] for k in ('full', 'title', 'prose', 'code')}, []
    for identity in ids:
        source = sources[identity]
        body = classify(source.body_html)
        if digest(body.rendered.text.encode()) != records[identity]['rendered_sha256']:
            raise ValueError('original source changed')
        fields = {'full': source.title + '\n\n' + body.rendered.text, 'title': source.title,
                  'prose': '\n'.join(body.rendered.text[r.start:r.end] for r in body.runs if r.channel == Channel.PROSE),
                  'code': '\n'.join(body.rendered.text[r.start:r.end] for r in body.runs if r.channel == Channel.CODE)}
        hashes.append({k: digest(v.encode()) for k, v in fields.items()})
        for k, value in fields.items():
            texts[k].append(value)
    before = time.perf_counter()
    char_words, char_counts = count_channels(*(texts[k] for k in ('full', 'title', 'prose', 'code')))
    word_counts = [matrix(records, ids, words, k) for k in ('title', 'prose', 'block-code')]
    if not np.array_equal(sum(word_counts), old_counts):
        raise ValueError('original word count conservation failed')
    timings = {'feature_counts_ms': (time.perf_counter() - before) * 1000}
    features, results = {}, {}
    positions = {i: n for n, i in enumerate(ids)}
    truth = [p['tag_overlap'] for p in pairs]
    for mode, terms, counts in (('words', words, word_counts), ('characters', char_words, char_counts)):
        features[mode] = {'terms': terms, 'counts': [sparse_rows(c) for c in counts]}
        whole = sum(counts)
        for view in View:
            before = time.perf_counter()
            mass = weighted(*counts, view)
            vectors, df, idf = idf_vectors(mass, whole)
            scores = [float(vectors[positions[p['left']]] @ vectors[positions[p['right']]]) for p in pairs]
            cuts = {f'{cut/10:.1f}': boolean_metrics(truth, [s >= cut/10 for s in scores]) for cut in range(1, 10)}
            key = f'{mode}/{view}'
            results[key] = {'scores': scores, 'rank_metrics': rank_metrics(pairs, np.asarray(scores)),
                           'cuts': {k: {'metrics': m, 'passes': passes(m)} for k, m in cuts.items()},
                           'df': df.tolist(), 'idf': idf.tolist(), 'vectors': sparse_rows(vectors),
                           'vector_sha256': digest(np.asarray(vectors, dtype='<f8').tobytes()),
                           'zero_sources': [i for i, v in zip(ids, vectors, strict=True) if not v.any()],
                           'positive_median': float(np.median([s for s, t in zip(scores, truth) if t])),
                           'negative_median': float(np.median([s for s, t in zip(scores, truth) if not t]))}
            timings[key] = (time.perf_counter() - before) * 1000
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed64-character-vs-word-similarity-no-topic-memberships',
            'policy': {'channel_pin': CHANNEL_PIN, 'graph_pin': GRAPH_PIN, 'archived_lock_sha256': archived,
                       'char_analyzer': 'char_wb', 'ngram_range': [3, 5], 'lowercase': True,
                       'char_vocabulary': 'calibration64-whole-source-only-DF2-max.40-cap8192',
                       'word_vocabulary': 'unchanged629-original-transductive-unigrams',
                       'channel_weighting': 'existing-L1-mean-nonempty', 'idf': 'log((1+N)/(1+DF))+1',
                       'normalization': 'L2-after-IDF', 'labels': 'none-padded-fragments-not-source-phrases',
                       'encoder_calls': 0, 'production_selection': None},
            'identities': ids, 'rows': pairs, 'source_hashes': hashes, 'features': features, 'results': results,
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('character_features.py', 'character_questions.py', 'channel_weights.py',
                           'graph_questions.py', 'question_channels.py', 'uv.lock')}, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'views': len(result['results']), 'total_ms': result['timings']['total_ms']}))


if __name__ == '__main__':
    main()
