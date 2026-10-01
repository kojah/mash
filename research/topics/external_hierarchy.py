"""Cut-free hierarchy retrieval and arrival stability on exposed frozen samples."""
import argparse
from dataclasses import asdict
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np
from threadpoolctl import threadpool_limits

from cluster_calibration import MODEL, TOKENIZER, plan, split
from external_analysis import evaluation_grid, weighted_metrics
from external_clustering import representations
from external_context import compare
from hierarchy_metrics import triplet_signature
from multiwoz import PINS, Speaker, load
from online_tree import Linkage, Mode, Tree
from reference_review import digest


CAPTURES = {'calibration': '18fdb773b198d3f592738b5d39aaa90c5339658e0b11e57aa0f7b619cd05da04',
            'reserved-evaluation': '8819efd81ba90111f56a9cd6b2c79c4202ff71ca4b98fba3b0877c04c89e1954'}


def frozen_phase(corpus, path, phase):
    raw = path.read_bytes()
    if digest(raw) != CAPTURES[phase]:
        raise ValueError('frozen phase capture drift')
    capture = json.loads(raw)
    if capture['phase'] != phase or capture['policy']['source_pins'] != PINS or capture['policy']['model'] != MODEL or capture['policy']['tokenizer'] != TOKENIZER:
        raise ValueError('source/model contract drift')
    versions = {n: importlib.metadata.version(n) for n in capture['versions']}
    if versions != capture['versions']:
        raise ValueError('frozen inference/evaluation dependency drift')
    original, calibration, reserved = split(corpus)
    expected_split = {name: [{'id': d.id, 'group': d.group} for d in ds] for name, ds in
                      [('exposed', original), ('calibration', calibration), ('reserved', reserved)]}
    if capture['split'] != expected_split:
        raise ValueError('source split drift')
    ds = calibration if phase == 'calibration' else reserved
    plans = plan(ds)
    labels = {(l.dialogue, l.ordinal): l for l in corpus.labels}
    expected = {}
    for d in ds:
        for t in d.turns:
            if t.speaker != Speaker.USER:
                continue
            l = labels[t.dialogue, t.ordinal]
            expected[f'{d.id}:{t.ordinal}'] = {'group': d.group, 'active': list(l.active),
                'unknown': [list(i) for i in l.unknown_intents], 'intents': [list(i) for i in l.declared_intents],
                'text_sha256': digest(t.text.encode())}
    if expected != capture['anchors'] or set(plans) != set(capture['records']):
        raise ValueError('source anchor or record coverage drift')
    vectors = {'current': {}, 'joined': {}}
    for key, views in plans.items():
        for view, texts in views.items():
            record = capture['records'][key][view]
            if record['input_hashes'] != [digest(t.encode()) for t in texts]:
                raise ValueError('original inference input changed')
            if record['reason'] is None:
                v = np.asarray(record['vector'], dtype=np.float64)
                if v.shape != (384,) or not np.isfinite(v).all() or not np.isclose(np.linalg.norm(v), 1, atol=1e-12, rtol=0):
                    raise ValueError('invalid frozen vector')
                vectors[view][key] = v
            elif record['vector'] is not None:
                raise ValueError('abstaining record contains a vector')
    identities = capture['fit_identities']
    eligible = sorted(i for i in set(vectors['current']) & set(vectors['joined'])
                      if capture['lexical']['records'][i]['reason'] is None)
    if identities != eligible:
        raise ValueError('matched fit support changed')
    views = representations(identities, vectors, capture['lexical'])
    rows = evaluation_grid(expected, vectors, identities)
    if rows != capture['rows']:
        raise ValueError('original complete pair grid changed')
    return capture, identities, views, rows


def fit_tree(identities, matrix, indices, mode, linkage=Linkage.CENTROID):
    if not identities or len(identities) > 1024 or len(set(identities)) != len(identities) or sorted(indices) != list(range(len(identities))):
        raise ValueError('bounded unique identities and complete arrival permutation required')
    if matrix.ndim != 2 or matrix.shape[0] != len(identities) or not 1 <= matrix.shape[1] <= 8192 or not np.isfinite(matrix).all():
        raise ValueError('bounded aligned finite matrix required')
    if not np.allclose(np.linalg.norm(matrix, axis=1), 1, atol=1e-12, rtol=0):
        raise ValueError('unit input vectors required')
    tree = Tree(mode, maximum_leaves=len(identities), maximum_dimensions=8192, linkage=linkage)
    start, cpu, samples = time.perf_counter(), time.process_time(), []
    with threadpool_limits(limits=1):
        for index in indices:
            before = time.perf_counter()
            tree.insert(identities[index], matrix[index])
            samples.append((time.perf_counter() - before) * 1000)
    return tree, {'fit_wall_ms': (time.perf_counter() - start) * 1000,
                  'fit_cpu_ms': (time.process_time() - cpu) * 1000,
                  'insert_p95_ms': float(np.percentile(samples, 95)), 'insert_ms': samples}


