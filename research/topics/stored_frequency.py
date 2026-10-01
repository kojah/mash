"""Post-hoc exact-input document-frequency control; no vector or reference refit."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path
import time

import numpy as np

from data import normalized
from external_context import compare
from question_transfer import rank_metrics
from reference_review import digest


CAPTURE = '270fdf50405136931791905b8555f5e4d7a35cb5a5fd935c4a201fd72b618167'


def frequency_weights(records):
    if len(records) < 2:
        raise ValueError('at least two source threads required')
    frequency = Counter(key for values in records.values() for key in set(values))
    n = len(records)
    denominator = 1 + math.log((n + 1) / 2)
    return {key: (1 + math.log((n + 1) / (count + 1))) / denominator
            for key, count in frequency.items()}, frequency


def experiment(capture):
    started = time.perf_counter()
    raw = capture.read_bytes()
    if digest(raw) != CAPTURE:
        raise ValueError('original stored-vector capture required')
    source = json.loads(raw)
    for name, expected in source['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('original stored-vector source contract drift')
    records = source['records']
    before = time.perf_counter()
    weights, frequency = frequency_weights({i: [m['input_sha256'] for m in r['messages']] for i, r in records.items()})
    matrices, directions, scales = {}, {}, {}
    for identity, record in records.items():
        matrix = np.asarray([m['direction'] for m in record['messages']], dtype=np.float64)
        scale = np.asarray([weights[m['input_sha256']] for m in record['messages']])
        matrices[identity], scales[identity] = matrix, scale
        directions[identity] = normalized((matrix * scale[:, None]).sum(0))
    weighting_ms = (time.perf_counter() - before) * 1000
    before = time.perf_counter()
    values = {'frequency-mean': [], 'frequency-maximum': []}
    for row in source['rows']:
        a, b = row['left'], row['right']
        similarities = np.clip(matrices[a] @ matrices[b].T, -1, 1)
        values['frequency-mean'].append(float(np.clip(directions[a] @ directions[b], -1, 1)))
        values['frequency-maximum'].append(float((similarities * scales[a][:, None] * scales[b][None, :]).max()))
    return {'version': 1, 'scope': 'exposed-posthoc-stored-input-frequency-control',
            'input_sha256': CAPTURE, 'source_code_pins': source['code_pins'],
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in ('stored_frequency.py', 'data.py', 'question_transfer.py', 'uv.lock')},
            'policy': {'frequency': 'one-per-thread-exact-input-sha256', 'formula': '(1+log((N+1)/(df+1)))/(1+log((N+1)/2))',
                       'inference_calls': 0, 'discarded_inputs': 0},
            'weights': weights, 'frequency': dict(frequency), 'rows': source['rows'], 'scores': values,
            'original_metrics': source['metrics'],
            'metrics': {name: rank_metrics(source['rows'], scores) for name, scores in values.items()},
            'timings': {'weighting_ms': weighting_ms, 'scoring_ms': (time.perf_counter() - before) * 1000,
                        'total_ms': (time.perf_counter() - started) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('capture', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.capture)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'metrics': result['metrics'], 'timings': result['timings']}))


if __name__ == '__main__':
    main()
