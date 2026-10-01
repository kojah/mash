"""Post-hoc title/body mass controls with frozen unit-topic memberships and vocabulary."""
import argparse
from enum import StrEnum
import json
from pathlib import Path
import time

import numpy as np

from external_context import compare
from phrase_labels import supported
from question_topic_labels import features
from question_topics import naming_metrics
from reference_review import digest
from stackoverflow_source import load, split
from subject_labels import label_scores, ranked_terms


TOPICS = 'de9b34a307d358c0458c1d55899c18cffedd9ea23f9e97d3dcea00751026898a'


class Scope(StrEnum):
    TITLE = 'title-only'
    BALANCED = 'equal-title-body-mass'


def balanced_counts(sources, feature, scope):
    scope = Scope(scope)
    counts = []
    for keys in sources:
        total = np.zeros(len(feature.terms))
        parents = {k.split('/')[1] for k in keys}
        for parent in sorted(parents):
            title = np.asarray(feature.matrix.getrow(feature.positions['title/' + parent]).toarray()).ravel()
            components = [title / title.sum()] if title.sum() else []
            if scope == Scope.BALANCED:
                bodies = [k for k in keys if k.startswith('body/' + parent + '/')]
                if bodies:
                    body = np.asarray(feature.matrix[[feature.positions[k] for k in bodies]].sum(0)).ravel()
                    if body.sum():
                        components.append(body / body.sum())
            if components:
                total += np.mean(components, axis=0)
        counts.append(total)
    return np.asarray(counts)


def experiment(root, capture, unit_capture):
    started = time.perf_counter()
    raw = capture.read_bytes()
    if digest(raw) != TOPICS:
        raise ValueError('original frozen topic capture required')
    original = json.loads(raw)
    for name, expected in original['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('original topic contract drift')
    unit_raw = unit_capture.read_bytes()
    if digest(unit_raw) != original['input_sha256']:
        raise ValueError('original unit source capture required')
    corpus = load(root)
    cal, reserved, background = split(corpus)
    reference = {str(r.id): r.tags for r in corpus.references}
    feature = features((*cal, *reserved), background, json.loads(unit_raw))
    if feature.terms != original['vocabulary']:
        raise ValueError('original label vocabulary changed')
    for key, expected in original['source_features'].items():
        row = feature.matrix.getrow(feature.positions[key])
        if {'text_sha256': digest(feature.texts[key].encode()), 'indices': row.indices.tolist(), 'counts': row.data.tolist()} != expected:
            raise ValueError('original source label counts changed')
    results, timings = {}, {}
    for phase, data in original['phases'].items():
        for run, value in data['runs'].items():
            if not run.endswith('/forward'):
                continue
            sources = [cluster['sources'] for cluster in value['labels']['clusters']]
            for scope in Scope:
                before = time.perf_counter()
                counts = balanced_counts(sources, feature, scope)
                baseline, contrastive = label_scores(counts, feature.background)
                scorers = {'ctfidf': baseline, 'background-idf': contrastive}
                clusters = [{'sources': keys, 'labels': {m: ranked_terms(scores[n], feature.terms) for m, scores in scorers.items()}}
                            for n, keys in enumerate(sources)]
                for cluster in clusters:
                    label_keys = [k for k in cluster['sources'] if scope == Scope.BALANCED or k.startswith('title/')]
                    if any(not supported(label['term'], [feature.texts[k] for k in label_keys])
                           for values in cluster['labels'].values() for label in values):
                        raise ValueError('label outside declared source scope')
                parent_top = {p: {m: ranked_terms(scores[tags].max(0), feature.terms) for m, scores in scorers.items()}
                              for p, tags in value['parent_memberships'].items()}
                labels = {'clusters': clusters, 'parent_top8': parent_top}
                key = phase + '/' + run + '/' + scope
                results[key] = {'scope': scope, 'labels': labels, 'parent_memberships': value['parent_memberships'],
                                'counts_sha256': digest(np.asarray(counts, dtype='<f8').tobytes()),
                                'counts_shape': list(counts.shape), 'counts_row_mass': counts.sum(1).tolist(),
                                'naming': naming_metrics(data['identities'], reference, value['parent_memberships'], labels, feature)}
                timings[key] = (time.perf_counter() - before) * 1000
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed-posthoc-fixed-membership-title-body-label-control',
            'input_sha256': TOPICS, 'unit_input_sha256': original['input_sha256'], 'inference_calls': 0,
            'policy': {'title_body_mass': 'equal-average-of-nonempty-L1-bags-per-parent-per-topic', 'top8': 8},
            'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in ('question_label_balance.py', 'question_topic_labels.py', 'subject_labels.py', 'uv.lock')},
            'results': results, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'capture', 'unit-capture', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root, args.capture, args.unit_capture)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'runs': len(result['results']), 'total_ms': result['timings']['total_ms']}))


if __name__ == '__main__':
    main()
