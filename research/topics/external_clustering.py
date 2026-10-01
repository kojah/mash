"""Frozen insert-only clustering diagnostics; annotations never enter fitting."""
import argparse
from dataclasses import asdict
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np
from sklearn.cluster import Birch
from sklearn.metrics import adjusted_rand_score
from threadpoolctl import threadpool_limits

from external_analysis import CAPTURE, prepare, evaluation_grid
from external_context import compare
from multiwoz import load
from online_tree import Mode, Tree
from reference_review import digest


LEXICAL = '21e9b729899037ac8f8680ffcc5b2515f2f76fb3cad2fdfcd2e2723744ea4b48'


def representations(identities, vectors, lexical):
    if not identities or len(set(identities)) != len(identities):
        raise ValueError('unique nonempty identities required')
    sparse = np.zeros((len(identities), len(lexical['vocabulary'])), dtype=np.float64)
    for row, identity in enumerate(identities):
        record = lexical['records'][identity]
        indices, values = record['indices'], record['values']
        if record['reason'] is not None or len(indices) != len(values) or len(set(indices)) != len(indices):
            raise ValueError('supported distinct lexical coordinates required')
        if any(type(i) is not int or not 0 <= i < sparse.shape[1] for i in indices):
            raise ValueError('invalid lexical coordinate')
        sparse[row, indices] = values
    result = {'current': np.stack([vectors['current'][i] for i in identities]),
              'joined': np.stack([vectors['joined'][i] for i in identities]), 'lexical': sparse}
    for name in ('current', 'joined'):
        result[f'{name}_75_lexical_25'] = np.concatenate((np.sqrt(.75) * result[name], .5 * sparse), axis=1)
    if any(not np.isfinite(v).all() or not np.allclose(np.linalg.norm(v, axis=1), 1, atol=1e-12, rtol=0)
           for v in result.values()):
        raise ValueError('finite unit representations required')
    return result


def memberships(clusters, identities):
    flattened = [i for c in clusters for i in c]
    if any(not c for c in clusters) or len(flattened) != len(set(flattened)) or set(flattened) != set(identities):
        raise ValueError('exact disjoint complete partition required')
    return {i: n for n, c in enumerate(sorted(tuple(sorted(c)) for c in clusters)) for i in c}


def boolean_metrics(truth, predictions):
    if len(truth) != len(predictions) or any(type(v) is not bool for v in (*truth, *predictions)):
        raise ValueError('aligned literal boolean outcomes required')
    counts = {'tp': 0, 'fp': 0, 'tn': 0, 'fn': 0}
    for gold, predicted in zip(truth, predictions):
        counts['tp' if predicted and gold else 'fp' if predicted else 'fn' if gold else 'tn'] += 1
    tp, fp, fn = counts['tp'], counts['fp'], counts['fn']
    return {**counts, 'pairs': len(truth), 'precision': tp / (tp + fp) if tp + fp else None,
            'recall': tp / (tp + fn) if tp + fn else None,
            'f1': 2 * tp / (2 * tp + fp + fn) if 2 * tp + fp + fn else None}


def pair_metrics(rows, assignment, gold_key):
    return boolean_metrics([row[gold_key] for row in rows],
                           [assignment[row['left']] == assignment[row['right']] for row in rows])


