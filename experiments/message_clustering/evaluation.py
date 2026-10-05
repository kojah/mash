"""Pair evaluation excludes ambiguous/unjudged cases, never invents negatives."""

from dataclasses import dataclass
from enum import StrEnum

from clustering import Result


class Verdict(StrEnum):
    RELATED = "related"
    UNRELATED = "unrelated"
    AMBIGUOUS = "ambiguous"


@dataclass(frozen=True)
class Pair:
    left: int
    right: int
    verdict: Verdict


@dataclass(frozen=True)
class PairMetrics:
    true_positive: int
    false_positive: int
    false_negative: int
    true_negative: int
    ambiguous: int

    @property
    def precision(self) -> float:
        count = self.true_positive + self.false_positive
        return self.true_positive / count if count else 0.0

    @property
    def recall(self) -> float:
        count = self.true_positive + self.false_negative
        return self.true_positive / count if count else 0.0

    @property
    def f1(self) -> float:
        denominator = 2 * self.true_positive + self.false_positive + self.false_negative
        return 2 * self.true_positive / denominator if denominator else 0.0


def evaluate_pairs(result: Result, pairs: tuple[Pair, ...]) -> PairMetrics:
    """Score explicitly judged distinct pairs against retained overlapping groups."""
    seen: set[tuple[int, int]] = set()
    tp = fp = fn = tn = ambiguous = 0
    for pair in pairs:
        identity = (min(pair.left, pair.right), max(pair.left, pair.right))
        if pair.left == pair.right or identity in seen:
            raise ValueError("pair references must be distinct and unique")
        if min(identity) < 0 or max(identity) >= len(result.memberships):
            raise ValueError("pair index is outside the thread")
        seen.add(identity)
        if pair.verdict == Verdict.AMBIGUOUS:
            ambiguous += 1
            continue
        left = {membership.group for membership in result.memberships[pair.left]}
        right = {membership.group for membership in result.memberships[pair.right]}
        predicted = bool(left & right)
        positive = pair.verdict == Verdict.RELATED
        tp += int(predicted and positive)
        fp += int(predicted and not positive)
        fn += int(not predicted and positive)
        tn += int(not predicted and not positive)
    return PairMetrics(tp, fp, fn, tn, ambiguous)
