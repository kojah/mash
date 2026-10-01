"""Frozen current/context views; feature construction receives no annotations."""
import argparse
import importlib.metadata
import json
from enum import StrEnum
from pathlib import Path
import time

import numpy as np

from data import normalized
from external_analysis import CAPTURE, prepare, evaluation_grid, summarize, weighted_metrics
from external_context import Control, compare
from multiwoz import load
from reference_review import digest


ANALYSIS = 'b31098e45cec9d521690e67fc395c90819ca7aad01dbc2b98672cb20dd587ec6'


class View(StrEnum):
    EQUAL = 'two_view_equal'
    CURRENT_75 = 'two_view_current_75'
    SHORT = 'joined_up_to_10_tokens'


def represent(current, joined, current_tokens, view):
    """Pinned cosine views or source token count only, never intent/domain labels."""
    for vector in (current, joined):
        if vector.shape != (384,) or not np.isfinite(vector).all() or not np.isclose(np.linalg.norm(vector), 1, atol=1e-12, rtol=0):
            raise ValueError('normalized finite 384-dimensional inputs required')
    if type(current_tokens) is not int or not 2 <= current_tokens <= 512:
        raise ValueError('exact bounded current tokenizer count required')
    if view == View.SHORT:
        return joined if current_tokens <= 10 else current
    if view not in (View.EQUAL, View.CURRENT_75):
        raise ValueError('unknown frozen multi-view control')
    weight = 0.5 if view == View.EQUAL else 0.75
    return normalized(np.concatenate((np.sqrt(weight) * current, np.sqrt(1 - weight) * joined)))


def derived_vectors(vectors, support, token_counts):
    # Restrict every control to the original common model support; no root fallback.
    result = {c: {key: vectors[c][key] for key in sorted(support)} for c in Control}
    for view in View:
        result[view] = {key: represent(vectors[Control.CURRENT][key], vectors[Control.JOINED][key],
                                       token_counts[key], view) for key in sorted(support)}
    return result


def experiment(root, capture_path, analysis_path):
    started = time.perf_counter()
    raw, raw_analysis = capture_path.read_bytes(), analysis_path.read_bytes()
    if digest(raw) != CAPTURE or digest(raw_analysis) != ANALYSIS:
        raise ValueError('frozen vector or bootstrap input drift')
    capture, archived = json.loads(raw), json.loads(raw_analysis)
    versions = {name: importlib.metadata.version(name) for name in ('numpy', 'scikit-learn')}
    if versions != archived['versions']:
        raise ValueError('frozen evaluation dependency versions changed')
    corpus = load(root)
    anchors, vectors, common, groups = prepare(corpus, capture)
    rows = evaluation_grid(anchors, vectors, common)
    if rows != archived['common_pairs'] or anchors != archived['anchors']:
        raise ValueError('complete original pair truth/scores or annotations changed')
    draws = archived['draws']
    if len(draws) != 1000 or any(set(d) != set(groups) or sum(d.values()) != 32 for d in draws):
        raise ValueError('whole-dialogue bootstrap coverage changed')
    token_counts = {key: capture['results']['current']['records'][key]['tokens'][0] for key in common}
    feature_start = time.perf_counter()
    derived = derived_vectors(vectors, common, token_counts)
    feature_ms = (time.perf_counter() - feature_start) * 1000
    score_start = time.perf_counter()
    for row in rows:
        for view in View:
            row['scores'][view] = float(derived[view][row['left']] @ derived[view][row['right']])
    score_ms = (time.perf_counter() - score_start) * 1000
    controls = (*Control, *View)
    slices = {'service': summarize(rows, 'service_overlap', draws, controls),
              'intent': summarize(rows, 'intent_overlap', draws, controls),
              'within_service_intent': summarize([r for r in rows if r['service_overlap']], 'intent_overlap', draws, controls)}
    # The original controls must reproduce every archived metric and replicate,
    # despite the reusable summarizer admitting additional explicit controls.
    for name, result in slices.items():
        original = archived['slices'][name]
        for key in ('pairs', 'positive', 'negative', 'groups', 'replicates', 'one_class_replicates'):
            if result[key] != original[key]:
                raise ValueError('original summary support changed')
        for c in Control:
            if result['metrics'][c] != original['metrics'][c]:
                raise ValueError('original metric changed')
        for index, replicate in enumerate(result['paired_replicate_deltas']):
            for c in (Control.MEAN, Control.JOINED):
                if replicate[c] != original['paired_replicate_deltas'][index][c]:
                    raise ValueError('original paired replicate drifted')
    domains = {}
    for service in sorted({s for a in anchors.values() if not a['unknown'] for s in a['active']}):
        selected = [r for r in rows if service in r['services']]
        gold = [service in r['shared_services'] for r in selected]
        domains[service] = {'pairs': len(selected), 'positive': sum(gold), 'metrics': {
            c: weighted_metrics(gold, [r['scores'][c] for r in selected], np.ones(len(selected))) for c in controls}}
    code = {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
            ('multiview.py', 'external_analysis.py', 'external_context.py', 'multiwoz.py', 'data.py')}
    return {'version': 1, 'vector_capture_sha256': CAPTURE, 'bootstrap_capture_sha256': ANALYSIS,
            'source_pins': capture['source_pins'], 'code_pins': code, 'versions': versions,
            'policy': {'equal_current_weight': 0.5, 'dominant_current_weight': 0.75, 'short_max_tokens_with_specials': 10,
                       'root_fallback': False}, 'selected_groups': capture['selected_groups'], 'anchors': anchors,
            'common_support': len(common), 'draws': draws, 'slices': slices, 'domains': domains,
            'records': {view: {key: {'current_tokens': token_counts[key], 'vector': derived[view][key].tolist(),
                                    'selected_view': ('joined' if token_counts[key] <= 10 else 'current') if view == View.SHORT else view}
                              for key in sorted(common)} for view in View},
            'common_pairs': rows, 'new_inference_calls': 0,
            'timings': {'features_ms': feature_ms, 'candidate_scores_ms': score_ms,
                        'total_after_imports_ms': (time.perf_counter() - started) * 1000}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--capture', type=Path, required=True)
    parser.add_argument('--analysis', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    result = experiment(args.root, args.capture, args.analysis)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as destination:
        json.dump(result, destination, sort_keys=True, allow_nan=False)
    print(json.dumps({name: {key: value for key, value in s.items() if key != 'paired_replicate_deltas'}
                      for name, s in result['slices'].items()}))


if __name__ == '__main__':
    main()
