"""Exposed programming-source representation calibration with an unchanged precision gate."""
import argparse
from dataclasses import asdict
from enum import StrEnum
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np

from data import normalized
from external_clustering import boolean_metrics
from external_context import compare
from external_hierarchy import fit_tree
from lexical_context import lexical_features, serialized_policy
from online_tree import Linkage, Mode
from question_html import render
from question_topics import parent_memberships
from question_transfer import question_pairs
from reference_review import digest
from stackoverflow_source import load, split


PINS = {'transfer-first.json': 'a77d5efeb2ce4a816419bf6ed3efc1c5bb0dfcde26988c91c6280293db619333',
        'body-first.json': '057763eb18aea347ad3740495e1e8c368fab2172f016868cbe9c69fc19f87f7f',
        'units-final-first.json': 'ede6119b31439830c7cbf1f01f53e185e8cffe3a4957722b23bb0b205b1850cd'}
CUTS = [.1, .2, .3, .4, .5, .6, .7, .8, .9]
MIN_PRECISION = .70
MIN_LINKS = 20


class View(StrEnum):
    TITLE = 'title'
    MEAN = 'unit-mean'
    LEXICAL = 'lexical'
    TITLE_FUSION = 'title75lex25'
    MEAN_FUSION = 'mean75lex25'
    UNITS = 'unit-union'


def fuse(dense, lexical):
    if dense.shape[0] != lexical.shape[0]:
        raise ValueError('aligned dense and lexical inputs required')
    return np.concatenate([np.sqrt(.75) * dense, np.sqrt(.25) * lexical], axis=1)


def passes(metric):
    return metric['precision'] is not None and metric['precision'] >= MIN_PRECISION and metric['tp'] + metric['fp'] >= MIN_LINKS


def choose(results, view, linkage, *, robust=True):
    candidates = []
    for key, result in results.items():
        if result['candidate']['view'] != view or result['candidate']['linkage'] != linkage:
            continue
        metrics = [r['metrics'] for r in result['orders'].values()] if robust else [result['orders']['forward']['metrics']]
        if all(passes(m) for m in metrics):
            candidates.append((key, min(m['recall'] for m in metrics), min(m['precision'] for m in metrics)))
    return min(candidates, key=lambda c: (-c[1], -c[2], c[0]))[0] if candidates else None


