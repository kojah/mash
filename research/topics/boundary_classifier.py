"""Bounded train-group boundary classifier; no model inference or downloads."""
import argparse
from dataclasses import asdict
import json
from pathlib import Path
import platform
import re
import resource
import time
import warnings

import numpy as np
import sklearn
from sklearn.exceptions import ConvergenceWarning
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import average_precision_score, roc_auc_score
from sklearn.model_selection import GroupKFold
from sklearn.preprocessing import StandardScaler

from boundary_scoring import calibrate, confusion, score_turns
from external_context import compare
from tiage import Shift, load, parse
from tiage_calibration import bootstrap, digest
from tiage_development import BLOB, SIZE, sealed
from tiage_capture import verify


FEATURES = ['adjacent_cosine', 'previous_two_cosine', 'previous_jaccard',
            'previous_two_jaccard', 'current_log_tokens', 'previous_log_tokens']
TOKEN = re.compile(r'(?u)\b\w\w+\b')
DEV_FIRST = 'dd5af8f64223b2d717d17ddc11c75bb9c5b523743feb00268593d82bff56e9ea'
DEV_REPEAT = 'c522dd40157acbd5eec3d4fe570c0b659e9b0d14c2cac365dc3fb22b8b9f93ab'


def features(texts, scores):
    if (not isinstance(texts, list) or not 2 <= len(texts) <= 100000
            or any(not isinstance(t, str) or not t.strip() or len(t.encode()) > 65536 for t in texts)):
        raise ValueError('complete bounded original turn texts required')
    scores = np.asarray(scores, dtype=np.float64)
    if scores.shape != (len(texts)-1, 2) or not np.isfinite(scores).all() or (np.abs(scores) > 1+1e-12).any():
        raise ValueError('aligned supported cosine scores required')
    tokens = [TOKEN.findall(t.lower()) for t in texts]
    sets = [set(t) for t in tokens]
    result = np.empty((len(texts)-1, len(FEATURES)))
    for i in range(1, len(texts)):
        previous_two = set().union(*sets[max(0, i-2):i])
        overlaps = []
        for context in [sets[i-1], previous_two]:
            union = sets[i] | context
            overlaps.append(len(sets[i] & context)/len(union) if union else 0.)
        result[i-1] = [*scores[i-1], *overlaps, np.log1p(len(tokens[i])), np.log1p(len(tokens[i-1]))]
    return result


def fit(x, y):
    if (x.ndim != 2 or x.shape[1] != len(FEATURES) or not len(x) or not np.isfinite(x).all()
            or y.shape != (len(x),) or y.dtype != np.bool_ or len(np.unique(y)) != 2):
        raise ValueError('finite aligned two-class training features required')
    scaler = StandardScaler().fit(x)
    model = LogisticRegression(C=1., solver='liblinear', max_iter=1000, random_state=20261002)
    with warnings.catch_warnings():
        warnings.simplefilter('error', ConvergenceWarning)
        model.fit(scaler.transform(x), y)
    state = {'mean': scaler.mean_.tolist(), 'scale': scaler.scale_.tolist(),
             'variance': scaler.var_.tolist(), 'coefficient': model.coef_[0].tolist(),
             'intercept': float(model.intercept_[0]), 'iterations': int(model.n_iter_[0]),
             'classes': model.classes_.tolist(), 'training_rows': len(x)}
    return scaler, model, state


def oof(x, y, groups):
    if groups.shape != (len(x),) or len(np.unique(groups)) < 5:
        raise ValueError('five or more aligned dialogue groups required')
    probability = np.full(len(x), np.nan);assignment = np.full(len(x), -1, dtype=np.int64);folds = []
    for number, (train, held) in enumerate(GroupKFold(n_splits=5).split(x, y, groups)):
        if set(groups[train]) & set(groups[held]):
            raise ValueError('dialogue leakage across training fold')
        scaler, model, state = fit(x[train], y[train])
        probability[held] = model.predict_proba(scaler.transform(x[held]))[:, 1]
        assignment[held] = number
        folds.append({'fold': number, 'training_groups': sorted(set(groups[train])),
                      'held_groups': sorted(set(groups[held])), 'model': state})
    if not np.isfinite(probability).all() or (assignment < 0).any():
        raise ValueError('incomplete OOF coverage')
    return probability, assignment, folds


def cohort(source, frozen, split):
    if split == 'train':
        _, pin, dialogues = load(source)
    else:
        raw = (source/'anno_dev.json').read_bytes();pin = verify(raw, BLOB, SIZE);dialogues = parse(raw)
        if digest(source/'capture.json') != frozen['capture_sha256']:
            raise ValueError('development capture drift')
    if pin != frozen['source_sha256'] or [d.id for d in dialogues] != frozen['dialogue_ids']:
        raise ValueError('source identity drift')
    root = frozen.pop('_root')
    for name, expected in frozen['files'].items():
        if digest(root/name) != expected:
            raise ValueError('frozen array drift')
    vectors = np.load(root/'turn-vectors.npy', allow_pickle=False)
    scores = np.load(root/'boundary-scores.npy', allow_pickle=False)
    support = np.load(root/'common-support.npy', allow_pickle=False)
    if (vectors.shape != (sum(len(d.turns) for d in dialogues), 384)
            or support.dtype != np.bool_ or support.shape != (len(scores),) or not support.all()):
        raise ValueError('complete supported source matrix required')
    blocks = [];boundaries = [];offset = 0;row = 0
    for d in dialogues:
        count = len(d.turns);local, missing = score_turns(vectors[offset:offset+count])
        if missing or not np.array_equal(local, scores[row:row+count-1]):
            raise ValueError('frozen source geometry drift')
        blocks.append(features([t.text for t in d.turns], local))
        for t in d.turns[1:]:
            boundaries.append({'id': f'{split}:{d.id}:{t.ordinal}', 'dialogue': d.id, 'ordinal': t.ordinal,
                               'previous_vector_row': offset+t.ordinal-1,
                               'current_vector_row': offset+t.ordinal, 'label': t.shift})
        offset += count;row += count-1
    if boundaries != frozen['boundaries'] or row != len(scores):
        raise ValueError('complete source boundary identity drift')
    return np.vstack(blocks), np.array([b['label'] == Shift.CHANGE for b in boundaries]), np.array([b['dialogue'] for b in boundaries]), boundaries


