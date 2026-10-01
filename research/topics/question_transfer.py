"""Frozen dialogue-cut transfer to programming titles; no new method selection."""
import argparse
from dataclasses import asdict
import importlib.metadata
import itertools
import json
from pathlib import Path
import time

import numpy as np
from sklearn.metrics import adjusted_rand_score, average_precision_score, roc_auc_score

from cluster_calibration import MODEL, TOKENIZER
from external_clustering import boolean_metrics, memberships
from external_context import compare
from external_hierarchy import fit_tree
from local_embeddings import Encoder
from online_tree import Linkage, Mode
from reference_review import digest
from stackoverflow_source import PINS, SEED, load, split


CALIBRATION_SHA = 'c0dcfd2f4db61702af8692db98ec2a61763f0ab11f4755962944f6833f17140f'
SETTINGS = {'average': {'view': 'current', 'linkage': 'average', 'cut': .4},
            'centroid': {'view': 'current', 'linkage': 'centroid', 'cut': .6}}


def contract(path):
    raw = path.read_bytes()
    if digest(raw) != CALIBRATION_SHA:
        raise ValueError('original frozen calibration hash required')
    frozen = json.loads(raw)
    for name, candidate in SETTINGS.items():
        key = frozen['winners'][name]['intent']
        if frozen['results'][key]['candidate'] != candidate:
            raise ValueError('frozen transfer setting drift')
    for name in ('online_tree.py', 'external_hierarchy.py', 'local_embeddings.py', 'data.py'):
        if digest(Path(__file__).with_name(name).read_bytes()) != frozen['code_pins'][name]:
            raise ValueError('frozen algorithm or encoder drift')
    for name, expected in frozen['versions'].items():
        if importlib.metadata.version(name) != expected:
            raise ValueError('frozen dependency drift')


def question_pairs(identities, references):
    return [{'left': a, 'right': b, 'tag_overlap': bool(set(references[a]) & set(references[b]))}
            for a, b in itertools.combinations(identities, 2)]


def rank_metrics(rows, scores):
    if len(rows) != len(scores) or not np.isfinite(scores).all():
        raise ValueError('aligned finite pair scores required')
    truth = [r['tag_overlap'] for r in rows]
    if any(type(v) is not bool for v in truth):
        raise ValueError('literal tag-overlap truth required')
    positives = sum(truth)
    return {'pairs': len(rows), 'positive_pairs': positives, 'negative_pairs': len(rows) - positives,
            'average_precision': float(average_precision_score(truth, scores)) if positives else None,
            'roc_auc': float(roc_auc_score(truth, scores)) if 0 < positives < len(rows) else None}


def partition_metrics(rows, assignment):
    metrics = boolean_metrics([r['tag_overlap'] for r in rows],
                              [assignment[r['left']] == assignment[r['right']] for r in rows])
    negatives = metrics['tn'] + metrics['fp']
    return {**metrics, 'specificity': metrics['tn'] / negatives if negatives else None}


