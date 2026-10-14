"""Channels are containers; messages and thread replies are entries in them.

Message IDs keep Slack's channel ID and timestamp string
(`C0123456789:1760000000.000100`); a float would lose precision. A reply
points at its thread's parent message with `reply_to`.
"""

import re
from dataclasses import dataclass

from mash_sdk import Attachment, Author, AuthorKind, KindRule, Object, Role
from pydantic import BaseModel, TypeAdapter

CHANNEL = "slack:channel"
MESSAGE = "slack:message"
KINDS = (
    KindRule(kind=CHANNEL, role=Role.container, root=True),
    KindRule(kind=MESSAGE, role=Role.entry, parents=[CHANNEL]),
)

# Messages people wrote; joins, leaves, topic changes, pins and tombstones of
# deleted thread parents are skipped.
CONTENT = {None, "bot_message", "file_share", "me_message", "thread_broadcast"}

# `<@U123>`, `<#C123|general>`, `<!here>`, `<https://example.com|label>`.
LINK = re.compile(r"<([^<>|]+)(?:\|([^<>]*))?>")
MENTION = re.compile(r"<@([UW][A-Z0-9]+)[|>]")


class Profile(BaseModel):
    display_name: str = ""
    real_name: str = ""


class User(BaseModel):
    id: str
    name: str = ""
    is_bot: bool = False
    profile: Profile = Profile()

    def display(self) -> str | None:
        return self.profile.display_name or self.profile.real_name or self.name or None


class File(BaseModel):
    id: str
    name: str | None = None
    title: str | None = None
    mimetype: str | None = None
    size: int | None = None
    permalink: str | None = None  # Absent on deleted or hidden files.


class Edited(BaseModel):
    ts: str


class BotProfile(BaseModel):
    name: str | None = None


class Message(BaseModel):
    ts: str
    text: str = ""
    subtype: str | None = None
    user: str | None = None
    bot_id: str | None = None
    username: str | None = None
    bot_profile: BotProfile | None = None
    thread_ts: str | None = None
    reply_count: int = 0
    latest_reply: str | None = None
    edited: Edited | None = None
    files: list[File] = []

    def reply_parent(self) -> str | None:
        return self.thread_ts if self.thread_ts not in (None, self.ts) else None


MESSAGES = TypeAdapter(list[Message])


class Channel(BaseModel):
    id: str
    name: str | None = None
    created: int | None = None
    is_private: bool = False
    is_archived: bool = False
    is_im: bool = False
    is_mpim: bool = False
    user: str | None = None  # The other person in a direct message.


@dataclass(frozen=True)
class Workspace:
    """From auth.test: the token's team and its web address."""

    id: str
    name: str
    url: str  # https://example.slack.com/


People = dict[str, User | None]  # None: users.info failed; the ID stays.


def milliseconds(ts: str) -> int:
    seconds, _, fraction = ts.partition(".")
    return int(seconds) * 1000 + int((fraction + "000")[:3])


def message_id(channel_id: str, ts: str) -> str:
    return f"{channel_id}:{ts}"


def mentions(item: Message) -> set[str]:
    return set(MENTION.findall(item.text))


def name(people: People, id: str) -> str | None:
    person = people.get(id)
    return None if person is None else person.display()


def render(text: str, people: People) -> str:
    """Slack's escaped mrkdwn with names in place of user and channel IDs."""

    def replace(match: re.Match[str]) -> str:
        target, label = match.group(1), match.group(2)
        if target.startswith("@"):
            return "@" + (label or name(people, target[1:]) or target[1:])
        if target.startswith("#"):
            return "#" + (label or target[1:])
        if target.startswith("!"):  # <!here>, <!subteam^S123|@team>
            return label or "@" + target[1:].split("^")[0]
        return f"{label} ({target})" if label and label != target else target

    text = LINK.sub(replace, text)
    return text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")


def author(item: Message, people: People) -> Author | None:
    if item.user is not None:
        person = people.get(item.user)
        if person is None:
            return Author(id=f"slack:{item.user}", kind=AuthorKind.unknown)
        kind = AuthorKind.bot if person.is_bot else AuthorKind.human
        return Author(id=f"slack:{item.user}", kind=kind, name=person.display())
    if item.bot_id is not None:
        label = item.bot_profile.name if item.bot_profile else item.username
        return Author(id=f"slack:{item.bot_id}", kind=AuthorKind.bot, name=label)
    return None


def chat(workspace: Workspace, channel: Channel, people: People) -> Object:
    title = f"#{channel.name}" if channel.name and not channel.is_mpim else None
    if channel.is_im and channel.user is not None:
        title = name(people, channel.user) or channel.user
    native = "im" if channel.is_im else "mpim" if channel.is_mpim else "channel"
    return Object(
        id=channel.id,
        role=Role.container,
        kind=CHANNEL,
        title=title or channel.name,
        url=f"{workspace.url}archives/{channel.id}",
        created_at=None if channel.created is None else channel.created * 1000,
        metadata={
            "workspace": {"id": workspace.id, "name": workspace.name},
            "native_kind": native,
            "private": channel.is_private,
            "archived": channel.is_archived,
        },
    )


def message(
    workspace: Workspace, channel_id: str, item: Message, people: People
) -> Object:
    parent = item.reply_parent()
    url = f"{workspace.url}archives/{channel_id}/p{item.ts.replace('.', '')}"
    if parent is not None:
        url += f"?thread_ts={parent}&cid={channel_id}"
    files = [file for file in item.files if file.permalink is not None]
    content = render(item.text, people) or "\n".join(
        f"(file: {file.title or file.name or file.id})" for file in files
    )
    return Object(
        id=message_id(channel_id, item.ts),
        role=Role.entry,
        kind=MESSAGE,
        parent=channel_id,
        reply_to=None if parent is None else message_id(channel_id, parent),
        author=author(item, people),
        content=content,
        url=url,
        created_at=milliseconds(item.ts),
        updated_at=None if item.edited is None else milliseconds(item.edited.ts),
        # File names and links only; the connector never downloads files.
        attachments=[
            Attachment(
                id=file.id,
                name=file.title or file.name or file.id,
                media_type=file.mimetype or "application/octet-stream",
                url=file.permalink,
                bytes=file.size,
            )
            for file in files[:32]
            if file.permalink is not None
        ],
        metadata={"native_ts": item.ts},
    )
