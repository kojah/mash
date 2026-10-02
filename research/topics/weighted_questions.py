"""Preregistered title/prose/code mass and whole-parent graph comparison."""
import argparse
import json
from pathlib import Path
import time

import numpy as np

from channel_weights import View, matrix, weighted
from external_context import compare
from graph_questions import evaluate, inputs, robust_selection
from lexical_memberships import eligibility
from reference_review import digest
from word_graph import Filter, assignments, communities, mixture, significance, source_mass


CHANNEL_PIN = 'ce527aac29659a72872e9430a8bfc534f5c05113f9e31ecdc638cffd22b0b634'
GRAPH_PIN = 'a37376e6d9c34e24f24ec5eb9d7aad3664887193fb063b9a6be0362ffb43a0a6'


def capture(root, name, pin):
    raw = (root / name).read_bytes()
    if digest(raw) != pin:
        raise ValueError('original source capture required')
    result = json.loads(raw)
    for name, expected in result['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('original source/method contract changed')
    return result


def experiment(root):
    started = time.perf_counter()
    channels = capture(root, 'channels-first.json', CHANNEL_PIN)
    original = capture(root, 'graph-first.json', GRAPH_PIN)
    ids, pairs, old_raw, _, words, _, _, old_lock = inputs(root)
    if ids != original['identities'] or pairs != original['rows'] or words != original['words']:
        raise ValueError('original exposed cohort required')
    records = {record['id']: record for record in channels['records']}
    if len(records) != len(channels['records']) or len(records) != 200:
        raise ValueError('complete unique source-channel cohort required')
    title, prose, code = [matrix(records, ids, words, field) for field in ('title', 'prose', 'block-code')]
    full = title + prose + code
    whole = np.array([[records[i]['title_counts'].get(w, 0) + records[i]['whole_counts'].get(w, 0)
                       for w in words] for i in ids])
    if not np.array_equal(full, whole):
        raise ValueError('full-source unigram conservation failed')
    views, graphs, models, results, timing = {}, {}, {}, {}, {}
    for view in View:
        before = time.perf_counter()
        counts = weighted(title, prose, code, view)
        mask = eligibility(counts, words, 2, .40)
        admitted = np.where(mask, counts, 0)
        views[view] = {'counts': counts.tolist(), 'eligible': mask.tolist(), 'admitted_counts': admitted.tolist(),
                       'eligible_words': int(mask.sum()), 'retained_mass': admitted.sum(1).tolist(),
                       'zero_evidence': [i for i, row in zip(ids, admitted, strict=True) if not row.any()]}
        timing[f'view/{view}'] = (time.perf_counter() - before) * 1000
        for policy in Filter:
            before = time.perf_counter()
            graph = significance(admitted, policy)
            key = f'{view}/{policy}'
            graphs[key] = {'df': graph.df.tolist(), 'pairs': graph.pairs.tolist(), 'counts': graph.counts.tolist(),
                           'probabilities': graph.probabilities.tolist(), 'selected': graph.selected.tolist(), 'cutoff': graph.cutoff}
            timing[f'graph/{key}'] = (time.perf_counter() - before) * 1000
            for seed in (17, 29, 101):
                before = time.perf_counter()
                groups, codelength = communities(graph, seed)
                hard = source_mass(admitted, groups)
                soft, phi, trajectory = mixture(admitted, groups)
                models[f'{key}/{seed}'] = {'groups': [list(g) for g in groups], 'codelength': codelength,
                    'covered_words': sum(map(len, groups)), 'isolated_words': len(words) - sum(map(len, groups)),
                    'hard_scores': hard.tolist(), 'mixture_scores': soft.tolist(), 'phi': phi.tolist(),
                    'likelihood': trajectory, 'status': 'supported' if groups else 'no-significant-edges'}
                timing[f'model/{key}/{seed}'] = (time.perf_counter() - before) * 1000
                for mode, scores in (('hard', hard), ('mixture', soft)):
                    for threshold in (.10, .20, .30):
                        for budget in (3, 8):
                            result_key = f'{key}/{mode}/{threshold:.2f}/{budget}/{seed}'
                            results[result_key] = evaluate(ids, pairs, assignments(scores, threshold, budget))
    timing['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed64calibration-title-prose-code-mass-no-semantic-ancestors',
            'policy': {'channel_pin': CHANNEL_PIN, 'graph_pin': GRAPH_PIN,
                       'vocabulary': 'frozen629-source-only-unigrams-transductive',
                       'views': [str(v) for v in View], 'empty_channels': 'mean-over-nonempty-admitted-channels',
                       'eligibility': 'DF2-through-.40-per-view-full629-node-test-family',
                       'mass_denominator': 'eligible-view-word-mass-including-isolates',
                       'model': original['policy'], 'archived_lock_sha256': old_lock, 'inference_calls': 0},
            'identities': ids, 'rows': pairs, 'words': words,
            'source_counts': {'title': title.tolist(), 'prose': prose.tolist(), 'block-code': code.tolist()},
            'old_unit_count_differences': {'entries': int((full != old_raw).sum()),
                                           'absolute_count_difference': int(np.abs(full - old_raw).sum())},
            'views': views, 'graphs': graphs, 'models': models, 'results': results, 'winner': robust_selection(results),
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('channel_weights.py', 'weighted_questions.py', 'channel_audit.py', 'graph_questions.py',
                           'word_graph.py', 'lexical_memberships.py', 'uv.lock')}, 'timings': timing}


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
    print(json.dumps({'stage': 'complete', 'configurations': len(result['results']), 'winner': result['winner'],
                      'total_ms': result['timings']['total_ms']}))


if __name__ == '__main__':
    main()
