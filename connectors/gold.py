"""Desired custom-gold API: user code projects typed silver outside the Mash server."""

from collections.abc import Sequence

from mash_sdk import MessageRef, SearchDocument, SilverSnapshot


async def project(snapshot: SilverSnapshot) -> Sequence[SearchDocument]:
    """Publish one evidence-backed search document per nonempty message."""
    return [
        SearchDocument(
            id=message.id,
            text=f"{snapshot.chat.title or ''}\n\n{message.content}".strip(),
            references=[
                MessageRef(
                    source_id=snapshot.source_id,
                    chat_id=snapshot.chat.id,
                    message_id=message.id,
                )
            ],
            metadata={"source_id": snapshot.source_id, "chat_id": snapshot.chat.id},
        )
        for message in snapshot.messages
        if message.content.strip()
    ]