def topology(tree, rows):
    tree.validate()
    nodes = dict(zip(tree.identities, tree.leaves))
    clades, canonical = {}, {}
    for node in tree.nodes.values():
        if node.children is None:
            continue
        members = sorted(tree.identities[i] for i in tree.leaf_indices(node))
        key = digest(json.dumps(members, separators=(',', ':')).encode())
        if len(members) != node.count or key in clades:
            raise ValueError('noncanonical or stale clade')
        clades[key] = members
        canonical[node.serial] = key
    pairs = []
    for row in rows:
        ancestor = tree.lca(nodes[row['left']], nodes[row['right']])
        pairs.append({'left': row['left'], 'right': row['right'], 'lca': canonical[ancestor.serial],
                      'leaves': ancestor.count, 'score': -ancestor.count})
    signature = triplet_signature(tree, maximum=1024, seed=20261001)
    # Preserve explicit JSON-array schema after a round-trip, without broad coercion.
    signature = {'seed': signature['seed'], 'triplets': [list(t) for t in signature['triplets']],
                 'closest_pairs': [list(t) for t in signature['closest_pairs']]}
    return {'clades': clades, 'root': canonical.get(tree.root.serial), 'pairs': pairs,
            'signature': signature, 'statistics': asdict(tree.statistics)}


def stability(forward, other):
    a, b = forward['signature'], other['signature']
    if a['seed'] != b['seed'] or a['triplets'] != b['triplets']:
        raise ValueError('triplet support changed')
    first = set(forward['clades']) - {forward['root']}
    second = set(other['clades']) - {other['root']}
    return {'triplets': len(a['triplets']),
            'triplet_agreement': sum(x == y for x, y in zip(a['closest_pairs'], b['closest_pairs'], strict=True)) / len(a['triplets']) if a['triplets'] else None,
            'clade_jaccard': len(first & second) / len(first | second) if first | second else None}


def metrics(rows, scores):
    if len(rows) != len(scores):
        raise ValueError('complete pair score coverage required')
    result = {}
    for name, gold in [('service', 'service_overlap'), ('intent', 'intent_overlap'), ('within_service_intent', 'intent_overlap')]:
        indices = [i for i, row in enumerate(rows) if name != 'within_service_intent' or row['service_overlap']]
        truth = [rows[i][gold] for i in indices]
        result[name] = {'pairs': len(indices), 'positive': sum(truth), 'negative': len(indices) - sum(truth),
                        'metrics': weighted_metrics(truth, [scores[i] for i in indices], np.ones(len(indices)))}
    return result


def experiment(root, calibration_path, evaluation_path, linkage=Linkage.CENTROID):
    start = time.perf_counter()
    corpus = load(root)
    results, timings, inputs_meta = {}, {}, {}
    for phase, path in [('calibration', calibration_path), ('reserved-evaluation', evaluation_path)]:
        capture, identities, views, rows = frozen_phase(corpus, path, phase)
        orders = {'forward': list(range(len(identities))), 'reverse': list(reversed(range(len(identities)))),
                  'shuffled': np.random.default_rng(17).permutation(len(identities)).tolist()}
        positions = {identity: i for i, identity in enumerate(identities)}
        inputs_meta[phase] = {'capture_sha256': CAPTURES[phase], 'identities': identities, 'rows': rows,
                              'arrival_orders': orders, 'dimensions': {v: m.shape[1] for v, m in views.items()},
                              'raw_cosine': {v: metrics(rows, [float(m[positions[r['left']]] @ m[positions[r['right']]]) for r in rows])
                                             for v, m in views.items()}, 'original_encoder': capture['encoder']}
        for view, matrix in views.items():
            for mode in Mode:
                forward = None
                for order, indices in orders.items():
                    key = f'{phase}/{view}/{mode}/{order}'
                    tree, timings[key] = fit_tree(identities, matrix, indices, mode, linkage)
                    before = time.perf_counter()
                    result = topology(tree, rows)
                    if forward is None:
                        forward = result
                    result['stability'] = stability(forward, result)
                    result['metrics'] = metrics(rows, [p['score'] for p in result['pairs']])
                    timings[key]['evaluation_ms'] = (time.perf_counter() - before) * 1000
                    results[key] = result
    timings['total_ms'] = (time.perf_counter() - start) * 1000
    return {'version': 2, 'scope': 'exposed-development-cut-free-hierarchy-diagnostic',
            'policy': {'score': 'negative-all-fit-leaf-count-of-LCA', 'triplets': 1024,
                       'triplet_seed': 20261001, 'shuffle_seed': 17, 'root_leaves_excluded_from_clade_jaccard': True,
                       'new_inference_calls': 0, 'threshold_selection': False, 'linkage': Linkage(linkage)},
            'versions': {n: importlib.metadata.version(n) for n in ('numpy', 'scikit-learn', 'threadpoolctl')},
            'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                ('external_hierarchy.py', 'cluster_calibration.py', 'external_clustering.py', 'external_analysis.py',
                 'online_tree.py', 'hierarchy_metrics.py', 'multiwoz.py', 'data.py', 'uv.lock')},
            'inputs': inputs_meta, 'results': results, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'calibration', 'evaluation', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git capture required')
    result = experiment(args.root, args.calibration, args.evaluation)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'controls': len(result['results']), 'new_inference_calls': 0,
                      'total_ms': result['timings']['total_ms']}))


if __name__ == '__main__':
    main()
