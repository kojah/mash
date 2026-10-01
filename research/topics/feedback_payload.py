"""Explicit submitted_feedback source envelope; not general semantic context."""
from dataclasses import dataclass
from enum import StrEnum


class Status(StrEnum):
    ABSENT = "absent"
    MALFORMED = "malformed"
    PAYLOAD = "payload"


@dataclass(frozen=True)
class Selection:
    status: Status
    start: int | None = None
    end: int | None = None


def select(text: str) -> Selection:
    opening, closing = "<submitted_feedback>", "</submitted_feedback>"
    if opening not in text and closing not in text:
        return Selection(Status.ABSENT)
    if text.count(opening) != 1 or text.count(closing) != 1:
        return Selection(Status.MALFORMED)
    start = text.index(opening) + len(opening)
    end = text.index(closing)
    if start >= end or not text[start:end].strip():
        return Selection(Status.MALFORMED)
    return Selection(Status.PAYLOAD, start, end)
