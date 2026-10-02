"""Bounded fixed-K online variational LDA control, not dynamic topic discovery."""
import time

import numpy as np
from scipy.sparse import csr_matrix
from sklearn.decomposition import LatentDirichletAllocation


def fit(counts, order, topics, epochs):
    counts = np.asarray(counts, dtype=np.float64)
    if (counts.ndim != 2 or not 1 <= counts.shape[0] <= 512 or not 1 <= counts.shape[1] <= 1024
            or not np.isfinite(counts).all() or (counts < 0).any() or not np.equal(counts, np.floor(counts)).all()
            or counts.sum() > 10_000_000):
        raise ValueError('bounded finite nonnegative integer source counts required')
    if (type(topics) is not int or not 2 <= topics <= 64 or type(epochs) is not int
            or not 1 <= epochs <= 20 or not isinstance(order, (list, tuple)) or len(order) != len(counts)
            or any(type(i) is not int for i in order) or sorted(order) != list(range(len(counts)))):
        raise ValueError('bounded topic/epoch counts and complete integer permutation required')
    active = [i for i in order if counts[i].any()]
    if not active:
        raise ValueError('at least one informative source required')
    model = LatentDirichletAllocation(n_components=topics, doc_topic_prior=1 / topics,
        topic_word_prior=1 / topics, learning_method='online', learning_decay=.7,
        learning_offset=10., batch_size=16, total_samples=len(active), max_iter=1,
        evaluate_every=-1, mean_change_tol=.001, max_doc_update_iter=100, n_jobs=1,
        random_state=17)
    matrix = csr_matrix(counts)
    initial = np.zeros((len(counts), topics))
    batches, timings = [], []
    started = time.perf_counter()
    for epoch in range(epochs):
        for start in range(0, len(active), 16):
            ids = active[start:start + 16]
            before = time.perf_counter()
            model.partial_fit(matrix[ids])
            timings.append((time.perf_counter() - before) * 1000)
            if epoch == 0:
                initial[ids] = model.transform(matrix[ids])
            batches.append({'epoch': epoch, 'sources': ids})
    final = np.zeros_like(initial)
    final[active] = model.transform(matrix[active])
    phi = model.components_ / model.components_.sum(1, keepdims=True)
    if (not np.isfinite(final).all() or not np.isfinite(phi).all()
            or not np.allclose(final[active].sum(1), 1, rtol=0, atol=1e-12)
            or not np.allclose(phi.sum(1), 1, rtol=0, atol=1e-12)):
        raise ValueError('finite normalized posterior required')
    return {'first_arrival_scores': initial.tolist(), 'final_scores': final.tolist(),
            'phi': phi.tolist(), 'components': model.components_.tolist(), 'batches': batches,
            'updates': len(batches), 'library_batch_counter': int(model.n_batch_iter_),
            'empty_sources': [i for i in order if not counts[i].any()]}, {
                'fit_transform_ms': (time.perf_counter() - started) * 1000,
                'update_ms': timings, 'update_p95_ms': float(np.percentile(timings, 95))}
