"""Evaluation-only concrete aspect coverage; never a selector or model input."""
import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import re

from compare_passage_ranking import METHODS
from passage_scope import Relevance, Scope, validate as validate_scope
from reference_review import digest, exact_keys
from section_experiment import stable


CONTROLS = {'standalone', 'section-child', 'section-one-child'}


@dataclass(frozen=True)
class Target:
    id: str
    scope: Scope


@dataclass(frozen=True)
class UnitTargets:
    source: str
    index: int
    text_sha256: str
    targets: tuple[str, ...]


@dataclass(frozen=True)
class Review:
    thread: str
    anchor: str
    targets: tuple[Target, ...]
    units: tuple[UnitTargets, ...]


def validate(annotation, manifest_bytes, scope_bytes):
    exact_keys(annotation, {'schema_version', 'scope', 'annotator', 'manifest_sha256', 'scope_review_sha256', 'anchors'})
    if (annotation['schema_version'] != 1 or annotation['scope'] != 'development-source-backed-target-coverage'
            or not isinstance(annotation['annotator'], str) or not annotation['annotator'].strip()
            or annotation['manifest_sha256'] != digest(manifest_bytes)
            or annotation['scope_review_sha256'] != digest(scope_bytes)):
        raise ValueError('target schema, exposure or source pin changed')
    judgments = validate_scope(json.loads(scope_bytes), manifest_bytes)
    scopes = {(j.thread, j.anchor, j.source, j.index): j for j in judgments}
    expected = {}
    for row in json.loads(manifest_bytes)['rows']:
        key = (row['thread'], row['anchor']['id'])
        anchor, units = expected.setdefault(key, (row['anchor'], {}))
        if anchor != row['anchor']:
            raise ValueError('conflicting target anchor')
        for u in row['units']:
            identity = (row['source']['id'], u['index'])
            if identity in units:
                raise ValueError('duplicate target source unit')
            units[identity] = (u, scopes[row['thread'], anchor['ordinal'], row['source']['ordinal'], u['index']])
    result, seen = [], set()
    for row in annotation['anchors']:
        exact_keys(row, {'thread', 'anchor', 'rationale', 'targets', 'units'})
        key = (row['thread'], row['anchor']['id'])
        if key not in expected or key in seen:
            raise ValueError('unexpected or duplicate target anchor')
        seen.add(key)
        anchor, original = expected[key]
        if (row['anchor'] != anchor or not isinstance(row['rationale'], str) or not row['rationale'].strip()
                or not 1 <= len(row['targets']) <= 32):
            raise ValueError('target anchor or disclosure changed')
        targets = {}
        for t in row['targets']:
            exact_keys(t, {'id', 'description', 'requested_scope'})
            scope = Scope(t['requested_scope'])
            if (not isinstance(t['id'], str) or not re.fullmatch(r'[a-z][a-z0-9-]{0,47}', t['id'])
                    or t['id'] in targets or scope not in (Scope.INCLUDED, Scope.EXCLUDED)
                    or not isinstance(t['description'], str) or not t['description'].strip()):
                raise ValueError('unsupported or duplicate concrete target')
            targets[t['id']] = Target(t['id'], scope)
        units, unit_seen, supported = [], set(), set()
        for u in row['units']:
            exact_keys(u, {'source_id', 'index', 'text_sha256', 'targets'})
            identity = (u['source_id'], u['index'])
            if (type(u['index']) is not int or identity not in original or identity in unit_seen
                    or not isinstance(u['targets'], list) or any(not isinstance(t, str) for t in u['targets'])
                    or len(set(u['targets'])) != len(u['targets']) or not set(u['targets']) <= set(targets)):
                raise ValueError('unknown, duplicate or incomplete target unit')
            unit_seen.add(identity)
            source, judgment = original[identity]
            if u['text_sha256'] != source['text_sha256']:
                raise ValueError('target source text changed')
            for target in u['targets']:
                allowed = {Scope.INCLUDED} if targets[target].scope == Scope.INCLUDED else {Scope.EXCLUDED, Scope.MIXED}
                if judgment.relevance != Relevance.TOPICAL or judgment.scope not in allowed:
                    raise ValueError('target evidence conflicts with topical/requested scope')
            supported.update(u['targets'])
            units.append(UnitTargets(u['source_id'], u['index'], u['text_sha256'], tuple(u['targets'])))
        if unit_seen != set(original) or supported != set(targets):
            raise ValueError('missing unit coverage or unsupported target')
        result.append(Review(*key, tuple(targets.values()), tuple(units)))
    if seen != set(expected):
        raise ValueError('incomplete target anchor review')
    return tuple(result)


