"""Docs become standalone documents; their comments live in a sibling container.

Mash never lets a document be a parent, so a Doc's comments sit in one
`google-docs:comments` container per Doc, which names the Doc in metadata.
"""

import hashlib
from datetime import datetime

from mash_sdk import Author, AuthorKind, KindRule, Metadata, Object, Role, Snapshot

from .drive import Comment, File, User

DOCUMENT = "google-docs:document"
COMMENTS = "google-docs:comments"
COMMENT = "google-docs:comment"
REPLY = "google-docs:reply"
KINDS = (
    KindRule(kind=DOCUMENT, role=Role.document, root=True),
    KindRule(kind=COMMENTS, role=Role.container, root=True),
    KindRule(kind=COMMENT, role=Role.entry, parents=[COMMENTS]),
    KindRule(kind=REPLY, role=Role.entry, parents=[COMMENTS]),
)
# Mash's per-object content limit; larger exports are cut and marked.
MAX_CONTENT_BYTES = 1024 * 1024


def document_id(file_id: str) -> str:
    return f"doc:{file_id}"


def comments_id(file_id: str) -> str:
    return f"comments:{file_id}"


def url(file_id: str) -> str:
    return f"https://docs.google.com/document/d/{file_id}/edit"


def milliseconds(value: datetime | None) -> int | None:
    return None if value is None else int(value.timestamp() * 1000)


def person(user: User) -> dict[str, str]:
    return {
        key: value
        for key, value in (("name", user.display_name), ("email", user.email_address))
        if value is not None
    }


def author(user: User | None) -> Author | None:
    if user is None:
        return None
    if user.permission_id is not None:
        id = f"google:{user.permission_id}"
    elif user.email_address is not None:
        id = user.email_address
    elif user.display_name is not None:
        # Drive may hide everything but the name; keep that author stable.
        id = f"name:{hashlib.sha256(user.display_name.encode()).hexdigest()[:32]}"
    else:
        return None
    return Author(id=id, kind=AuthorKind.human, name=user.display_name)


def document(file: File, text: str) -> Object:
    encoded = text.encode()
    metadata: Metadata = {
        "parents": list(file.parents),
        "owners": [person(owner) for owner in file.owners],
    }
    if file.drive_id is not None:
        metadata["shared_drive"] = file.drive_id
    if file.last_modifying_user is not None:
        metadata["last_modified_by"] = person(file.last_modifying_user)
    if len(encoded) > MAX_CONTENT_BYTES:
        text = encoded[:MAX_CONTENT_BYTES].decode(errors="ignore")
        metadata["truncated"] = True
    return Object(
        id=document_id(file.id),
        role=Role.document,
        kind=DOCUMENT,
        title=file.name,
        url=url(file.id),
        content=text,
        created_at=milliseconds(file.created_time),
        updated_at=milliseconds(file.modified_time),
        metadata=metadata,
    )


def discussion(file: File, comments: list[Comment]) -> Snapshot:
    """Every live comment and reply; omitted ones were deleted in Drive."""
    container = Object(
        id=comments_id(file.id),
        role=Role.container,
        kind=COMMENTS,
        title=f"Comments: {file.name}",
        url=url(file.id),
        created_at=milliseconds(file.created_time),
        metadata={"document": document_id(file.id)},
    )
    entries: list[Object] = []
    for comment in comments:
        if comment.deleted:
            continue
        comment_id = f"comment:{file.id}:{comment.id}"
        quoted = comment.quoted_file_content and comment.quoted_file_content.value
        metadata: Metadata = {"resolved": comment.resolved}
        if quoted:
            metadata["quoted"] = quoted
        entries.append(
            Object(
                id=comment_id,
                role=Role.entry,
                kind=COMMENT,
                parent=container.id,
                author=author(comment.author),
                # The quoted passage gives the comment its meaning in search.
                content=f"> {quoted}\n\n{comment.content}"
                if quoted
                else comment.content,
                created_at=milliseconds(comment.created_time),
                updated_at=milliseconds(comment.modified_time),
                url=f"{url(file.id)}?disco={comment.id}",
                metadata=metadata,
            )
        )
        for reply in comment.replies:
            if reply.deleted:
                continue
            entries.append(
                Object(
                    id=f"reply:{file.id}:{comment.id}:{reply.id}",
                    role=Role.entry,
                    kind=REPLY,
                    parent=container.id,
                    reply_to=comment_id,
                    author=author(reply.author),
                    # A bare resolve or reopen has no text of its own.
                    content=reply.content or f"({reply.action or 'empty reply'})",
                    created_at=milliseconds(reply.created_time),
                    updated_at=milliseconds(reply.modified_time),
                    metadata={"action": reply.action} if reply.action else None,
                )
            )
    return Snapshot(container=container, objects=entries, complete=True)
