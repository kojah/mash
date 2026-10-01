"""Verify all fixed-membership field-label controls and source coverage."""
import argparse
import json
from pathlib import Path

from compare_spans import stable
from feedback_fields import Selection
from field_labels import Score
from reference_review import digest


def compare(first, second):
    for report in (first, second):
        if report.get('schema_version') != 1 or report.get('scope') != 'development-fixed-membership-feedback-field-labels':
            raise ValueError('unsupported field-label report')
        modes = report['captures']
        if len(modes) != len(Selection) or {m['selection'] for m in modes} != set(Selection):
            raise ValueError('incomplete or duplicate field selections')
        expected = None
        for mode in modes:
            groups = mode['clusters']
            if len(groups) != report['groups'] or any(set(g['labels']) != set(Score) for g in groups):
                raise ValueError('incomplete label scorer grid')
            members = [k for g in groups for k in g['members']]
            keys = [tuple(sorted(k.items())) for k in members]
            covered = [tuple(sorted(c['key'].items())) for c in mode['coverage']]
            if len(keys) != report['records'] or len(set(keys)) != len(keys) or sorted(keys) != sorted(covered):
                raise ValueError('incomplete or duplicate label source coverage')
            identity = [g['members'] for g in groups]
            if expected is not None and identity != expected:
                raise ValueError('label filtering changed frozen memberships')
            expected = identity
    def strip(value):
        if isinstance(value, dict):
            return {k: strip(v) for k, v in value.items() if k not in {'parsing_ms', 'label_ms', 'linux_vm_hwm_mib'}}
        if isinstance(value, list):
            return [strip(v) for v in value]
        return value
    if strip(stable(first)) != strip(stable(second)):
        raise ValueError('field-label nonmeasurement fields differ')
    return {'schema_version': 1, 'scope': 'field-label-repeat-verification', 'controls': 9,
            'identical_nonmeasurement_fields': True,
            'elapsed_ms': [r['elapsed_ms_after_imports'] for r in (first, second)],
            'peak_rss_mib': [r['peak_rss_mib'] for r in (first, second)],
            'linux_vm_hwm_mib': [r['linux_vm_hwm_mib'] for r in (first, second)]}


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
