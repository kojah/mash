"""Exposed hard-case passage diagnostic; oracle source shortlist, not topic quality."""
import argparse
from collections import Counter
from dataclasses import asdict
import json
from pathlib import Path
import time

from local_embeddings import Encoder
from packet_reference_review import validate as validate_references
from passage_scope import Relevance, Scope, capture, validate
from reference_review import digest
from span_selection import Query, query_inputs, rank_spans


def counts(selected, judgments):
    labels = [judgments[i] for i in selected]
    return {'selected': len(labels), 'topical': sum(j.relevance == Relevance.TOPICAL for j in labels),
            'included': sum(j.scope == Scope.INCLUDED for j in labels),
            'mixed': sum(j.scope == Scope.MIXED for j in labels),
            'excluded': sum(j.scope == Scope.EXCLUDED for j in labels),
            'unspecified': sum(j.scope == Scope.UNSPECIFIED for j in labels),
            'non_topical': sum(j.relevance != Relevance.TOPICAL for j in labels)}


def recency(keys):
    """Newest record first; original reading order within that record."""
    return tuple(sorted(range(len(keys)), key=lambda i: (-keys[i]['source_ordinal'], keys[i]['index'])))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('contexts', 'packets', 'source-review', 'reference-review', 'selection',
                 'manifest', 'annotations', 'snapshot', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private ranking output')
    started = time.perf_counter()
    raw = {name: getattr(args, name).read_bytes() for name in
           ('contexts', 'packets', 'source_review', 'reference_review', 'selection', 'manifest', 'annotations')}
    validate_references(raw['contexts'], raw['packets'], raw['source_review'], json.loads(raw['reference_review']))
    manifest = json.loads(raw['manifest'])
    if any(manifest['input_sha256'][k] != digest(raw[k]) for k in manifest['input_sha256']):
        raise ValueError('passage manifest input changed')
    for name, expected in manifest['code_sha256'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('source-unit implementation changed')
    context_report = json.loads(raw['contexts'])
    if manifest['rows'] != capture(context_report, json.loads(raw['selection'])['pairs']):
        raise ValueError('source-unit capture changed')
    judged = validate(json.loads(raw['annotations']), raw['manifest'])
    labels = {(j.thread, j.anchor, j.source, j.index): j for j in judged}
    contexts = {(c['thread'], x['anchor']['ordinal']): x for c in context_report['cases']
                for x in c['controls']['raw64']['contexts']}
    turns = {(c['thread'], t['ordinal']): t for c in json.loads(raw['packets'])['cases'] for t in c['turns']}
    groups = {}
    for row in manifest['rows']:
        groups.setdefault((row['thread'], row['anchor']['ordinal']), []).append(row)
    encoder = Encoder(args.snapshot)
    reports = []
    for key, rows in sorted(groups.items()):
        context, turn = contexts[key], turns[key]
        if digest(turn['text'].encode()) != context['anchor']['searchable_sha256']:
            raise ValueError('query anchor text changed')
        entries = {e['ordinal']: e for e in context['display']}
        keys, texts, judgments = [], [], []
        for row in rows:
            source = row['source']['ordinal']
            for unit in row['units']:
                keys.append({'source_id': row['source']['id'], 'source_ordinal': source, **unit})
                texts.append(entries[source]['text'][unit['start']:unit['end']])
                judgments.append(labels[*key, source, unit['index']])
        lengths = [len(encoder.tokenizer.encode(t).ids) for t in texts]
        methods = {}
        for method in ('recency', *Query):
            if method == 'recency':
                ranking = [(i, None) for i in recency(keys)]
                query_lengths, status = [], 'ranked'
            else:
                queries = query_inputs(turn['text'], context, method)
                query_lengths = [len(encoder.tokenizer.encode(q).ids) for q in queries]
                if not queries or any(not 2 <= n <= 512 for n in (*lengths, *query_lengths)):
                    ranking, status = [], 'unsupported-query-or-candidate-window'
                else:
                    try:
                        ranking = rank_spans(queries, tuple(texts), encoder)
                        status = 'ranked'
                    except ValueError as error:
                        if str(error) != 'invalid or cancelling topic vector':
                            raise
                        ranking, status = [], 'canceling-query-direction'
            methods[method] = {'status': status, 'query_tokens': query_lengths,
                               'ranking': [{**keys[i], 'cosine': score} for i, score in ranking],
                               'topk': {str(k): counts([i for i, _ in ranking[:k]], judgments) for k in (1, 3)}
                                       if status == 'ranked' else None}
        reports.append({'thread': key[0], 'anchor': context['anchor'], 'candidate_units': len(keys),
                        'candidate_tokens': lengths,
                        'reference_counts': dict(Counter(f'{j.relevance}/{j.scope}' for j in judgments)), 'methods': methods})
    report = {'schema_version': 1, 'scope': 'development-two-axis-passage-ranking-diagnostic',
              'input_sha256': {k: digest(v) for k, v in raw.items()},
              'code_sha256': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                              ('passage_ranking.py', 'span_selection.py', 'local_embeddings.py', 'data.py', 'uv.lock')},
              'parameters': {'topk': [1, 3], 'candidate_scope': 'post-ranking-oracle-source-pairs',
                             'cosine_ties': 'ascending-manifest-source-order-then-unit-index',
                             'recency_ties': 'ascending-source-unit-reading-order'},
              'encoder': asdict(encoder.statistics), 'reports': reports,
              'elapsed_ms_after_imports': (time.perf_counter() - started) * 1000}
    with args.output.open('x') as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({'anchors': len(reports), 'calls': encoder.statistics.calls,
                      'inference_ms': encoder.statistics.inference_ms, 'elapsed_ms': report['elapsed_ms_after_imports']}))


if __name__ == '__main__':
    main()
