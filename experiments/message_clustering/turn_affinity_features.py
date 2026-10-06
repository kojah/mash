"""Symmetric, thread-local features and a clique cover for learned turn affinity."""

import re
from dataclasses import dataclass

import numpy as np
from pydantic import model_validator
from sklearn.feature_extraction.text import TfidfVectorizer

from clustering import VectorMatrix
from corpus import Record
from followup_inputs import lean_prompt
from temporal_smoothing import Setting, lineages, smooth
from turn_study_corpus import Entry

WORDS = re.compile(r"[\w.-]+", re.UNICODE)
NUMBERS = re.compile(r"(?i)\bv?\d+(?:\.\d+)*\b")
IDENTIFIERS = re.compile(r"\b[\w]+(?:[_./:-][\w]+)+\b|\b[A-Z][a-z]+[A-Z]\w*\b")
NAMES = (
    "code_cos",
    "general_cos",
    "code_smooth_cos",
    "general_smooth_cos",
    "prompt_word_cos",
    "prompt_char_cos",
    "answer_word_cos",
    "joint_word_cos",
    "prompt_token_jaccard",
    "number_jaccard",
    "both_numbers",
    "number_conflict",
    "identifier_jaccard",
    "both_identifiers",
    "identifier_conflict",
    "prompt_min_log",
    "prompt_max_log",
    "prompt_length_ratio",
    "answer_min_log",
    "answer_max_log",
    "answer_length_ratio",
    "turn_distance_log",
    "relative_distance",
    "adjacent",
    "short_prompt",
)


class LinearScore(Record):
    """Portable affine-logistic score without sklearn objects or pickle loading."""

    features: tuple[str, ...]
    weights: tuple[float, ...]
    bias: float

    @model_validator(mode="after")
    def validate_space(self) -> "LinearScore":
        if self.features not in (NAMES[:4], NAMES) or len(self.weights) != len(
            self.features
        ):
            raise ValueError("linear affinity feature space differs from its contract")
        if not np.isfinite((*self.weights, self.bias)).all():
            raise ValueError("linear affinity weights must be finite")
        return self

    def predict(self, matrix: VectorMatrix) -> np.ndarray:
        if matrix.ndim != 2 or matrix.shape[1] < len(self.weights):
            raise ValueError("linear affinity inputs do not cover the feature space")
        logits = matrix[:, : len(self.weights)] @ np.asarray(self.weights) + self.bias
        if not np.isfinite(logits).all():
            raise ValueError("linear affinity logits must be finite")
        return 1 / (1 + np.exp(-np.clip(logits, -709, 709)))


@dataclass(frozen=True)
class Features:
    index: int
    owners: tuple[str, ...]
    values: VectorMatrix
    pairs: tuple[tuple[int, int], ...]


def overlap(left: frozenset[str], right: frozenset[str]) -> tuple[float, float, float]:
    both = bool(left and right)
    return (
        len(left & right) / len(left | right) if left | right else 0.0,
        float(both),
        float(both and not left & right),
    )


def lexical(texts: tuple[str, ...], characters: bool = False) -> VectorMatrix:
    vectorizer = (
        TfidfVectorizer(analyzer="char_wb", ngram_range=(3, 5), max_features=20000)
        if characters
        else TfidfVectorizer(stop_words="english", sublinear_tf=True)
    )
    try:
        values = vectorizer.fit_transform(texts)
        return np.asarray((values @ values.T).toarray(), dtype=np.float64)
    except ValueError as error:
        if "empty vocabulary" not in str(error):
            raise
        return np.zeros((len(texts), len(texts)))


