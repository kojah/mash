"""Complete, tokenizer-checked body spans; no token decoding or prefix truncation."""
from dataclasses import asdict, dataclass
from enum import StrEnum
import re

from question_html import Kind, Rendered
from reference_review import digest


class Method(StrEnum):
    WHOLE = 'whole-if-supported'
    BLOCK = 'recorded-block-boundary'
    WHITESPACE = 'recursive-whitespace-midpoint'
    CODEPOINT = 'recursive-codepoint-midpoint'


class Reason(StrEnum):
    WHITESPACE = 'whitespace-only'
    WINDOW = 'unsplittable-model-window'
    COUNT = 'nonempty-unit-cap'
    EMPTY = 'empty-displayed-body'


@dataclass(frozen=True)
class Unit:
    start: int
    end: int
    byte_start: int
    byte_end: int
    piece_indices: tuple[int, ...]
    method: Method
    tokens: int | None
    reason: Reason | None
    body_sha256: str
    input_sha256: str


@dataclass(frozen=True)
class Plan:
    units: tuple[Unit, ...]
    reason: Reason | None
    tokenization_calls: int
    source_sha256: str
    displayed_sha256: str


def capture_plan(value: Plan):
    """Emit arrays explicitly; decoding JSON must preserve the comparison schema."""
    result = asdict(value)
    result['units'] = [dict(asdict(u), piece_indices=list(u.piece_indices)) for u in value.units]
    return result


def plan(title: str, body: Rendered, token_count, maximum_tokens=512, maximum_units=32):
    if (not isinstance(title, str) or not title.strip() or type(maximum_tokens) is not int
            or maximum_tokens < 2 or type(maximum_units) is not int or maximum_units < 1):
        raise ValueError('valid source title and literal positive caps required')
    prefix, text = title + '\n\n', body.text
    counts = {}

    def count(start, end):
        key = (start, end)
        if key not in counts:
            value = token_count(prefix + text[start:end])
            if type(value) is not int or value < 0:
                raise ValueError('literal tokenizer count required')
            counts[key] = value
        return counts[key]

    def unit(start, end, method, reason=None):
        original = text[start:end]
        return Unit(start, end, len(text[:start].encode()), len(text[:end].encode()),
                    tuple(i for i, p in enumerate(body.pieces) if p.rendered_start < end and start < p.rendered_end),
                    method, count(start, end) if original.strip() else None, reason,
                    digest(original.encode()), digest((prefix + original).encode()))

    def subdivide(start, end, method):
        if not text[start:end].strip():
            return [unit(start, end, method, Reason.WHITESPACE)]
        if 2 <= count(start, end) <= maximum_tokens:
            return [unit(start, end, method)]
        if end - start <= 1:
            return [unit(start, end, method, Reason.WINDOW)]
        midpoint = (start + end) // 2
        boundaries = [start + m.end() for m in re.finditer(r'\s+', text[start:end]) if start < start + m.end() < end]
        if boundaries:
            split = min(boundaries, key=lambda p: (abs(p - midpoint), p))
            new_method = Method.WHITESPACE
        else:
            split, new_method = midpoint, Method.CODEPOINT
        return subdivide(start, split, new_method) + subdivide(split, end, new_method)

    if text.strip() and 2 <= count(0, len(text)) <= maximum_tokens:
        units = [unit(0, len(text), Method.WHOLE)]
    else:
        boundaries = sorted({0, len(text), *(p.rendered_end for p in body.pieces if p.kind == Kind.SEPARATOR)})
        units = [u for a, b in zip(boundaries, boundaries[1:]) for u in subdivide(a, b, Method.BLOCK)]
    cursor = 0
    for u in units:
        if u.start != cursor or u.end <= u.start:
            raise ValueError('incomplete or overlapping displayed-body unit coverage')
        cursor = u.end
    if cursor != len(text):
        raise ValueError('incomplete displayed-body unit coverage')
    reason = (Reason.EMPTY if not text.strip() else
              Reason.WINDOW if any(u.reason == Reason.WINDOW for u in units) else
              Reason.COUNT if sum(u.reason is None for u in units) > maximum_units else None)
    return Plan(tuple(units), reason, len(counts), body.source_sha256, digest(text.encode()))
