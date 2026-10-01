"""Fixed source-unit/parent-mean topic membership and label diagnostics, without inference."""
import argparse
from dataclasses import asdict
from enum import StrEnum
import importlib.metadata
import json
from pathlib import Path
import time

import numpy as np
from sklearn.metrics import adjusted_rand_score

from external_clustering import boolean_metrics, memberships
from external_context import compare
from external_hierarchy import fit_tree
from online_tree import Linkage, Mode
from question_topic_labels import features, topic_labels
from question_transfer import SETTINGS, question_pairs
from reference_review import digest
from stackoverflow_source import load, split


CAPTURE = 'ede6119b31439830c7cbf1f01f53e185e8cffe3a4957722b23bb0b205b1850cd'


class View(StrEnum):
    MEAN = 'parent-mean'
    UNITS = 'unit-union'


def parent_memberships(groups, parents):
    memberships(groups, list(parents))  # Verify complete disjoint point partition.
    result = {parent: set() for parent in parents.values()}
    for n, group in enumerate(groups):
        for point in group:
            result[parents[point]].add(n)
    return {key: sorted(value) for key, value in result.items()}


def naming_metrics(ids, references, membership, labels, feature):
    positions = {t: n for n, t in enumerate(feature.terms)}
    result = {}
    for method in ('ctfidf', 'background-idf'):
        totals = {'reference_tags': 0, 'candidate_available': 0, 'union_hits': 0,
                  'top8_hits': 0, 'union_emitted_terms': 0, 'top8_emitted_terms': 0}
        rows = []
        for identity in ids:
            tagset = set(references[identity])
            source = np.asarray(feature.matrix[[feature.positions[k] for k in feature.parent_sources[identity]]].sum(0)).ravel()
            available = {t for t in tagset if t in positions and source[positions[t]] > 0}
            union = {v['term'] for n in membership[identity] for v in labels['clusters'][n]['labels'][method]}
            top = {v['term'] for v in labels['parent_top8'][identity][method]}
            hits, top_hits = sorted(tagset & union), sorted(tagset & top)
            for name, value in [('reference_tags', len(tagset)), ('candidate_available', len(available)),
                                ('union_hits', len(hits)), ('top8_hits', len(top_hits)),
                                ('union_emitted_terms', len(union)), ('top8_emitted_terms', len(top))]:
                totals[name] += value
            rows.append({'identity': identity, 'available_tags': sorted(available), 'union_hits': hits,
                         'top8_hits': top_hits, 'union_terms': len(union), 'top8_terms': len(top)})
        result[method] = {'totals': totals, 'rows': rows}
    return result


