"""Original-pair turn evaluation with separate cross-turn evidence."""

import multiprocessing as mp
import time
from dataclasses import dataclass
from enum import StrEnum
from multiprocessing.connection import Connection

import numpy as np
from sklearn.exceptions import ConvergenceWarning
from threadpoolctl import threadpool_limits

from alternatives import Setting, partition
from clustering import (
    Configuration,
    Membership,
    Result,
    VectorMatrix,
    primary_partition,
    result_from_partition,
)
from corpus import Record, Thread
from evaluation import Pair, PairMetrics, evaluate_pairs
from preparation import potential_substantive
from references import ThreadReference
from turns import Turn, Unit


class Counts(Record):
    tp: int
    fp: int
    fn: int
    tn: int
    ambiguous: int

    @classmethod
    def from_metrics(cls, value: PairMetrics) -> "Counts":
        return cls(
            tp=value.true_positive,
            fp=value.false_positive,
            fn=value.false_negative,
            tn=value.true_negative,
            ambiguous=value.ambiguous,
        )

    @property
    def precision(self) -> float:
        return self.tp / (self.tp + self.fp) if self.tp + self.fp else 0

    @property
    def recall(self) -> float:
        return self.tp / (self.tp + self.fn) if self.tp + self.fn else 0

    @property
    def f1(self) -> float:
        denominator = 2 * self.tp + self.fp + self.fn
        return 2 * self.tp / denominator if denominator else 0


class Status(StrEnum):
    COMPLETE = "complete"
    NONCONVERGED = "nonconverged"
    ERROR = "error"
    TIMEOUT = "timeout"


class Fit(Record):
    status: Status
    all_pairs: Counts | None = None
    cross_turn: Counts | None = None
    primary_cross_turn: Counts | None = None
    seconds: float = 0
    groups: int = 0


@dataclass(frozen=True)
class Inputs:
    thread: Thread
    turns: tuple[Turn, ...]
    units: tuple[Unit, ...]
    vectors: VectorMatrix
    reference: ThreadReference


def source_result(result: Result, inputs: Inputs, primary: bool = False) -> Result:
    """Union memberships from passages back to sources; never force one label."""
    memberships: list[dict[int, float]] = [{} for _ in inputs.thread.messages]
    retained = {group.id for group in result.groups}
    for index, unit in enumerate(inputs.units):
        selected = (
            (Membership(result.partition[index], 1),)
            if primary and result.partition[index] in retained
            else (() if primary else result.memberships[index])
        )
        for source in unit.represented_sources:
            for member in selected:
                memberships[source][member.group] = max(
                    memberships[source].get(member.group, -1), member.cosine
                )
    return Result(
        (),
        (),
        tuple(
            tuple(Membership(group, score) for group, score in values.items())
            for values in memberships
        ),
    )


def cross_pairs(inputs: Inputs) -> tuple[Pair, ...]:
    owners = {source: turn.id for turn in inputs.turns for source in turn.sources}
    # Excluded envelopes cannot form invented cross-turn relationships.
    return tuple(
        pair
        for pair in inputs.reference.pairs()
        if pair.left in owners
        and pair.right in owners
        and owners[pair.left] != owners[pair.right]
    )


def bundling(inputs: Inputs) -> Fit:
    memberships: list[tuple[Membership, ...]] = [() for _ in inputs.thread.messages]
    for index, turn in enumerate(inputs.turns):
        for source in turn.sources:
            memberships[source] = (Membership(index, 1),)
    result = Result((), (), tuple(memberships))
    return Fit(
        status=Status.COMPLETE,
        all_pairs=Counts.from_metrics(evaluate_pairs(result, inputs.reference.pairs())),
        cross_turn=Counts.from_metrics(evaluate_pairs(result, cross_pairs(inputs))),
    )


def add_bundles(result: Result, inputs: Inputs) -> Result:
    """Diagnostic union with turn bundles; cannot create a cross-turn match."""
    offset = 1 + max((m.group for row in result.memberships for m in row), default=-1)
    rows = list(result.memberships)
    for index, turn in enumerate(inputs.turns):
        for source in turn.sources:
            rows[source] += (Membership(offset + index, 1),)
    return Result(result.partition, result.groups, tuple(rows))


def fit(
    inputs: Inputs, setting: Configuration | Setting, bundle_overlay: bool = False
) -> Fit:
    with threadpool_limits(limits=1):
        start = time.perf_counter()
        if not len(inputs.units):
            labels = ()
        else:
            labels = (
                primary_partition(inputs.vectors, setting)
                if isinstance(setting, Configuration)
                else partition(inputs.vectors, setting)
            )
        supported = {
            turn.id
            for turn in inputs.turns
            if any(
                potential_substantive(inputs.thread.messages[i].projection)
                for i in turn.sources
            )
        }
        retained = {
            label
            for label in set(labels)
            if label >= 0
            and len(
                {
                    unit.turn_id
                    for unit, group in zip(inputs.units, labels, strict=True)
                    if group == label and unit.turn_id in supported
                }
            )
            >= 2
        }
        labels = tuple(label if label in retained else -1 for label in labels)
        result = result_from_partition(
            inputs.vectors, labels, 0.35, np.ones(len(labels), dtype=np.bool_)
        )
        expanded = source_result(result, inputs)
        if bundle_overlay:
            expanded = add_bundles(expanded, inputs)
        return Fit(
            status=Status.COMPLETE,
            all_pairs=Counts.from_metrics(
                evaluate_pairs(expanded, inputs.reference.pairs())
            ),
            cross_turn=Counts.from_metrics(
                evaluate_pairs(expanded, cross_pairs(inputs))
            ),
            primary_cross_turn=Counts.from_metrics(
                evaluate_pairs(source_result(result, inputs, True), cross_pairs(inputs))
            ),
            seconds=time.perf_counter() - start,
            groups=len(result.groups),
        )


def child(
    channel: Connection, inputs: Inputs, setting: Configuration | Setting
) -> None:
    with channel:
        try:
            outcome = fit(inputs, setting)
        except ConvergenceWarning:
            outcome = Fit(status=Status.NONCONVERGED)
        except (ValueError, MemoryError, np.linalg.LinAlgError):
            outcome = Fit(status=Status.ERROR)
        channel.send(outcome.model_dump())


def isolated(inputs: Inputs, setting: Configuration | Setting) -> Fit:
    context = mp.get_context("fork")
    receiver, sender = context.Pipe(duplex=False)
    process = context.Process(target=child, args=(sender, inputs, setting))
    process.start()
    sender.close()
    try:
        if receiver.poll(30):
            try:
                return Fit.model_validate(receiver.recv())
            except EOFError:
                return Fit(status=Status.ERROR)
        return Fit(status=Status.TIMEOUT)
    finally:
        receiver.close()
        process.join(timeout=0.1)
        if process.is_alive():
            process.terminate()
            process.join(timeout=1)
        if process.is_alive():
            process.kill()
            process.join()
        process.close()
