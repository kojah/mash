"""Evaluation-only intent specificity and paired whole-dialogue uncertainty."""
import argparse
from collections import Counter
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np
from sklearn.metrics import average_precision_score, roc_auc_score

from external_context import Control, SEED, inputs
from multiwoz import PINS, Speaker, load, select
from reference_review import digest


CAPTURE = '62eff98049bd418f359500c5c2d7b9c4bab2f879d48b639dcb3e6dd8e8f74b0c'
BOOTSTRAP_SEED = 20261001
REPLICATES = 1000


def prepare(corpus, capture):
    """Revalidate every source identity/input hash without inference or label features."""
    dialogues, duplicates = select(corpus, SEED)
    if capture['source_pins'] != PINS or capture['seed'] != SEED or duplicates != capture['source']['duplicates']:
        raise ValueError('source or selection drift')
    if capture['selected_groups'] != [{'id': d.id, 'group': d.group} for d in dialogues]:
        raise ValueError('selected dialogue groups drifted')
    labels = {(l.dialogue, l.ordinal): l for l in corpus.labels}
    anchors, vectors = {}, {c: {} for c in Control}
    for dialogue in dialogues:
        for turn in dialogue.turns:
            if turn.speaker != Speaker.USER:
                continue
            identity = f'{turn.dialogue}:{turn.ordinal}'
            label = labels[turn.dialogue, turn.ordinal]
            expected = {'group': dialogue.group, 'active': list(label.active),
                        'unknown': [list(i) for i in label.unknown_intents], 'text_sha256': digest(turn.text.encode())}
            if capture['anchors'].get(identity) != expected:
                raise ValueError('source anchor/annotation drift')
            anchors[identity] = {**expected, 'intents': [list(i) for i in label.declared_intents]}
            preceding = dialogue.turns[turn.ordinal - 1] if turn.ordinal else None
            for control in Control:
                record = capture['results'][control]['records'][identity]
                texts = inputs(turn, preceding, control)
                if record['input_hashes'] != [digest(t.encode()) for t in texts]:
                    raise ValueError('original model input changed')
                expected_reason = None if texts else 'root-context'
                if record['reason'] != expected_reason:
                    raise ValueError('unsupported frozen window or root policy drift')
                if expected_reason:
                    if record['vector'] is not None:
                        raise ValueError('abstaining record has a vector')
                else:
                    vector = np.asarray(record['vector'], dtype=np.float64)
                    if vector.shape != (384,) or not np.isfinite(vector).all() or not np.isclose(np.linalg.norm(vector), 1, atol=1e-12, rtol=0):
                        raise ValueError('invalid frozen vector')
                    vectors[control][identity] = vector
    if set(anchors) != set(capture['anchors']) or any(set(capture['results'][c]['records']) != set(anchors) for c in Control):
        raise ValueError('incomplete or extra anchor coverage')
    common = set.intersection(*(set(v) for v in vectors.values()))
    if len(common) != capture['common_support']:
        raise ValueError('common support drift')
    return anchors, vectors, common, [d.group for d in dialogues]


def evaluation_grid(anchors, vectors, support):
    rows = []
    identities = sorted(support)
    for offset, left in enumerate(identities):
        for right in identities[offset + 1:]:
            a, b = anchors[left], anchors[right]
            if a['group'] == b['group'] or a['unknown'] or b['unknown'] or not a['active'] or not b['active']:
                continue
            shared = set(a['active']) & set(b['active'])
            rows.append({'left': left, 'right': right, 'groups': [a['group'], b['group']],
                         'services': sorted(set(a['active']) | set(b['active'])), 'shared_services': sorted(shared),
                         'service_overlap': bool(shared),
                         'intent_overlap': bool(set(map(tuple, a['intents'])) & set(map(tuple, b['intents']))),
                         'scores': {c: float(vectors[c][left] @ vectors[c][right]) for c in Control}})
    return rows


def weighted_metrics(gold, scores, weights):
    weights = np.asarray(weights, dtype=np.float64)
    if weights.shape != np.shape(gold) or not np.isfinite(weights).all() or np.any(weights < 0):
        raise ValueError('invalid pair multiplicities')
    positive = float(weights[np.asarray(gold, dtype=bool)].sum())
    negative = float(weights[~np.asarray(gold, dtype=bool)].sum())
    if not positive or not negative:
        return None
    return {'ap': float(average_precision_score(gold, scores, sample_weight=weights)),
            'auc': float(roc_auc_score(gold, scores, sample_weight=weights))}


