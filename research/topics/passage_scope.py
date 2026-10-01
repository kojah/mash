"""Source-exact diagnostic fixtures; topicality is separate from requested scope."""
import argparse
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum
import json
from pathlib import Path

from dialogue_budget import Control
from packet_reference_review import validate as validate_references
from reference_review import digest, exact_keys
from source_spans import structural_spans


class Relevance(StrEnum):
    TOPICAL = 'topical'
    BACKGROUND = 'background'
    PROCEDURE = 'procedure'
    METADATA = 'metadata'


class Scope(StrEnum):
    INCLUDED = 'included'
    EXCLUDED = 'excluded'
    MIXED = 'mixed'
    UNSPECIFIED = 'unspecified'
    NA = 'not-applicable'


@dataclass(frozen=True)
class Judgment:
    thread: str
    anchor: int
    source: int
    index: int
    relevance: Relevance
    scope: Scope


def intervals(text):
    """Preserve source bytes and all gaps; code-point and UTF-8 coordinates agree."""
    boundaries, cursor, result = [0], 0, []
    for character in text:
        boundaries.append(boundaries[-1] + len(character.encode('utf-8')))

    def unit(start, end, kind):
        return {'start': start, 'end': end, 'byte_start': boundaries[start],
                'byte_end': boundaries[end], 'kind': kind,
                'text_sha256': digest(text[start:end].encode('utf-8'))}

    gaps = []
    for span in structural_spans(text):
        if cursor < span.start:
            gaps.append(unit(cursor, span.start, 'structural-exclusion'))
        result.append({'index': len(result), **unit(span.start, span.end, span.kind)})
        cursor = span.end
    if cursor < len(text):
        gaps.append(unit(cursor, len(text), 'structural-exclusion'))
    return {'source_chars': len(text), 'source_bytes': boundaries[-1],
            'units': result, 'excluded_gaps': gaps}


def capture(context, pairs):
    """All units in explicitly chosen source pairs; no judgment-based segmentation."""
    available = {}
    for case in context['cases']:
        for item in case['controls'][Control.RAW]['contexts']:
            if (item.get('maximum_users') != 3 or item.get('maximum_visible') is not None
                    or item['bounds'] != [64, 16384, 32768]):
                raise ValueError('original raw64 context bounds changed')
            key = (case['thread'], item['anchor']['ordinal'])
            if key in available:
                raise ValueError('duplicate context anchor')
            available[key] = item
    rows, seen = [], set()
    for pair in pairs:
        exact_keys(pair, {'thread', 'anchor', 'source'})
        key = (pair['thread'], pair['anchor'], pair['source'])
        if (type(pair['anchor']) is not int or type(pair['source']) is not int
                or key in seen or key[:2] not in available):
            raise ValueError('invalid or duplicate diagnostic source pair')
        seen.add(key)
        item = available[key[:2]]
        entries = [e for e in item['display'] if e['ordinal'] == pair['source']]
        if len(entries) != 1:
            raise ValueError('source must be uniquely displayed')
        entry = entries[0]
        identity = {k: entry[k] for k in item['anchor']}
        if (entry['role'] != 'assistant' or entry['display_status'] != 'included'
                or not isinstance(entry['text'], str) or pair['source'] >= pair['anchor']
                or identity not in item['traversed']
                or digest(entry['text'].encode()) != identity['searchable_sha256']):
            raise ValueError('source is not intact causal assistant evidence')
        rows.append({'thread': pair['thread'], 'anchor': item['anchor'], 'source': identity,
                     'context_sha256': item['context_sha256'], **intervals(entry['text'])})
    if not rows:
        raise ValueError('empty diagnostic')
    return rows