def experiment(root, snapshot, calibration):
    started = time.perf_counter()
    contract(calibration)
    corpus = load(root)
    cal, reserved, _ = split(corpus)
    reference = {str(r.id): r.tags for r in corpus.references}
    encoder = Encoder(snapshot)
    if encoder.statistics.model_sha256 != MODEL or encoder.statistics.tokenizer_sha256 != TOKENIZER:
        raise ValueError('pinned installed assets required')
    forbidden = {q.title for q in cal}
    phases, timings = {}, {}
    for phase, questions in [('calibration-source', cal), ('reserved-source', reserved)]:
        records = {}
        for question in sorted(questions, key=lambda q: q.id):
            tokens = len(encoder.tokenizer.encode(question.title).ids)
            reason = ('exact-calibration-title' if phase == 'reserved-source' and question.title in forbidden
                      else 'model-window' if not 2 <= tokens <= 512 else None)
            vector = encoder.encode(question.title) if reason is None else None
            records[str(question.id)] = {'group': question.group, 'source_sha256': question.source_sha256,
                                         'title_sha256': digest(question.title.encode()), 'tokens': tokens,
                                         'reason': reason, 'vector': vector.tolist() if vector is not None else None}
        identities = [i for i, r in records.items() if r['reason'] is None]
        if len(identities) < 2:
            raise ValueError('insufficient supported titles; do not resample')
        vectors = np.asarray([records[i]['vector'] for i in identities], dtype=np.float64)
        rows = question_pairs(identities, reference)
        positions = {i: n for n, i in enumerate(identities)}
        scores = [float(np.clip(vectors[positions[r['left']]] @ vectors[positions[r['right']]], -1, 1)) for r in rows]
        orders = {'forward': list(range(len(identities))), 'reverse': list(reversed(range(len(identities)))),
                  'shuffled': np.random.default_rng(17).permutation(len(identities)).tolist()}
        results = {}
        for name, candidate in SETTINGS.items():
            forward = None
            for order, indices in orders.items():
                tree, timing = fit_tree(identities, vectors, indices, Mode.GRINCH, Linkage(candidate['linkage']))
                fit_stats = asdict(tree.statistics)
                tree.validate()
                clusters = sorted(sorted(tree.identities[i] for i in members) for members in tree.cut(candidate['cut']))
                assignment = memberships(clusters, identities)
                if forward is None:
                    forward = assignment
                results[f'{name}/{order}'] = {'candidate': candidate, 'clusters': clusters,
                                             'singletons': sum(len(c) == 1 for c in clusters),
                                             'metrics': partition_metrics(rows, assignment), 'fit_statistics': fit_stats,
                                             'forward_ari': float(adjusted_rand_score([forward[i] for i in identities],
                                                                                    [assignment[i] for i in identities]))}
                timings[f'{phase}/{name}/{order}'] = timing
        phases[phase] = {'records': records, 'fit_identities': identities, 'rows': rows,
                         'title_cosine_scores': scores, 'title_cosine': rank_metrics(rows, scores),
                         'arrival_orders': orders, 'results': results,
                         'baselines': {'all_connected': partition_metrics(rows, dict.fromkeys(identities, 0)),
                                       'singletons': partition_metrics(rows, {i: n for n, i in enumerate(identities)})}}
    stats = asdict(encoder.statistics)
    for key in ('inference_ms', 'tokenization_ms'):
        timings[key] = stats.pop(key)
    timings['total_ms'] = (time.perf_counter() - started) * 1000
    return {'version': 1, 'scope': 'fixed-programming-title-transfer-no-selection',
            'policy': {'seed': SEED, 'source_pins': PINS, 'calibration_sha256': CALIBRATION_SHA,
                       'settings': SETTINGS, 'shuffle_seed': 17, 'forward': 'question-id-ascending',
                       'input': 'single-html-unescape-title-only', 'body': 'retained-unused',
                       'tags': 'all-source-tags-evaluation-only'}, 'encoder': stats,
            'code_pins': {name: digest(Path(__file__).with_name(name).read_bytes()) for name in
                          ('question_transfer.py', 'stackoverflow_source.py', 'external_clustering.py',
                           'external_hierarchy.py', 'online_tree.py', 'local_embeddings.py',
                           'cluster_calibration.py', 'data.py', 'external_context.py', 'reference_review.py', 'uv.lock')},
            'versions': {name: importlib.metadata.version(name) for name in
                         ('numpy', 'scikit-learn', 'onnxruntime', 'tokenizers')},
            'phases': phases, 'timings': timings}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('root', 'snapshot', 'calibration', 'output'):
        parser.add_argument(f'--{name}', type=Path, required=True)
    parser.add_argument('--compare', type=Path)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    result = experiment(args.root, args.snapshot, args.calibration)
    if args.compare:
        compare(json.loads(args.compare.read_text()), result)
    with args.output.open('x') as output:
        json.dump(result, output, sort_keys=True, allow_nan=False)
    print(json.dumps({'calls': result['encoder']['calls'], 'support': {p: len(v['fit_identities']) for p, v in result['phases'].items()}}))


if __name__ == '__main__':
    main()
