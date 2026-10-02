"""Training-only positive-boundary acceptance; unresolved is not no-change."""
from fractions import Fraction

import numpy as np

from boundary_scoring import calibrate, confusion


def validate(probabilities, labels):
    p = np.asarray(probabilities, dtype=np.float64);y = np.asarray(labels)
    if (p.ndim != 1 or not 0 < len(p) <= 100000 or y.shape != p.shape or y.dtype != np.bool_
            or not np.isfinite(p).all() or (p < 0).any() or (p > 1).any()):
        raise ValueError('bounded complete finite probabilities and boolean labels required')
    return p, y


def select(probabilities, labels):
    p, y = validate(probabilities, labels)
    candidates = []
    for row in calibrate(-p, y)['candidates']:
        c = row['counts'];n = c['tp']+c['fp'];precision = Fraction(c['tp'], n) if n else None
        candidates.append({'cutoff': -row['threshold'], 'counts': c, 'accepted': n,
                           'coverage': n/len(p), 'risk': c['fp']/n if n else None,
                           'eligible': n >= 20 and precision >= Fraction(4, 5)})
    eligible = [r for r in candidates if r['eligible']]
    winner = max(eligible, key=lambda r: (r['accepted'], Fraction(r['counts']['tp'], r['accepted']), r['cutoff'])) if eligible else None
    return {'status': 'supported' if winner else 'unsupported', 'precision_target': .8,
            'minimum_accepted': 20, 'winner': winner, 'candidates': candidates}


def evaluate(probabilities, labels, cutoff):
    p, y = validate(probabilities, labels)
    if cutoff is not None and (type(cutoff) is not float or not np.isfinite(cutoff) or not -1 <= cutoff <= 1):
        raise ValueError('finite probability cutoff or explicit unsupported policy required')
    accepted = p > cutoff if cutoff is not None else np.zeros(len(p), dtype=bool)
    c = confusion(y, accepted);n = int(accepted.sum())
    return {'accepted': n, 'unresolved': len(p)-n, 'coverage': n/len(p),
            'precision': c['precision'], 'boundary_recall': c['recall'],
            'risk': c['fp']/n if n else None, 'counts': c,
            'unresolved_is_not_no_change': True}, accepted


def reliability(probabilities, labels):
    p, y = validate(probabilities, labels)
    index = np.minimum((10*p).astype(np.int64), 9);bins = []
    for i in range(10):
        mask = index == i;n = int(mask.sum())
        mean = float(p[mask].mean()) if n else None;frequency = float(y[mask].mean()) if n else None
        bins.append({'bin': i, 'lower': i/10, 'upper': (i+1)/10, 'upper_inclusive': i == 9,
                     'count': n, 'changes': int(y[mask].sum()), 'mean_probability': mean,
                     'change_frequency': frequency, 'absolute_gap': abs(mean-frequency) if n else None})
    epsilon = float(np.finfo(np.float64).eps);clipped = np.clip(p, epsilon, 1-epsilon)
    return {'support': len(p), 'prevalence': float(y.mean()), 'brier': float(np.mean((p-y)**2)),
            'log_loss': float(-np.mean(y*np.log(clipped)+(~y)*np.log1p(-clipped))),
            'log_loss_clip_epsilon': epsilon,
            'ece': sum(b['count']/len(p)*b['absolute_gap'] for b in bins if b['count']), 'bins': bins}