def metrics(y, p, cutoff, groups):
    prediction = p > cutoff
    records = [{'id': g, **confusion(y[groups == g], prediction[groups == g])} for g in sorted(set(groups))]
    return {'counts': confusion(y, prediction), 'average_precision': float(average_precision_score(y, p)),
            'roc_auc': float(roc_auc_score(y, p)), 'per_dialogue': records, 'bootstrap': bootstrap(records)}


def experiment(a):
    begin = time.perf_counter();cpu = time.process_time()
    train = sealed(a.calibration, a.calibration_repeat)
    dev = json.loads((a.development/'results.json').read_text())
    second = json.loads((a.development_repeat/'results.json').read_text())
    if digest(a.development/'results.json') != DEV_FIRST or digest(a.development_repeat/'results.json') != DEV_REPEAT:
        raise ValueError('sealed original development artifacts required')
    compare(dev, second)
    for name, expected in second['files'].items():
        if digest(a.development_repeat/name) != expected:raise ValueError('development repeat array drift')
    train['_root'] = a.calibration;dev['_root'] = a.development
    x, y, groups, boundaries = cohort(a.training_source, train, 'train')
    dx, dy, dgroups, dboundaries = cohort(a.development_source, dev, 'dev')
    preflight_ms = (time.perf_counter()-begin)*1000
    started = time.perf_counter();p, assignments, folds = oof(x, y, groups)
    calibration = calibrate(-p, y);cutoff = -calibration['winner']['threshold']
    scaler, model, state = fit(x, y)
    dp = model.predict_proba(scaler.transform(dx))[:, 1]
    fitting_ms = (time.perf_counter()-started)*1000
    files = {}
    for name, value in [('training-features.npy', x), ('development-features.npy', dx),
                        ('training-oof-probability.npy', p), ('training-folds.npy', assignments),
                        ('development-probability.npy', dp)]:
        path = a.output_directory/name;np.save(path, value, allow_pickle=False);files[name] = digest(path)
    result = {'schema_version': 1, 'scope': 'exploratory-supervised-boundaries-not-selected-topics',
              'python': platform.python_version(), 'sklearn': sklearn.__version__, 'features': FEATURES,
              'protocol': {'folds': 5, 'group': 'dialogue', 'C': 1., 'solver': 'liblinear', 'max_iter': 1000,
                           'seed': 20261002, 'prediction': 'probability_strictly_greater_than_cutoff',
                           'tokens': TOKEN.pattern, 'empty_jaccard': 0., 'new_inference_calls': 0,
                           'test_read': False, 'development_previously_exposed': True},
              'source_pins': {'train': train['source_sha256'], 'dev': dev['source_sha256'],
                              'training_results': digest(a.calibration/'results.json'), 'development_results': DEV_FIRST},
              'boundaries': {'train': boundaries, 'dev': dboundaries}, 'files': files,
              'folds': folds, 'model': state, 'calibration': calibration, 'cutoff': cutoff,
              'results': {'training_oof': metrics(y, p, cutoff, groups), 'development': metrics(dy, dp, cutoff, dgroups)},
              'baselines': {'adjacent_development': dev['results']['adjacent']['slices']['matched']['counts'],
                            'all_change': confusion(dy, np.ones(len(dy), dtype=bool)),
                            'no_change': confusion(dy, np.zeros(len(dy), dtype=bool))},
              'overlap': dev['overlap'],
              'code_pins': {name: digest(Path(__file__).with_name(name)) for name in
                            ['boundary_classifier.py', 'boundary_scoring.py', 'tiage.py', 'tiage_development.py', 'uv.lock']}}
    result['timings'] = {'preflight_ms': preflight_ms, 'fitting_ms': fitting_ms,
                         'total_ms': (time.perf_counter()-begin)*1000, 'process_cpu_ms': (time.process_time()-cpu)*1000,
                         'peak_rss_kib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss}
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for key in ['calibration', 'calibration-repeat', 'development', 'development-repeat',
                'training-source', 'development-source', 'output-directory']:
        p.add_argument('--'+key, required=True, type=Path)
    p.add_argument('--compare', type=Path);a = p.parse_args()
    if a.output_directory.exists() or a.output_directory.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('fresh outside-Git output required')
    a.output_directory.mkdir(parents=True);result = experiment(a)
    if a.compare:compare(json.loads(a.compare.read_text()), result)
    with (a.output_directory/'results.json').open('x') as f:json.dump(result, f, indent=2, allow_nan=False)
    print(json.dumps({'cutoff': result['cutoff'], 'results': {k: v['counts'] for k, v in result['results'].items()},
                      'timings': result['timings']}), flush=True)


if __name__ == '__main__':main()
