"""Exposed-sample linkage ablation with exact legacy-decision preservation."""
import argparse
import json
from pathlib import Path
import time

from external_context import compare
from external_hierarchy import experiment as hierarchy_experiment
from online_tree import Linkage
from reference_review import digest


LEGACY = 'd2dbbb8de3583d4bc362e104bb8fc25e8b9b1a6798bb197a3a0f9b9fb6f96411'


def compare_legacy(legacy, current):
    if set(legacy) != set(current) or legacy['version'] != 1 or current['version'] != 2:
        raise ValueError('unexpected legacy schema transition')
    if current['policy'] != {**legacy['policy'], 'linkage': Linkage.CENTROID}:
        raise ValueError('legacy centroid policy changed')
    for key in legacy:
        if key not in ('version', 'policy', 'code_pins', 'timings') and legacy[key] != current[key]:
            raise ValueError(f'legacy centroid decisions diverged at {key}')


def experiment(root, calibration, evaluation, legacy_path):
    started = time.perf_counter()
    raw = legacy_path.read_bytes()
    if digest(raw) != LEGACY:
        raise ValueError('immutable legacy input drift')
    legacy = json.loads(raw)
    captures, timings = {}, {}
    for linkage in Linkage:
        capture = hierarchy_experiment(root, calibration, evaluation, linkage)
        if linkage == Linkage.CENTROID:
            compare_legacy(legacy, capture)
        timings[linkage] = capture.pop('timings')
        captures[linkage] = capture
    a, b = captures[Linkage.CENTROID], captures[Linkage.AVERAGE]
    if a['inputs'] != b['inputs'] or a['versions'] != b['versions'] or set(a['results']) != set(b['results']):
        raise ValueError('linkage comparison support changed')
    differences = {}
    for key, left in a['results'].items():
        right = b['results'][key]
        differences[key] = {'metrics': {slice_name: {metric: right['metrics'][slice_name]['metrics'][metric] - left['metrics'][slice_name]['metrics'][metric]
                                                     for metric in ('ap', 'auc')} for slice_name in left['metrics']},
                            'stability': {metric: right['stability'][metric] - left['stability'][metric]
                                          for metric in ('triplet_agreement', 'clade_jaccard')}}
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'legacy_sha256': LEGACY, 'legacy_decisions_preserved': True,
            'scope': 'exposed-development-linkage-ablation-not-new-heldout',
            'code_sha256': digest(Path(__file__).read_bytes()), 'captures': captures,
            'average_minus_centroid': differences, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'calibration', 'evaluation', 'legacy', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root, args.calibration, args.evaluation, args.legacy)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'legacy_decisions_preserved': result['legacy_decisions_preserved'],
                      'controls': sum(len(c['results']) for c in result['captures'].values()),
                      'total_ms': result['timings']['total_ms']}))


if __name__ == '__main__':
    main()
