"""Audit frozen classifier probabilities and apply one training-only precision gate."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import resource
import time

import numpy as np

from external_context import compare
from selective_boundaries import evaluate, reliability, select
from tiage import load, parse


FIRST = 'ad576d5be58021915fa1c8270f03bd15e93e0a9230b59d7185728556ef63cfdd'
REPEAT = '497d7f64efee187a9e10c791ed645d2ca520083e46ee66def1ce130e569fe728'


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def experiment(a):
    started = time.perf_counter();cpu = time.process_time()
    if digest(a.classifier/'results.json') != FIRST or digest(a.classifier_repeat/'results.json') != REPEAT:
        raise ValueError('frozen first/repeat classifier outputs required')
    reference = json.loads((a.classifier/'results.json').read_text())
    repeat = json.loads((a.classifier_repeat/'results.json').read_text());compare(reference, repeat)
    for root, r in [(a.classifier, reference), (a.classifier_repeat, repeat)]:
        for name, pin in r['files'].items():
            if digest(root/name) != pin:raise ValueError('frozen classifier array drift')
        for name, pin in r['code_pins'].items():
            if digest(Path(__file__).with_name(name)) != pin:raise ValueError('frozen classifier implementation drift')
    _, training_pin, train = load(a.training_source)
    raw = (a.development_source/'anno_dev.json').read_bytes();dev = parse(raw)
    if training_pin != reference['source_pins']['train'] or hashlib.sha256(raw).hexdigest() != reference['source_pins']['dev']:
        raise ValueError('frozen original source drift')
    labels = {};groups = {};boundaries = {}
    for split, dialogues in [('train', train), ('dev', dev)]:
        records = [];offset = 0
        for d in dialogues:
            for t in d.turns[1:]:records.append({'id': f'{split}:{d.id}:{t.ordinal}', 'dialogue': d.id,
                 'ordinal': t.ordinal, 'previous_vector_row': offset+t.ordinal-1,
                 'current_vector_row': offset+t.ordinal, 'label': t.shift})
            offset += len(d.turns)
        if records != reference['boundaries'][split]:raise ValueError('complete original boundary identity drift')
        boundaries[split] = records;labels[split] = np.array([r['label'] == '1' for r in records])
        groups[split] = np.array([r['dialogue'] for r in records])
    p = np.load(a.classifier/'training-oof-probability.npy', allow_pickle=False)
    dp = np.load(a.classifier/'development-probability.npy', allow_pickle=False)
    preflight_ms = (time.perf_counter()-started)*1000;begin = time.perf_counter()
    policy = select(p, labels['train']);cutoff = policy['winner']['cutoff'] if policy['winner'] else None
    results = {};accepted = {};calibration_ms = (time.perf_counter()-begin)*1000
    for split, probabilities in [('train', p), ('dev', dp)]:
        result, mask = evaluate(probabilities, labels[split], cutoff)
        result['reliability'] = reliability(probabilities, labels[split])
        result['per_dialogue'] = [{'id': g, **evaluate(probabilities[groups[split] == g], labels[split][groups[split] == g], cutoff)[0]}
                                 for g in sorted(set(groups[split]))]
        results[split] = result
        accepted[split] = [r['id'] for r, selected in zip(boundaries[split], mask) if selected]
    return {'schema_version': 1, 'scope': 'exploratory-selective-positive-boundaries-not-topic-selection',
            'python': platform.python_version(), 'classifier_sha256': FIRST, 'classifier_repeat_sha256': REPEAT,
            'source_pins': reference['source_pins'], 'overlap': reference['overlap'],
            'protocol': {'comparison': 'probability_strictly_greater_than_cutoff', 'precision_target': .8,
                         'minimum_accepted': 20, 'unsupported_fallback': None, 'unaccepted': 'unresolved_not_no_change',
                         'reliability_bins': 10, 'development_previously_exposed': True,
                         'new_training_or_inference_calls': 0, 'test_read': False},
            'policy': policy, 'results': results, 'accepted_boundary_ids': accepted,
            'code_pins': {name: digest(Path(__file__).with_name(name)) for name in
                          ['tiage_selective.py', 'selective_boundaries.py', 'boundary_scoring.py', 'tiage.py', 'uv.lock']},
            'timings': {'preflight_ms': preflight_ms, 'calibration_ms': calibration_ms,
                        'total_ms': (time.perf_counter()-started)*1000, 'process_cpu_ms': (time.process_time()-cpu)*1000,
                        'peak_rss_kib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ['classifier', 'classifier-repeat', 'training-source', 'development-source', 'output-directory']:
        p.add_argument('--'+key, required=True, type=Path)
    p.add_argument('--compare', type=Path);a = p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);r = experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()), r)
    with (a.output_directory/'results.json').open('x') as f:json.dump(r, f, indent=2, allow_nan=False)
    print(json.dumps({'status': r['policy']['status'], 'winner': r['policy']['winner'],
                      'results': {k: {f: v[f] for f in ['accepted', 'unresolved', 'coverage', 'precision', 'boundary_recall']}
                                  for k, v in r['results'].items()}, 'timings': r['timings']}), flush=True)


if __name__ == '__main__':main()
