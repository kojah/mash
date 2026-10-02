"""Preregistered online count-mixture comparison on exposed programming sources."""
import argparse
import json
from pathlib import Path
import time

import numpy as np

from external_context import compare
from graph_questions import evaluate, inputs, robust_selection
from online_lda import fit
from reference_review import digest
from weighted_questions import GRAPH_PIN, capture
from word_graph import assignments


def experiment(root):
    started = time.perf_counter()
    original = capture(root, 'graph-first.json', GRAPH_PIN)
    ids, pairs, counts, _, words, _, _, archived_lock = inputs(root)
    if ids != original['identities'] or words != original['words'] or counts.tolist() != original['raw_counts']:
        raise ValueError('original count/source contract required')
    orders = {'forward': list(range(len(ids))), 'reverse': list(reversed(range(len(ids)))),
              'shuffle17': np.random.default_rng(17).permutation(len(ids)).tolist()}
    models, results, timings = {}, {}, {}
    for topics in (8, 16, 32):
        for epochs in (1, 10):
            for name, order in orders.items():
                key = f'k{topics}/epochs{epochs}/{name}'
                model, timing = fit(counts, order, topics, epochs)
                phi = np.asarray(model['phi'])
                labels = []
                for topic in range(topics):
                    candidates = sorted(range(len(words)), key=lambda j: (-phi[topic, j], words[j]))[:8]
                    labels.append([{'term': words[j], 'probability': float(phi[topic, j]),
                                    'source_support': [ids[i] for i in range(len(ids)) if counts[i, j] > 0]}
                                   for j in candidates])
                model['labels'] = labels
                models[key] = model
                timings[key] = timing
                for threshold in (.10, .20, .30):
                    for budget in (3, 8):
                        final = assignments(model['final_scores'], threshold, budget)
                        initial = assignments(model['first_arrival_scores'], threshold, budget)
                        result = evaluate(ids, pairs, final)
                        result['first_arrival_memberships'] = dict(zip(ids, initial, strict=True))
                        result['changed_sources'] = [i for i, a, b in zip(ids, initial, final, strict=True) if set(a) != set(b)]
                        results[f'k{topics}/epochs{epochs}/{threshold:.2f}/{budget}/{name}'] = result
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed64-fixedK-online-LDA-no-hierarchy-or-lifecycle',
            'policy': {'graph_capture_sha256': GRAPH_PIN, 'archived_lock_sha256': archived_lock,
                       'topics': [8, 16, 32], 'epochs': [1, 10], 'batch_size': 16, 'model_seed': 17,
                       'alpha_eta': '1/K', 'learning_decay': .7, 'learning_offset': 10,
                       'total_samples': 'number-of-nonempty-sources', 'max_doc_update_iter': 100,
                       'mean_change_tol': .001, 'n_jobs': 1, 'embedding_inference_calls': 0,
                       'vocabulary': 'original629-source-only-transductive-unigrams',
                       'labels': 'top8-topic-word-probability-with-lexical-ties-and-original-source-support',
                       'membership': 'final-posterior-threshold-.10/.20/.30-relative-.5-budget3/8'},
            'identities': ids, 'rows': pairs, 'words': words, 'counts': counts.tolist(), 'orders': orders,
            'models': models, 'results': results, 'winner': robust_selection(results),
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('online_lda.py', 'lda_questions.py', 'graph_questions.py', 'word_graph.py',
                           'weighted_questions.py', 'uv.lock')}, 'timings': timings}


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
    print(json.dumps({'stage': 'complete', 'fits': len(result['models']), 'configurations': len(result['results']),
                      'winner': result['winner'], 'total_ms': result['timings']['total_ms']}))


if __name__ == '__main__':
    main()
