"""Bounded batch word-community/mixture diagnostic, not an online topic contract."""
from dataclasses import dataclass
from enum import StrEnum

import numpy as np
from scipy.sparse import csr_matrix
from scipy.stats import hypergeom


class Filter(StrEnum):
    RAW = 'p05'
    BH = 'bh05'


@dataclass(frozen=True)
class Graph:
    df: np.ndarray
    pairs: np.ndarray
    counts: np.ndarray
    probabilities: np.ndarray
    selected: np.ndarray
    cutoff: float


def checked_counts(values):
    values = np.asarray(values, dtype=float)
    if values.ndim != 2 or not values.shape[0] or not np.isfinite(values).all() or np.any(values < 0):
        raise ValueError('finite nonnegative aligned document-word matrix required')
    return values


def significance(values, policy):
    """One vote/document; BH family includes every possible unordered word pair."""
    values = checked_counts(values)
    policy = Filter(policy)
    if values.shape[1] > 1024:
        raise ValueError('graph word cap exceeded')
    presence = (values > 0).astype(np.int64)
    df = presence.sum(0)
    left, right = np.triu_indices(values.shape[1], 1)
    counts = (presence.T @ presence)[left, right]
    probabilities = hypergeom.sf(counts - 1, len(values), df[left], df[right])
    cutoff = .05
    if policy == Filter.BH:
        ordered = np.sort(probabilities)
        accepted = ordered <= .05 * np.arange(1, len(ordered) + 1) / max(len(ordered), 1)
        cutoff = float(ordered[accepted][-1]) if accepted.any() else 0.0
    selected = (counts > 0) & (probabilities <= cutoff)
    return Graph(df, np.column_stack((left, right)), counts, probabilities, selected, cutoff)


def communities(graph, seed):
    """Canonical word partitions; isolated words do not become singleton topics."""
    from infomap import Network, Options, run
    edges = graph.pairs[graph.selected]
    if not len(edges):
        return (), None
    network = Network()
    for (left, right), weight in zip(edges, graph.counts[graph.selected], strict=True):
        network.add_link(int(left), int(right), float(weight))
    result = run(network, seed=seed, num_trials=10, two_level=True,
                 options=Options(flow_model='undirected', num_threads=1, silent=True))
    modules = result.modules()
    if set(modules) != set(edges.ravel().tolist()):
        raise ValueError('graph partition lost or added source words')
    groups = tuple(sorted(tuple(sorted(w for w, m in modules.items() if m == module))
                          for module in set(modules.values())))
    return groups, float(result.codelength)


def source_mass(values, groups):
    values = checked_counts(values)
    flat = [word for group in groups for word in group]
    if any(not group for group in groups) or len(flat) != len(set(flat)) or any(w < 0 or w >= values.shape[1] for w in flat):
        raise ValueError('nonempty disjoint source-word communities required')
    totals = values.sum(1)
    mass = np.column_stack([values[:, list(group)].sum(1) for group in groups]) if groups else np.zeros((len(values), 0))
    return np.divide(mass, totals[:, None], out=np.zeros_like(mass), where=totals[:, None] > 0)


def em_step(counts, theta, phi):
    """Sparse observed-entry E/M update; no N*V*K responsibility tensor."""
    coo = counts.tocoo()
    if coo.nnz * theta.shape[1] > 2_000_000:
        raise ValueError('sparse responsibility cap exceeded')
    weights = theta[coo.row] * phi[:, coo.col].T
    probability = weights.sum(1)
    if not np.isfinite(probability).all() or np.any(probability <= 0):
        raise ValueError('observed words require positive finite model probability')
    expected = weights * (coo.data / probability)[:, None]
    document = np.zeros_like(theta)
    word = np.zeros_like(phi.T)
    np.add.at(document, coo.row, expected)
    np.add.at(word, coo.col, expected)
    for output in (document, word.T):
        total = output.sum(1, keepdims=True)
        np.divide(output, total, out=output, where=total > 0)
    return document, word.T, float(np.dot(coo.data, np.log(probability)))


def mixture(values, groups):
    values = checked_counts(values)
    mass = source_mass(values, groups)
    if not groups:
        return mass, np.zeros((0, values.shape[1])), []
    covered = sorted(w for group in groups for w in group)
    counts = csr_matrix(values[:, covered])
    if counts.nnz * len(groups) > 2_000_000:
        raise ValueError('sparse responsibility cap exceeded')
    theta = mass + .001
    theta /= theta.sum(1, keepdims=True)
    theta[np.asarray(counts.sum(1)).ravel() == 0] = 0
    positions = {word: n for n, word in enumerate(covered)}
    phi = np.full((len(groups), len(covered)), .001)
    global_counts = np.asarray(counts.sum(0)).ravel()
    for topic, group in enumerate(groups):
        indices = [positions[w] for w in group]
        phi[topic, indices] += global_counts[indices]
    phi /= phi.sum(1, keepdims=True)
    trajectory = []
    for _ in range(50):
        theta, phi, likelihood = em_step(counts, theta, phi)
        if trajectory and likelihood < trajectory[-1] - 1e-10 * max(1, abs(trajectory[-1])):
            raise ValueError('EM weighted likelihood decreased')
        trajectory.append(likelihood)
    # Measure the final parameters too; each preceding likelihood is pre-update.
    _, _, likelihood = em_step(counts, theta, phi)
    if likelihood < trajectory[-1] - 1e-10 * max(1, abs(trajectory[-1])):
        raise ValueError('final EM likelihood decreased')
    trajectory.append(likelihood)
    full_phi = np.zeros((len(groups), values.shape[1]))
    full_phi[:, covered] = phi
    return theta * mass.sum(1, keepdims=True), full_phi, trajectory


def assignments(scores, threshold, budget):
    scores = checked_counts(scores)
    if not 0 < threshold <= 1 or budget < 1:
        raise ValueError('positive bounded threshold and topic budget required')
    output = []
    for row in scores:
        best = float(row.max()) if len(row) else 0
        candidates = [i for i, score in enumerate(row) if score >= threshold and score >= .5 * best]
        output.append(sorted(candidates, key=lambda i: (-row[i], i))[:budget])
    return output
