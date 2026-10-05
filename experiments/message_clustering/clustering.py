"""Experimental grouping only; see docs/message-clustering-evaluation.md.

Inputs are unique source projections in source order, all from one thread and
embedding generation. Indices map back to caller-owned source identities. No
canonical history is changed, and run-local group IDs are not durable topics.
"""

import math
from dataclasses import dataclass
from enum import StrEnum

import numpy as np
from numpy.typing import NDArray
from sklearn.cluster import AgglomerativeClustering, Birch

VectorMatrix = NDArray[np.float64]


class Method(StrEnum):
    LEADER = "leader"
    BIRCH = "birch"
    AVERAGE_LINK = "average_link"


@dataclass(frozen=True)
class Configuration:
    method: Method
    cosine_threshold: float
    radius: float | None = None

    def __post_init__(self) -> None:
        if not 0 < self.cosine_threshold < 1:
            raise ValueError("cosine threshold must be between zero and one")
        if self.method == Method.BIRCH:
            if (
                self.radius is None
                or not math.isfinite(self.radius)
                or self.radius <= 0
            ):
                raise ValueError("BIRCH requires a positive finite radius")
        elif self.radius is not None:
            raise ValueError("radius is only meaningful for BIRCH")


CONFIGURATIONS = tuple(
    Configuration(method, threshold)
    for method in (Method.LEADER, Method.AVERAGE_LINK)
    for threshold in (0.35, 0.50, 0.65)
) + tuple(
    Configuration(Method.BIRCH, threshold, radius)
    for radius, threshold in ((0.40, 0.65), (0.50, 0.50), (0.60, 0.35))
)


@dataclass(frozen=True)
class Group:
    id: int
    primary_members: tuple[int, ...]
    representative: int


@dataclass(frozen=True)
class Membership:
    group: int
    cosine: float


@dataclass(frozen=True)
class Result:
    # Partitions before retention/soft assignment must remain independently
    # inspectable: overlap must not conceal a poor initial partition.
    partition: tuple[int, ...]
    groups: tuple[Group, ...]
    memberships: tuple[tuple[Membership, ...], ...]


def decode_stored_vector(
    blob: bytes, dimensions: int, norm: float
) -> NDArray[np.float64]:
    """Decode Rust's signed-i8 message codec, rejecting corrupt metadata."""
    if dimensions <= 0 or len(blob) != dimensions:
        raise ValueError("stored vector dimensions do not match bytes")
    vector = np.frombuffer(blob, dtype=np.int8).astype(np.float64)
    actual_norm = float(np.linalg.norm(vector))
    if not math.isfinite(norm) or norm <= 0 or actual_norm == 0:
        raise ValueError("stored vector norm must be positive and finite")
    # Rust records an f32 square-root norm; this tolerates its rounding, not
    # an unrelated norm or interpreting negative components as unsigned bytes.
    if not math.isclose(actual_norm, norm, rel_tol=1e-5, abs_tol=1e-5):
        raise ValueError("stored vector norm does not match bytes")
    return vector / actual_norm


def normalized_vectors(vectors: VectorMatrix) -> VectorMatrix:
    """Return unit float64 vectors without centering or changing their basis."""
    if vectors.ndim != 2 or vectors.shape[1] == 0:
        raise ValueError("vectors must have shape (messages, positive dimensions)")
    if not np.isfinite(vectors).all():
        raise ValueError("vectors must be finite")
    norms = np.linalg.norm(vectors, axis=1)
    if np.any(norms == 0) or not np.isfinite(norms).all():
        raise ValueError("vector norms must be positive and finite")
    return vectors / norms[:, None]


