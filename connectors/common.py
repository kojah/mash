"""Small source-side types shared by the connector blueprints."""

from dataclasses import dataclass
from datetime import datetime
from enum import Enum
from typing import Literal

from mash_sdk import Attachment, Author


class Operation(Enum):
    REFRESH_CHAT = "refresh-chat"
    REFRESH_MESSAGE = "refresh-message"
    DELETE_CHAT = "delete-chat"
    DELETE_MESSAGE = "delete-message"


@dataclass(frozen=True)
class Sender:
    id: str
    name: str | None
    kind: Literal["human", "bot", "unknown"]

    def to_author(self) -> Author:
        return Author(id=self.id, name=self.name, kind=self.kind)


@dataclass(frozen=True)
class File:
    id: str
    name: str
    media_type: str
    url: str
    size: int | None

    def to_attachment(self) -> Attachment:
        return Attachment(
            id=self.id,
            name=self.name,
            media_type=self.media_type,
            url=self.url,
            bytes=self.size,
        )


def milliseconds(value: datetime | None) -> int | None:
    """Mash timestamps are Unix milliseconds; missing times remain unknown."""
    if value is None:
        return None
    if value.tzinfo is None:
        raise ValueError("source timestamps must include a timezone")
    return int(value.timestamp() * 1000)
