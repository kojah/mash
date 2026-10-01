"""Require complete source-unit rankings and exact nonmeasurement repeats."""
import argparse
import json
import math
from pathlib import Path

from reference_review import digest
from span_selection import Query


METHODS = {'recency', *Query}


def stable(value):
    if isinstance(value, dict):
        return {k: stable(v) for k, v in value.items() if k not in
                {'elapsed_ms_after_imports', 'inference_ms', 'tokenization_ms'}}
    if isinstance(value, list):
        return [stable(v) for v in value]
    return value


def compare(first, second, manifest_bytes):
    manifest = json.loads(manifest_bytes)
    expected = {}
    for row in manifest['rows']:
        key = (row['thread'], row['anchor']['id'])
        units = expected.setdefault(key, {})
        for unit in row['units']:
            identity = (row['source']['id'], unit['index'], unit['text_sha256'])
            if identity in units:
                raise ValueError('duplicate manifest unit')
            units[identity] = {'source_id': row['source']['id'], 'source_ordinal': row['source']['ordinal'], **unit}
    if not expected:
        raise ValueError('empty passage manifest')
    for report in (first, second):
        if (report.get('schema_version') != 1 or report.get('scope') != 'development-two-axis-passage-ranking-diagnostic'
                or report['input_sha256']['manifest'] != digest(manifest_bytes)):
            raise ValueError('unsupported or changed passage ranking source')
        seen = set()
        for row in report['reports']:
            key = (row['thread'], row['anchor']['id'])
            if key in seen or key not in expected or set(row['methods']) != METHODS:
                raise ValueError('incomplete or duplicate passage ranking grid')
            seen.add(key)
            if row['candidate_units'] != len(expected[key]):
                raise ValueError('candidate unit coverage changed')
            for name, method in row['methods'].items():
                ranking = method['ranking']
                identities = [(r['source_id'], r['index'], r['text_sha256']) for r in ranking]
                if method['status'] == 'ranked':
                    if len(identities) != len(set(identities)) or set(identities) != set(expected[key]):
                        raise ValueError('ranking misses or duplicates source units')
                    if set(method['topk']) != {'1', '3'}:
                        raise ValueError('incomplete passage evaluation cutoffs')
                    for entry in ranking:
                        identity = (entry['source_id'], entry['index'], entry['text_sha256'])
                        if {k: v for k, v in entry.items() if k != 'cosine'} != expected[key][identity]:
                            raise ValueError('ranking source identity or coordinates changed')
                        score = entry['cosine']
                        if (name == 'recency' and score is not None) or (name != 'recency' and
                                (type(score) not in (int, float) or not math.isfinite(score) or not -1 <= score <= 1)):
                            raise ValueError('invalid ranking score')
                    for k, counts in method['topk'].items():
                        fields = {'selected', 'topical', 'included', 'mixed', 'excluded', 'unspecified', 'non_topical'}
                        if (set(counts) != fields or any(type(v) is not int or v < 0 for v in counts.values())
                                or counts['selected'] != min(int(k), len(ranking))
                                or counts['topical'] + counts['non_topical'] != counts['selected']
                                or sum(counts[f] for f in ('included', 'mixed', 'excluded', 'unspecified')) != counts['topical']):
                            raise ValueError('inconsistent two-axis selection counts')
                elif method['status'] in ('unsupported-query-or-candidate-window', 'canceling-query-direction'):
                    if ranking or method['topk'] is not None:
                        raise ValueError('abstention has ranked evidence')
                else:
                    raise ValueError('unknown passage ranking status')
        if seen != set(expected):
            raise ValueError('missing passage anchor')
    if stable(first) != stable(second):
        raise ValueError('passage ranking nonmeasurement fields differ')
    def summarize(rows):
        summary = {}
        for name in sorted(METHODS):
            methods = [r['methods'][name] for r in rows if r['methods'][name]['status'] == 'ranked']
            summary[name] = {'supported_anchors': len(methods), 'topk': {str(k):
                             {field: sum(m['topk'][str(k)][field] for m in methods) for field in
                              ('selected', 'topical', 'included', 'mixed', 'excluded', 'unspecified', 'non_topical')}
                             for k in (1, 3)}}
        return summary
    common = [r for r in first['reports'] if all(m['status'] == 'ranked' for m in r['methods'].values())]
    return {'identical_nonmeasurement_fields': True, 'methods': summarize(first['reports']),
            'common_query_support': summarize(common),
            'elapsed_ms': [r['elapsed_ms_after_imports'] for r in (first, second)],
            'inference_ms': [r['encoder']['inference_ms'] for r in (first, second)],
            'calls': first['encoder']['calls'], 'cache_hits': first['encoder']['cache_hits']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('first', 'second', 'manifest', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private summary')
    first, second, manifest = (getattr(args, n).read_bytes() for n in ('first', 'second', 'manifest'))
    report = {**compare(json.loads(first), json.loads(second), manifest),
              'capture_sha256': [digest(first), digest(second)], 'manifest_sha256': digest(manifest),
              'comparator_sha256': digest(Path(__file__).read_bytes())}
    with args.output.open('x') as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps(report))


if __name__ == '__main__':
    main()
