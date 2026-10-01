"""Complete stored-vector record-control repeats and matched-source summaries."""
import argparse
import json
from pathlib import Path

from reference_review import digest
from span_selection import Query


METHODS = {'recency-all', 'recency-full-window', *Query}


def stable(value):
    if isinstance(value, dict):
        return {k: stable(v) for k, v in value.items() if k not in {'selection_ms', 'preflight_ms', 'ranking_ms_with_validation'}}
    if isinstance(value, list):
        return [stable(v) for v in value]
    return value


def summary(report):
    rows = report['reports']
    common = [r for r in rows if all(m['cited_record_hits'] is not None for m in r['methods'].values())]
    def counts(values):
        return {'evaluated': len(values), **{f'hit_at_{k}': sum(v[f'hit_at_{k}'] for v in values) for k in (1, 3)}}
    return {'per_method': {method: counts([r['methods'][method]['cited_record_hits'] for r in rows
                              if r['methods'][method]['cited_record_hits'] is not None]) for method in sorted(METHODS)},
            'common_query_reference_support': {method: counts([r['methods'][method]['cited_record_hits'] for r in common])
                                               for method in sorted(METHODS)}}


def compare(first, second):
    for report in (first, second):
        if report.get('schema_version') != 1 or report.get('scope') != 'development-stored-reference-record-selection':
            raise ValueError('unsupported record-selection capture')
        sources = {(r['thread'], r['identity']['id']): r for r in report['inventory']}
        if len(sources) != len(report['inventory']):
            raise ValueError('duplicate record source inventory')
        anchors = [(r['thread'], r['anchor']['id']) for r in report['reports']]
        if not anchors or len(set(anchors)) != len(anchors) or not set(anchors) <= set(sources):
            raise ValueError('invalid record-selection anchor coverage')
        for row in report['reports']:
            if set(row['methods']) != METHODS:
                raise ValueError('incomplete record-selection grid')
            for method in row['methods'].values():
                seen = set()
                for entry in method['ranking']:
                    key = (row['thread'], entry['id'])
                    if key not in sources or key in seen or sources[key]['identity']['role'] != 'assistant':
                        raise ValueError('ranking is not unique assistant source coverage')
                    seen.add(key)
    if stable(first) != stable(second):
        raise ValueError('record-selection nonmeasurement fields differ')
    return {'schema_version': 1, 'scope': 'record-selection-repeat-verification', 'identical_nonmeasurement_fields': True,
            **summary(first), 'selection_ms': [sum(r['selection_ms'] for r in c['reports']) for c in (first, second)],
            'preflight_ms': [c['preflight_ms'] for c in (first, second)],
            'total_ms': [c['ranking_ms_with_validation'] for c in (first, second)]}


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
        json.dump(result, output, indent=2, allow_nan=False)
    print(json.dumps(result))


if __name__ == '__main__':
    main()
