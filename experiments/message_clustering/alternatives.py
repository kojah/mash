"""Fixed second-study candidates; original study settings remain unchanged."""

import warnings
from dataclasses import dataclass
from enum import StrEnum

import numpy as np
from sklearn.cluster import (
    OPTICS,
    AffinityPropagation,
    AgglomerativeClustering,
    SpectralClustering,
)
from sklearn.exceptions import ConvergenceWarning

from clustering import VectorMatrix


class Alternative(StrEnum):
    COMPLETE_LINK = "complete_link"
    DP_MEANS = "dp_means"
    OPTICS = "optics"
    SPECTRAL = "spectral"
    AFFINITY = "affinity"


@dataclass(frozen=True)
class Setting:
    method: Alternative
    parameter: float

    def __post_init__(self) -> None:
        if self.method == Alternative.SPECTRAL:
            if self.parameter not in (4, 8, 16):
                raise ValueError("unregistered spectral cluster count")
        elif not 0 < self.parameter < 1:
            raise ValueError("parameter must be between zero and one")


SETTINGS = tuple(
    Setting(method, parameter)
    for method, parameters in (
        (Alternative.COMPLETE_LINK, (0.35, 0.50, 0.65)),
        (Alternative.DP_MEANS, (0.35, 0.50, 0.65)),
        (Alternative.OPTICS, (0.03, 0.05, 0.10)),
        (Alternative.SPECTRAL, (4, 8, 16)),
        (Alternative.AFFINITY, (0.25, 0.50, 0.75)),
    )
    for parameter in parameters
)


def dp_means(vectors: VectorMatrix, penalty: float) -> tuple[int, ...]:
    """Euclidean DP-means with arithmetic centres and fixed source-order ties.

    Implements the paper's assignment/create/recompute loop, starting at the
    global mean. Centres are not renormalized. Squared penalty is 2*(1-cosine)
    for unit-vector pairs; this is not equivalent for later arithmetic centres.
    An iteration cap is a reported failure, not an accepted partial fit.
    """
    centres = [vectors.mean(axis=0)]
    previous: tuple[int, ...] | None = None
    for _ in range(100):
        assigned: list[int] = []
        for vector in vectors:
            distances = np.sum((np.stack(centres) - vector) ** 2, axis=1)
            closest = int(np.argmin(distances))
            if distances[closest] > penalty:
                closest = len(centres)
                centres.append(vector.copy())
            assigned.append(closest)
        occupied = sorted(set(assigned))
        remap = {label: index for index, label in enumerate(occupied)}
        labels = tuple(remap[label] for label in assigned)
        centres = [
            vectors[np.array(assigned) == label].mean(axis=0) for label in occupied
        ]
        if labels == previous:
            return labels
        previous = labels
    raise ConvergenceWarning("DP-means did not stabilize in 100 iterations")


def partition(vectors: VectorMatrix, setting: Setting) -> tuple[int, ...]:
    """Fit one fixed setting. Caller supplies unique normalized projections."""
    n = len(vectors)
    if n <= 1:
        return (0,) * n
    with warnings.catch_warnings():
        # A non-converged fit cannot quietly count as a quality success.
        warnings.simplefilter("error", ConvergenceWarning)
        if setting.method == Alternative.DP_MEANS:
            labels = dp_means(vectors, 2 * (1 - setting.parameter))
        elif setting.method == Alternative.COMPLETE_LINK:
            labels = AgglomerativeClustering(
                n_clusters=None,
                metric="cosine",
                linkage="complete",
                distance_threshold=1 - setting.parameter,
            ).fit_predict(vectors)
        elif setting.method == Alternative.OPTICS:
            labels = OPTICS(
                min_samples=min(5, n),
                min_cluster_size=min(3, n),
                metric="cosine",
                algorithm="brute",
                cluster_method="xi",
                xi=setting.parameter,
                n_jobs=1,
            ).fit_predict(vectors)
        else:
            similarities = np.clip(vectors @ vectors.T, -1, 1)
            if setting.method == Alternative.AFFINITY:
                # Negative squared Euclidean distance for normalized messages.
                affinity = -2 * (1 - similarities)
                preference = float(
                    np.quantile(affinity[np.triu_indices(n, k=1)], setting.parameter)
                )
                labels = AffinityPropagation(
                    affinity="precomputed",
                    preference=preference,
                    damping=0.8,
                    max_iter=300,
                    convergence_iter=30,
                    random_state=0,
                ).fit_predict(affinity)
            else:
                # Nonnegative heat-kernel weights; no training or PCA.
                affinity = np.exp(-(1 - similarities) / 0.2)
                labels = SpectralClustering(
                    n_clusters=min(int(setting.parameter), n - 1),
                    affinity="precomputed",
                    assign_labels="cluster_qr",
                    eigen_solver="arpack",
                    eigen_tol=1e-5,
                    random_state=0,
                ).fit_predict(affinity)
    remap: dict[int, int] = {}
    return tuple(
        -1 if label < 0 else remap.setdefault(int(label), len(remap))
        for label in labels
    )
