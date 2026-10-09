"""Experimental typed connectors with durable local and shared delivery."""

from ._delivery import DeliveryError
from ._generated.batch import Attachment, Author, AuthorKind, Batch, Role
from ._generated.batch import ChangeUpsertObject as Object
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
    "Connector",
    "Debounce",
    "Delivery",
    "DeliveryError",
    "Destination",
    "DocumentSnapshot",
    "InvalidBatch",
    "Metadata",
    "Object",
    "Receipt",
    "Role",
    "Snapshot",
    "encode_batch",
]
