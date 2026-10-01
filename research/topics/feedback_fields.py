"""Exact field roles for two exposed feedback layouts; no semantic rewriting."""
from dataclasses import dataclass
from enum import StrEnum
import re

from source_spans import Kind, structural_spans


class Role(StrEnum):
    TARGET = "task-target"
    FEEDBACK = "feedback"
    PAGE = "page"
    VIEWPORT = "viewport"
    LOCATION = "location"
    ELEMENT_PATH = "element-path"
    NEARBY = "nearby-text"
    FRAMING = "framing"
    UNKNOWN = "unknown-content"


class Selection(StrEnum):
    ALL = "full-payload"
    TASK = "target-feedback-unknown"
    FEEDBACK = "feedback-unknown"


METADATA = frozenset({Role.PAGE, Role.VIEWPORT, Role.LOCATION, Role.ELEMENT_PATH, Role.NEARBY})
TASK = frozenset({Role.TARGET, Role.FEEDBACK, Role.UNKNOWN})
FIELDS = {"Page": Role.PAGE, "Viewport": Role.VIEWPORT, "Location": Role.LOCATION,
          "Element path": Role.ELEMENT_PATH, "Nearby text": Role.NEARBY, "Feedback": Role.FEEDBACK}
FIELD = re.compile(r"^\s*\*\*([^*]+):\*\*\s*")
TARGET = re.compile(r"^\s*###\s+\d+\.\s*(?:h[1-6]\s+)?")


@dataclass(frozen=True)
class Span:
    start: int
    end: int
    role: Role


def parse(text: str) -> tuple[Span, ...]:
    first = text.lstrip().splitlines()[0] if text.strip() else ""
    standard = first.startswith("## Page Feedback:")
    newly = first == "## Newly submitted page feedback"
    if not (standard or newly):
        raise ValueError("unsupported feedback field layout; do not guess")
    code = [s for s in structural_spans(text) if s.kind == Kind.CODE]
    result, cursor, active = [], 0, Role.UNKNOWN

    def emit(start, end, role):
        if end > start:
            if result and result[-1].end == start and result[-1].role == role:
                previous = result.pop()
                result.append(Span(previous.start, end, role))
            else:
                result.append(Span(start, end, role))

    for line in text.splitlines(keepends=True):
        start, end = cursor, cursor + len(line)
        cursor = end
        if any(s.start <= start < s.end for s in code):
            emit(start, end, active)
            continue
        if not line.strip():
            emit(start, end, Role.FRAMING)
            continue
        field, target = FIELD.match(line), TARGET.match(line)
        if field:
            role = FIELDS.get(field.group(1), Role.UNKNOWN)
            emit(start, start + field.end(), Role.FRAMING)
            emit(start + field.end(), end, role)
            active = role
        elif target:
            emit(start, start + target.end(), Role.FRAMING)
            emit(start + target.end(), end, Role.TARGET)
            active = Role.FEEDBACK if newly else Role.UNKNOWN
        elif line.lstrip().startswith("## Page Feedback:"):
            prefix = line.index("## Page Feedback:") + len("## Page Feedback:")
            emit(start, start + prefix, Role.FRAMING)
            emit(start + prefix, end, Role.PAGE)
            active = Role.UNKNOWN
        elif line.strip() == "## Newly submitted page feedback":
            emit(start, end, Role.FRAMING)
            active = Role.UNKNOWN
        else:
            emit(start, end, active)
    if cursor != len(text) or not result or result[0].start != 0 or result[-1].end != len(text):
        raise ValueError("feedback field source coverage invariant failed")
    if any(a.end != b.start for a, b in zip(result, result[1:])):
        raise ValueError("feedback field gaps or overlaps")
    return tuple(result)


def included(role: Role, selection: Selection) -> bool:
    if selection == Selection.ALL:
        return True
    return role in (TASK if selection == Selection.TASK else {Role.FEEDBACK, Role.UNKNOWN})


def records(text: str, spans: tuple[Span, ...], selection: Selection) -> tuple[str, ...]:
    # Separate fields remain separate documents: no joined-source bigrams.
    return (text,) if selection == Selection.ALL else tuple(text[s.start:s.end] for s in spans if included(s.role, selection))
