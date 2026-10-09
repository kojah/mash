"""Turn snapshots and refreshed changes into the batches that need sending."""

from collections.abc import Iterator, Sequence
from dataclasses import dataclass, field
from datetime import datetime

import orjson

from ._generated.batch import (
    ChangeUpsertChat,
    ChangeUpsertDocument,
    ChangeUpsertMessage,
)
from ._state import RecordRow, StateStore
from .changes import Changes, Mutation
from .codec import Contract, fingerprint
from .runner import DocumentSnapshot, Snapshot

# Room for the batch envelope (identities, revision, brackets) under the byte limit.
ENVELOPE_BYTES = 4096
MAX_MUTATIONS = 512


@dataclass(frozen=True)
class Plan:
    changes: Changes
    document_scopes: dict[str, frozenset[str]] = field(default_factory=dict)


def unchanged_removed(
    store: StateStore, destination_id: str, source_id: str, changes: Changes
) -> Changes:
    """Drop upserts whose acknowledged fingerprint already matches."""
    kept: list[Mutation] = []
    for mutation in changes.mutations:
        match mutation:
            case ChangeUpsertChat():
                kind = "chat"
            case ChangeUpsertMessage():
                kind = "message"
            case ChangeUpsertDocument():
                kind = "document"
            case _:
                kept.append(mutation)
                continue
        known = store.fingerprints(
            destination_id=destination_id,
            source_id=source_id,
            kind=kind,
            ids=[mutation.id],
        )
        if known.get(mutation.id) != fingerprint(mutation):
            kept.append(mutation)
    return Changes(tuple(kept))


def omitted(row: RecordRow, present: set[str], since: int | None) -> bool:
    if row.id in present:
        return False
    # An unknown creation time cannot be placed inside a bounded window.
    return since is None or row.created_at is not None and row.created_at >= since


def milliseconds(value: datetime | None) -> int | None:
    if value is None:
        return None
    if value.tzinfo is None:
        raise ValueError("snapshot windows need timezone-aware datetimes")
    return int(value.timestamp() * 1000)


def chat_plan(
    store: StateStore, destination_id: str, source_id: str, snapshot: Snapshot
) -> Plan:
    changes = Changes.upsert(snapshot.chat, *snapshot.messages)
    if snapshot.complete:
        since = milliseconds(snapshot.created_since)
        present = {message.id for message in snapshot.messages}
        # Mash rejects deleting a message that a message in the same batch
        # replies to; a parent the source still references is not deleted.
        referenced = {m.reply_to for m in snapshot.messages if m.reply_to is not None}
        gone = sorted(
            row.id
            for row in store.chat_messages(
                destination_id=destination_id,
                source_id=source_id,
                chat_id=snapshot.chat.id,
            )
            if omitted(row, present, since) and row.id not in referenced
        )
        changes = changes + Changes.delete_messages(*gone)
    return Plan(unchanged_removed(store, destination_id, source_id, changes))


def document_plan(
    store: StateStore, destination_id: str, source_id: str, snapshot: DocumentSnapshot
) -> Plan:
    changes = Changes.upsert(*snapshot.documents)
    if not snapshot.complete:
        return Plan(unchanged_removed(store, destination_id, source_id, changes))
    scope = frozenset(snapshot.scope_ids)
    since = milliseconds(snapshot.created_since)
    present = {document.id for document in snapshot.documents}
    # Covered only when this snapshot spans every scope the document came from,
    # so narrowing a scope never deletes what it excludes.
    gone = sorted(
        row.id
        for row in store.scoped_documents(
            destination_id=destination_id, source_id=source_id
        )
        if row.scope is not None and row.scope <= scope and omitted(row, present, since)
    )
    changes = unchanged_removed(store, destination_id, source_id, changes)
    # The latest complete snapshot containing a document defines its scope.
    # Sent documents record it on ACK; unchanged ones are already acknowledged.
    scopes = {
        mutation.id: scope
        for mutation in changes.mutations
        if isinstance(mutation, ChangeUpsertDocument)
    }
    store.assign_scope(
        destination_id=destination_id,
        source_id=source_id,
        ids=sorted(present - scopes.keys()),
        scope=scope,
    )
    return Plan(changes + Changes.delete_documents(*gone), scopes)


def snapshot_plan(
    store: StateStore,
    destination_id: str,
    source_id: str,
    snapshot: Snapshot | DocumentSnapshot,
) -> Plan:
    match snapshot:
        case Snapshot():
            return chat_plan(store, destination_id, source_id, snapshot)
        case DocumentSnapshot():
            return document_plan(store, destination_id, source_id, snapshot)


def batches(plan: Plan, contract: Contract) -> Iterator[Plan]:
    """Split under the mutation-count and encoded-size limits, preserving order."""
    budget = contract.limits.batch_bytes - ENVELOPE_BYTES
    current: list[Mutation] = []
    size = 0
    for mutation in plan.changes.mutations:
        cost = len(orjson.dumps(mutation.model_dump(mode="json"))) + 1
        if current and (len(current) == MAX_MUTATIONS or size + cost > budget):
            yield subset(plan, current)
            current, size = [], 0
        current.append(mutation)
        size += cost
    if current:
        yield subset(plan, current)


def subset(plan: Plan, mutations: Sequence[Mutation]) -> Plan:
    ids = {m.id for m in mutations if isinstance(m, ChangeUpsertDocument)}
    return Plan(
        Changes(tuple(mutations)),
        {id: scope for id, scope in plan.document_scopes.items() if id in ids},
    )
