"""Channels and threads are containers; their messages are entries.

IDs are namespaced because a thread started from a message shares that
message's snowflake. Threads are root containers like channels: each is
selected on its own, and its parent channel is metadata.
"""

from datetime import datetime

import hikari
from mash_sdk import Attachment, Author, AuthorKind, KindRule, Metadata, Object, Role

CHANNEL = "discord:channel"
THREAD = "discord:thread"
MESSAGE = "discord:message"
KINDS = (
    KindRule(kind=CHANNEL, role=Role.container, root=True),
    KindRule(kind=THREAD, role=Role.container, root=True),
    KindRule(kind=MESSAGE, role=Role.entry, parents=[CHANNEL, THREAD]),
)

# What people and bots say; joins, pins, boosts, thread creation and other
# system notices are skipped.
CONVERSATION = {
    hikari.MessageType.DEFAULT,
    hikari.MessageType.REPLY,
    hikari.MessageType.CHAT_INPUT,
    hikari.MessageType.CONTEXT_MENU_COMMAND,
}


def milliseconds(value: datetime | None) -> int | None:
    return None if value is None else int(value.timestamp() * 1000)


def chat_id(channel: int) -> str:
    return f"channel:{channel}"


def message_id(message: int) -> str:
    return f"message:{message}"


def guild_of(channel: hikari.TextableChannel) -> int | None:
    return channel.guild_id if isinstance(channel, hikari.GuildChannel) else None


def link(channel: hikari.TextableChannel, *rest: int) -> str:
    guild = guild_of(channel)
    path = "/".join(str(part) for part in (channel.id, *rest))
    return f"https://discord.com/channels/{'@me' if guild is None else guild}/{path}"


def chat(channel: hikari.TextableChannel) -> Object:
    thread = isinstance(channel, hikari.GuildThreadChannel)
    guild = guild_of(channel)
    metadata: Metadata = {}
    if guild is not None:
        metadata["guild_id"] = str(guild)
    if thread:
        metadata["parent_channel_id"] = str(channel.parent_id)
    return Object(
        id=chat_id(channel.id),
        role=Role.container,
        kind=THREAD if thread else CHANNEL,
        title=channel.name,
        url=link(channel),
        created_at=milliseconds(channel.id.created_at),
        metadata=metadata or None,
    )


def author(message: hikari.Message) -> Author:
    user = message.author
    bot = user.is_bot or user.is_system or message.webhook_id is not None
    return Author(
        id=f"discord:{user.id}",
        kind=AuthorKind.bot if bot else AuthorKind.human,
        name=user.global_name or user.username,
    )


def attachment(item: hikari.Attachment) -> Attachment:
    # A reference only: Discord's CDN URLs are signed and expire; nothing is downloaded.
    return Attachment(
        id=f"attachment:{item.id}",
        name=item.filename,
        media_type=item.media_type or "application/octet-stream",
        url=item.url,
        bytes=item.size,
    )


def message(channel: hikari.TextableChannel, item: hikari.Message) -> Object | None:
    """None for system notices and for messages with no text or files."""
    if item.type not in CONVERSATION or not (item.content or item.attachments):
        return None
    reference = item.message_reference
    reply = (
        reference.id
        if item.type == hikari.MessageType.REPLY
        and reference is not None
        and reference.channel_id == channel.id
        else None
    )
    metadata: Metadata | None = None
    if reference is not None and reply is None:
        # Forwards and cross-channel references are not same-chat replies.
        metadata = {
            "native_reference": {
                "channel_id": str(reference.channel_id),
                "message_id": None if reference.id is None else str(reference.id),
            }
        }
    return Object(
        id=message_id(item.id),
        role=Role.entry,
        kind=MESSAGE,
        parent=chat_id(channel.id),
        content=item.content or "",
        author=author(item),
        reply_to=None if reply is None else message_id(reply),
        created_at=milliseconds(item.timestamp),
        updated_at=milliseconds(item.edited_timestamp),
        url=link(channel, item.id),
        attachments=[attachment(file) for file in item.attachments],
        metadata=metadata,
    )
