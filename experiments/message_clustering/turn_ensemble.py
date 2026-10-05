"""Fixed namespaced union/intersection without transitive group merging."""

from collections import Counter
from enum import StrEnum


class Mode(StrEnum):
    A = "parent_a"
    B = "parent_b"
    UNION = "union"
    INTERSECTION = "intersection"


def predictions(a: dict[str, bool], b: dict[str, bool], mode: Mode) -> dict[str, bool]:
    if a.keys() != b.keys():
        raise ValueError("parent predictions must cover identical pairs")
    return {
        identity: left
        if mode == Mode.A
        else b[identity]
        if mode == Mode.B
        else left or b[identity]
        if mode == Mode.UNION
        else left and b[identity]
        for identity, left in a.items()
    }


def memberships(
    a: dict[str, frozenset[int]], b: dict[str, frozenset[int]], mode: Mode
) -> dict[str, frozenset[tuple[int, int]]]:
    if a.keys() != b.keys():
        raise ValueError("parent memberships must cover identical turns")
    result = {}
    for owner, left in a.items():
        right = b[owner]
        if mode == Mode.A:
            groups = {(0, group) for group in left}
        elif mode == Mode.B:
            groups = {(1, group) for group in right}
        elif mode == Mode.UNION:
            groups = {(0, group) for group in left} | {(1, group) for group in right}
        else:
            groups = {(first, second) for first in left for second in right}
        result[owner] = frozenset(groups)
    if mode == Mode.INTERSECTION:
        support = Counter(group for groups in result.values() for group in groups)
        result = {
            owner: frozenset(group for group in groups if support[group] >= 2)
            for owner, groups in result.items()
        }
    return result
