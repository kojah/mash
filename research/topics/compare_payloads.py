"""Complete explicit-payload experiment repeat comparison."""
import argparse
import json
from pathlib import Path

from compare_spans import stable
from reference_review import digest
from unit_grouping import Method


def compare(first, second):
    for capture in (first, second):
        if capture.get('schema_version') != 1 or capture.get('scope') != 'development-explicit-feedback-payload-pilot':
            raise ValueError('unsupported payload experiment')
        modes = [c['mode'] for c in capture['captures']]
        if len(modes) != 2 or set(modes) != {'intact', 'explicit-feedback-payload'}:
            raise ValueError('incomplete or duplicate payload representations')
        for mode in capture['captures']:
            keys = [(r['method'], r['reverse']) for r in mode['reports']]
            expected = {(m, reverse) for m in Method for reverse in (False, True)}
            if len(keys) != len(expected) or set(keys) != expected:
                raise ValueError('incomplete or duplicate payload control grid')
        active = [{tuple(sorted(v['key'].items())) for v in c['coverage'] if v['encoded']} for c in capture['captures']]
        if active[0] != active[1] or len(active[0]) != capture['matched_records']:
            raise ValueError('representations have different model-window support')
    # selection_ms is a measured stage duration, not an algorithm score.
    def without_selection(value):
        if isinstance(value, dict):
            return {k: without_selection(v) for k, v in value.items() if k != 'selection_ms'}
        if isinstance(value, list):
            return [without_selection(v) for v in value]
        return value
    if without_selection(stable(first)) != without_selection(stable(second)):
        raise ValueError('payload capture nonmeasurement fields differ')
    return {'schema_version': 1, 'scope': 'explicit-payload-repeat-verification',
            'identical_nonmeasurement_fields': True, 'controls': 16,
            'elapsed_ms': [r['elapsed_ms_after_imports'] for r in (first, second)],
            'cpu_ms': [r['cpu_ms'] for r in (first, second)],
            'peak_rss_mib': [r['peak_rss_mib'] for r in (first, second)]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('first', 'second', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use private artifact output')
    raw = [p.read_bytes() for p in (args.first, args.second)]
    summary = {**compare(*(json.loads(r) for r in raw)), 'capture_sha256': [digest(r) for r in raw]}
    with args.output.open('x') as output:
        json.dump(summary, output, indent=2, allow_nan=False)
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