def validate(annotation, manifest_bytes):
    exact_keys(annotation, {'schema_version', 'scope', 'annotator', 'manifest_sha256', 'rows'})
    if (annotation['schema_version'] != 1 or annotation['scope'] != 'development-two-axis-passage-review'
            or not isinstance(annotation['annotator'], str) or not annotation['annotator'].strip()
            or annotation['manifest_sha256'] != digest(manifest_bytes)):
        raise ValueError('review schema, disclosure or manifest changed')
    manifest = json.loads(manifest_bytes)
    if manifest.get('schema_version') != 1 or manifest.get('scope') != 'development-source-passage-scope-diagnostic':
        raise ValueError('unsupported passage manifest')
    sources = {(r['thread'], r['anchor']['ordinal'], r['source']['ordinal']): r for r in manifest['rows']}
    if len(sources) != len(manifest['rows']):
        raise ValueError('duplicate source manifest')
    seen, result = set(), []
    for row in annotation['rows']:
        exact_keys(row, {'thread', 'anchor', 'source', 'rationale', 'units'})
        key = (row['thread'], row['anchor']['ordinal'], row['source']['ordinal'])
        if key not in sources or key in seen:
            raise ValueError('unexpected or duplicate source review')
        seen.add(key)
        source = sources[key]
        if (row['anchor'] != source['anchor'] or row['source'] != source['source']
                or not isinstance(row['rationale'], str) or not row['rationale'].strip()
                or len(row['units']) != len(source['units'])):
            raise ValueError('source identity, rationale or unit coverage changed')
        for expected, item in zip(source['units'], row['units'], strict=True):
            exact_keys(item, {*expected, 'relevance', 'requested_scope'})
            if ({k: item[k] for k in expected} != expected
                    or any(type(item[k]) is not int for k in ('index', 'start', 'end', 'byte_start', 'byte_end'))):
                raise ValueError('reordered unit, offset or source hash changed')
            relevance, scope = Relevance(item['relevance']), Scope(item['requested_scope'])
            if (relevance == Relevance.TOPICAL) == (scope == Scope.NA):
                raise ValueError('topical units need scope; other units use not-applicable')
            result.append(Judgment(*key, item['index'], relevance, scope))
    if seen != set(sources):
        raise ValueError('incomplete diagnostic review')
    return tuple(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('contexts', 'packets', 'source-review', 'reference-review', 'selection', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--annotations', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private output')
    raw = {name: getattr(args, name).read_bytes() for name in
           ('contexts', 'packets', 'source_review', 'reference_review', 'selection')}
    validate_references(raw['contexts'], raw['packets'], raw['source_review'], json.loads(raw['reference_review']))
    selection = json.loads(raw['selection'])
    exact_keys(selection, {'schema_version', 'scope', 'exposure', 'pairs'})
    if (selection['schema_version'] != 1 or selection['scope'] != 'post-ranking-hard-case-diagnostic'
            or not isinstance(selection['exposure'], str) or not selection['exposure'].strip()):
        raise ValueError('explicit post-ranking source-scope disclosure required')
    manifest = {'schema_version': 1, 'scope': 'development-source-passage-scope-diagnostic',
                'input_sha256': {k: digest(v) for k, v in raw.items()},
                'code_sha256': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                                ('passage_scope.py', 'source_spans.py', 'packet_reference_review.py', 'reference_review.py',
                                 'packet_review.py', 'dialogue_budget.py', 'preceding_context.py', 'uv.lock')},
                'rows': capture(json.loads(raw['contexts']), selection['pairs']),
                'inference_calls': 0, 'offset_units': ['unicode-code-points', 'utf-8-bytes']}
    manifest_bytes = json.dumps(manifest, indent=2, allow_nan=False).encode()
    if args.annotations:
        judgments = validate(json.loads(args.annotations.read_bytes()), manifest_bytes)
        manifest = {'schema_version': 1, 'scope': 'development-passage-scope-review-summary',
                    'manifest_sha256': digest(manifest_bytes), 'annotation_sha256': digest(args.annotations.read_bytes()),
                    'source_pairs': len(manifest['rows']), 'units': len(judgments),
                    'axes': dict(Counter(f'{j.relevance}/{j.scope}' for j in judgments)),
                    'inference_calls': 0}
    with args.output.open('xb') as output:
        output.write(json.dumps(manifest, indent=2, allow_nan=False).encode())
    print(json.dumps({'source_pairs': len(selection['pairs']), 'inference_calls': 0,
                      'output_sha256': digest(args.output.read_bytes())}))


if __name__ == '__main__':
    main()
