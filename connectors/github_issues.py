"""Idealized GitHub Issues connector: issue chat, body message, flat comment messages."""

from collections.abc import AsyncIterator, Sequence
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Literal, Protocol

from mash_sdk import (
    Changes,
    Chat,
    Connector,
    Debounce,
    Delivery,
    Destination,
    Message,
    Snapshot,
)

from .common import Operation, Sender, milliseconds
from .scope import Scope


@dataclass(frozen=True)
class Issue:
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


@dataclass(frozen=True)
class Comment:
    id: str
    issue_id: str
    content: str
    author: Sender
    url: str
    created_at: datetime
    updated_at: datetime


@dataclass(frozen=True)
class Event:
    operation: Operation
    repository: str
    issue_id: str
    comment_id: str | None = None


class GitHub(Protocol):
    """Adapter owns App auth, webhook verification, polling, and all pagination."""

    def issues(self, repositories: Sequence[str]) -> AsyncIterator[Issue]: ...
    async def issue(self, repository: str, id: str) -> Issue: ...
    def comments(
        self, issue: Issue, *, since: datetime | None
    ) -> AsyncIterator[Comment]: ...
    async def comment(self, repository: str, id: str) -> Comment: ...
    def events(self, repositories: Sequence[str]) -> AsyncIterator[Delivery[Event]]: ...


def chat(issue: Issue) -> Chat:
    return Chat(
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
        },
    )


def body(issue: Issue) -> Message:
    return Message(
        id=f"issue:{issue.id}:body",
        chat_id=issue.id,
        content=issue.body,
        author=issue.author.to_author(),
        url=issue.url,
        created_at=milliseconds(issue.created_at),
        updated_at=milliseconds(issue.updated_at),
    )


def message(comment: Comment) -> Message:
    # GitHub issue comments have no native reply hierarchy. Do not invent one.
    return Message(
        id=f"comment:{comment.id}",
        chat_id=comment.issue_id,
        content=comment.content,
        author=comment.author.to_author(),
        url=comment.url,
        created_at=milliseconds(comment.created_at),
        updated_at=milliseconds(comment.updated_at),
    )


async def backfill(source: GitHub, scope: Scope) -> AsyncIterator[Snapshot]:
    since = scope.cutoff()
    async for issue in source.issues(scope.ids):
        if issue.repository not in scope.ids:
            continue
        comments = [
            message(comment)
            async for comment in source.comments(issue, since=since)
            if since is None or comment.created_at >= since
        ]
        messages = (
            [body(issue)] if since is None or issue.created_at >= since else []
        ) + comments
        # Old issues may contain recent comments, so issue creation is not a discovery cutoff.
        if messages:
            yield Snapshot(
                chat=chat(issue), messages=messages, complete=True, created_since=since
            )


async def refresh(source: GitHub, event: Event, scope: Scope) -> Changes:
    if event.repository not in scope.ids:
        return Changes.empty()
    match event.operation:
        case Operation.DELETE_CHAT:
            return Changes.delete_chats(event.issue_id)
        case Operation.REFRESH_CHAT:
            # Closing/relabeling is an upsert, not deletion.
            issue = await source.issue(event.repository, event.issue_id)
            if scope.includes(issue.repository, issue.created_at):
                return Changes.upsert(chat(issue), body(issue))
            return Changes.upsert(chat(issue))
        case Operation.DELETE_MESSAGE | Operation.REFRESH_MESSAGE:
            if event.comment_id is None:
                raise ValueError("comment events require a comment ID")
            if event.operation == Operation.DELETE_MESSAGE:
                return Changes.delete_messages(f"comment:{event.comment_id}")
            comment = await source.comment(event.repository, event.comment_id)
            if not scope.includes(event.repository, comment.created_at):
                return Changes.empty()
            issue = await source.issue(event.repository, event.issue_id)
            return Changes.upsert(chat(issue), message(comment))


async def run(
    source: GitHub,
    *,
    installation_scope: str,
    scope: Scope,
    destination: Destination,
    state_path: Path,
) -> None:
    async with Connector(
        source_id=f"github:{installation_scope}",
        destination=destination,
        state_path=state_path,
    ) as connector:
        await connector.run(
            backfill=backfill(source, scope),
            events=source.events(scope.ids),
            refresh=lambda event: refresh(source, event, scope),
            debounce=Debounce(
                quiet_seconds=scope.quiet_seconds,
                max_wait_seconds=scope.max_wait_seconds,
            ),
        )