def experiment(root):
    started = time.perf_counter()
    saved = {}
    for name, pin in PINS.items():
        raw = (root / name).read_bytes()
        if digest(raw) != pin:
            raise ValueError('original capture required: ' + name)
        saved[name] = json.loads(raw)
        for module, expected in saved[name]['code_pins'].items():
            if digest(Path(__file__).with_name(module).read_bytes()) != expected:
                raise ValueError('frozen source contract drift: ' + module)
    title, body, units = [saved[n] for n in PINS]
    if serialized_policy() != body['policy']['lexical']:
        raise ValueError('original lexical policy drift')
    corpus = load(root)
    cal, _, background = split(corpus)
    questions = {str(q.id): q for q in cal}
    ids = units['phases']['calibration-source']['identities']
    if len(ids) != 64 or set(ids) != set(questions):
        raise ValueError('complete original calibration support required')
    text = {i: questions[i].title + '\n\n' + render(questions[i].body_html).text for i in ids}
    for i in ids:
        q = questions[i]
        if digest(q.title.encode()) != title['phases']['calibration-source']['records'][i]['title_sha256'] or digest(text[i].encode()) != body['records'][i]['input_sha256']:
            raise ValueError('calibration source identity drift')
    bg_text = sorted({q.title + '\n\n' + render(q.body_html).text for q in background}, key=lambda t: (digest(t.encode()), t))
    if [digest(t.encode()) for t in bg_text] != body['background']['input_hashes']:
        raise ValueError('disjoint background identity drift')
    vectorizer, sparse, nnz = lexical_features(bg_text, [text[i] for i in ids])
    if vectorizer.get_feature_names_out().tolist() != body['background']['vocabulary'] or vectorizer.idf_.tolist() != body['background']['idf'] or nnz != body['background']['nnz']:
        raise ValueError('frozen lexical basis drift')
    for n, i in enumerate(ids):
        row = sparse.getrow(n)
        if {'indices': row.indices.tolist(), 'values': row.data.tolist()} != body['lexical_records'][i]:
            raise ValueError('frozen lexical source vector drift')
    titles = np.array([title['phases']['calibration-source']['records'][i]['vector'] for i in ids])
    means = np.array([units['mean_vectors'][i] for i in ids])
    point_ids = [i + '/' + h for i in ids for h in dict.fromkeys(units['input_hashes'][i])]
    unit_vectors = np.array([units['vectors'][p.split('/')[1]] for p in point_ids])
    for i, mean in zip(ids, means):
        original = np.array([units['vectors'][h] for h in dict.fromkeys(units['input_hashes'][i])])
        if not np.array_equal(normalized(original.mean(0)), mean):
            raise ValueError('frozen unit aggregation drift')
    lexical = sparse.toarray()
    views = {View.TITLE: titles, View.MEAN: means, View.LEXICAL: lexical,
             View.TITLE_FUSION: fuse(titles, lexical), View.MEAN_FUSION: fuse(means, lexical), View.UNITS: unit_vectors}
    reference = {str(r.id): r.tags for r in corpus.references if str(r.id) in questions}
    rows = question_pairs(ids, reference)
    if rows != units['phases']['calibration-source']['rows']:
        raise ValueError('natural calibration pair grid drift')
    results, timings, stats = {}, {}, {}
    for view, matrix in views.items():
        points = point_ids if view == View.UNITS else ids
        parents = {p: p.split('/')[0] for p in points}
        orders = {'forward': list(range(len(points))), 'reverse': list(reversed(range(len(points)))),
                  'shuffled': np.random.default_rng(17).permutation(len(points)).tolist()}
        for linkage in Linkage:
            for order, indices in orders.items():
                tree, timing = fit_tree(points, matrix, indices, Mode.GRINCH, linkage)
                tree.validate()
                stats[f'{view}/{linkage}/{order}'] = asdict(tree.statistics)
                timings[f'{view}/{linkage}/{order}'] = timing
                for cut in CUTS:
                    groups = sorted(sorted(tree.identities[n] for n in group) for group in tree.cut(cut))
                    tags = parent_memberships(groups, parents)
                    predictions = [bool(set(tags[r['left']]) & set(tags[r['right']])) for r in rows]
                    metric = boolean_metrics([r['tag_overlap'] for r in rows], predictions)
                    key = f'{view}/{linkage}/{cut:.1f}'
                    result = results.setdefault(key, {'candidate': {'view': view, 'linkage': linkage, 'cut': cut}, 'orders': {}})
                    result['orders'][order] = {'groups': groups, 'parent_memberships': tags, 'predictions': predictions,
                        'metrics': metric, 'passes': passes(metric),
                        'single_parent_topics': sum(len({parents[p] for p in g}) == 1 for g in groups),
                        'question_topic_p95': float(np.percentile([len(t) for t in tags.values()], 95)),
                        'maximum_question_topics': max(map(len, tags.values()))}
    winners = {str(v): {str(l): choose(results, v, l) for l in Linkage} for v in View}
    forward = {str(v): {str(l): choose(results, v, l, robust=False) for l in Linkage} for v in View}
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed-original-question-calibration-only-no-evaluation',
            'policy': {'input_hashes': PINS, 'cuts': CUTS, 'minimum_precision': MIN_PRECISION, 'minimum_links': MIN_LINKS,
                       'selection': 'all-orders-pass-then-max-worst-recall-worst-precision-lexical-key',
                       'source': 'original-calibration64-only', 'evaluation': 'requires-new-groups', 'new_inference': 0},
            'identities': ids, 'rows': rows, 'views': {str(v): {'shape': list(m.shape), 'sha256': digest(np.asarray(m, dtype='<f8').tobytes())} for v, m in views.items()},
            'results': results, 'winners': winners, 'forward_only_winners': forward, 'fit_statistics': stats,
            'versions': {n: importlib.metadata.version(n) for n in ('numpy', 'scikit-learn')},
            'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                          ('question_calibration.py', 'online_tree.py', 'external_hierarchy.py', 'question_topics.py',
                           'lexical_context.py', 'data.py', 'uv.lock')}, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'candidates': len(result['results']), 'winners': result['winners']}))


if __name__ == '__main__':
    main()
