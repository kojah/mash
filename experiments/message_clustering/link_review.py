"""Outcome-free selection of changed links and distinct matched controls."""

from dataclasses import dataclass
from enum import StrEnum

from corpus import Record, digest

SEED = "mash-turn-link-review-20261006-v1"


class Reason(StrEnum):
    CONTEXT_ADD = "context_addition"
    CONTEXT_REMOVE = "context_removal"
    FUSION = "fusion_change"
    CONTROL = "stable_control"


class Match(StrEnum):
    EXACT = "same_history_distance_band"
    NEAREST = "same_history_nearest_distance"
    BROADER = "same_provider_node_length_distance_band"
    UNMATCHED = "unmatched"


@dataclass(frozen=True)
class Candidate:
    id: str
    thread_index: int
    a: str
    b: str
    distance: int
    long: bool
    provider: str
    node: str
    predictions: tuple[bool, ...]

    @property
    def stable(self) -> bool:
        return len(set(self.predictions)) == 1


class Draw(Record):
    pair_id: str
    reasons: tuple[Reason, ...]
    control_for: str | None = None
    match: Match | None = None


def select(
    changed: tuple[Candidate, ...], stable: tuple[Candidate, ...]
) -> tuple[Draw, ...]:
    """No labels or score tuning; hash quotas and matching are fixed prospectively."""
    selected: dict[str, set[Reason]] = {}
    for addition, reason in (
        (True, Reason.CONTEXT_ADD),
        (False, Reason.CONTEXT_REMOVE),
    ):
        for long in (False, True):
            pool = (
                c
                for c in changed
                if c.long == long
                and c.predictions[2] != c.predictions[0]
                and c.predictions[2] == addition
            )
            for case in sorted(pool, key=lambda c: digest(SEED + reason + c.id))[:6]:
                selected.setdefault(case.id, set()).add(reason)
    for long in (False, True):
        pool = (
            c
            for c in changed
            if c.long == long and c.predictions[1] != c.predictions[0]
        )
        for case in sorted(pool, key=lambda c: digest(SEED + Reason.FUSION + c.id))[
            :12
        ]:
            selected.setdefault(case.id, set()).add(Reason.FUSION)
    by_id = {c.id: c for c in changed}
    controls = []
    for identity in sorted(selected, key=lambda i: digest(SEED + "match" + i)):
        case = by_id[identity]
        available = tuple(
            c
            for c in stable
            if c.id not in selected and c.predictions[0] == case.predictions[0]
        )
        exact = tuple(
            c
            for c in available
            if c.thread_index == case.thread_index
            and (c.distance <= 4) == (case.distance <= 4)
        )
        nearest = tuple(c for c in available if c.thread_index == case.thread_index)
        broader = tuple(
            c
            for c in available
            if c.provider == case.provider
            and c.node == case.node
            and c.long == case.long
            and (c.distance <= 4) == (case.distance <= 4)
        )
        pool, match = (
            (exact, Match.EXACT)
            if exact
            else (nearest, Match.NEAREST)
            if nearest
            else (broader, Match.BROADER)
        )
        if not pool:
            controls.append(
                Draw(
                    pair_id=identity,
                    reasons=tuple(sorted(selected[identity])),
                    match=Match.UNMATCHED,
                )
            )
            continue
        control = min(
            pool,
            key=lambda c: (
                abs(c.distance - case.distance) if match == Match.NEAREST else 0,
                digest(SEED + "control" + identity + c.id),
            ),
        )
        selected[control.id] = {Reason.CONTROL}
        controls.append(
            Draw(
                pair_id=control.id,
                reasons=(Reason.CONTROL,),
                control_for=identity,
                match=match,
            )
        )
    unpaired = {c.pair_id for c in controls if c.match == Match.UNMATCHED}
    cases = tuple(
        Draw(pair_id=i, reasons=tuple(sorted(reasons)))
        for i, reasons in selected.items()
        if Reason.CONTROL not in reasons and i not in unpaired
    )
    return cases + tuple(controls)
