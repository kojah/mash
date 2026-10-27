"""Desired custom-gold API: user code projects typed silver outside the Mash server."""

from collections.abc import Sequence

from mash_sdk import (
    DocumentRef,
    DocumentSilverSnapshot,
    MessageRef,
    Role,
    SearchDocument,
    SilverSnapshot,
)


async def project(
    snapshot: SilverSnapshot | DocumentSilverSnapshot,
) -> Sequence[SearchDocument]:
    """Publish evidence-backed gold for standalone documents and container entries."""
    if isinstance(snapshot, DocumentSilverSnapshot):
        document = snapshot.document
        return [
            SearchDocument(
                id=document.id,
                text=f"{document.title or ''}\n\n{document.content or ''}".strip(),
                references=[
                    DocumentRef(source_id=snapshot.source_id, document_id=document.id)
                ],
                metadata={"source_id": snapshot.source_id, "document_id": document.id},
            )
        ]
    return [
        SearchDocument(
            id=entry.id,
            text=f"{snapshot.container.title or ''}\n\n{entry.content or ''}".strip(),
            references=[
                MessageRef(
                    source_id=snapshot.source_id,
                    chat_id=snapshot.container.id,
                    message_id=entry.id,
                )
            ],
            metadata={
                "source_id": snapshot.source_id,
                "chat_id": snapshot.container.id,
            },
        )
        for entry in snapshot.objects
        if entry.role == Role.entry and (entry.content or "").strip()
    ]
