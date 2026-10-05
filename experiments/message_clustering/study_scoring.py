"""Finite registered turn comparison; no annotation I/O or confirmation access."""

import time
from dataclasses import dataclass
from enum import StrEnum
from fractions import Fraction

import numpy as np
from sklearn.feature_extraction.text import TfidfVectorizer

from alternatives import Alternative, Setting, partition
from blind_turn_pairs import Verdict
from clustering import (
    Configuration,
    Method,
    VectorMatrix,
    primary_partition,
    result_from_partition,
)
from corpus import Record
from study_artifacts import View
from turn_evaluation import Counts


class Family(StrEnum):
    LEADER = "leader"
    AVERAGE = "average_link"
    COMPLETE = "complete_link"
    BIRCH = "birch"


class Candidate(Record):
    view: View
    family: Family
    cutoff: float
    radius: float | None = None

    @property
    def name(self) -> str:
        return f"{self.view}:{self.family}:{self.cutoff:.2f}"


CANDIDATES = tuple(
    Candidate(
        view=view,
        family=family,
        cutoff=cutoff,
        radius=radius if family == Family.BIRCH else None,
    )
    for view in View
    for family in Family
    for cutoff, radius in ((0.65, 0.75), (0.75, 0.60), (0.85, 0.45))
)


@dataclass(frozen=True)
class JudgedPair:
    id: str
    a: str
    b: str
    verdict: Verdict


@dataclass(frozen=True)
class Assignments:
    overlapping: dict[str, frozenset[int]]
    primary: dict[str, frozenset[int]]
    seconds: float
    group_sizes: tuple[int, ...]


def fit(
    vectors: VectorMatrix, owners: tuple[str, ...], candidate: Candidate
) -> Assignments:
    """Keep distinct-turn groups; union passage memberships by owning turn.

    The existing overlap rule keeps centres within 0.05 of the best score,
    additionally requiring the registered cutoff. It does not feed back into fit.
    """
    if len(vectors) != len(owners):
        raise ValueError("each clustering point needs a turn identity")
    started = time.perf_counter()
    setting = (
        Setting(Alternative.COMPLETE_LINK, candidate.cutoff)
        if candidate.family == Family.COMPLETE
        else Configuration(Method(candidate.family), candidate.cutoff, candidate.radius)
    )
    labels = (
        partition(vectors, setting)
        if isinstance(setting, Setting)
        else primary_partition(vectors, setting)
    )
    retained = {
        group
        for group in set(labels)
        if group >= 0
        and len(
            {
                owner
                for owner, label in zip(owners, labels, strict=True)
                if label == group
            }
        )
        >= 2
    }
    labels = tuple(label if label in retained else -1 for label in labels)
    result = result_from_partition(
        vectors, labels, candidate.cutoff, np.ones(len(labels), dtype=np.bool_)
    )
    primary: dict[str, set[int]] = {owner: set() for owner in owners}
    overlapping: dict[str, set[int]] = {owner: set() for owner in owners}
    for owner, label, memberships in zip(
        owners, labels, result.memberships, strict=True
    ):
        if label >= 0:
            primary[owner].add(label)
        overlapping[owner].update(member.group for member in memberships)
    return Assignments(
        overlapping={owner: frozenset(groups) for owner, groups in overlapping.items()},
        primary={owner: frozenset(groups) for owner, groups in primary.items()},
        seconds=time.perf_counter() - started,
        group_sizes=tuple(
            len({owners[i] for i in group.primary_members}) for group in result.groups
        ),
    )