def coverage(chosen, review):
    units = {(u.source, u.index): u for u in review.units}
    seen, covered = set(), set()
    for choice in chosen:
        unit = choice['unit']
        identity = (unit['source_id'], unit['index'])
        if identity in seen or identity not in units or unit['text_sha256'] != units[identity].text_sha256:
            raise ValueError('selected target evidence is not unique original source')
        seen.add(identity)
        covered.update(units[identity].targets)
    included = {t.id for t in review.targets if t.scope == Scope.INCLUDED}
    excluded = {t.id for t in review.targets if t.scope == Scope.EXCLUDED}
    return {'targets_total': len(review.targets), 'covered_total': len(covered),
            'included_total': len(included), 'included_covered': len(covered & included),
            'excluded_total': len(excluded), 'excluded_covered': len(covered & excluded),
            'covered_ids': sorted(covered)}


def evaluate(report, reviews):
    known = {(r.thread, r.anchor): r for r in reviews}
    rows, seen = [], set()
    for row in report['reports']:
        key = (row['thread'], row['anchor']['id'])
        if key not in known or key in seen or set(row['controls']) != CONTROLS:
            raise ValueError('incomplete target control/anchor grid')
        seen.add(key)
        controls = {}
        for name, methods in row['controls'].items():
            if set(methods) != METHODS:
                raise ValueError('incomplete target query grid')
            controls[name] = {}
            for query, method in methods.items():
                if method['status'] == 'ranked':
                    if set(method['budgets']) != {'1', '3'}:
                        raise ValueError('incomplete target selection budgets')
                    if any(len(s['chosen']) > int(b) for b, s in method['budgets'].items()):
                        raise ValueError('target selection exceeds its unique-unit budget')
                    controls[name][query] = {b: coverage(s['chosen'], known[key]) for b, s in method['budgets'].items()}
                elif method['status'] in ('unsupported-query-or-candidate-window', 'canceling-query-direction') and method['budgets'] is None:
                    controls[name][query] = None
                else:
                    raise ValueError('unsupported target ranking status')
        rows.append({'thread': key[0], 'anchor': row['anchor'], 'controls': controls})
    if seen != set(known):
        raise ValueError('missing target anchor')

    def totals(values):
        result = {}
        for control in sorted(CONTROLS):
            result[control] = {}
            for query in sorted(METHODS):
                supported = [r['controls'][control][query] for r in values if r['controls'][control][query] is not None]
                result[control][query] = {'supported_anchors': len(supported), 'budgets': {
                    str(b): {field: sum(s[str(b)][field] for s in supported) for field in
                             ('targets_total', 'covered_total', 'included_total', 'included_covered', 'excluded_total', 'excluded_covered')}
                    for b in (1, 3)}}
        return result

    common = [r for r in rows if all(s is not None for s in r['controls']['standalone'].values())]
    return {'rows': rows, 'own_support': totals(rows), 'common_query_support': totals(common)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('first', 'second', 'manifest', 'scope-review', 'annotations', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private target output')
    raw = {n: getattr(args, n).read_bytes() for n in ('first', 'second', 'manifest', 'scope_review', 'annotations')}
    first, second = json.loads(raw['first']), json.loads(raw['second'])
    if stable(first) != stable(second):
        raise ValueError('section allocation repeats differ')
    if (first.get('schema_version') != 1 or first.get('scope') != 'development-bounded-section-association-control'
            or first['input_sha256']['manifest'] != digest(raw['manifest'])
            or first['input_sha256']['annotations'] != digest(raw['scope_review'])):
        raise ValueError('target selections or source pins changed')
    for name, expected in first['code_sha256'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('allocation implementation changed')
    reviews = validate(json.loads(raw['annotations']), raw['manifest'], raw['scope_review'])
    report = {'schema_version': 1, 'scope': 'development-concrete-target-coverage-diagnostic',
              'input_sha256': {k: digest(v) for k, v in raw.items()},
              'code_sha256': digest(Path(__file__).read_bytes()), 'identical_selection_repeats': True,
              'inference_calls': 0, **evaluate(first, reviews)}
    with args.output.open('x') as output:
        json.dump(report, output, indent=2, allow_nan=False)
    print(json.dumps({'reviewed_anchors': len(reviews), 'targets': sum(len(r.targets) for r in reviews), 'inference_calls': 0}))


if __name__ == '__main__':
    main()
