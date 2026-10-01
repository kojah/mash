"""Zero-inference heading navigation controls on exposed oracle-source diagnostics."""
import argparse
from dataclasses import asdict
import json
from pathlib import Path
import time

from compare_passage_ranking import METHODS, compare as compare_baselines
from passage_ranking import counts
from passage_scope import capture, validate
from reference_review import digest
from section_links import Allocation, sections, select


CONTROLS = ('standalone', 'section-child')
MEASUREMENTS = {'layout_ms', 'selection_ms', 'elapsed_ms_after_imports'}


def stable(value):
    if isinstance(value, dict):
        return {k: stable(v) for k, v in value.items() if k not in MEASUREMENTS}
    if isinstance(value, (list, tuple)):
        return [stable(v) for v in value]
    return value


def run(context, manifest, judgments, baseline, *, one_child=False):
    contexts = {(c['thread'], x['anchor']['ordinal']): x for c in context['cases']
                for x in c['controls']['raw64']['contexts']}
    labels = {(j.thread, j.anchor, j.source, j.index): j for j in judgments}
    links, evidence = {}, {}
    started = time.perf_counter()
    for row in manifest['rows']:
        key = (row['thread'], row['anchor']['ordinal'])
        captured = contexts[key]
        text, = (e['text'] for e in captured['display'] if e['ordinal'] == row['source']['ordinal'])
        associations = sections(text, row['units'])
        local = links.setdefault(key, {})
        for association in associations:
            head = (row['source']['id'], association.head)
            if head in local:
                raise ValueError('duplicate section introduction')
            local[head] = tuple((row['source']['id'], i) for i in association.children)
        evidence.setdefault(key, []).extend({'source': row['source'], **asdict(s)} for s in associations)
    layout_ms = (time.perf_counter() - started) * 1000
    started = time.perf_counter()
    reports = []
    for original in baseline['reports']:
        key = (original['thread'], original['anchor']['ordinal'])
        controls = {}
        for control in (*CONTROLS, *(['section-one-child'] if one_child else [])):
            methods = {}
            for name in sorted(METHODS):
                method = original['methods'][name]
                selected = {}
                if method['status'] == 'ranked':
                    for budget in (1, 3):
                        chosen, visited = select(method['ranking'], links[key] if control != 'standalone' else {}, budget,
                                                 allocation=Allocation.ONE_CHILD if control == 'section-one-child' else Allocation.GREEDY)
                        selected_labels = [labels[*key, c['unit']['source_ordinal'], c['unit']['index']] for c in chosen]
                        measured = counts(range(len(chosen)), selected_labels)
                        if control == 'standalone' and measured != method['topk'][str(budget)]:
                            raise ValueError('standalone counts differ from frozen passage review')
                        selected[str(budget)] = {'chosen': chosen, 'visited_rank_entries': visited, 'counts': measured}
                methods[name] = {'status': method['status'], 'budgets': selected if method['status'] == 'ranked' else None}
            controls[control] = methods
        reports.append({'thread': key[0], 'anchor': original['anchor'], 'candidate_units': original['candidate_units'],
                        'associations': evidence[key], 'controls': controls})
    return {'reports': reports, 'layout_ms': layout_ms, 'selection_ms': (time.perf_counter() - started) * 1000}


