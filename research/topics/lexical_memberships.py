"""Source-term multi-membership diagnostic; not a graph/topic-model replication."""
import argparse
import ast
from enum import StrEnum
import json
from pathlib import Path
import subprocess
import time

import numpy as np

from external_clustering import boolean_metrics
from external_context import compare
from phrase_labels import supported
from question_calibration import passes
from question_label_balance import balanced_counts, Scope
from question_topic_labels import features
from question_transfer import question_pairs
from reference_review import digest
from stackoverflow_source import load, split
from subject_labels import label_scores, ranked_terms


TOPIC_PIN = 'de9b34a307d358c0458c1d55899c18cffedd9ea23f9e97d3dcea00751026898a'
BALANCE_PIN = '6f9a46d7d0f9f1d58fed4bc37725c16e4af01b59e14676b6d9706e945918db4f'
BASELINE = 'd06475dce0dc93596541f0b21f0a313646030967'


class Scorer(StrEnum):
    BASE = 'ctfidf'
    BACKGROUND = 'background-idf'


def eligibility(counts, terms, minimum, maximum):
    if counts.ndim != 2 or counts.shape[1] != len(terms) or not len(counts) or minimum < 1 or not 0 < maximum <= 1:
        raise ValueError('bounded aligned source counts required')
    if not np.isfinite(counts).all() or np.any(counts < 0):
        raise ValueError('finite nonnegative counts required')
    df = (counts > 0).sum(0)
    letters = np.array([any(c.isalpha() for c in term) for term in terms], dtype=bool)
    return (df >= minimum) & (df <= maximum * len(counts)) & letters


def selection(results):
    eligible = [(key, r['metrics']) for key, r in results.items() if passes(r['metrics'])]
    return min(eligible, key=lambda r: (-r[1]['recall'], -r[1]['precision'], r[0]))[0] if eligible else None


def scoring_contract(expected_hash):
    repo = Path(__file__).resolve().parents[2]
    original = subprocess.run(['git', 'show', BASELINE + ':research/topics/subject_labels.py'],
                              cwd=repo, capture_output=True, check=True).stdout
    if digest(original) != expected_hash:
        raise ValueError('original committed scoring source required')
    def function(raw):
        return next(n for n in ast.parse(raw).body if isinstance(n, ast.FunctionDef) and n.name == 'label_scores')
    if ast.dump(function(original)) != ast.dump(function(Path(__file__).with_name('subject_labels.py').read_bytes())):
        raise ValueError('original scoring formula changed')


