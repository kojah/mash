"""Desired custom-gold API: user code projects typed silver outside the Mash server."""

from collections.abc import Sequence

from mash_sdk import (
    DocumentRef,
    DocumentSilverSnapshot,
    MessageRef,
    SearchDocument,
    SilverSnapshot,
)


async def project(
    snapshot: SilverSnapshot | DocumentSilverSnapshot,
) -> Sequence[SearchDocument]:
    """Publish evidence-backed gold for standalone documents and chat messages."""
    if isinstance(snapshot, DocumentSilverSnapshot):
        document = snapshot.document
        return [
            SearchDocument(
                id=document.id,
                text=f"{document.title or ''}\n\n{document.content}".strip(),
                references=[
                    DocumentRef(source_id=snapshot.source_id, document_id=document.id)
                ],
                metadata={"source_id": snapshot.source_id, "document_id": document.id},
            )
        ]
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
