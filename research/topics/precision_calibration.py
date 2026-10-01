"""Fresh two-phase proxy precision calibration and multi-tag thread diagnostics."""
import argparse
from dataclasses import asdict
import importlib.metadata
import itertools
import json
from pathlib import Path
import time

import numpy as np
from sklearn.metrics import adjusted_rand_score

from cluster_calibration import MODEL, TOKENIZER, evaluate_partition, extract, plan, split
from external_analysis import evaluation_grid
from external_clustering import boolean_metrics, memberships, representations
from external_context import compare
from external_hierarchy import fit_tree
from lexical_context import background, lexical_features, serialized_policy
from multiwoz import Corpus, PINS, load, select
from online_tree import Linkage, Mode
from reference_review import digest


SEED = 'mash-topic-multiwoz-precision-v1-20261001'
CUTS = [.1, .2, .3, .4, .5, .6, .7, .8, .9]
MIN_PRECISION = .70
MIN_LINKS = 20


def fresh_split(corpus):
    old = tuple(d for group in split(corpus) for d in group)
    exposed = {d.group for d in old}
    remaining = Corpus(tuple(d for d in corpus.dialogues if d.group not in exposed),
                       corpus.labels, corpus.spans, corpus.copies)
    chosen, _ = select(remaining, SEED)
    return old, chosen[:16], chosen[16:]


def pins():
    return {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
            ('precision_calibration.py', 'cluster_calibration.py', 'external_clustering.py', 'external_analysis.py',
             'external_hierarchy.py', 'online_tree.py', 'local_embeddings.py', 'lexical_context.py',
             'external_context.py', 'multiwoz.py', 'data.py', 'uv.lock')}


def target_pass(metric):
    return metric['precision'] is not None and metric['precision'] >= MIN_PRECISION and metric['tp'] + metric['fp'] >= MIN_LINKS


def choose(results, linkage, objective):
    eligible = [(key, r['metrics'][objective]) for key, r in results.items()
                if r['candidate']['linkage'] == linkage and target_pass(r['metrics'][objective])]
    return min(eligible, key=lambda item: (-item[1]['recall'], -item[1]['precision'], item[0]))[0] if eligible else None


def thread_metrics(clusters, identities, anchors, selected_groups):
    assignment = memberships(clusters, identities)
    tags = {g: set() for g in selected_groups}
    services, intents = {g: set() for g in selected_groups}, {g: set() for g in selected_groups}
    for identity in identities:
        anchor = anchors[identity]
        group = anchor['group']
        tags[group].add(assignment[identity])
        if not anchor['unknown'] and anchor['active']:
            services[group].update(anchor['active'])
            intents[group].update(map(tuple, anchor['intents']))
    valid = sorted(g for g in selected_groups if services[g])
    rows = [{'left': a, 'right': b, 'service_overlap': bool(services[a] & services[b]),
             'intent_overlap': bool(intents[a] & intents[b]), 'predicted_overlap': bool(tags[a] & tags[b])}
            for a, b in itertools.combinations(valid, 2)]
    return {'eligible_groups': valid, 'no_eligible_proxy_groups': sorted(set(selected_groups) - set(valid)),
            'direct_tags': {g: sorted(t) for g, t in tags.items()}, 'rows': rows,
            'metrics': {o: boolean_metrics([r[f'{o}_overlap'] for r in rows], [r['predicted_overlap'] for r in rows])
                        for o in ('service', 'intent')}}


def partition(tree, cutoff):
    tree.validate()
    return sorted(sorted(tree.identities[i] for i in members) for members in tree.cut(cutoff))


