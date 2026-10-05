"""Aggregate the registered strata and costs without fitting or choosing a method."""

from enum import StrEnum

import numpy as np

from blind_turn_pairs import Label, Reference, Stratum
from corpus import Record
from run_study_development import Row
from study_artifacts import Cost, View
from study_scoring import JudgedPair, combine, score
from turn_evaluation import Counts


class Dimension(StrEnum):
    ALL = "all"
    PROVIDER = "provider"
    NODE = "node"
    LENGTH = "length"
    STRATUM = "sampling_stratum"
    BATCH = "sampling_batch"


class Slice(Record):
    name: str
    dimension: Dimension
    value: str
    threads: int
    counts: Counts
    primary_counts: Counts


def slices(
    rows: tuple[Row, ...], labels: dict[str, Label], references: dict[str, Reference]
) -> tuple[Slice, ...]:
    """Keep micro confusion counts, including negative-only and ambiguous pairs."""
    buckets: dict[tuple[str, Dimension, str], list[tuple[int, Counts, Counts]]] = {}
    for row in rows:
        for dimension, value in (
            (Dimension.ALL, "all"),
            (Dimension.PROVIDER, row.provider),
            (Dimension.NODE, row.node),
            (Dimension.LENGTH, "8+" if row.prompted_turns >= 8 else "2-7"),
        ):
            buckets.setdefault((row.name, dimension, value), []).append(
                (row.thread_index, row.counts, row.primary_counts)
            )
        for dimension, values in (
            (Dimension.STRATUM, tuple(stratum.value for stratum in Stratum)),
            (Dimension.BATCH, ("0", "1")),
        ):
            for value in values:
                selected = tuple(
                    p
                    for p in row.predictions
                    if (
                        p.stratum.value
                        if dimension == Dimension.STRATUM
                        else str(p.batch)
                    )
                    == value
                )
                if not selected:
                    continue
                pairs = tuple(
                    JudgedPair(
                        p.pair_id,
                        references[p.pair_id].a_turn_id,
                        references[p.pair_id].b_turn_id,
                        labels[p.pair_id].verdict,
                    )
                    for p in selected
                )
                buckets.setdefault((row.name, dimension, value), []).append(
                    (
                        row.thread_index,
                        score(pairs, {p.pair_id: p.overlapping for p in selected}),
                        score(pairs, {p.pair_id: p.primary for p in selected}),
                    )
                )
    return tuple(
        Slice(
            name=name,
            dimension=dimension,
            value=value,
            threads=len({index for index, _, _ in entries}),
            counts=combine(tuple(counts for _, counts, _ in entries)),
            primary_counts=combine(tuple(counts for _, _, counts in entries)),
        )
        for (name, dimension, value), entries in sorted(buckets.items())
    )


class FitCost(Record):
    name: str
    threads: int
    p50_seconds: float
    p95_seconds: float
    groups: int
    largest_group: int
    assignment_coverage: float | None


def fit_costs(rows: tuple[Row, ...]) -> tuple[FitCost, ...]:
    result = []
    for name in sorted({row.name for row in rows}):
        selected = tuple(row for row in rows if row.name == name)
        covered = tuple(row for row in selected if row.assigned_turns is not None)
        result.append(
            FitCost(
                name=name,
                threads=len(selected),
                p50_seconds=float(np.quantile([row.seconds for row in selected], 0.50)),
                p95_seconds=float(np.quantile([row.seconds for row in selected], 0.95)),
                groups=sum(len(row.group_sizes) for row in selected),
                largest_group=max(
                    (size for row in selected for size in row.group_sizes), default=0
                ),
                assignment_coverage=(
                    sum(row.assigned_turns or 0 for row in covered)
                    / sum(row.prompted_turns for row in covered)
                    if covered
                    else None
                ),
            )
        )
    return tuple(result)


class BuildCost(Record):
    view: View
    threads: int
    p50_seconds: float
    p95_seconds: float
    new_vectors: int
    input_units: int
    input_tokens: int
    discarded_tokens: int
    clustering_points: int


def build_costs(
    costs: tuple[Cost, ...], thread_indices: frozenset[int]
) -> tuple[BuildCost, ...]:
    expected = {(index, view) for index in thread_indices for view in View}
    actual = {(cost.thread_index, cost.view) for cost in costs}
    if actual != expected or len(costs) != len(expected):
        raise ValueError("build costs must cover each thread/view exactly once")
    return tuple(
        BuildCost(
            view=view,
            threads=len(thread_indices),
            p50_seconds=float(
                np.quantile([c.seconds for c in costs if c.view == view], 0.50)
            ),
            p95_seconds=float(
                np.quantile([c.seconds for c in costs if c.view == view], 0.95)
            ),
            new_vectors=sum(c.new_vectors for c in costs if c.view == view),
            input_units=sum(c.input_units for c in costs if c.view == view),
            input_tokens=sum(c.input_tokens for c in costs if c.view == view),
            discarded_tokens=sum(c.discarded_tokens for c in costs if c.view == view),
            clustering_points=sum(c.clustering_points for c in costs if c.view == view),
        )
        for view in View
    )
