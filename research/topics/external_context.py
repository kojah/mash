"""Frozen external context controls; labels enter only cross-dialogue evaluation."""
import argparse
from collections import Counter
from dataclasses import asdict
from enum import StrEnum
import json
from pathlib import Path
import time

import numpy as np
from sklearn.metrics import average_precision_score, roc_auc_score

from data import normalized
from local_embeddings import Encoder
from multiwoz import PINS, Speaker, Turn, load, select
from reference_review import digest


SEED = 'mash-topic-multiwoz-v1-20261001'


class Control(StrEnum):
    CURRENT = 'current'
    MEAN = 'mean'
    JOINED = 'joined'


def inputs(current: Turn, preceding: Turn | None, control: Control):
    """Original utterances only. Refuse nonadjacent or cross-dialogue context."""
    if current.speaker != Speaker.USER:
        raise ValueError('only USER anchors are defined')
    if (preceding is None) != (current.ordinal == 0):
        raise ValueError('only source roots may lack preceding context')
    if preceding is not None and (preceding.dialogue != current.dialogue
            or preceding.ordinal + 1 != current.ordinal or preceding.speaker != Speaker.SYSTEM):
        raise ValueError('context must be the immediately preceding SYSTEM turn')
    if control == Control.CURRENT:
        return (current.text,)
    if preceding is None:
        return ()
    if control == Control.MEAN:
        return (current.text, preceding.text)
    if control == Control.JOINED:
        return (preceding.text + '\n\n' + current.text,)
    raise ValueError('unknown context control')


def pairs(anchors, vectors, support):
    """Only supported nonempty declared-intent proxies; no same-dialogue pairs."""
    identities = sorted(support)
    result = []
    for offset, left in enumerate(identities):
        for right in identities[offset + 1:]:
            a, b = anchors[left], anchors[right]
            if a['group'] == b['group'] or a['unknown'] or b['unknown'] or not a['active'] or not b['active']:
                continue
            result.append({'left': left, 'right': right,
                           'overlap': bool(set(a['active']) & set(b['active'])),
                           'cosine': float(vectors[left] @ vectors[right])})
    return result


def metrics(rows):
    positives = sum(r['overlap'] for r in rows)
    result = {'pairs': len(rows), 'positive': positives, 'negative': len(rows) - positives,
              'average_precision': None, 'roc_auc': None}
    if positives and positives != len(rows):
        gold = [r['overlap'] for r in rows]
        scores = [r['cosine'] for r in rows]
        result.update(average_precision=float(average_precision_score(gold, scores)),
                      roc_auc=float(roc_auc_score(gold, scores)))
    return result


def experiment(root, snapshot):
    started = time.perf_counter()
    corpus = load(root)
    dialogues, duplicates = select(corpus, SEED)
    labels = {(l.dialogue, l.ordinal): l for l in corpus.labels}
    encoder = Encoder(snapshot)
    anchors, plans, tokens = {}, {}, {}
    # Preflight the whole fixed selection, including the unique encoding cap,
    # before any model call. Unsupported windows abstain, never truncate.
    for dialogue in dialogues:
        for current in dialogue.turns:
            if current.speaker != Speaker.USER:
                continue
            identity = f'{dialogue.id}:{current.ordinal}'
            label = labels[current.dialogue, current.ordinal]
            anchors[identity] = {'group': dialogue.group, 'active': list(label.active),
                                 'unknown': [list(i) for i in label.unknown_intents],
                                 'text_sha256': digest(current.text.encode())}
            preceding = dialogue.turns[current.ordinal - 1] if current.ordinal else None
            for control in Control:
                texts = inputs(current, preceding, control)
                for text in texts:
                    if text not in tokens:
                        tokens[text] = len(encoder.tokenizer.encode(text).ids)
                reason = ('root-context' if not texts else
                          'model-window' if any(not 2 <= tokens[t] <= 512 or len(t.encode()) > 65536 for t in texts)
                          else None)
                plans[control, identity] = (texts, reason)
    eligible = {t for texts, reason in plans.values() if reason is None for t in texts}
    if len(eligible) > 1024:
        raise ValueError('fixed selection exceeds encoder cap; do not resample')
    vectors = {control: {} for control in Control}
    records = {control: {} for control in Control}
    for (control, identity), (texts, reason) in plans.items():
        record = {'reason': reason, 'input_hashes': [digest(t.encode()) for t in texts],
                  'tokens': [tokens[t] for t in texts], 'vector': None}
        if reason is None:
            encoded = [encoder.encode(t) for t in texts]
            vector = encoded[0] if control != Control.MEAN else normalized(encoded[0] + encoded[1])
            vectors[control][identity] = vector
            record['vector'] = vector.tolist()
        records[control][identity] = record
    common = set.intersection(*(set(v) for v in vectors.values()))
    results = {}
    for control in Control:
        own = pairs(anchors, vectors[control], set(vectors[control]))
        matched = pairs(anchors, vectors[control], common)
        results[control] = {'supported': len(vectors[control]),
                            'abstentions': dict(Counter(r['reason'] for r in records[control].values() if r['reason'])),
                            'own': metrics(own), 'common': metrics(matched),
                            'records': records[control], 'common_pairs': matched, 'own_pairs': own}
    statistics = asdict(encoder.statistics)
    timings = {k: statistics.pop(k) for k in ('inference_ms', 'tokenization_ms')}
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    code = {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
            ('external_context.py', 'multiwoz.py', 'local_embeddings.py', 'data.py')}
    return {'version': 1, 'seed': SEED, 'source_pins': PINS, 'code_pins': code,
            'selected_groups': [{'id': d.id, 'group': d.group} for d in dialogues],
            'source': {'dialogues': len(corpus.dialogues), 'turns': sum(len(d.turns) for d in corpus.dialogues),
                       'duplicates': duplicates, 'spans': len(corpus.spans), 'copies': len(corpus.copies),
                       'copy_origin_differences': sum(not c.source_values_equal for c in corpus.copies)},
            'anchors': anchors, 'common_support': len(common), 'unique_preflight_inputs': len(eligible),
            'encoder': statistics, 'timings': timings, 'results': results}


def compare(left, right):
    # Retain vectors, complete pair grids, support, asset/source/code pins and
    # inference counts. Only explicitly nondeterministic timings are excluded.
    if set(left) != set(right) or 'timings' not in left:
        raise ValueError('capture schema changed')
    if {k: v for k, v in left.items() if k != 'timings'} != {k: v for k, v in right.items() if k != 'timings'}:
        raise ValueError('external context repeat diverged')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--snapshot', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    result = experiment(args.root, args.snapshot)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as destination:
        json.dump(result, destination, sort_keys=True, allow_nan=False)
    print(json.dumps({'common_support': result['common_support'], 'encoder': result['encoder'],
                      'timings': result['timings'], 'results': {c: {k: v for k, v in r.items()
                        if k not in ('records', 'common_pairs', 'own_pairs')} for c, r in result['results'].items()}}))


if __name__ == '__main__':
    main()