class OnlineLeader:
    """Append-only cosine centroids; edits/deletions require thread replay.

    Store unnormalized sums: averaging already-normalized centroids would give
    recent messages excessive weight. First-created group wins exact ties.
    """

    def __init__(self, cosine_threshold: float) -> None:
        Configuration(Method.LEADER, cosine_threshold)
        self.threshold = cosine_threshold
        self.sums: list[NDArray[np.float64]] = []
        self.labels: list[int] = []

    def append(self, vector: NDArray[np.float64]) -> int:
        unit = normalized_vectors(vector.reshape(1, -1))[0]
        if self.sums:
            if unit.shape != self.sums[0].shape:
                raise ValueError("vector dimensions changed during insertion")
            centroids = normalized_vectors(np.stack(self.sums))
            scores = centroids @ unit
            closest = int(np.argmax(scores))
            if scores[closest] >= self.threshold:
                self.sums[closest] = self.sums[closest] + unit
                self.labels.append(closest)
                return closest
        label = len(self.sums)
        self.sums.append(unit.copy())
        self.labels.append(label)
        return label


def primary_partition(
    vectors: VectorMatrix, configuration: Configuration
) -> tuple[int, ...]:
    """Fit a partition; BIRCH reassigns all messages to final leaf centres."""
    if len(vectors) <= 1:
        return (0,) * len(vectors)
    if configuration.method == Method.LEADER:
        model = OnlineLeader(configuration.cosine_threshold)
        labels = [model.append(vector) for vector in vectors]
    elif configuration.method == Method.BIRCH:
        model_birch = Birch(
            threshold=configuration.radius,
            branching_factor=50,
            n_clusters=None,
            compute_labels=False,
        )
        for vector in vectors:
            model_birch.partial_fit(vector.reshape(1, -1))
        # partial_fit.labels_ covers only the latest batch, not the thread.
        labels = [int(label) for label in model_birch.predict(vectors)]
    else:
        model_average = AgglomerativeClustering(
            n_clusters=None,
            metric="cosine",
            linkage="average",
            distance_threshold=1 - configuration.cosine_threshold,
        )
        labels = [int(label) for label in model_average.fit_predict(vectors)]
    # Third-party labels have no meaningful order. Canonicalize by first member.
    remap: dict[int, int] = {}
    return tuple(remap.setdefault(label, len(remap)) for label in labels)


def cluster(
    vectors: VectorMatrix,
    configuration: Configuration,
    substantive: NDArray[np.bool_] | None = None,
) -> Result:
    """Fit unique projections; retain groups with >=2 substantive messages.

    Soft memberships use final primary-group centroids, never feed back into
    fitting, and reject distant primary assignments too. Caller must preserve
    duplicate-to-source mappings and handle missing embeddings separately.
    """
    units = normalized_vectors(vectors)
    eligible = (
        np.ones(len(units), dtype=np.bool_) if substantive is None else substantive
    )
    if eligible.shape != (len(units),):
        raise ValueError("substantive mask must match message count")
    partition = primary_partition(units, configuration)
    groups: list[Group] = []
    centroids: list[NDArray[np.float64]] = []
    for label in sorted(set(partition)):
        members = tuple(
            index for index, group in enumerate(partition) if group == label
        )
        if sum(bool(eligible[index]) for index in members) < 2:
            continue
        mean = units[list(members)].mean(axis=0)
        norm = float(np.linalg.norm(mean))
        if norm == 0:
            continue
        centroid = mean / norm
        representative = members[int(np.argmax(units[list(members)] @ centroid))]
        groups.append(Group(label, members, representative))
        centroids.append(centroid)
    centres = np.stack(centroids) if centroids else np.empty((0, units.shape[1]))
    memberships: list[tuple[Membership, ...]] = []
    for unit in units:
        scores = np.clip(centres @ unit, -1, 1) if centroids else np.empty(0)
        best = float(scores.max()) if len(scores) else -1.0
        memberships.append(
            tuple(
                Membership(group.id, float(score))
                for group, score in zip(groups, scores, strict=True)
                if score >= configuration.cosine_threshold and score >= best - 0.05
            )
        )
    return Result(partition, tuple(groups), tuple(memberships))
