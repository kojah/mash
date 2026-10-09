"""Typed mutations; absence is never converted into a deletion here."""

from dataclasses import dataclass
from typing import TypeAlias

from ._generated.batch import Batch, ChangeDeleteObject, ChangeUpsertObject

Mutation: TypeAlias = ChangeUpsertObject | ChangeDeleteObject


@dataclass(frozen=True)
class Changes:
    mutations: tuple[Mutation, ...] = ()

    @classmethod
    def empty(cls) -> "Changes":
        return cls()

    @classmethod
    def upsert(cls, *objects: ChangeUpsertObject) -> "Changes":
        return cls(tuple(item.model_copy(deep=True) for item in objects))

    @classmethod
    def delete(cls, *ids: str) -> "Changes":
        """Delete objects and, through their parent edges, their descendants."""
        return cls(tuple(ChangeDeleteObject(id=id) for id in ids))

    def __add__(self, other: "Changes") -> "Changes":
        return Changes(self.mutations + other.mutations)

    def to_batch(
        self, *, source_id: str, batch_id: str, expected_revision: int
    ) -> Batch:
        """Bind stable retry identity and ordering; an empty batch is invalid."""
        return Batch(
            source_id=source_id,
            batch_id=batch_id,
            expected_revision=expected_revision,
            changes=[mutation.model_copy(deep=True) for mutation in self.mutations],
        )