def summarize(report):
    rows = report['reports']
    controls = tuple(rows[0]['controls'])
    if any(set(r['controls']) != set(controls) for r in rows):
        raise ValueError('incomplete section control grid')
    common = [r for r in rows if all(m['status'] == 'ranked' for m in r['controls']['standalone'].values())]

    def totals(values):
        result = {}
        for control in controls:
            result[control] = {}
            for name in sorted(METHODS):
                supported = [r['controls'][control][name] for r in values if r['controls'][control][name]['status'] == 'ranked']
                result[control][name] = {'supported_anchors': len(supported), 'budgets': {
                    str(b): {f: sum(m['budgets'][str(b)]['counts'][f] for m in supported) for f in
                             ('selected', 'topical', 'included', 'mixed', 'excluded', 'unspecified', 'non_topical')}
                    for b in (1, 3)}}
        return result

    return {'own_support': totals(rows), 'common_query_support': totals(common),
            'associations': sum(len(r['associations']) for r in rows),
            'capped_associations': sum(len(s['children']) < s['total_children'] for r in rows for s in r['associations'])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('baseline-first', 'baseline-second', 'contexts', 'manifest', 'annotations', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--repeat', type=Path)
    parser.add_argument('--summary', type=Path)
    parser.add_argument('--one-child', action='store_true', help='also test one child per ranked hit')
    args = parser.parse_args()
    if args.summary and not args.repeat:
        raise ValueError('repeat capture required for summary')
    targets = [args.output, *([args.summary] if args.summary else [])]
    if (len({p.resolve() for p in targets}) != len(targets) or any(p.exists()
            or p.resolve().is_relative_to(Path(__file__).resolve().parents[2]) for p in targets)):
        raise ValueError('use distinct fresh private output paths')
    started = time.perf_counter()
    raw = {n: getattr(args, n).read_bytes() for n in
           ('baseline_first', 'baseline_second', 'contexts', 'manifest', 'annotations')}
    first, second = json.loads(raw['baseline_first']), json.loads(raw['baseline_second'])
    compare_baselines(first, second, raw['manifest'])
    if any(first['input_sha256'][n] != digest(raw[n]) for n in ('contexts', 'manifest', 'annotations')):
        raise ValueError('frozen ranking source or review changed')
    manifest, context = json.loads(raw['manifest']), json.loads(raw['contexts'])
    if manifest['input_sha256']['contexts'] != digest(raw['contexts']):
        raise ValueError('source-unit context changed')
    for source in (manifest, first):
        for name, expected in source['code_sha256'].items():
            if digest(Path(__file__).with_name(name).read_bytes()) != expected:
                raise ValueError('frozen baseline implementation changed')
    pairs = [{'thread': r['thread'], 'anchor': r['anchor']['ordinal'], 'source': r['source']['ordinal']} for r in manifest['rows']]
    if capture(context, pairs) != manifest['rows']:
        raise ValueError('source units or causal scope changed')
    judgments = validate(json.loads(raw['annotations']), raw['manifest'])
    result = run(context, manifest, judgments, first, one_child=args.one_child)
    report = {'schema_version': 1, 'scope': 'development-bounded-section-association-control',
              'input_sha256': {k: digest(v) for k, v in raw.items()},
              'code_sha256': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                              ('section_links.py', 'section_experiment.py', 'compare_passage_ranking.py')},
              'parameters': {'controls': (*CONTROLS, *(['section-one-child'] if args.one_child else [])), 'budgets': [1, 3], 'maximum_children': 4,
                             'candidate_scope': 'unchanged-post-ranking-oracle-source-pairs',
                             'no_recursive_expansion': True},
              'inference_calls': 0, **result, 'elapsed_ms_after_imports': (time.perf_counter() - started) * 1000}
    encoded = json.dumps(report, indent=2, allow_nan=False).encode()
    if args.repeat:
        previous_raw = args.repeat.read_bytes()
        previous = json.loads(previous_raw)
        if stable(previous) != stable(report):
            raise ValueError('section control nonmeasurement fields differ')
    with args.output.open('xb') as output:
        output.write(encoded)
    if args.summary:
        summary = {'identical_nonmeasurement_fields': True, **summarize(report),
                   'capture_sha256': [digest(previous_raw), digest(encoded)],
                   'code_sha256': report['code_sha256'],
                   'layout_ms': [r['layout_ms'] for r in (previous, report)],
                   'selection_ms': [r['selection_ms'] for r in (previous, report)],
                   'elapsed_ms': [r['elapsed_ms_after_imports'] for r in (previous, report)], 'inference_calls': 0}
        with args.summary.open('x') as output:
            json.dump(summary, output, indent=2, allow_nan=False)
    print(json.dumps({'inference_calls': 0, 'layout_ms': result['layout_ms'],
                      'selection_ms': result['selection_ms'], 'repeat_verified': bool(args.repeat)}))


if __name__ == '__main__':
    main()
