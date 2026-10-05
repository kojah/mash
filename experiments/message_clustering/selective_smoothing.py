"""One fixed short-turn semantic gate; no parameter-selection surface."""

import numpy as np

from build_input_followup import Built
from clustering import VectorMatrix
from corpus import Record, Role
from followup_inputs import Variant
from temporal_smoothing import Setting, smooth

NAME = "selective:0.10:w4:s0.55:p32:t128"


class Eligibility(Record):
    turn_id: str
    prompt_tokens: int
    own_tokens: int
    eligible: bool


def eligibility(built: Built) -> tuple[Eligibility, ...]:
    if built.variant != Variant.LEAN:
        raise ValueError("shortness must use the frozen own-input inventory")
    result = []
    for owner in built.turn_ids:
        inputs = tuple(i for i in built.inputs if i.turn_id == owner)
        prompt_tokens = sum(i.tokens for i in inputs if i.role == Role.USER)
        own_tokens = sum(i.tokens for i in inputs)
        result.append(
            Eligibility(
                turn_id=owner,
                prompt_tokens=prompt_tokens,
                own_tokens=own_tokens,
                eligible=prompt_tokens <= 32 and own_tokens <= 128,
            )
        )
    return tuple(result)


def refine(
    vectors: VectorMatrix,
    owners: tuple[str, ...],
    ancestors: dict[str, frozenset[str]],
    short: frozenset[str],
) -> VectorMatrix:
    """Gate neighbours semantically; use original vectors in one simultaneous pass."""
    result = smooth(vectors, owners, ancestors, Setting(alpha=0, window=0))
    if not short <= set(owners):
        raise ValueError("eligible targets must belong to the vector inventory")
    for i, owner in enumerate(owners):
        if owner not in short:
            continue
        neighbors = [
            j
            for j in range(max(0, i - 4), min(len(owners), i + 5))
            if j != i
            and (owners[j] in ancestors[owner] or owner in ancestors[owners[j]])
        ]
        if not neighbors:
            continue
        cosine = vectors[neighbors] @ vectors[i]
        weights = np.exp(-np.abs(np.asarray(neighbors) - i) / 2) * np.maximum(
            0, cosine - 0.55
        )
        total = weights.sum()
        if total <= 0:
            continue
        mean = (weights[:, None] * vectors[neighbors]).sum(axis=0) / total
        norm = np.linalg.norm(mean)
        if norm <= 1e-12:
            continue
        mixed = 0.9 * vectors[i] + 0.1 * mean / norm
        result[i] = mixed / np.linalg.norm(mixed)
    return result