def build(
    entry: Entry, owners: tuple[str, ...], code: VectorMatrix, general: VectorMatrix
) -> Features:
    turns = {t.id: t for t in entry.captured.turns if t.prompts}
    if (
        len(owners) != len(code)
        or len(owners) != len(general)
        or set(owners) != set(turns)
    ):
        raise ValueError("affinity features require aligned, complete turn ownership")
    thread = entry.captured.thread
    prompts = tuple(
        "\n\n".join(lean_prompt(thread, i).text for i in turns[o].prompts)
        for o in owners
    )
    answers = tuple(
        "\n\n".join(
            thread.messages[i].projection
            for i in turns[o].answers
            if thread.messages[i].projection.strip()
        )
        for o in owners
    )
    tokens = tuple(frozenset(WORDS.findall(p.lower())) for p in prompts)
    numbers = tuple(frozenset(NUMBERS.findall(p.lower())) for p in prompts)
    identifiers = tuple(
        frozenset(s.lower() for s in IDENTIFIERS.findall(p)) for p in prompts
    )
    ancestry = lineages(thread, entry.captured.turns)
    dense = tuple(
        v @ v.T
        for v in (
            code,
            general,
            smooth(code, owners, ancestry, Setting(alpha=0.10, window=4)),
            smooth(general, owners, ancestry, Setting(alpha=0.10, window=4)),
        )
    )
    sparse = (
        lexical(prompts),
        lexical(prompts, True),
        lexical(answers),
        lexical(tuple(p + "\n\n" + a for p, a in zip(prompts, answers, strict=True))),
    )
    pairs = tuple((i, j) for i in range(len(owners)) for j in range(i + 1, len(owners)))
    values = []
    for i, j in pairs:
        p, q = len(WORDS.findall(prompts[i])), len(WORDS.findall(prompts[j]))
        a, b = len(WORDS.findall(answers[i])), len(WORDS.findall(answers[j]))
        values.append(
            [
                *(v[i, j] for v in dense),
                *(v[i, j] for v in sparse),
                overlap(tokens[i], tokens[j])[0],
                *overlap(numbers[i], numbers[j]),
                *overlap(identifiers[i], identifiers[j]),
                np.log1p(min(p, q)),
                np.log1p(max(p, q)),
                min(p, q) / max(p, q, 1),
                np.log1p(min(a, b)),
                np.log1p(max(a, b)),
                min(a, b) / max(a, b, 1),
                np.log1p(j - i),
                (j - i) / max(len(owners) - 1, 1),
                float(j - i == 1),
                float(min(p, q) <= 4),
            ]
        )
    matrix = np.asarray(values, dtype=np.float64).reshape((-1, len(NAMES)))
    if not np.isfinite(matrix).all():
        raise ValueError("nonfinite affinity features")
    return Features(entry.captured.index, owners, matrix, pairs)


def edge_cover(
    probabilities: VectorMatrix, cutoff: float
) -> tuple[tuple[int, ...], ...]:
    """Greedy cliques plus uncovered edges; never add unsupported transitive links."""
    n = len(probabilities)
    if probabilities.shape != (n, n) or not np.allclose(probabilities, probabilities.T):
        raise ValueError("graph affinities must be square and symmetric")
    if not np.isfinite(probabilities).all() or not 0 < cutoff < 1:
        raise ValueError("graph affinities and threshold must be finite and bounded")
    edges = {
        (i, j)
        for i in range(n)
        for j in range(i + 1, n)
        if probabilities[i, j] >= cutoff
    }
    remaining = set(edges)
    groups: set[tuple[int, ...]] = set()
    for seed in range(n):
        neighbors = sorted(
            (j for j in range(n) if j != seed and probabilities[seed, j] >= cutoff),
            key=lambda j: (-probabilities[seed, j], j),
        )
        members = [seed]
        for other in neighbors:
            if all(probabilities[other, m] >= cutoff for m in members):
                members.append(other)
        group = tuple(sorted(members))
        covered = {
            (group[i], group[j])
            for i in range(len(group))
            for j in range(i + 1, len(group))
        }
        if len(group) >= 2 and covered & remaining:
            groups.add(group)
            remaining.difference_update(covered)
    groups.update(remaining)
    return tuple(sorted(groups))
