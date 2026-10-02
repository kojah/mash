"""Preregistered exposed-calibration word graph and sparse mixture comparison."""
import argparse
import json
from pathlib import Path
import subprocess
import time
import tomllib

import infomap
import numpy as np
import scipy

from external_clustering import boolean_metrics
from external_context import compare
from lexical_memberships import TOPIC_PIN, BALANCE_PIN, Scorer, eligibility, scoring_contract
from question_calibration import passes
from question_label_balance import Scope, balanced_counts
from question_topic_labels import features
from question_transfer import question_pairs
from reference_review import digest
from stackoverflow_source import load, split
from subject_labels import label_scores, ranked_terms
from word_graph import Filter, assignments, communities, mixture, significance, source_mass


def original_lock(expected):
    """Allow only the explicit graph extra; old captures retain their strict guards."""
    repo = Path(__file__).resolve().parents[2]
    archived = subprocess.run(['git', 'show', '65ea577:research/topics/uv.lock'],
                              cwd=repo, capture_output=True, check=True).stdout
    if digest(archived) != expected:
        raise ValueError('original science lock pin changed')
    def packages(raw):
        grouped = {}
        for package in tomllib.loads(raw)['package']:
            grouped.setdefault(package['name'], []).append(package)
        return grouped
    old = packages(archived.decode())
    current = packages(Path(__file__).with_name('uv.lock').read_text())
    if set(current) - set(old) != {'infomap'} or set(old) - set(current):
        raise ValueError('unexpected dependency addition/removal')
    for name, package in old.items():
        if name != 'mash-topic-research' and package != current[name]:
            raise ValueError('original science dependency changed')
    root = 'mash-topic-research'
    if old[root][0]['dependencies'] != current[root][0]['dependencies'] or current['infomap'][0]['version'] != '2.15.1':
        raise ValueError('base dependency or graph version changed')
    import sklearn
    for name, version in [('numpy', np.__version__), ('scipy', scipy.__version__),
                          ('scikit-learn', sklearn.__version__), ('infomap', infomap.__version__)]:
        expected = {'numpy': '2.4.6', 'scipy': '1.17.1', 'scikit-learn': '1.9.1', 'infomap': '2.15.1'}
        if version != expected[name] or version not in {p['version'] for p in current[name]}:
            raise ValueError('installed science package differs from lock')
    return digest(archived)


def inputs(root):
    topic_raw = (root / 'topics-first.json').read_bytes()
    balance_raw = (root / 'label-balance-first.json').read_bytes()
    if digest(topic_raw) != TOPIC_PIN or digest(balance_raw) != BALANCE_PIN:
        raise ValueError('original immutable captures required')
    topic, balance = json.loads(topic_raw), json.loads(balance_raw)
    scoring_contract(topic['code_pins']['subject_labels.py'])
    old_lock = original_lock(topic['code_pins']['uv.lock'])
    for name, expected in topic['code_pins'].items():
        if name not in ('subject_labels.py', 'uv.lock') and digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('unrelated original source contract changed')
    unit_raw = (root / 'units-final-first.json').read_bytes()
    if digest(unit_raw) != topic['input_sha256']:
        raise ValueError('original unit capture required')
    corpus = load(root)
    cal, reserved, background = split(corpus)
    feature = features((*cal, *reserved), background, json.loads(unit_raw))
    if feature.terms != topic['vocabulary']:
        raise ValueError('original vocabulary changed')
    for key, record in topic['source_features'].items():
        row = feature.matrix.getrow(feature.positions[key])
        if {'text_sha256': digest(feature.texts[key].encode()), 'indices': row.indices.tolist(), 'counts': row.data.tolist()} != record:
            raise ValueError('original source feature drift')
    ids = topic['phases']['calibration-source']['identities']
    if len(ids) != 64 or set(ids) != {str(q.id) for q in cal}:
        raise ValueError('original exposed cohort required')
    sources = [list(feature.parent_sources[i]) for i in ids]
    raw = np.array([np.asarray(feature.matrix[[feature.positions[k] for k in keys]].sum(0)).ravel() for keys in sources])
    counts = balanced_counts(sources, feature, Scope.BALANCED)
    base, contrast = label_scores(counts, feature.background)
    old = balance['results']['calibration-source/parent-mean/centroid/forward/equal-title-body-mass']['labels']['parent_top8']
    for n, identity in enumerate(ids):
        for scorer, values in ((Scorer.BASE, base), (Scorer.BACKGROUND, contrast)):
            if ranked_terms(values[n], feature.terms) != old[identity][scorer]:
                raise ValueError('original balanced ranking changed')
    refs = {str(r.id): set(r.tags) for r in corpus.references if str(r.id) in ids}
    pairs = question_pairs(ids, refs)
    if pairs != topic['phases']['calibration-source']['rows']:
        raise ValueError('original natural pair grid required')
    mask = eligibility(raw, feature.terms, 2, .40) & np.array([' ' not in t for t in feature.terms])
    if int(mask.sum()) > 1024:
        raise ValueError('graph word cap exceeded; do not truncate after outcomes')
    return ids, pairs, raw[:, mask], counts[:, mask], [t for t, keep in zip(feature.terms, mask, strict=True) if keep], base[:, mask], contrast[:, mask], old_lock