def pair_weights(rows, counts):
    """Resampling duplicates original groups, never creates same-source pairs."""
    if any(type(n) is not int or n < 0 for n in counts.values()):
        raise ValueError('invalid group multiplicities')
    return np.asarray([counts[r['groups'][0]] * counts[r['groups'][1]] for r in rows], dtype=np.int64)


def summarize(rows, gold_key, draws, controls=tuple(Control), baseline=Control.CURRENT):
    if not controls or len(set(controls)) != len(controls) or baseline not in controls:
        raise ValueError('unique controls and an included baseline are required')
    gold = np.asarray([r[gold_key] for r in rows], dtype=bool)
    scores = {c: np.asarray([r['scores'][c] for r in rows]) for c in controls}
    base = {c: weighted_metrics(gold, scores[c], np.ones(len(rows))) for c in controls}
    repeats, unsupported = [], 0
    for counts in draws:
        weights = pair_weights(rows, counts)
        values = {c: weighted_metrics(gold, scores[c], weights) for c in controls}
        if values[baseline] is None:
            unsupported += 1
            continue
        repeats.append({c: {m: values[c][m] - values[baseline][m] for m in ('ap', 'auc')}
                        for c in controls if c != baseline})
    intervals = {c: {m: np.percentile([r[c][m] for r in repeats], [2.5, 50, 97.5]).tolist() if repeats else None
                      for m in ('ap', 'auc')} for c in controls if c != baseline}
    return {'pairs': len(rows), 'positive': int(gold.sum()), 'negative': int((~gold).sum()),
            'groups': len({g for r in rows for g in r['groups']}), 'metrics': base,
            'replicates': len(draws), 'one_class_replicates': unsupported,
            'paired_delta_percentiles_2_5_50_97_5': intervals, 'paired_replicate_deltas': repeats}


def analyze(root, capture_path):
    start = time.perf_counter()
    raw = capture_path.read_bytes()
    if digest(raw) != CAPTURE:
        raise ValueError('frozen input capture changed')
    capture = json.loads(raw)
    corpus = load(root)
    anchors, vectors, common, groups = prepare(corpus, capture)
    rows = evaluation_grid(anchors, vectors, common)
    for control in Control:
        expected = [{'left': r['left'], 'right': r['right'], 'overlap': r['service_overlap'], 'cosine': r['scores'][control]}
                    for r in rows]
        if expected != capture['results'][control]['common_pairs']:
            raise ValueError('complete original common pair grid drifted')
    rng = np.random.default_rng(BOOTSTRAP_SEED)
    draws = []
    for _ in range(REPLICATES):
        sampled = Counter(rng.choice(groups, size=len(groups), replace=True).tolist())
        draws.append({g: sampled[g] for g in groups})
    slices = {'service': summarize(rows, 'service_overlap', draws),
              'intent': summarize(rows, 'intent_overlap', draws),
              'within_service_intent': summarize([r for r in rows if r['service_overlap']], 'intent_overlap', draws)}
    for service in sorted({s for a in anchors.values() if not a['unknown'] for s in a['active']}):
        selected = [{**r, 'domain_overlap': service in r['shared_services']} for r in rows if service in r['services']]
        slices['domain:' + service] = summarize(selected, 'domain_overlap', draws)
    code = {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
            ('external_analysis.py', 'multiwoz.py', 'external_context.py')}
    return {'version': 1, 'source_pins': PINS, 'input_capture_sha256': CAPTURE, 'historical_code_pins': capture['code_pins'],
            'code_pins': code, 'versions': {name: importlib.metadata.version(name) for name in ('numpy', 'scikit-learn')},
            'bootstrap_seed': BOOTSTRAP_SEED, 'draws': draws, 'selected_groups': capture['selected_groups'],
            'anchors': anchors, 'common_pairs': rows, 'slices': slices, 'new_inference_calls': 0,
            'timings': {'analysis_ms': (time.perf_counter() - start) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--capture', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    result = analyze(args.root, args.capture)
    if args.compare:
        from external_context import compare
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as destination:
        json.dump(result, destination, sort_keys=True, allow_nan=False)
    print(json.dumps({key: {k: v for k, v in row.items() if k != 'paired_replicate_deltas'}
                      for key, row in result['slices'].items()}))


if __name__ == '__main__':
    main()
