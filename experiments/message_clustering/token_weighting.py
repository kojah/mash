"""Thread-local word weights applied after contextual inference, before the codec."""

import math
import re
from collections import Counter
from dataclasses import dataclass
from enum import StrEnum

import numpy as np
from numpy.typing import NDArray
from tokenizers import Encoding

from local_encoder import Encoder, Selection, pool_vectors, selection_mask
from local_representation import CODE

WORD = re.compile(r"(?<![\w./:+-])[A-Za-z]+(?:['’][A-Za-z]+)?(?![\w./:+-])")
FUNCTION = frozenset(
    "a an the i me my mine we us our ours you your yours he him his she her hers "
    "it its they them their theirs this that these those am is are was were be "
    "been being have has had do does did and or but if then than as at by for "
    "from in into of on to with so also very just um uh uhh umm basically "
    "actually hello hi hey thanks thank please".split()
)
NEGATION = frozenset(
    "no not never neither nor without cannot can't won't don't doesn't didn't "
    "isn't aren't wasn't weren't shouldn't wouldn't couldn't mustn't".split()
)


class Policy(StrEnum):
    UNIFORM = "uniform"
    FUNCTION = "function"
    IDF = "idf"
    SIF = "sif"


@dataclass(frozen=True)
class Word:
    start: int
    end: int
    text: str


def words(text: str) -> tuple[Word, ...]:
    protected = tuple((m.start(), m.end()) for m in CODE.finditer(text))
    return tuple(
        Word(m.start(), m.end(), m.group().lower().replace("’", "'"))
        for m in WORD.finditer(text)
        if not any(m.start() < right and m.end() > left for left, right in protected)
        and (m.group().islower() or m.group().lower() in FUNCTION)
        and m.group().lower().replace("’", "'") not in NEGATION
        and not m.group().lower().replace("’", "'").endswith("n't")
    )


@dataclass(frozen=True)
class Frequencies:
    documents: int
    counts: Counter[str]
    df: Counter[str]

    @classmethod
    def from_messages(cls, messages: tuple[str, ...]) -> "Frequencies":
        counts: Counter[str] = Counter()
        df: Counter[str] = Counter()
        for text in messages:
            terms = tuple(w.text for w in words(text))
            counts.update(terms)
            df.update(set(terms))
        return cls(len(messages), counts, df)

    def weight(self, word: str, policy: Policy) -> float:
        if policy == Policy.UNIFORM:
            return 1.0
        if policy == Policy.FUNCTION:
            return 0.25 if word in FUNCTION else 1.0
        if not self.documents or word not in self.counts:
            return 1.0
        if policy == Policy.IDF:
            return max(
                0.25,
                (1 + math.log((self.documents + 1) / (self.df[word] + 1)))
                / (1 + math.log(self.documents + 1)),
            )
        probability = self.counts[word] / self.counts.total()
        return max(0.10, 0.001 / (0.001 + probability))


def weights(
    text: str,
    encoded: Encoding,
    mask: NDArray[np.int64],
    frequency: Frequencies,
    policy: Policy,
) -> NDArray[np.float32]:
    """Specials/code stay full weight; subtokens inherit their whole word weight."""
    result = mask.astype(np.float32)
    spans = words(text)
    cursor = 0
    for index, (left, right) in enumerate(encoded.offsets):
        if right <= left:
            continue
        while cursor < len(spans) and spans[cursor].end <= left:
            cursor += 1
        if cursor < len(spans):
            word = spans[cursor]
            if left < word.end and right > word.start:
                result[index] *= frequency.weight(word.text, policy)
    return result


def encode_policies(
    encoder: Encoder,
    text: str,
    frequency: Frequencies,
    selection: Selection | None = None,
) -> tuple[dict[Policy, NDArray[np.float64]], dict[Policy, int]]:
    """One singleton inference; four policies never alter the input or attention."""
    encoded, attention, hidden = encoder.states((text,))
    mask = selection_mask((text,), encoded, attention, (selection,))
    vectors = {}
    coverage = {}
    for policy in Policy:
        weighted = weights(text, encoded[0], mask[0], frequency, policy)
        vectors[policy] = pool_vectors(hidden, weighted[None])[0]
        coverage[policy] = int(np.count_nonzero((weighted < mask[0]) & (mask[0] > 0)))
    return vectors, coverage
