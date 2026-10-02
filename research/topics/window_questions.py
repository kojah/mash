"""Frozen local-window word-community comparator on exposed question sources."""
import argparse
from dataclasses import asdict
import json
from pathlib import Path
import time

import numpy as np

from external_context import compare
from graph_questions import evaluate, inputs, robust_selection
from question_topic_labels import features
from reference_review import digest
from source_windows import incidence, windows
from stackoverflow_source import load, split
from word_graph import Filter, assignments, communities, mixture, significance, source_mass


BASELINE_PIN = 'a37376e6d9c34e24f24ec5eb9d7aad3664887193fb063b9a6be0362ffb43a0a6'


def experiment(root):
    started = time.perf_counter()
    baseline_raw = (root / 'graph-first.json').read_bytes()
    if digest(baseline_raw) != BASELINE_PIN:
        raise ValueError('original whole-question graph capture required')
    baseline = json.loads(baseline_raw)
    for name, expected in baseline['code_pins'].items():
        if digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('whole-question method/source changed')
    ids, pairs, raw, counts, words, _, _, old_lock = inputs(root)
    if (ids != baseline['identities'] or pairs != baseline['rows'] or words != baseline['words']
            or raw.tolist() != baseline['raw_counts'] or counts.tolist() != baseline['balanced_counts']):
        raise ValueError('matched calibration inputs changed')
    corpus = load(root)
    cal, reserved, background = split(corpus)
    feature = features((*cal, *reserved), background, json.loads((root / 'units-final-first.json').read_text()))
    original = json.loads((root / 'topics-first.json').read_text())
    for identity in ids:
        for key in feature.parent_sources[identity]:
            if digest(feature.texts[key].encode()) != original['source_features'][key]['text_sha256']:
                raise ValueError('local-window source text drift')
    policies, graphs, models, results, timing = {}, {}, {}, {}, {}
    for budget in (16, 32, 64):
        before = time.perf_counter()
        records, metadata = [], []
        for identity in ids:
            for key in feature.parent_sources[identity]:
                for row in windows(feature.texts[key], words, budget):
                    records.append(row)
                    metadata.append({'parent': identity, 'source': key, **asdict(row)})
        values = incidence(records, len(words))
        # Exact unigram totals are unchanged by the window partition. Presence
        # may repeat across windows, so it cannot be summed as token frequency.
        reconstructed = np.zeros_like(raw)
        positions = {word: n for n, word in enumerate(words)}
        from phrase_labels import TOKEN
        for n, identity in enumerate(ids):
            for key in feature.parent_sources[identity]:
                for token in TOKEN.finditer(feature.texts[key]):
                    if token.group().lower() in positions:
                        reconstructed[n, positions[token.group().lower()]] += 1
        if not np.array_equal(reconstructed, raw):
            raise ValueError('window tokenizer differs from original unigram counts')
        # Explicit array at serialization boundary (dataclass.words is a tuple).
        for row in metadata:
            row['words'] = list(row['words'])
        policies[str(budget)] = {'windows': metadata, 'count': len(records),
            'empty_evidence_windows': int((~values.any(1)).sum()),
            'token_count': sum(r.token_count for r in records),
            'incidence_sha256': digest(values.astype(np.uint8).tobytes())}
        timing[f'windows/{budget}'] = (time.perf_counter() - before) * 1000
        for policy in Filter:
            before = time.perf_counter()
            graph = significance(values, policy)
            graphs[f'{budget}/{policy}'] = {'df': graph.df.tolist(), 'pairs': graph.pairs.tolist(),
                'counts': graph.counts.tolist(), 'probabilities': graph.probabilities.tolist(),
                'selected': graph.selected.tolist(), 'cutoff': graph.cutoff}
            timing[f'graph/{budget}/{policy}'] = (time.perf_counter() - before) * 1000
            for seed in (17, 29, 101):
                before = time.perf_counter()
                groups, codelength = communities(graph, seed)
                hard = source_mass(counts, groups)
                soft, phi, trajectory = mixture(counts, groups)
                models[f'{budget}/{policy}/{seed}'] = {'groups': [list(g) for g in groups],
                    'codelength': codelength, 'covered_words': sum(map(len, groups)),
                    'isolated_words': len(words) - sum(map(len, groups)),
                    'hard_scores': hard.tolist(), 'mixture_scores': soft.tolist(),
                    'phi': phi.tolist(), 'likelihood': trajectory,
                    'status': 'supported' if groups else 'no-significant-edges'}
                timing[f'model/{budget}/{policy}/{seed}'] = (time.perf_counter() - before) * 1000
                for mode, scores in (('hard', hard), ('mixture', soft)):
                    for threshold in (.10, .20, .30):
                        for tag_budget in (3, 8):
                            key = f'{budget}/{policy}/{mode}/{threshold:.2f}/{tag_budget}/{seed}'
                            results[key] = evaluate(ids, pairs, assignments(scores, threshold, tag_budget))
    timing['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'exposed64calibration-source-local-token-window-graph-no-semantic-ancestors',
            'policy': {'baseline_sha256': BASELINE_PIN, 'windows': [16, 32, 64], 'window_cap': 4096,
                       'window_unit': 'all-lexical-tokens-before-eligible-word-filtering',
                       'dependence': 'windows-from-same-parent-not-independent',
                       'model': baseline['policy'], 'archived_lock_sha256': old_lock, 'inference_calls': 0},
            'identities': ids, 'rows': pairs, 'words': words, 'raw_counts': raw.tolist(),
            'balanced_counts': counts.tolist(), 'source_windows': policies,
            'graphs': graphs, 'models': models, 'results': results, 'winner': robust_selection(results),
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('source_windows.py', 'window_questions.py', 'word_graph.py', 'graph_questions.py',
                           'phrase_labels.py', 'question_topic_labels.py', 'uv.lock')}, 'timings': timing}


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
    print(json.dumps({'stage': 'complete', 'configurations': len(result['results']),
                      'winner': result['winner'], 'timings': result['timings']}))


if __name__ == '__main__':
    main()
