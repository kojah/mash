"""Require a complete, identical nonmeasurement dialogue-budget repeat."""
import argparse
import json
from pathlib import Path

from dialogue_budget import Control
from reference_review import digest


def stable(value):
    if isinstance(value, dict):
        return {k: stable(v) for k, v in value.items() if k != 'capture_ms'}
    if isinstance(value, list):
        return [stable(v) for v in value]
    return value


def compare(first, second):
    for report in (first, second):
        if report.get('schema_version') != 1 or report.get('scope') != 'development-visible-dialogue-budget-control':
            raise ValueError('unsupported dialogue-budget capture')
        threads = [c['thread'] for c in report['cases']]
        if len(set(threads)) != len(threads):
            raise ValueError('duplicate dialogue source group')
        count = 0
        for case in report['cases']:
            if set(case['controls']) != set(Control):
                raise ValueError('incomplete dialogue control grid')
            anchors = [[s['anchor'] for s in case['controls'][control]['contexts']] for control in Control]
            if anchors[0] != anchors[1] or len({a['id'] for a in anchors[0]}) != len(anchors[0]):
                raise ValueError('changed or duplicate source anchor coverage')
            count += len(anchors[0])
        if not count:
            raise ValueError('empty dialogue control scope')
    if stable(first) != stable(second):
        raise ValueError('dialogue-budget nonmeasurement fields differ')
    return {'schema_version': 1, 'scope': 'dialogue-budget-repeat-verification',
            'identical_nonmeasurement_fields': True, 'controls': 2, 'anchors': count,
            'capture_ms': {control: [sum(c['controls'][control]['capture_ms'] for c in r['cases'])
                                     for r in (first, second)] for control in Control}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('first', 'second', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private summary output')
    raw = [p.read_bytes() for p in (args.first, args.second)]
    result = {**compare(*(json.loads(r) for r in raw)), 'capture_sha256': [digest(r) for r in raw]}
    with args.output.open('x') as output:
        json.dump(result, output, indent=2)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
