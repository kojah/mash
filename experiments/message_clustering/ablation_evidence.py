"""Fixed confidence/fusion factors with no label-dependent group construction."""

import hashlib
import itertools
import re
from collections import Counter
from dataclasses import dataclass

import numpy as np
from sklearn.feature_extraction.text import ENGLISH_STOP_WORDS

from clustering import (
    Configuration,
    Method,
    VectorMatrix,
    primary_partition,
    result_from_partition,
)
from corpus import Record, Thread
from followup_inputs import lean_prompt
from turns import Turn

WORDS = re.compile(r"[a-z][a-z0-9_+-]+")
STOP = frozenset(ENGLISH_STOP_WORDS) | {
    "thanks",
    "thank",
    "hello",
    "hi",
    "hey",
    "okay",
    "ok",
    "sure",
    "please",
    "yes",
    "yeah",
    "done",
}


class Cell(Record):
    context: bool
    fusion: bool
    generic: bool

    @property
    def name(self) -> str:
        return f"c{int(self.context)}f{int(self.fusion)}g{int(self.generic)}"


CELLS = tuple(
    Cell(context=c, fusion=f, generic=g)
    for c, f, g in itertools.product((False, True), repeat=3)
)


class Background(Record):
    documents: int
    document_frequency: dict[str, int]
    stoplist_sha256: str = hashlib.sha256("\n".join(sorted(STOP)).encode()).hexdigest()


def terms(text: str) -> frozenset[str]:
    return frozenset(WORDS.findall(text.lower())) - STOP


def turn_terms(thread: Thread, turn: Turn) -> frozenset[str]:
    prompts = tuple(lean_prompt(thread, i).text for i in turn.prompts)
    answers = tuple(i for i in turn.answers if thread.messages[i].projection.strip())
    text = "\n".join(
        prompts + ((thread.messages[answers[-1]].projection,) if answers else ())
    )
    return terms(text)


def generic(text: str, background: Background) -> bool:
    content = terms(text)
    return len(content) < 3 or not any(
        5 * background.document_frequency.get(t, 0) <= background.documents
        for t in content
    )


@dataclass(frozen=True)
class Evidence:
    owners: tuple[str, ...]
    partition: tuple[int, ...]
    confidences: tuple[dict[int, float], ...]


def fit(vectors: VectorMatrix, owners: tuple[str, ...]) -> Evidence:
    if len(vectors) != len(owners):
        raise ValueError("point ownership differs from vectors")
    labels = primary_partition(vectors, Configuration(Method.BIRCH, 0.75, 0.60))
    retained = {
        g
        for g in set(labels)
        if g >= 0
        and len({o for o, label in zip(owners, labels, strict=True) if label == g}) >= 2
    }
    labels = tuple(g if g in retained else -1 for g in labels)
    result = result_from_partition(
        vectors, labels, 0.75, np.ones(len(labels), dtype=np.bool_)
    )
    return Evidence(
        owners,
        labels,
        tuple({m.group: m.cosine for m in members} for members in result.memberships),
    )


def memberships(
    evidence: Evidence, cutoff: float, penalties: tuple[float, ...]
) -> dict[str, frozenset[int]]:
    if len(penalties) != len(evidence.owners):
        raise ValueError("each point needs its confidence penalty")
    result: dict[str, set[int]] = {owner: set() for owner in evidence.owners}
    for owner, scores, penalty in zip(
        evidence.owners, evidence.confidences, penalties, strict=True
    ):
        result[owner].update(
            g for g, score in scores.items() if score - penalty >= cutoff
        )
    return {owner: frozenset(groups) for owner, groups in result.items()}


def fuse(
    a: Evidence, b: Evidence, penalties: tuple[float, ...], consensus: bool
) -> dict[str, frozenset[tuple[int, int, int]]]:
    low_a = memberships(a, 0.75, (0.0,) * len(a.owners))
    low_b = memberships(b, 0.75, penalties)
    if low_a.keys() != low_b.keys():
        raise ValueError("fusion views must cover identical turns")
    high_a = memberships(a, 0.80, (0.0,) * len(a.owners)) if consensus else low_a
    high_b = memberships(b, 0.80, penalties) if consensus else low_b
    result = {}
    for owner in low_a:
        groups = {(0, g, -1) for g in high_a[owner]} | {
            (1, g, -1) for g in high_b[owner]
        }
        if consensus:
            groups |= {
                (2, first, second) for first in low_a[owner] for second in low_b[owner]
            }
        result[owner] = frozenset(groups)
    support = Counter(g for groups in result.values() for g in groups)
    return {
        owner: frozenset(g for g in groups if support[g] >= 2)
        for owner, groups in result.items()
    }