def experiment(root, snapshot, calibration_path=None, expected_sha=None):
    started = time.perf_counter()
    corpus = load(root)
    old, calibration, reserved = fresh_split(corpus)
    manifest = {name: [{'id': d.id, 'group': d.group} for d in ds] for name, ds in
                [('exposed', old), ('calibration', calibration), ('reserved', reserved)]}
    policy = {'seed': SEED, 'cuts': CUTS, 'minimum_precision': MIN_PRECISION, 'minimum_links': MIN_LINKS,
              'tie': 'maximum-recall-then-precision-then-lexicographic-candidate',
              'lexical': serialized_policy(), 'dense_weight': .75, 'shuffle_seed': 17,
              'source_pins': PINS, 'model': MODEL, 'tokenizer': TOKENIZER, 'objectives': ['service', 'intent']}
    versions = {n: importlib.metadata.version(n) for n in ('numpy', 'scikit-learn', 'onnxruntime', 'tokenizers')}
    frozen = None
    if calibration_path:
        raw = calibration_path.read_bytes()
        if expected_sha is None or digest(raw) != expected_sha:
            raise ValueError('explicit frozen calibration hash required')
        frozen = json.loads(raw)
        if frozen['phase'] != 'calibration' or frozen['policy'] != policy or frozen['split'] != manifest or frozen['code_pins'] != pins() or frozen['versions'] != versions:
            raise ValueError('frozen calibration contract drift')
        recomputed = {l: {o: choose(frozen['results'], l, o) for o in policy['objectives']} for l in Linkage}
        if recomputed != frozen['winners']:
            raise ValueError('calibration winner drift')
    selected_groups = {d.group for d in (*old, *calibration, *reserved)}
    background_texts, provenance = background(corpus.dialogues, selected_groups)
    forbidden = {t for views in plan(calibration).values() for texts in views.values() for t in texts} if frozen else frozenset()
    ds = reserved if frozen else calibration
    anchors, vectors, records, originals, encoder, timings = extract(corpus, ds, snapshot, forbidden)
    supported = sorted(set(vectors['current']) & set(vectors['joined']))
    vectorizer, features, background_nnz = lexical_features(background_texts, [originals[i] for i in supported])
    lexical = {'vocabulary': vectorizer.get_feature_names_out().tolist(), 'records': {
        identity: {'reason': None if features[row].nnz else 'zero-lexical',
                   'indices': features[row].indices.tolist(), 'values': features[row].data.tolist()}
        for row, identity in enumerate(supported)}}
    basis = {'vocabulary': lexical['vocabulary'], 'idf': vectorizer.idf_.tolist(),
             'background': provenance, 'background_nnz': background_nnz}
    if frozen and basis != frozen['basis']:
        raise ValueError('lexical basis drift')
    identities = [i for i in supported if lexical['records'][i]['reason'] is None]
    views = representations(identities, vectors, lexical)
    rows = evaluation_grid(anchors, vectors, identities)
    if any(not any(r[k] for r in rows) or not any(not r[k] for r in rows) for k in ('service_overlap', 'intent_overlap')):
        raise ValueError('both proxy classes required; do not resample')
    groups = [d.group for d in ds]
    results, fit_stats = {}, {}
    orders = {'forward': list(range(len(identities)))}
    if frozen:
        orders.update(reverse=list(reversed(orders['forward'])), shuffled=np.random.default_rng(17).permutation(len(identities)).tolist())
        chosen = {key: frozen['results'][key]['candidate'] for w in frozen['winners'].values() for key in w.values() if key is not None}
        chosen['fixed-current-centroid-0.7'] = {'view': 'current', 'linkage': Linkage.CENTROID, 'cut': .7}
    else:
        chosen = {f'{view}/{l}/{cut:.1f}': {'view': view, 'linkage': l, 'cut': cut}
                  for view in views for l in Linkage for cut in CUTS}
    # Reuse each tree across cuts; fit stats are frozen before cut-score work.
    for view, linkage in sorted({(c['view'], c['linkage']) for c in chosen.values()}):
        forward_assignments = {}
        for order, indices in orders.items():
            tree, timing = fit_tree(identities, views[view], indices, Mode.GRINCH, Linkage(linkage))
            tree_key = f'{view}/{linkage}/{order}'
            timings[tree_key] = timing
            fit_stats[tree_key] = asdict(tree.statistics)
            before = time.perf_counter()
            for key, candidate in chosen.items():
                if candidate['view'] != view or candidate['linkage'] != linkage:
                    continue
                clusters = partition(tree, candidate['cut'])
                assigned = memberships(clusters, identities)
                if order == 'forward':
                    forward_assignments[key] = assigned
                metrics = evaluate_partition(clusters, identities, rows)
                name = f'{key}/{order}' if frozen else key
                results[name] = {'candidate': candidate, 'clusters': clusters, 'singletons': sum(len(c) == 1 for c in clusters),
                                 'metrics': metrics, 'target_pass': {o: target_pass(metrics[o]) for o in policy['objectives']},
                                 'forward_ari': float(adjusted_rand_score([forward_assignments[key][i] for i in identities], [assigned[i] for i in identities])),
                                 'threads': thread_metrics(clusters, identities, anchors, groups)}
            timings[tree_key]['cut_evaluation_ms'] = (time.perf_counter() - before) * 1000
    winners = frozen['winners'] if frozen else {l: {o: choose(results, l, o) for o in policy['objectives']} for l in Linkage}
    baselines = {name: {'units': evaluate_partition(clusters, identities, rows),
                        'threads': thread_metrics(clusters, identities, anchors, groups)}
                 for name, clusters in [('all_one', [identities]), ('singletons', [[i] for i in identities])]}
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'phase': 'reserved-evaluation' if frozen else 'calibration', 'policy': policy,
            'calibration_sha256': expected_sha if frozen else None, 'split': manifest, 'versions': versions, 'code_pins': pins(),
            'basis': basis, 'anchors': anchors, 'records': records, 'lexical': lexical, 'encoder': encoder,
            'dense_common_support': len(supported), 'fit_identities': identities, 'rows': rows,
            'arrival_orders': orders, 'fit_statistics': fit_stats, 'results': results, 'winners': winners,
            'baselines': baselines, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'snapshot', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--calibration', type=Path)
    parser.add_argument('--calibration-sha256')
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if bool(args.calibration) != bool(args.calibration_sha256):
        raise ValueError('evaluation requires both calibration path and its frozen hash')
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git capture required')
    result = experiment(args.root, args.snapshot, args.calibration, args.calibration_sha256)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'phase': result['phase'], 'winners': result['winners'], 'fit_support': len(result['fit_identities']),
                      'pairs': len(result['rows']), 'calls': result['encoder']['calls']}))


if __name__ == '__main__':
    main()