def experiment(root):
    started = time.perf_counter()
    topic_raw, balance_raw = [(root / n).read_bytes() for n in ('topics-first.json', 'label-balance-first.json')]
    if digest(topic_raw) != TOPIC_PIN or digest(balance_raw) != BALANCE_PIN:
        raise ValueError('original topic and balance capture required')
    topic, balance = json.loads(topic_raw), json.loads(balance_raw)
    scoring_contract(topic['code_pins']['subject_labels.py'])
    for name, expected in topic['code_pins'].items():
        if name != 'subject_labels.py' and digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('unrelated source contract drift')
    unit_raw = (root / 'units-final-first.json').read_bytes()
    if digest(unit_raw) != topic['input_sha256']:
        raise ValueError('original complete unit capture required')
    corpus = load(root)
    cal, reserved, background = split(corpus)
    feature = features((*cal, *reserved), background, json.loads(unit_raw))
    if feature.terms != topic['vocabulary']:
        raise ValueError('frozen vocabulary changed')
    for key, record in topic['source_features'].items():
        row = feature.matrix.getrow(feature.positions[key])
        if {'text_sha256': digest(feature.texts[key].encode()), 'indices': row.indices.tolist(), 'counts': row.data.tolist()} != record:
            raise ValueError('original source count drift')
    ids = topic['phases']['calibration-source']['identities']
    if set(ids) != {str(q.id) for q in cal} or len(ids) != 64:
        raise ValueError('original complete calibration cohort required')
    sources = [list(feature.parent_sources[i]) for i in ids]
    raw = np.array([np.asarray(feature.matrix[[feature.positions[k] for k in keys]].sum(0)).ravel() for keys in sources])
    counts = balanced_counts(sources, feature, Scope.BALANCED)
    base, contrast = label_scores(counts, feature.background)
    scores = {Scorer.BASE: base, Scorer.BACKGROUND: contrast}
    old = balance['results']['calibration-source/parent-mean/centroid/forward/equal-title-body-mass']['labels']['parent_top8']
    for n, i in enumerate(ids):
        for scorer, values in scores.items():
            if ranked_terms(values[n], feature.terms) != old[i][scorer]:
                raise ValueError('original singleton balanced scores/ranking drift')
    reference = {str(r.id): set(r.tags) for r in corpus.references if str(r.id) in ids}
    rows = question_pairs(ids, reference)
    if rows != topic['phases']['calibration-source']['rows']:
        raise ValueError('natural pair grid drift')
    results = {}
    before = time.perf_counter()
    for minimum in (2, 3):
        for maximum in (.20, .40):
            mask = eligibility(raw, feature.terms, minimum, maximum)
            for scorer, values in scores.items():
                for budget in (1, 3, 8):
                    names = {i: ranked_terms(np.where(mask, values[n], 0), feature.terms, limit=budget) for n, i in enumerate(ids)}
                    if any(not supported(label['term'], [feature.texts[k] for k in feature.parent_sources[i]]) for i, labels in names.items() for label in labels):
                        raise ValueError('membership outside original source')
                    memberships = {i: sorted(label['term'] for label in labels) for i, labels in names.items()}
                    topics = {term: [i for i in ids if term in memberships[i]] for term in sorted({t for terms in memberships.values() for t in terms})}
                    predictions = [bool(set(memberships[r['left']]) & set(memberships[r['right']])) for r in rows]
                    metric = boolean_metrics([r['tag_overlap'] for r in rows], predictions)
                    available = sum(sum(t in feature.terms and mask[feature.terms.index(t)] and raw[n, feature.terms.index(t)] > 0 for t in reference[i]) for n, i in enumerate(ids))
                    key = f'{minimum}/{maximum:.2f}/{scorer}/{budget}'
                    results[key] = {'policy': {'minimum_df': minimum, 'maximum_df_fraction': maximum, 'scorer': scorer, 'budget': budget},
                        'eligible_vocabulary': int(mask.sum()), 'labels': names, 'memberships': memberships, 'topics': topics,
                        'predictions': predictions, 'metrics': metric, 'passes': passes(metric),
                        'unassigned': [i for i in ids if not memberships[i]], 'single_parent_topics': sum(len(p) == 1 for p in topics.values()),
                        'question_topic_p95': float(np.percentile([len(t) for t in memberships.values()], 95)),
                        'naming': {'reference_tags': sum(map(len, reference.values())), 'eligible_source_tags': int(available),
                                   'exact_hits': sum(len(reference[i] & set(memberships[i])) for i in ids),
                                   'emitted_memberships': sum(map(len, memberships.values()))}}
    timing = {'diagnostic_ms': (time.perf_counter() - before) * 1000, 'total_ms': (time.perf_counter() - started) * 1000}
    return {'version': 1, 'scope': 'exposed-calibration-literal-source-term-multimembership-no-graph-inference',
            'policy': {'topic_pin': TOPIC_PIN, 'balance_pin': BALANCE_PIN, 'calibration': 'original-exposed64-only',
                       'vocabulary': 'frozen-source-only-transductive-128-plus72background', 'minimum_precision': .70,
                       'minimum_links': 20, 'letters': 'at-least-one-alphabetic-codepoint', 'ancestors': 'undefined'},
            'inference_calls': 0, 'identities': ids, 'rows': rows, 'term_df': (raw > 0).sum(0).tolist(),
            'raw_counts_sha256': digest(np.asarray(raw, dtype='<i8').tobytes()),
            'balanced_counts_sha256': digest(np.asarray(counts, dtype='<f8').tobytes()),
            'results': results, 'winner': selection(results),
            'code_pins': {n: digest(Path(__file__).with_name(n).read_bytes()) for n in
                          ('lexical_memberships.py', 'subject_labels.py', 'question_topic_labels.py', 'question_label_balance.py',
                           'question_calibration.py', 'phrase_labels.py', 'uv.lock')}, 'timings': timing}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'configurations': len(result['results']), 'winner': result['winner']}))


if __name__ == '__main__':
    main()
