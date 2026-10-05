"""Retain original cached window vectors with owning-turn support identities."""

import hashlib
import sqlite3
from dataclasses import dataclass
from enum import StrEnum

import numpy as np

from analyze_input_followup import validate_sources
from build_input_followup import Built
from clustering import VectorMatrix
from corpus import Role, Thread
from followup_inputs import Variant
from turn_artifact import Artifact
from turns import Turn, Unit


class Representation(StrEnum):
    CONTEXT_WINDOWS = "context_windows"
    FULL_ANSWERS = "context_prompts_full_answers"


@dataclass(frozen=True)
class Points:
    vectors: VectorMatrix
    owners: tuple[str, ...]
    text_hashes: tuple[str, ...]


def cached_vector(cache: sqlite3.Connection, fingerprint: str, text: str) -> np.ndarray:
    text_hash = hashlib.sha256(text.encode()).hexdigest()
    row = cache.execute(
        "SELECT vector FROM vectors WHERE generation=? AND text_hash=?",
        (fingerprint, text_hash),
    ).fetchone()
    if not row:
        raise ValueError("frozen window vector is missing; inference is forbidden")
    vector = np.frombuffer(row[0], dtype="<f8")
    if (
        vector.shape != (384,)
        or not np.isfinite(vector).all()
        or not np.isclose(np.linalg.norm(vector), 1.0, atol=1e-9, rtol=0)
    ):
        raise ValueError("cached window vector is invalid")
    return vector


def validate_raw_unit(unit: Unit, turn: Turn, thread: Thread) -> Role:
    sources = {span.source for span in unit.spans}
    if not sources or sources != set(unit.represented_sources):
        raise ValueError("raw unit source identities disagree")
    if sources <= set(turn.answers):
        role = Role.ASSISTANT
    elif sources <= set(turn.prompts):
        role = Role.USER
    else:
        raise ValueError("raw unit crosses a turn or role boundary")
    cursor = 0
    for span in unit.spans:
        message = thread.messages[span.source]
        if message.id != span.message_id or not 0 <= span.start < span.end <= len(
            message.projection
        ):
            raise ValueError("raw unit has an invalid source range")
        literal = message.projection[span.start : span.end]
        start = unit.text.find(literal, cursor)
        if start < cursor or unit.text[cursor:start].strip():
            raise ValueError("raw unit differs from source text")
        cursor = start + len(literal)
    if unit.text[cursor:].strip():
        raise ValueError("raw unit has unaccounted source text")
    return role


def prepare_points(
    built: Built,
    original: Artifact,
    thread: Thread,
    turns: tuple[Turn, ...],
    cache: sqlite3.Connection,
    fingerprint: str,
    representation: Representation,
) -> Points:
    if built.variant != Variant.CONTEXT_MEAN or any(
        item.target for item in built.inputs
    ):
        raise ValueError("window screen requires original whole-context inputs")
    validate_sources(built, thread, turns)
    expected = tuple(turn for turn in turns if turn.prompts)
    if original.turns != expected or built.turn_ids != tuple(
        turn.id for turn in expected
    ):
        raise ValueError("raw and context window owners disagree")
    texts: list[tuple[str, str]] = [
        (item.turn_id, item.content.text)
        for item in built.inputs
        if representation == Representation.CONTEXT_WINDOWS or item.role == Role.USER
    ]
    if representation == Representation.FULL_ANSWERS:
        owners = {turn.id: turn for turn in expected}
        for unit in original.units:
            if unit.turn_id not in owners:
                raise ValueError("raw unit has no owning prompted turn")
            if validate_raw_unit(unit, owners[unit.turn_id], thread) == Role.ASSISTANT:
                texts.append((unit.turn_id, unit.text))
    if {owner for owner, _ in texts} != set(built.turn_ids):
        raise ValueError("window points must cover every prompted turn")
    # Fit sees chronological turns, then their original within-role window order.
    positions = {owner: index for index, owner in enumerate(built.turn_ids)}
    texts.sort(key=lambda item: positions[item[0]])
    return Points(
        vectors=np.stack(
            [cached_vector(cache, fingerprint, text) for _, text in texts]
        ),
        owners=tuple(owner for owner, _ in texts),
        text_hashes=tuple(
            hashlib.sha256(text.encode()).hexdigest() for _, text in texts
        ),
    )