def evaluate(ids, pairs, tags):
    memberships = dict(zip(ids, tags, strict=True))
    predictions = [bool(set(memberships[r['left']]) & set(memberships[r['right']])) for r in pairs]
    metrics = boolean_metrics([r['tag_overlap'] for r in pairs], predictions)
    return {'memberships': memberships, 'predictions': predictions, 'metrics': metrics, 'passes': passes(metrics),
            'unassigned': [i for i in ids if not memberships[i]],
            'emitted_memberships': sum(map(len, tags))}


def robust_selection(results):
    candidates = {}
    for key, result in results.items():
        policy = key.rsplit('/', 1)[0]
        candidates.setdefault(policy, []).append(result['metrics'])
    eligible = [(key, min(r['recall'] for r in rows), min(r['precision'] for r in rows))
                for key, rows in candidates.items() if len(rows) == 3 and all(passes(r) for r in rows)]
    return min(eligible, key=lambda row: (-row[1], -row[2], row[0]))[0] if eligible else None


def experiment(root):
    started = time.perf_counter()
    ids, pairs, raw, counts, words, base, contrast, old_lock = inputs(root)
    graphs, models, results, literals, timing = {}, {}, {}, {}, {}
    for scorer, values in ((Scorer.BASE, base), (Scorer.BACKGROUND, contrast)):
        for budget in (1, 3, 8):
            tags = [[r['term'] for r in ranked_terms(row, words, limit=budget)] for row in values]
            literals[f'{scorer}/{budget}'] = evaluate(ids, pairs, tags)
    for policy in Filter:
        before = time.perf_counter()
        graph = significance(raw, policy)
        graphs[policy] = {'df': graph.df.tolist(), 'pairs': graph.pairs.tolist(), 'counts': graph.counts.tolist(),
                          'probabilities': graph.probabilities.tolist(), 'selected': graph.selected.tolist(), 'cutoff': graph.cutoff}
        timing[f'graph/{policy}'] = (time.perf_counter() - before) * 1000
        for seed in (17, 29, 101):
            before = time.perf_counter()
            groups, codelength = communities(graph, seed)
            hard = source_mass(counts, groups)
            soft, phi, trajectory = mixture(counts, groups)
            models[f'{policy}/{seed}'] = {'groups': [list(g) for g in groups], 'codelength': codelength,
                'covered_words': sum(map(len, groups)), 'isolated_words': len(words) - sum(map(len, groups)),
                'hard_scores': hard.tolist(), 'mixture_scores': soft.tolist(), 'phi': phi.tolist(), 'likelihood': trajectory,
                'status': 'supported' if groups else 'no-significant-edges'}
            timing[f'model/{policy}/{seed}'] = (time.perf_counter() - before) * 1000
            for mode, scores in (('hard', hard), ('mixture', soft)):
                for threshold in (.10, .20, .30):
                    for budget in (3, 8):
                        key = f'{policy}/{mode}/{threshold:.2f}/{budget}/{seed}'
                        results[key] = evaluate(ids, pairs, assignments(scores, threshold, budget))
    timing['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed64calibration-batch-hypergeometric-infomap-sparse-plsa-no-semantic-ancestors',
            'policy': {'seeds': [17, 29, 101], 'trials': 10, 'threads': 1, 'em_iterations': 50,
                       'initialization_epsilon': .001, 'relative_threshold': .5, 'minimum_df': 2, 'maximum_df': .40,
                       'graph_word_cap': 1024, 'responsibility_cap': 2_000_000, 'archived_lock_sha256': old_lock,
                       'vocabulary': 'original-transductive-source-only-unigrams', 'inference_calls': 0},
            'identities': ids, 'rows': pairs, 'words': words, 'raw_counts': raw.tolist(), 'balanced_counts': counts.tolist(),
            'graphs': graphs, 'models': models, 'literal_controls': literals, 'results': results,
            'winner': robust_selection(results),
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('word_graph.py', 'graph_questions.py', 'uv.lock', 'pyproject.toml', 'lexical_memberships.py',
                           'question_topic_labels.py', 'question_label_balance.py', 'subject_labels.py')},
            'timings': timing}


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
    print(json.dumps({'stage': 'complete', 'words': len(result['words']), 'winner': result['winner'], 'timings': result['timings']}))


if __name__ == '__main__':
    main()
