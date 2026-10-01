"""Matched label-scoring/ranking audit against the pinned Git reference, no inference."""
import argparse
import ast
from dataclasses import dataclass
import json
from pathlib import Path
import subprocess
import time

import numpy as np

from external_context import compare
from question_label_balance import balanced_counts
from question_topic_labels import features
from reference_review import digest
from stackoverflow_source import load, split
from subject_labels import label_scores, ranked_terms


BASELINE = 'd06475dce0dc93596541f0b21f0a313646030967'
TOPICS = 'de9b34a307d358c0458c1d55899c18cffedd9ea23f9e97d3dcea00751026898a'
BALANCE = '6f9a46d7d0f9f1d58fed4bc37725c16e4af01b59e14676b6d9706e945918db4f'


@dataclass
class Workload:
    sources: list[list[str]]
    parents: dict[str, list[int]]
    scope: str | None
    counts_sha256: str
    expected: dict


def reference(expected_hash):
    """Read only our pinned committed function; never execute corpus source."""
    repo = Path(__file__).resolve().parents[2]
    raw = subprocess.run(['git', 'show', BASELINE + ':research/topics/subject_labels.py'],
                         cwd=repo, check=True, capture_output=True).stdout
    if digest(raw) != expected_hash:
        raise ValueError('committed reference hash mismatch')
    function = [n for n in ast.parse(raw).body if isinstance(n, ast.FunctionDef) and n.name == 'ranked_terms']
    if len(function) != 1:
        raise ValueError('one committed reference function required')
    namespace = {'np': np}
    exec(compile(ast.Module(body=function, type_ignores=[]), '<pinned-git-ranker>', 'exec'), namespace)
    return namespace['ranked_terms']


def evaluate(work, feature, ranker):
    if work.scope is None:
        counts = np.vstack([np.asarray(feature.matrix[[feature.positions[k] for k in keys]].sum(0)).ravel()
                            for keys in work.sources])
        dtype = '<i8'
    else:
        counts = balanced_counts(work.sources, feature, work.scope)
        dtype = '<f8'
    if digest(np.asarray(counts, dtype=dtype).tobytes()) != work.counts_sha256:
        raise ValueError('frozen label count drift')
    baseline, adjusted = label_scores(counts, feature.background)
    scorers = {'ctfidf': baseline, 'background-idf': adjusted}
    clusters = [{method: ranker(values[n], feature.terms) for method, values in scorers.items()}
                for n in range(len(work.sources))]
    parents = {p: {method: ranker(values[ids].max(0), feature.terms) for method, values in scorers.items()}
               for p, ids in work.parents.items()}
    return {'clusters': clusters, 'parents': parents}


def experiment(root):
    started = time.perf_counter()
    topic_raw, balance_raw = [(root / name).read_bytes() for name in ('topics-first.json', 'label-balance-first.json')]
    if digest(topic_raw) != TOPICS or digest(balance_raw) != BALANCE:
        raise ValueError('original topic and balance captures required')
    topics, balance = json.loads(topic_raw), json.loads(balance_raw)
    original = reference(topics['code_pins']['subject_labels.py'])
    # Only ranking implementation changes. Old experiment guards stay strict;
    # this new, explicitly versioned audit validates every unrelated source pin.
    for name, expected in (topics['code_pins'] | balance['code_pins']).items():
        if name != 'subject_labels.py' and digest(Path(__file__).with_name(name).read_bytes()) != expected:
            raise ValueError('unrelated frozen source contract drift')
    unit_raw = (root / 'units-final-first.json').read_bytes()
    if digest(unit_raw) != topics['input_sha256']:
        raise ValueError('original unit capture required')
    corpus = load(root)
    cal, reserved, background = split(corpus)
    feature = features((*cal, *reserved), background, json.loads(unit_raw))
    if feature.terms != topics['vocabulary']:
        raise ValueError('original label vocabulary drift')
    for key, expected in topics['source_features'].items():
        row = feature.matrix.getrow(feature.positions[key])
        if {'text_sha256': digest(feature.texts[key].encode()), 'indices': row.indices.tolist(), 'counts': row.data.tolist()} != expected:
            raise ValueError('frozen source count drift')
    workloads = {}
    for phase, data in topics['phases'].items():
        for run, saved in data['runs'].items():
            names = saved['labels']
            workloads[phase + '/' + run] = Workload([c['sources'] for c in names['clusters']],
                saved['parent_memberships'], None, names['counts_sha256'],
                {'clusters': [c['labels'] for c in names['clusters']], 'parents': names['parent_top8']})
    for key, saved in balance['results'].items():
        names = saved['labels']
        workloads[key] = Workload([c['sources'] for c in names['clusters']], saved['parent_memberships'],
            saved['scope'], saved['counts_sha256'],
            {'clusters': [c['labels'] for c in names['clusters']], 'parents': names['parent_top8']})
    outcomes, timings = {}, {}
    for trial in range(2):
        methods = [('committed-full-sort', original), ('bounded-positive-topk', ranked_terms)]
        if trial:
            methods.reverse()
        for method, ranker in methods:
            wall, cpu = time.perf_counter(), time.process_time()
            per_workload = {}
            for key, work in workloads.items():
                start = time.perf_counter()
                actual = evaluate(work, feature, ranker)
                per_workload[key] = (time.perf_counter() - start) * 1000
                if actual != work.expected:
                    raise ValueError('term/score/parent-budget parity failure: ' + key)
                encoded = json.dumps(actual, sort_keys=True, allow_nan=False).encode()
                outcomes[key] = {'output_sha256': digest(encoded), 'counts_sha256': work.counts_sha256,
                                 'topic_count': len(work.sources), 'parents': len(work.parents)}
            timing = {'wall_ms': (time.perf_counter() - wall) * 1000, 'cpu_ms': (time.process_time() - cpu) * 1000,
                      'per_workload_ms': per_workload}
            timings[f'{trial}/{method}'] = timing
            print(json.dumps({'trial': trial, 'method': method, 'wall_ms': timing['wall_ms']}), flush=True)
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'matched-frozen-count-scoring-ranking-and-parent-budget-audit',
            'baseline_commit': BASELINE, 'input_hashes': {'topics': TOPICS, 'balance': BALANCE},
            'inference_calls': 0, 'workloads': outcomes, 'trials': 2,
            'policy': {'order': 'baseline-optimized-optimized-baseline', 'diversity': 'tested-separately',
                       'timed': 'count-reconstruction-hash-scoring-ranking-parent-budget-plus-parity-and-output-hash',
                       'excluded': 'imports-source-feature-building-source-support-review-JSON-writing-full-ingestion'},
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('label_ranking_benchmark.py', 'subject_labels.py', 'question_topic_labels.py',
                           'question_label_balance.py', 'uv.lock')}, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'stage': 'complete', 'workloads': len(result['workloads']), 'all_labels_exact': True}))


if __name__ == '__main__':
    main()
