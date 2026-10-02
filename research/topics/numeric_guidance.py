"""Candidate exact binary64 bins and rational rank fusion; not production policy."""
from dataclasses import dataclass
from fractions import Fraction
import math


SCALE = 10**12
RRF_K = 60


def finite(value: float) -> float:
    if type(value) is not float or not math.isfinite(value):
        raise ValueError('finite binary64 float required')
    return value


def identity(value: int) -> int:
    if type(value) is not int or value < 0:
        raise ValueError('nonnegative integer identity required')
    return value


def nearest_even(value: Fraction) -> int:
    quotient, remainder = divmod(value.numerator, value.denominator)
    twice = 2 * remainder
    return quotient + int(twice > value.denominator or
                          (twice == value.denominator and quotient % 2 != 0))


def key(score: float) -> int:
    return nearest_even(Fraction.from_float(finite(score)) * SCALE)


@dataclass(frozen=True)
class Stability:
    lower_key: int
    upper_key: int
    key_stable: bool
    eligibility_stable: bool

    @property
    def stable(self) -> bool:
        return self.key_stable and self.eligibility_stable


def stability(score: float, absolute_error: float) -> Stability:
    center = Fraction.from_float(finite(score))
    error = Fraction.from_float(finite(absolute_error))
    if error < 0:
        raise ValueError('nonnegative absolute error required')
    lower, upper = center - error, center + error
    lo, hi = nearest_even(lower * SCALE), nearest_even(upper * SCALE)
    return Stability(lo, hi, lo == hi, (lower > 0) == (upper > 0))


@dataclass(frozen=True)
class Ranked:
    id: int
    score: float
    key: int
    rank: int


def ranked(scores: list[float], ids: list[int]) -> tuple[Ranked, ...]:
    if len(scores) != len(ids):
        raise ValueError('aligned scores and identities required')
    checked_ids = [identity(i) for i in ids]
    if len(set(checked_ids)) != len(ids):
        raise ValueError('unique identities required')
    pairs = [(finite(score), i) for score, i in zip(scores, checked_ids)]
    eligible = [(key(score), i, score) for score, i in pairs if score > 0]
    eligible.sort(key=lambda row: (-row[0], row[1]))
    return tuple(Ranked(i, score, bucket, rank) for rank, (bucket, i, score)
                 in enumerate(eligible, 1))


def fused(source_rank: int, parent_rank: int | None) -> Fraction:
    if type(source_rank) is not int or source_rank < 1:
        raise ValueError('positive source ordinal rank required')
    if parent_rank is not None and (type(parent_rank) is not int or parent_rank < 1):
        raise ValueError('positive parent ordinal rank or None required')
    return Fraction(1, RRF_K + source_rank) + (
        Fraction(1, RRF_K + parent_rank) if parent_rank is not None else 0)


@dataclass(frozen=True)
class Match:
    source: Ranked
    parent_id: int
    parent: Ranked | None
    score: Fraction


@dataclass(frozen=True)
class Retrieval:
    matches: tuple[Match, ...]
    candidate_sources: int
    positive_parents: int


def retrieve(source_scores: list[float], source_ids: list[int], owners: list[int],
             parent_scores: list[float], parent_ids: list[int]) -> Retrieval:
    sources = ranked(source_scores, source_ids)
    parents = ranked(parent_scores, parent_ids)
    if len(owners) != len(source_ids):
        raise ValueError('aligned parent ownership required')
    checked = [identity(i) for i in owners]
    if not set(checked).issubset(parent_ids):
        raise ValueError('all source parents must be within scope')
    ownership = dict(zip(source_ids, checked))
    positive = {row.id: row for row in parents}
    matches = []
    for source in sources:
        parent_id = ownership[source.id]
        parent = positive.get(parent_id)
        matches.append(Match(source, parent_id, parent,
                             fused(source.rank, parent.rank if parent else None)))
    matches.sort(key=lambda row: (-row.score, row.source.id))
    return Retrieval(tuple(matches[:20]), len(sources), len(parents))