def experiment(root, capture_path):
    started = time.perf_counter()
    raw = capture_path.read_bytes()
    if digest(raw) != CAPTURE:
        raise ValueError('original complete-unit capture required')
    capture = json.loads(raw)
    for name, expected in capture['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('original complete-unit source contract drift')
    if SETTINGS != {'average': {'view': 'current', 'linkage': 'average', 'cut': .4},
                    'centroid': {'view': 'current', 'linkage': 'centroid', 'cut': .6}}:
        raise ValueError('transferred frozen cuts changed')
    corpus = load(root)
    cal, reserved, background = split(corpus)
    reference = {str(r.id): r.tags for r in corpus.references}
    before = time.perf_counter()
    feature = features((*cal, *reserved), background, capture)
    timings = {'features_ms': (time.perf_counter() - before) * 1000}
    phases = {}
    for phase, questions in [('calibration-source', cal), ('reserved-source', reserved)]:
        ids = capture['phases'][phase]['identities']
        if set(ids) != {str(q.id) for q in questions}:
            raise ValueError('question support drift')
        rows = question_pairs(ids, reference)
        if rows != capture['phases'][phase]['rows']:
            raise ValueError('original reference pair grid changed')
        runs = {}
        for view in View:
            point_ids = ids if view == View.MEAN else [i + '/' + h for i in ids for h in dict.fromkeys(capture['input_hashes'][i])]
            parents = {p: p if view == View.MEAN else feature.unit_parents[p] for p in point_ids}
            source_map = feature.parent_sources if view == View.MEAN else feature.unit_sources
            matrix = np.array([capture['mean_vectors'][p] if view == View.MEAN else capture['vectors'][p.split('/')[1]] for p in point_ids])
            orders = {'forward': list(range(len(point_ids))), 'reverse': list(reversed(range(len(point_ids)))),
                      'shuffled': np.random.default_rng(17).permutation(len(point_ids)).tolist()}
            for name, candidate in SETTINGS.items():
                forward = forward_pairs = None
                for order, indices in orders.items():
                    tree, timing = fit_tree(point_ids, matrix, indices, Mode.GRINCH, Linkage(candidate['linkage']))
                    tree.validate()
                    groups = sorted(sorted(tree.identities[i] for i in members) for members in tree.cut(candidate['cut']))
                    assigned = memberships(groups, point_ids)
                    tags = parent_memberships(groups, parents)
                    predictions = [bool(set(tags[r['left']]) & set(tags[r['right']])) for r in rows]
                    positive_pairs = {n for n, p in enumerate(predictions) if p}
                    if forward is None:
                        forward, forward_pairs = assigned, positive_pairs
                    before = time.perf_counter()
                    labels = topic_labels(groups, source_map, tags, feature)
                    timing['label_ms'] = (time.perf_counter() - before) * 1000
                    key = f'{view}/{name}/{order}'
                    runs[key] = {'view': view, 'candidate': candidate, 'groups': groups, 'parent_memberships': tags,
                                 'single_parent_topics': sum(len({parents[p] for p in group}) == 1 for group in groups),
                                 'question_topic_p95': float(np.percentile([len(t) for t in tags.values()], 95)),
                                 'maximum_question_topics': max(len(t) for t in tags.values()),
                                 'predictions': predictions, 'metrics': boolean_metrics([r['tag_overlap'] for r in rows], predictions),
                                 'forward_point_ari': float(adjusted_rand_score([forward[p] for p in point_ids], [assigned[p] for p in point_ids])),
                                 'forward_question_pair_jaccard': len(positive_pairs & forward_pairs) / len(positive_pairs | forward_pairs) if positive_pairs | forward_pairs else None,
                                 'labels': labels, 'naming': naming_metrics(ids, reference, tags, labels, feature),
                                 'fit_statistics': asdict(tree.statistics)}
                    timings[phase + '/' + key] = timing
                    print(json.dumps({'phase': phase, 'run': key, 'topics': len(groups), 'metrics': runs[key]['metrics']}), flush=True)
        phases[phase] = {'identities': ids, 'rows': rows, 'runs': runs}
    source_records = {}
    for key, position in feature.positions.items():
        row = feature.matrix.getrow(position)
        source_records[key] = {'text_sha256': digest(feature.texts[key].encode()), 'indices': row.indices.tolist(), 'counts': row.data.tolist()}
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed-fixed-question-unit-membership-and-label-controls',
            'input_sha256': CAPTURE, 'inference_calls': 0,
            'policy': {'settings': SETTINGS, 'top_per_topic': 8, 'top_per_parent': 8, 'maximum_vocabulary': 8192,
                       'title_count': 'once-per-parent-per-topic', 'background_df': 'one-vote-per-disjoint-question',
                       'vocabulary': 'transductive-source-only-selected-and-background', 'ancestry': 'not-semantic-evaluated'},
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('question_topics.py', 'question_topic_labels.py', 'external_hierarchy.py', 'online_tree.py',
                           'external_clustering.py', 'phrase_labels.py', 'subject_labels.py', 'question_transfer.py', 'uv.lock')},
            'versions': {name: importlib.metadata.version(name) for name in ('numpy', 'scikit-learn')},
            'vocabulary': feature.terms, 'source_features': source_records, 'parent_sources': feature.parent_sources,
            'background_questions': [str(q.id) for q in background], 'phases': phases, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'capture', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root, args.capture)
    # Tuples remain internal; emit the declared arrays explicitly at the boundary.
    result['parent_sources'] = {k: list(v) for k, v in result['parent_sources'].items()}
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'runs': sum(len(p['runs']) for p in result['phases'].values()), 'vocabulary': len(result['vocabulary'])}))


if __name__ == '__main__':
    main()
