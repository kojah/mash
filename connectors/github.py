"""Idealized GitHub connector: issues/PRs are chats with comments and reviews."""

from collections.abc import AsyncIterator, Sequence
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from pathlib import Path
from typing import Literal, Protocol

from mash_sdk import (
    Changes,
    Connector,
    Debounce,
    Delivery,
    Destination,
    Object,
    Role,
    Snapshot,
)

from .common import Operation, Sender, milliseconds
from .scope import Scope


class ItemKind(StrEnum):
    ISSUE = "issue"
    PULL_REQUEST = "pull-request"


@dataclass(frozen=True)
class Item:
    """Adapter-normalized identity shared by the issues and PR endpoints."""

    id: str
    number: int
    repository: str
    title: str
    body: str
    author: Sender
    url: str
    created_at: datetime
    updated_at: datetime
    state: Literal["open", "closed"]
    labels: Sequence[str]
    kind: ItemKind
    merged: bool = False
    draft: bool = False
    base_branch: str | None = None
    head_branch: str | None = None


class CommentKind(StrEnum):
    COMMENT = "comment"
    REVIEW = "review"
    REVIEW_COMMENT = "review-comment"


class ReviewState(StrEnum):
    APPROVED = "approved"
    CHANGES_REQUESTED = "changes-requested"
    COMMENTED = "commented"
    DISMISSED = "dismissed"
    PENDING = "pending"


@dataclass(frozen=True)
class Comment:
    id: str
    item_id: str
    content: str
    author: Sender
    url: str
    created_at: datetime | None
    updated_at: datetime | None
    kind: CommentKind
    reply_to_id: str | None = None
    review_state: ReviewState | None = None
    path: str | None = None
    line: int | None = None
    side: Literal["LEFT", "RIGHT"] | None = None
    diff_hunk: str | None = None


@dataclass(frozen=True)
class Event:
    operation: Operation
    repository: str
    item_id: str
    item_kind: ItemKind
    comment_id: str | None = None
    comment_kind: CommentKind = CommentKind.COMMENT


class GitHub(Protocol):
    """Adapter owns App auth, webhook verification, polling, and all pagination."""

    def items(
        self, repositories: Sequence[str], kinds: Sequence[ItemKind]
    ) -> AsyncIterator[Item]:
        """Deduplicate PRs also returned by the issues API; use one canonical ID."""
        ...

    async def item(self, repository: str, id: str) -> Item: ...
    def comments(self, item: Item, *, since: datetime | None) -> AsyncIterator[Comment]:
        """All pages of comments, reviews and inline review replies for a PR."""
        ...

    async def comment(self, repository: str, id: str, kind: CommentKind) -> Comment: ...
    def events(
        self, repositories: Sequence[str], kinds: Sequence[ItemKind]
    ) -> AsyncIterator[Delivery[Event]]: ...


def chat(issue: Item) -> Object:
    return Object(
        role=Role.container,
        kind=f"github:{issue.kind.value}",
        id=issue.id,
        title=issue.title,
        url=issue.url,
        created_at=milliseconds(issue.created_at),
        updated_at=milliseconds(issue.updated_at),
        metadata={
            "repository": issue.repository,
            "number": issue.number,
            "state": issue.state,
            "labels": list(issue.labels),
            "native_kind": issue.kind.value,
            "merged": issue.merged,
            "draft": issue.draft,
            "base_branch": issue.base_branch,
            "head_branch": issue.head_branch,
        },
    )


def body(issue: Item) -> Object:
    return Object(
        role=Role.entry,
        kind="github:description",
        id=f"item:{issue.id}:body",
        parent=issue.id,
        content=issue.body,
        author=issue.author.to_author(),
        url=issue.url,
        created_at=milliseconds(issue.created_at),
        updated_at=milliseconds(issue.updated_at),
    )


def message(comment: Comment) -> Object:
    # General discussion is flat; preserve actual inline review replies only.
    parent = comment.reply_to_id if comment.kind == CommentKind.REVIEW_COMMENT else None
    return Object(
        role=Role.entry,
        kind=f"github:{comment.kind.value}",
        id=f"{comment.kind.value}:{comment.id}",
        parent=comment.item_id,
        content=comment.content,
        author=comment.author.to_author(),
        url=comment.url,
        created_at=milliseconds(comment.created_at),
        updated_at=milliseconds(comment.updated_at),
        reply_to=f"review-comment:{parent}" if parent else None,
        metadata={
            "native_kind": comment.kind.value,
            "review_state": comment.review_state.value
            if comment.review_state
            else None,
            "path": comment.path,
            "line": comment.line,
            "side": comment.side,
            "diff_hunk": comment.diff_hunk,
        },
    )


async def backfill(
    source: GitHub, scope: Scope, kinds: Sequence[ItemKind]
) -> AsyncIterator[Snapshot]:
    since = scope.cutoff()
    async for issue in source.items(scope.ids, kinds):
        if issue.repository not in scope.ids or issue.kind not in kinds:
            continue
        comments = [
            message(comment)
            async for comment in source.comments(issue, since=since)
            if since is None
            or comment.created_at is not None
            and comment.created_at >= since
        ]
        messages = (
            [body(issue)] if since is None or issue.created_at >= since else []
        ) + comments
        # Old issues may contain recent comments, so issue creation is not a discovery cutoff.
        if messages:
            yield Snapshot(
                container=chat(issue),
                objects=messages,
                complete=True,
                created_since=since,
            )


async def refresh(
    source: GitHub, event: Event, scope: Scope, kinds: Sequence[ItemKind]
) -> Changes:
    if event.repository not in scope.ids or event.item_kind not in kinds:
        return Changes.empty()
    match event.operation:
        case Operation.DELETE_CHAT:
            return Changes.delete(event.item_id)
        case Operation.REFRESH_CHAT:
            # Closing/relabeling is an upsert, not deletion.
            issue = await source.item(event.repository, event.item_id)
            if scope.includes(issue.repository, issue.created_at):
                return Changes.upsert(chat(issue), body(issue))
            return Changes.upsert(chat(issue))
        case Operation.DELETE_MESSAGE | Operation.REFRESH_MESSAGE:
            if event.comment_id is None:
                raise ValueError("comment events require a comment ID")
            if event.operation == Operation.DELETE_MESSAGE:
                return Changes.delete(f"{event.comment_kind.value}:{event.comment_id}")
            comment = await source.comment(
                event.repository, event.comment_id, event.comment_kind
            )
            if not scope.includes(event.repository, comment.created_at):
                return Changes.empty()
            issue = await source.item(event.repository, event.item_id)
            return Changes.upsert(chat(issue), message(comment))


async def run(
    source: GitHub,
    *,
    installation_scope: str,
    scope: Scope,
    destinations: Sequence[Destination],
    state_path: Path,
    kinds: Sequence[ItemKind] = (ItemKind.ISSUE, ItemKind.PULL_REQUEST),
) -> None:
    async with Connector(
        source_id=f"github:{installation_scope}",
        destinations=destinations,
        state_path=state_path,
    ) as connector:
        await connector.run(
            backfill=backfill(source, scope, kinds),
            events=source.events(scope.ids, kinds),
            event_type=Event,
            refresh=lambda event: refresh(source, event, scope, kinds),
            debounce=Debounce(
                quiet_seconds=scope.quiet_seconds,
                max_wait_seconds=scope.max_wait_seconds,
            ),
        )
