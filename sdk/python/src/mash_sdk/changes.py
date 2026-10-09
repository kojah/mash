"""Typed mutations; absence is never converted into a deletion here."""

from dataclasses import dataclass
from typing import TypeAlias

from ._generated.batch import (
    Batch,
    ChangeDeleteChat,
    ChangeDeleteDocument,
    ChangeDeleteMessage,
    ChangeUpsertChat,
    ChangeUpsertDocument,
    ChangeUpsertMessage,
)

Mutation: TypeAlias = (
    ChangeUpsertChat
    | ChangeUpsertMessage
    | ChangeUpsertDocument
    | ChangeDeleteChat
    | ChangeDeleteMessage
    | ChangeDeleteDocument
)


@dataclass(frozen=True)
class Changes:
    mutations: tuple[Mutation, ...] = ()

    @classmethod
    def empty(cls) -> "Changes":
        return cls()

    @classmethod
    def upsert(
        cls, *records: ChangeUpsertChat | ChangeUpsertMessage | ChangeUpsertDocument
    ) -> "Changes":
        return cls(tuple(record.model_copy(deep=True) for record in records))

    @classmethod
    def delete_chats(cls, *ids: str) -> "Changes":
        return cls(tuple(ChangeDeleteChat(id=id) for id in ids))

    @classmethod
    def delete_messages(cls, *ids: str) -> "Changes":
        return cls(tuple(ChangeDeleteMessage(id=id) for id in ids))

    @classmethod
    def delete_documents(cls, *ids: str) -> "Changes":
        return cls(tuple(ChangeDeleteDocument(id=id) for id in ids))

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
