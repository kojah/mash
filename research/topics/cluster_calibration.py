"""Two-phase, source-group-separated development calibration; no production selection."""
import argparse
from dataclasses import asdict
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np

from external_analysis import evaluation_grid
from external_clustering import fit, memberships, pair_metrics, representations
from external_context import Control, SEED as ORIGINAL_SEED, compare, inputs
from lexical_context import background, lexical_features, serialized_policy
from local_embeddings import Encoder
from multiwoz import Corpus, PINS, Speaker, load, select
from reference_review import digest


SEED = 'mash-topic-multiwoz-calibration-v1-20261001'
GRIDS = {'birch': [.3, .4, .5, .6, .7, .8], 'grinch': [.1, .3, .5, .6, .7, .9]}
MODEL = 'afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1'
TOKENIZER = 'da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0'


def split(corpus):
    original, _ = select(corpus, ORIGINAL_SEED)
    exposed = {d.group for d in original}
    remaining = Corpus(tuple(d for d in corpus.dialogues if d.group not in exposed),
                       corpus.labels, corpus.spans, corpus.copies)
    chosen, _ = select(remaining, SEED)
    return original, chosen[:16], chosen[16:]


def pins():
    return {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
            ('cluster_calibration.py', 'external_clustering.py', 'external_analysis.py', 'online_tree.py',
             'local_embeddings.py', 'lexical_context.py', 'external_context.py', 'multiwoz.py', 'data.py', 'uv.lock')}


def plan(dialogues):
    return {f'{d.id}:{t.ordinal}': {c: inputs(t, d.turns[t.ordinal - 1] if t.ordinal else None, c)
                                  for c in (Control.CURRENT, Control.JOINED)}
            for d in dialogues for t in d.turns if t.speaker == Speaker.USER}


def extract(corpus, dialogues, snapshot, forbidden=frozenset()):
    start = time.perf_counter()
    encoder = Encoder(snapshot)
    if (encoder.statistics.model_sha256, encoder.statistics.tokenizer_sha256) != (MODEL, TOKENIZER):
        raise ValueError('installed model/tokenizer drift')
    plans, tokens, reasons = plan(dialogues), {}, {}
    for identity, views in plans.items():
        reasons[identity] = {}
        for view, texts in views.items():
            for text in texts:
                if text not in tokens:
                    tokens[text] = len(encoder.tokenizer.encode(text).ids)
            reasons[identity][view] = ('root-context' if not texts else
                'model-window' if any(not 2 <= tokens[t] <= 512 or len(t.encode()) > 65536 for t in texts) else
                'exact-calibration-input' if any(t in forbidden for t in texts) else None)
    eligible = {t for identity, views in plans.items() for view, texts in views.items()
                if reasons[identity][view] is None for t in texts}
    if len(eligible) > 1024:
        raise ValueError('frozen phase exceeds inference cap; do not resample')
    records, vectors, anchors = {}, {'current': {}, 'joined': {}}, {}
    labels = {(l.dialogue, l.ordinal): l for l in corpus.labels}
    originals = {}
    for d in dialogues:
        for turn in d.turns:
            if turn.speaker != Speaker.USER:
                continue
            key = f'{d.id}:{turn.ordinal}'
            originals[key] = turn.text
            label = labels[turn.dialogue, turn.ordinal]
            anchors[key] = {'group': d.group, 'active': list(label.active),
                            'unknown': [list(i) for i in label.unknown_intents],
                            'intents': [list(i) for i in label.declared_intents], 'text_sha256': digest(turn.text.encode())}
            records[key] = {}
            for view, texts in plans[key].items():
                reason = reasons[key][view]
                vector = encoder.encode(texts[0]) if reason is None else None
                if vector is not None:
                    vectors[view][key] = vector
                records[key][view] = {'reason': reason, 'input_hashes': [digest(t.encode()) for t in texts],
                                      'tokens': [tokens[t] for t in texts],
                                      'vector': vector.tolist() if vector is not None else None}
    stats = asdict(encoder.statistics)
    timings = {k: stats.pop(k) for k in ('inference_ms', 'tokenization_ms')}
    timings['extraction_ms'] = (time.perf_counter() - start) * 1000
    return anchors, vectors, records, originals, stats, timings


def choose(candidates, objective):
    supported = [(key, c['metrics'][objective]['f1']) for key, c in candidates.items()
                 if c['metrics'][objective]['f1'] is not None]
    if not supported:
        raise ValueError('unsupported calibration objective; do not resample')
    return min(supported, key=lambda item: (-item[1], item[0]))[0]


def evaluate_partition(clusters, identities, rows):
    assigned = memberships(clusters, identities)
    return {'service': pair_metrics(rows, assigned, 'service_overlap'),
            'intent': pair_metrics(rows, assigned, 'intent_overlap'),
            'within_service_intent': pair_metrics([r for r in rows if r['service_overlap']], assigned, 'intent_overlap')}


