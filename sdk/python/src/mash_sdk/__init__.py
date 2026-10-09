"""Experimental typed connectors with durable local and shared delivery."""

from ._delivery import DeliveryError
from ._generated.batch import Attachment, Author, AuthorKind, Batch
from ._generated.batch import ChangeUpsertChat as Chat
from ._generated.batch import ChangeUpsertDocument as Document
from ._generated.batch import ChangeUpsertMessage as Message
from ._generated.receipt import Receipt
from ._types import Metadata
from .changes import Changes
from .codec import InvalidBatch, encode_batch
from .connector import Connector
from .destination import Destination
from .runner import Debounce, Delivery, DocumentSnapshot, Snapshot

__all__ = [
    "Attachment",
    "Author",
    "AuthorKind",
    "Batch",
    "Changes",
    "Chat",
    "Connector",
    "Debounce",
    "Delivery",
    "DeliveryError",
    "Destination",
    "Document",
    "DocumentSnapshot",
    "InvalidBatch",
    "Message",
    "Metadata",
    "Receipt",
    "Snapshot",
    "encode_batch",
]
