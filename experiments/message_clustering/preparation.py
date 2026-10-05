"""Deterministic source-to-unit mapping and optional frozen context view."""

from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray

from clustering import VectorMatrix, decode_stored_vector
from corpus import Thread
from references import ThreadReference


@dataclass(frozen=True)
class Prepared:
    vectors: VectorMatrix
    substantive: NDArray[np.bool_]
    source_to_unit: tuple[int, ...]
    unit_to_source: tuple[int, ...]
    contextualized: int
    unreviewed_assumed_substantive: int


def potential_substantive(text: str) -> bool:
    """Structural prose proxy, never reference labels or inferred topic content.

    Exclude whole injected instruction/envelope records from minimum-support
    counts. Keep vectors unchanged; this is not semantic denoising. Human/agent
    judgments remain separate and are used only for reported quality/coverage.
    """
    return len(text.split()) >= 5 and not text.lstrip().startswith(
        ("# AGENTS.md", "<skill>", "[gas-city]", "<hook_prompt", "<task-notification>")
    )


def prepare(
    thread: Thread, reference: ThreadReference, context: bool = False
) -> Prepared:
    """Deduplicate exact prepared text, retaining every original source index.

    Unreviewed projections are potential substantive units, not human-verified
    ones. Only reviewed substantive messages enter measured coverage. This
    assumption must be disclosed alongside group quality claims.
    """
    ordinals = {message.ordinal: index for index, message in enumerate(thread.messages)}
    source_vectors = np.stack(
        [
            decode_stored_vector(bytes.fromhex(m.vector_hex), m.dimensions, m.norm)
            for m in thread.messages
        ]
    )
    vectors: list[NDArray[np.float64]] = []
    substantive: list[bool] = []
    unit_to_source: list[int] = []
    source_to_unit: list[int] = []
    seen: dict[tuple[str, str | None], int] = {}
    contextualized = unreviewed = 0
    for index, message in enumerate(thread.messages):
        parent: int | None = None
        if context and message.context_parent != -1:
            parent = (
                (index - 1 if index else None)
                if message.context_parent is None
                else ordinals.get(message.context_parent)
            )
            if parent is not None and not potential_substantive(
                thread.messages[parent].projection
            ):
                parent = None
        parent_text = thread.messages[parent].projection if parent is not None else None
        identity = (message.projection, parent_text)
        if identity in seen:
            source_to_unit.append(seen[identity])
            continue
        unit = len(vectors)
        seen[identity] = unit
        source_to_unit.append(unit)
        unit_to_source.append(index)
        vector = source_vectors[index]
        if parent is not None:
            vector = 0.7 * vector + 0.3 * source_vectors[parent]
            vector = vector / np.linalg.norm(vector)
            contextualized += 1
        vectors.append(vector)
        eligible = potential_substantive(message.projection)
        substantive.append(eligible)
        unreviewed += int(eligible and index not in reference.reviewed_indices)
    return Prepared(
        np.stack(vectors),
        np.array(substantive, dtype=np.bool_),
        tuple(source_to_unit),
        tuple(unit_to_source),
        contextualized,
        unreviewed,
    )