def score(pairs: tuple[JudgedPair, ...], predictions: dict[str, bool]) -> Counts:
    if len({pair.id for pair in pairs}) != len(pairs):
        raise ValueError("judged pair IDs must be unique")
    if set(predictions) != {pair.id for pair in pairs}:
        raise ValueError("predictions must cover judged pairs exactly")
    tp = fp = fn = tn = ambiguous = 0
    for pair in pairs:
        if pair.verdict == Verdict.AMBIGUOUS:
            ambiguous += 1
            continue
        predicted = predictions[pair.id]
        positive = pair.verdict == Verdict.RELATED
        tp += int(predicted and positive)
        fp += int(predicted and not positive)
        fn += int(not predicted and positive)
        tn += int(not predicted and not positive)
    return Counts(tp=tp, fp=fp, fn=fn, tn=tn, ambiguous=ambiguous)


def connections(
    pairs: tuple[JudgedPair, ...], memberships: dict[str, frozenset[int]]
) -> dict[str, bool]:
    return {pair.id: bool(memberships[pair.a] & memberships[pair.b]) for pair in pairs}


def lexical_baselines(
    prompts: tuple[str, ...], owners: tuple[str, ...], pairs: tuple[JudgedPair, ...]
) -> dict[str, dict[str, bool]]:
    """Pairwise lexical cosine and disjoint chronological windows, without fit."""
    if len(prompts) != len(owners) or len(set(owners)) != len(owners):
        raise ValueError("baseline input requires one prompt per distinct turn")
    indices = {owner: i for i, owner in enumerate(owners)}
    try:
        vectors = TfidfVectorizer(stop_words="english").fit_transform(prompts)
        similarity = (vectors @ vectors.T).toarray()
    except ValueError as error:
        if "empty vocabulary" not in str(error):
            raise
        similarity = np.zeros((len(prompts), len(prompts)))
    result = {"no_cross_turn": {pair.id: False for pair in pairs}}
    for width in (2, 4):
        result[f"chronological_{width}"] = {
            pair.id: indices[pair.a] // width == indices[pair.b] // width
            for pair in pairs
        }
    for cutoff in (0.65, 0.75, 0.85):
        result[f"tfidf_{cutoff:.2f}"] = {
            pair.id: bool(similarity[indices[pair.a], indices[pair.b]] >= cutoff)
            for pair in pairs
        }
    return result


def combine(counts: tuple[Counts, ...]) -> Counts:
    return Counts(
        tp=sum(c.tp for c in counts),
        fp=sum(c.fp for c in counts),
        fn=sum(c.fn for c in counts),
        tn=sum(c.tn for c in counts),
        ambiguous=sum(c.ambiguous for c in counts),
    )


def absolute_gate(counts: Counts) -> bool:
    return counts.precision >= 0.85 and counts.recall >= 0.70 and counts.f1 >= 0.75


def minimum_gain(selected: Counts, baseline: Counts) -> bool:
    """Compare the registered 0.10 gain exactly from integer confusion counts."""

    def exact_f1(counts: Counts) -> Fraction:
        denominator = 2 * counts.tp + counts.fp + counts.fn
        return Fraction(2 * counts.tp, denominator) if denominator else Fraction(0)

    return exact_f1(selected) - exact_f1(baseline) >= Fraction(1, 10)


class Summary(Record):
    name: str
    counts: Counts
    seconds: float


class Decision(Record):
    baseline: str
    selected: str | None
    requires_gain: bool


def choose(candidates: tuple[Summary, ...], baselines: tuple[Summary, ...]) -> Decision:
    """Freeze one development choice; complex methods must earn their extra gain."""

    def order(value: Summary) -> tuple[float, float, str]:
        return (-value.counts.f1, value.seconds, value.name)

    baseline = min(baselines, key=order)
    eligible = tuple(
        candidate
        for candidate in candidates
        if absolute_gate(candidate.counts)
        and minimum_gain(candidate.counts, baseline.counts)
    )
    if eligible:
        return Decision(
            baseline=baseline.name,
            selected=min(eligible, key=order).name,
            requires_gain=True,
        )
    return Decision(
        baseline=baseline.name,
        selected=baseline.name if absolute_gate(baseline.counts) else None,
        requires_gain=False,
    )
