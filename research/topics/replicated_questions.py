"""Preregistered distinct-parent replication/weight controls on frozen windows."""
import argparse
import json
from pathlib import Path
import time

import numpy as np

from external_context import compare
from graph_questions import evaluate, inputs, robust_selection
from parent_support import Weight, replicated, supports
from reference_review import digest
from word_graph import Graph, assignments, communities, mixture, source_mass


WINDOW_PIN = '62133ec99657c8bd9bf6e6e86473218a2e27347aae206d46922a337e347e0f50'


def experiment(root):
    started = time.perf_counter()
    raw_capture = (root / 'windows-first.json').read_bytes()
    if digest(raw_capture) != WINDOW_PIN:
        raise ValueError('original local-window capture required')
    baseline = json.loads(raw_capture)
    for name, expected in baseline['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('original window method/source changed')
    ids, pairs, raw, counts, words, _, _, old_lock = inputs(root)
    if (ids != baseline['identities'] or pairs != baseline['rows'] or words != baseline['words']
            or raw.tolist() != baseline['raw_counts'] or counts.tolist() != baseline['balanced_counts']):
        raise ValueError('matched question inputs changed')
    support_records, graphs, models, results, timing = {}, {}, {}, {}, {}
    for size in ('16', '32', '64'):
        before = time.perf_counter()
        original = baseline['graphs'][size + '/p05']
        support, events = supports(baseline['source_windows'][size]['windows'], ids, len(words), original['pairs'])
        support_records[size] = {'counts': support.tolist(), 'pair_expansion_events': events}
        timing[f'support/{size}'] = (time.perf_counter() - before) * 1000
        for policy in ('p05', 'bh05'):
            source = baseline['graphs'][size + '/' + policy]
            if source['pairs'] != original['pairs']:
                raise ValueError('original complete pair family changed')
            graph = Graph(np.array(source['df']), np.array(source['pairs']), np.array(source['counts']),
                          np.array(source['probabilities']), np.array(source['selected']), source['cutoff'])
            for weight in Weight:
                weighted = replicated(graph, support, weight)
                graph_key = f'{size}/{policy}/{weight}'
                graphs[graph_key] = {'source_graph': size + '/' + policy,
                                    'weights': weighted.counts.tolist(), 'selected': weighted.selected.tolist(),
                                    'edges': int(weighted.selected.sum())}
                for seed in (17, 29, 101):
                    before = time.perf_counter()
                    groups, codelength = communities(weighted, seed)
                    hard = source_mass(counts, groups)
                    soft, phi, trajectory = mixture(counts, groups)
                    models[f'{graph_key}/{seed}'] = {'groups': [list(g) for g in groups], 'codelength': codelength,
                        'covered_words': sum(map(len, groups)), 'isolated_words': len(words) - sum(map(len, groups)),
                        'hard_scores': hard.tolist(), 'mixture_scores': soft.tolist(), 'phi': phi.tolist(),
                        'likelihood': trajectory, 'status': 'supported' if groups else 'no-replicated-edges'}
                    timing[f'model/{graph_key}/{seed}'] = (time.perf_counter() - before) * 1000
                    for mode, scores in (('hard', hard), ('mixture', soft)):
                        for threshold in (.10, .20, .30):
                            for budget in (3, 8):
                                key = f'{graph_key}/{mode}/{threshold:.2f}/{budget}/{seed}'
                                results[key] = evaluate(ids, pairs, assignments(scores, threshold, budget))
    timing['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed64calibration-replicated-local-pairs-no-semantic-ancestors',
            'policy': {'window_capture_sha256': WINDOW_PIN, 'minimum_distinct_parents': 2,
                       'pair_expansion_cap': 2_000_000, 'weights': [str(w) for w in Weight],
                       'probabilities': 'original-window-tests-unchanged-no-FDR-guarantee',
                       'model': baseline['policy'], 'archived_lock_sha256': old_lock, 'inference_calls': 0},
            'identities': ids, 'rows': pairs, 'words': words, 'balanced_counts': counts.tolist(),
            'parent_support': support_records, 'graphs': graphs, 'models': models, 'results': results,
            'winner': robust_selection(results),
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('parent_support.py', 'replicated_questions.py', 'word_graph.py', 'graph_questions.py',
                           'window_questions.py', 'uv.lock')}, 'timings': timing}


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
