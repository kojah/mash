"""Bounded exact-source subunits, without rewriting or semantic interpretation."""
from dataclasses import dataclass
from enum import StrEnum
import math
import re

from pair_review import Key
from source_spans import Kind
from span_units import Unit


CAP = 8
WORD = re.compile(r"\S+")
# Conservative punctuation: do not split technical dots, inline code or quotes.
PROTECTED = re.compile(r'`[^`]*`|"(?:\\.|[^"\\])*"')
BOUNDARY = re.compile(r"[.!?](?=\s+[A-Z0-9])|;(?=\s)")


class Mode(StrEnum):
    ORIGINAL = "original"
    SENTENCES = "sentences-semicolons"
    WINDOWS = "windows16-overlap4-cap8"


@dataclass(frozen=True)
class Fragment:
    parent: Key
    index: int
    unit: Unit

    def metadata(self):
        return {"parent": vars(self.parent), "fragment": self.index, **self.unit.metadata()}


def bounds(text: str, kind: Kind, mode: Mode) -> tuple[tuple[int, int], ...]:
    if not text:
        return ()
    if mode == Mode.ORIGINAL or kind in (Kind.CODE, Kind.HEADING):
        return ((0, len(text)),)
    if mode == Mode.SENTENCES:
        protected = [(m.start(), m.end()) for m in PROTECTED.finditer(text)]
        # Unmatched inline code/quotes conservatively shield the rest of the unit.
        for character in ('`', '"'):
            uncovered = [i for i, c in enumerate(text) if c == character
                         and not any(a <= i < b for a, b in protected)]
            if uncovered:
                protected.append((uncovered[0], len(text)))
        ends = [m.end() for m in BOUNDARY.finditer(text)
                if not any(a <= m.start() < b for a, b in protected)] + [len(text)]
        starts = [0] + ends[:-1]
        segments = [(a, b) for a, b in zip(starts, ends) if text[a:b].strip()]
        if len(segments) > CAP:
            # Merge adjacent pieces, preserving all text and bounding encoder work.
            groups = [segments[i * len(segments) // CAP:(i + 1) * len(segments) // CAP] for i in range(CAP)]
            segments = [(group[0][0], group[-1][1]) for group in groups]
        return tuple(segments)
    words = list(WORD.finditer(text))
    if len(words) <= 16:
        return ((0, len(text)),)
    width = max(16, math.ceil((len(words) + 4 * (CAP - 1)) / CAP))
    stride = width - 4
    result = []
    for start in range(0, len(words), stride):
        end = min(start + width, len(words))
        result.append((0 if start == 0 else words[start].start(),
                       len(text) if end == len(words) else words[end].start()))
        if end == len(words):
            break
    if len(result) > CAP:
        raise ValueError("fragment window cap invariant failed")
    return tuple(result)


def split(units: tuple[Unit, ...], mode: Mode) -> tuple[Fragment, ...]:
    result = []
    for unit in units:
        for i, (start, end) in enumerate(bounds(unit.text, unit.kind, mode)):
            # Stable, injective experiment keys; parent metadata retains original key.
            key = Key(unit.key.thread, unit.key.ordinal, unit.key.index * CAP + i)
            child = Unit(key, unit.role, unit.kind, unit.start + start, unit.start + end,
                         unit.source_id, unit.source_sha256, unit.text[start:end])
            result.append(Fragment(unit.key, i, child))
    return tuple(result)