def fit(identities, matrix, indices, method, threshold=None):
    if sorted(indices) != list(range(len(identities))):
        raise ValueError('arrival must be an exact permutation')
    if matrix.ndim != 2 or matrix.shape[0] != len(identities) or not 1 <= len(identities) <= 1024 or not 1 <= matrix.shape[1] <= 8192:
        raise ValueError('bounded aligned matrix required')
    if not np.isfinite(matrix).all() or not np.allclose(np.linalg.norm(matrix, axis=1), 1, atol=1e-12, rtol=0):
        raise ValueError('finite unit vectors required')
    tree = Tree(Mode.GRINCH, maximum_leaves=len(identities), maximum_dimensions=8192) if method == 'grinch' else None
    if method not in ('grinch', 'birch'):
        raise ValueError('unknown method')
    threshold = (.4 if method == 'birch' else .7) if threshold is None else threshold
    if not np.isfinite(threshold) or not 0 < threshold <= 1:
        raise ValueError('bounded positive threshold required')
    model = Birch(threshold=threshold, branching_factor=64, n_clusters=None) if tree is None else None
    ordered = [identities[i] for i in indices]
    samples = []
    start, cpu = time.perf_counter(), time.process_time()
    with threadpool_limits(limits=1):
        for i in indices:
            before = time.perf_counter()
            if tree is not None:
                tree.insert(identities[i], matrix[i])
            else:
                model.partial_fit(matrix[i:i + 1])
            samples.append((time.perf_counter() - before) * 1000)
    timings = {'fit_wall_ms': (time.perf_counter() - start) * 1000,
               'fit_cpu_ms': (time.process_time() - cpu) * 1000,
               'insert_p95_ms': float(np.percentile(samples, 95)), 'insert_ms': samples}
    if tree is not None:
        tree.validate()
        clusters = [sorted(ordered[i] for i in members) for members in tree.cut(threshold)]
    else:
        # Final nearest-CF prediction is not the insertion-time membership.
        with threadpool_limits(limits=1):
            assigned = model.predict(matrix)
        clusters = [[identities[i] for i in range(len(identities)) if assigned[i] == label]
                    for label in sorted(set(map(int, assigned)))]
    clusters = sorted(clusters)
    memberships(clusters, identities)
    return {'clusters': clusters, 'sizes': sorted(map(len, clusters)),
            'singletons': sum(len(c) == 1 for c in clusters),
            'statistics': asdict(tree.statistics) if tree is not None else None}, timings


def experiment(root, capture_path, lexical_path):
    raw, lex_raw = capture_path.read_bytes(), lexical_path.read_bytes()
    if digest(raw) != CAPTURE or digest(lex_raw) != LEXICAL:
        raise ValueError('frozen representation capture drift')
    capture, lexical = json.loads(raw), json.loads(lex_raw)
    versions = {n: importlib.metadata.version(n) for n in ('numpy', 'scikit-learn')}
    if versions != lexical['versions']:
        raise ValueError('frozen dependency version drift')
    anchors, vectors, common, groups = prepare(load(root), capture)
    if anchors != lexical['anchors'] or set(lexical['records']) != common:
        raise ValueError('lexical/source coverage drift')
    identities = sorted(common)
    views = representations(identities, vectors, lexical)
    rows = evaluation_grid(anchors, vectors, common)
    # Preserve complete archived pair truth, independently of fitted memberships.
    for original, archived in zip(rows, lexical['common_pairs'], strict=True):
        if any(original[k] != archived[k] for k in original if k != 'scores'):
            raise ValueError('evaluation pair drift')
    orders = {'forward': list(range(len(identities))), 'reverse': list(reversed(range(len(identities)))),
              'shuffled': np.random.default_rng(17).permutation(len(identities)).tolist()}
    results, timings = {}, {}
    for view, matrix in views.items():
        for method in ('birch', 'grinch'):
            forward = None
            for order, indices in orders.items():
                key = f'{view}/{method}/{order}'
                fitted, timings[key] = fit(identities, matrix, indices, method)
                assigned = memberships(fitted['clusters'], identities)
                if forward is None:
                    forward = assigned
                fitted['forward_ari'] = float(adjusted_rand_score([forward[i] for i in identities], [assigned[i] for i in identities]))
                fitted['metrics'] = {'service': pair_metrics(rows, assigned, 'service_overlap'),
                                     'intent': pair_metrics(rows, assigned, 'intent_overlap'),
                                     'within_service_intent': pair_metrics([r for r in rows if r['service_overlap']], assigned, 'intent_overlap')}
                results[key] = fitted
    return {'version': 1, 'vector_capture_sha256': CAPTURE, 'lexical_capture_sha256': LEXICAL,
            'versions': versions, 'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                ('external_clustering.py', 'external_analysis.py', 'online_tree.py', 'data.py', 'multiwoz.py')},
            'parameters': {'birch_threshold': .4, 'branching_factor': 64, 'n_clusters': None,
                           'tree_cut': .7, 'shuffle_seed': 17, 'dense_weight': .75,
                           'maximum_dimensions': 8192, 'maximum_leaves': len(identities)},
            'identities': identities, 'groups': groups, 'arrival_orders': orders,
            'fit_support': len(common), 'evaluation_pairs': len(rows),
            'dimensions': {k: v.shape[1] for k, v in views.items()},
            'results': results, 'new_inference_calls': 0, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'capture', 'lexical', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh capture outside Git required')
    result = experiment(args.root, args.capture, args.lexical)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({k: {n: v for n, v in r.items() if n not in ('clusters', 'statistics', 'sizes')}
                      for k, r in result['results'].items()}))


if __name__ == '__main__':
    main()