def experiment(root, snapshot, calibration_path=None):
    started = time.perf_counter()
    corpus = load(root)
    original, calibration, reserved = split(corpus)
    split_manifest = {name: [{'id': d.id, 'group': d.group} for d in ds] for name, ds in
                      [('exposed', original), ('calibration', calibration), ('reserved', reserved)]}
    versions = {n: importlib.metadata.version(n) for n in ('numpy', 'scikit-learn', 'onnxruntime', 'tokenizers')}
    policy = {'seed': SEED, 'grid': GRIDS, 'tie': 'maximum-F1-then-lexicographic-candidate',
              'lexical': serialized_policy(), 'dense_weight': .75, 'shuffle_seed': 17,
              'objectives': ['service', 'intent'], 'source_pins': PINS, 'model': MODEL, 'tokenizer': TOKENIZER}
    frozen = None
    if calibration_path:
        frozen = json.loads(calibration_path.read_bytes())
        if frozen['phase'] != 'calibration' or frozen['policy'] != policy or frozen['split'] != split_manifest or frozen['code_pins'] != pins() or frozen['versions'] != versions:
            raise ValueError('calibration contract drift')
        if frozen['winners'] != {o: choose(frozen['results'], o) for o in policy['objectives']}:
            raise ValueError('calibration winner drift')
    feature_start = time.perf_counter()
    selected = {d.group for d in (*original, *calibration, *reserved)}
    background_texts, provenance = background(corpus.dialogues, selected)
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
        raise ValueError('frozen background basis drift')
    identities = [i for i in supported if lexical['records'][i]['reason'] is None]
    views = representations(identities, vectors, lexical)
    rows = evaluation_grid(anchors, vectors, identities)
    # Selection cannot silently proceed on a one-class calibration/evaluation sample.
    if any(not any(r[k] for r in rows) or not any(not r[k] for r in rows) for k in ('service_overlap', 'intent_overlap')):
        raise ValueError('both proxy classes required; do not resample')
    timings['preparation_ms'] = (time.perf_counter() - feature_start) * 1000
    candidates = ({key: {'view': view, 'method': method, 'threshold': threshold}
                   for view in views for method, thresholds in GRIDS.items() for threshold in thresholds
                   for key in [f'{view}/{method}/{threshold:.1f}']} if frozen is None else
                  {key: frozen['results'][key]['candidate'] for key in sorted(set(frozen['winners'].values()))})
    if frozen:
        candidates.update({'fixed-current-birch': {'view': 'current', 'method': 'birch', 'threshold': .4},
                           'fixed-current-tree': {'view': 'current', 'method': 'grinch', 'threshold': .7}})
    orders = {'forward': list(range(len(identities)))}
    if frozen:
        orders.update(reverse=list(reversed(orders['forward'])), shuffled=np.random.default_rng(17).permutation(len(identities)).tolist())
    results = {}
    from sklearn.metrics import adjusted_rand_score
    for key, candidate in candidates.items():
        forward = None
        for order, indices in orders.items():
            name = f'{key}/{order}' if frozen else key
            fitted, timings[name] = fit(identities, views[candidate['view']], indices, candidate['method'], candidate['threshold'])
            assigned = memberships(fitted['clusters'], identities)
            if forward is None:
                forward = assigned
            fitted.update(candidate=candidate, metrics=evaluate_partition(fitted['clusters'], identities, rows),
                          forward_ari=float(adjusted_rand_score([forward[i] for i in identities], [assigned[i] for i in identities])))
            results[name] = fitted
    winners = frozen['winners'] if frozen else {o: choose(results, o) for o in policy['objectives']}
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'phase': 'reserved-evaluation' if frozen else 'calibration', 'policy': policy,
            'split': split_manifest, 'code_pins': pins(), 'versions': versions,
            'calibration_sha256': digest(calibration_path.read_bytes()) if frozen else None,
            'basis': basis, 'anchors': anchors, 'records': records, 'lexical': lexical,
            'dense_common_support': len(supported), 'fit_identities': identities,
            'rows': rows, 'encoder': encoder, 'arrival_orders': orders, 'results': results, 'winners': winners,
            'baselines': {'all_one': evaluate_partition([identities], identities, rows),
                          'singletons': evaluate_partition([[i] for i in identities], identities, rows)}, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'snapshot', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--calibration', type=Path)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root, args.snapshot, args.calibration)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'phase': result['phase'], 'winners': result['winners'], 'fit_support': len(result['fit_identities']),
                      'pairs': len(result['rows']), 'calls': result['encoder']['calls']}))


if __name__ == '__main__':
    main()
