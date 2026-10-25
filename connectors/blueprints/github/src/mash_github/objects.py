"""Issues and PRs are containers; descriptions, comments and reviews are entries.

IDs use GitHub's numeric issue IDs, which survive repository renames and
transfers. A PR is listed through the issues API, so it has one canonical ID.
"""

from datetime import datetime

from githubkit.utils import UNSET
from githubkit_schemas.latest.models import (
    Issue,
    IssueComment,
    PullRequestReview,
    PullRequestReviewComment,
    SimpleUser,
)
from mash_sdk import Author, AuthorKind, KindRule, Metadata, Object, Role

ISSUE = "github:issue"
PULL_REQUEST = "github:pull-request"
DESCRIPTION = "github:description"
COMMENT = "github:comment"
REVIEW = "github:review"
REVIEW_COMMENT = "github:review-comment"
KINDS = (
    KindRule(kind=ISSUE, role=Role.container, root=True),
    KindRule(kind=PULL_REQUEST, role=Role.container, root=True),
    KindRule(kind=DESCRIPTION, role=Role.entry, parents=[ISSUE, PULL_REQUEST]),
    KindRule(kind=COMMENT, role=Role.entry, parents=[ISSUE, PULL_REQUEST]),
    KindRule(kind=REVIEW, role=Role.entry, parents=[PULL_REQUEST]),
    KindRule(kind=REVIEW_COMMENT, role=Role.entry, parents=[PULL_REQUEST]),
)

# Keeps metadata well under Mash's 256 KiB limit.
MAX_HUNK_CHARS = 4096


def milliseconds(value: datetime | None) -> int | None:
    return None if value is None else int(value.timestamp() * 1000)


def chat_id(issue: Issue) -> str:
    return f"issue:{issue.id}"


def author(user: SimpleUser | None) -> Author | None:
    if user is None:  # A deleted account.
        return None
    kind = AuthorKind.bot if user.type == "Bot" else AuthorKind.human
    return Author(id=f"github:{user.id}", kind=kind, name=user.login)


def present(**values: object) -> Metadata:
    return {
        key: value
        for key, value in values.items()
        if value is not None and value is not UNSET
    }


def chat(repository: str, issue: Issue) -> Object:
    labels = [label if isinstance(label, str) else label.name for label in issue.labels]
    pull = issue.pull_request or None
    return Object(
        id=chat_id(issue),
        role=Role.container,
        kind=PULL_REQUEST if pull else ISSUE,
        title=issue.title,
        url=issue.html_url,
        created_at=milliseconds(issue.created_at),
        updated_at=milliseconds(issue.updated_at),
        metadata=present(
            repository=repository,
            number=issue.number,
            state=issue.state,
            state_reason=issue.state_reason,
            labels=labels,
            draft=issue.draft if pull else None,
            merged=bool(pull.merged_at) if pull else None,
        ),
    )


def description(issue: Issue) -> Object | None:
    if not issue.body:
        return None
    return Object(
        id=f"{chat_id(issue)}:body",
        role=Role.entry,
        kind=DESCRIPTION,
        parent=chat_id(issue),
        author=author(issue.user),
        content=issue.body,
        url=issue.html_url,
        created_at=milliseconds(issue.created_at),
        updated_at=milliseconds(issue.updated_at),
    )


def comment(issue: Issue, item: IssueComment) -> Object:
    return Object(
        id=f"comment:{item.id}",
        role=Role.entry,
        kind=COMMENT,
        parent=chat_id(issue),
        author=author(item.user),
        content=item.body or "",
        url=item.html_url,
        created_at=milliseconds(item.created_at),
        updated_at=milliseconds(item.updated_at),
    )


def review(issue: Issue, item: PullRequestReview) -> Object | None:
    submitted = item.submitted_at or None
    if submitted is None:  # Pending reviews are private drafts.
        return None
    state = item.state.lower()
    return Object(
        id=f"review:{item.id}",
        role=Role.entry,
        kind=REVIEW,
        parent=chat_id(issue),
        author=author(item.user),
        # An approval without text still says something in search and views.
        content=item.body or f"({state.replace('_', ' ')})",
        url=item.html_url,
        created_at=milliseconds(submitted),
        metadata={"review_state": state},
    )


def review_comment(issue: Issue, item: PullRequestReviewComment) -> Object:
    reply = item.in_reply_to_id or None
    return Object(
        id=f"review-comment:{item.id}",
        role=Role.entry,
        kind=REVIEW_COMMENT,
        parent=chat_id(issue),
        # Inline replies keep their thread; general discussion stays flat.
        reply_to=None if reply is None else f"review-comment:{reply}",
        author=author(item.user),
        content=item.body,
        url=item.html_url,
        created_at=milliseconds(item.created_at),
        updated_at=milliseconds(item.updated_at),
        metadata=present(
            review=item.pull_request_review_id,
            path=item.path,
            line=item.line,
            side=item.side,
            # A hunk can span a whole new file; its tail locates the comment.
            diff_hunk=item.diff_hunk[-MAX_HUNK_CHARS:],
        ),
    )
