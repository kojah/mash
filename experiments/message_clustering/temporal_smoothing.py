"""Single-pass, branch-aware attraction between nearby original turn vectors."""

import numpy as np

from clustering import VectorMatrix
from corpus import Record, Thread
from followup_inputs import ancestor
from turns import Turn


class Setting(Record):
    alpha: float
    window: int

    @property
    def name(self) -> str:
        return f"temporal:{self.alpha:.2f}:w{self.window}"


SETTINGS = (Setting(alpha=0, window=0),) + tuple(
    Setting(alpha=alpha, window=window) for alpha in (0.05, 0.10) for window in (2, 4)
)


def lineages(thread: Thread, turns: tuple[Turn, ...]) -> dict[str, frozenset[str]]:
    result: dict[str, frozenset[str]] = {}
    for index, turn in enumerate(turns):
        if turn.id in result:
            raise ValueError("turn identities must be unique")
        parent = ancestor(thread, turns, index)
        result[turn.id] = (
            (result[parent.turn_id] | {parent.turn_id}) if parent else frozenset()
        )
    return result


def smooth(
    vectors: VectorMatrix,
    owners: tuple[str, ...],
    ancestors: dict[str, frozenset[str]],
    setting: Setting,
) -> VectorMatrix:
    """Use original neighbors only; never iterate or cross sibling branches."""
    if (
        not 0 <= setting.alpha <= 1
        or setting.window < 0
        or setting.alpha > 0
        and setting.window == 0
    ):
        raise ValueError("invalid temporal smoothing parameters")
    if (
        vectors.ndim != 2
        or len(vectors) != len(owners)
        or len(set(owners)) != len(owners)
        or not set(owners) <= set(ancestors)
        or not np.isfinite(vectors).all()
        or not np.allclose(np.linalg.norm(vectors, axis=1), 1.0, atol=1e-9, rtol=0)
    ):
        raise ValueError("smoothing requires finite unit vectors and unique owners")
    result = vectors.copy()
    if setting.alpha == 0:
        return result
    for index, owner in enumerate(owners):
        neighbors = [
            other
            for other in range(
                max(0, index - setting.window),
                min(len(owners), index + setting.window + 1),
            )
            if other != index
            and (owners[other] in ancestors[owner] or owner in ancestors[owners[other]])
        ]
        if not neighbors:
            continue
        weights = np.array(
            [setting.window + 1 - abs(index - other) for other in neighbors]
        )
        mean = np.average(vectors[neighbors], axis=0, weights=weights)
        norm = np.linalg.norm(mean)
        if norm <= 1e-12:
            continue
        mixed = (1 - setting.alpha) * vectors[index] + setting.alpha * mean / norm
        result[index] = mixed / np.linalg.norm(mixed)
    return result


def links(
    owners: tuple[str, ...], memberships: dict[str, frozenset[int]]
) -> np.ndarray:
    """Co-membership avoids comparing arbitrary group-number assignments."""
    return np.array(
        [[bool(memberships[a] & memberships[b]) for b in owners] for a in owners],
        dtype=np.bool_,
    )
